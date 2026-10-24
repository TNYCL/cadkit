//! Lit, depth-tested rendering of [`MeshScene`]s with an orbit camera.

use cadkit_scene::{Mesh3d, MeshBudget, MeshScene, relative_positions};
use wgpu::util::DeviceExt;

use crate::{GpuError, check, scopes};

/// Position (3 x f32) and linear color (4 x f32).
const VERTEX_BYTES: usize = 28;
/// Upper bounds of [`mesh_budget`], protecting integrated GPU memory: 224 MB of vertices
/// and 192 MB of indices.
const MAX_SCENE_VERTICES: usize = 8_000_000;
const MAX_SCENE_TRIANGLES: usize = 16_000_000;

/// A perspective camera orbiting `target`, with +Z up.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct OrbitCamera {
    /// Point looked at, in model units.
    pub target: [f64; 3],
    /// Azimuth of the eye around +Z, radians counter-clockwise from +X.
    pub yaw: f64,
    /// Elevation of the eye, radians; must stay inside (-pi/2, pi/2).
    pub pitch: f64,
    /// Eye distance from the target, in model units.
    pub distance: f64,
    /// Vertical field of view, radians, within (0.01, 3).
    pub vertical_fov: f64,
}

impl OrbitCamera {
    /// A camera at `distance` from `target`, looking from azimuth `yaw` and elevation `pitch`.
    pub fn new(target: [f64; 3], yaw: f64, pitch: f64, distance: f64, vertical_fov: f64) -> Self {
        Self {
            target,
            yaw,
            pitch,
            distance,
            vertical_fov,
        }
    }

    /// Column-major view-projection matrix (Direct3D clip depth 0..1) relative to the target.
    pub fn matrix(&self, aspect: f32) -> Result<[f32; 16], GpuError> {
        self.matrix_relative(aspect, self.target, 1e-4)
    }

    /// View-projection matrix for vertices given relative to `origin`; the target offset is
    /// subtracted in f64 and must convert to f32 within `max_coordinate_error`.
    pub fn matrix_relative(
        &self,
        aspect: f32,
        origin: [f64; 3],
        max_coordinate_error: f64,
    ) -> Result<[f32; 16], GpuError> {
        if !self.yaw.is_finite()
            || !self.target.iter().all(|v| v.is_finite())
            || !self.pitch.is_finite()
            || !self.distance.is_finite()
            || self.distance <= 0.
            || !self.vertical_fov.is_finite()
            || self.vertical_fov <= 0.01
            || self.vertical_fov >= 3.
            || !aspect.is_finite()
            || aspect <= 0.
            || self.pitch.abs() >= std::f64::consts::FRAC_PI_2 - 1e-4
        {
            return Err(GpuError::InvalidCamera);
        }
        let radius = self.distance as f32;
        if !radius.is_finite() || !(1e-5..=1e12).contains(&radius) {
            return Err(GpuError::CameraRange);
        }
        let target = camera_offset(self.target, origin, max_coordinate_error)?;
        let target = glam::Vec3::from_array(target);
        let eye = glam::Vec3::new(
            (self.yaw.cos() * self.pitch.cos()) as f32,
            (self.yaw.sin() * self.pitch.cos()) as f32,
            self.pitch.sin() as f32,
        ) * radius;
        let view = glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::ZERO, glam::Vec3::Z)
            * glam::Mat4::from_translation(-target);
        let projection = glam::camera::rh::proj::directx::perspective(
            self.vertical_fov as f32,
            aspect,
            (radius * 0.001).max(1e-6),
            radius * 100.,
        );
        Ok((projection * view).to_cols_array())
    }
}

fn camera_offset(target: [f64; 3], origin: [f64; 3], max_error: f64) -> Result<[f32; 3], GpuError> {
    if !max_error.is_finite() || max_error <= 0. {
        return Err(GpuError::CameraRange);
    }
    let mut local = [0f32; 3];
    for ((slot, coordinate), start) in local.iter_mut().zip(target).zip(origin) {
        let delta = coordinate - start;
        let virtual_start = coordinate - delta;
        let virtual_coordinate = delta + virtual_start;
        let subtraction_error = (coordinate - virtual_coordinate) + (virtual_start - start);
        let encoded = delta as f32;
        let error = (subtraction_error.abs() + (delta - f64::from(encoded)).abs()).next_up();
        // Even an exactly represented large offset can swallow small geometry in the matrix
        // product.
        let max_offset = max_error / f64::from(f32::EPSILON) * 0.5;
        if !delta.is_finite()
            || !encoded.is_finite()
            || !error.is_finite()
            || error > max_error
            || delta.abs() > max_offset
        {
            return Err(GpuError::CameraRange);
        }
        *slot = encoded;
    }
    Ok(local)
}

/// Scene budget derived from the device: one vertex and one index buffer must fit
/// `max_buffer_size`, capped at 8M vertices and 16M triangles and never below
/// [`MeshBudget::default`].
pub fn mesh_budget(device: &wgpu::Device) -> MeshBudget {
    let limit = usize::try_from(device.limits().max_buffer_size).unwrap_or(usize::MAX);
    let default = MeshBudget::default();
    MeshBudget::new(
        (limit / VERTEX_BYTES)
            .min(MAX_SCENE_VERTICES)
            .max(default.max_vertices),
        (limit / (3 * std::mem::size_of::<u32>()))
            .min(MAX_SCENE_TRIANGLES)
            .max(default.max_triangles),
    )
}

/// Pipeline for [`MeshScene`]s: `Depth32Float` depth, single-sampled, opaque.
pub struct MeshRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// GPU buffers of a prepared scene.
pub struct PreparedMeshes {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    camera: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    index_count: u32,
    source: Vec<Mesh3d>,
    revision: u64,
    origin: [f64; 3],
}

impl MeshRenderer {
    /// Creates the pipeline for color targets of `format`.
    pub async fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Result<Self, GpuError> {
        let (validation, allocation) = scopes(device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cadkit mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadkit mesh camera"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cadkit mesh pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadkit mesh"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        check(validation, allocation).await?;
        Ok(Self { pipeline, layout })
    }

    /// Uploads a complete `scene` with vertices relative to the camera target.
    pub async fn prepare(
        &self,
        device: &wgpu::Device,
        scene: &MeshScene,
        camera: OrbitCamera,
        aspect: f32,
        max_coordinate_error: f64,
    ) -> Result<PreparedMeshes, GpuError> {
        self.prepare_scene(device, scene, camera, aspect, max_coordinate_error, false)
            .await
    }

    /// Like [`Self::prepare`], but also accepts a scene marked incomplete (an interactive
    /// preview that reports its missing objects elsewhere).
    pub async fn prepare_preview(
        &self,
        device: &wgpu::Device,
        scene: &MeshScene,
        camera: OrbitCamera,
        aspect: f32,
        max_coordinate_error: f64,
    ) -> Result<PreparedMeshes, GpuError> {
        self.prepare_scene(device, scene, camera, aspect, max_coordinate_error, true)
            .await
    }

    async fn prepare_scene(
        &self,
        device: &wgpu::Device,
        scene: &MeshScene,
        camera: OrbitCamera,
        aspect: f32,
        max_coordinate_error: f64,
        allow_partial: bool,
    ) -> Result<PreparedMeshes, GpuError> {
        if !allow_partial && !scene.complete {
            return Err(GpuError::IncompleteScene);
        }
        let matrix = camera.matrix(aspect)?;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut vertex_count = 0u32;
        for mesh in &scene.meshes {
            let (encoded_vertices, encoded_indices) =
                encode_mesh(mesh, camera.target, max_coordinate_error, vertex_count)?;
            vertex_count = u32::try_from(mesh.positions.len())
                .ok()
                .and_then(|n| vertex_count.checked_add(n))
                .ok_or(GpuError::BufferLimit)?;
            vertices.extend(encoded_vertices);
            indices.extend(encoded_indices);
        }
        if vertices.is_empty() || indices.is_empty() {
            return Err(GpuError::EmptyScene);
        }
        let index_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let limit = device.limits().max_buffer_size;
        if vertices.len() as u64 > limit || index_bytes.len() as u64 > limit {
            return Err(GpuError::BufferLimit);
        }
        let index_count = u32::try_from(indices.len()).map_err(|_| GpuError::BufferLimit)?;
        let (validation, allocation) = scopes(device);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cadkit mesh vertices"),
            contents: &vertices,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cadkit mesh indices"),
            contents: &index_bytes,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cadkit mesh camera"),
            contents: &matrix_bytes(&matrix),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadkit mesh camera"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        check(validation, allocation).await?;
        Ok(PreparedMeshes {
            vertices: vertex_buffer,
            indices: index_buffer,
            camera: uniform,
            bind_group,
            index_count,
            source: scene.meshes.clone(),
            revision: scene.revision,
            origin: camera.target,
        })
    }

    /// Records the draw of `prepared`.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>, prepared: &PreparedMeshes) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &prepared.bind_group, &[]);
        pass.set_vertex_buffer(0, prepared.vertices.slice(..));
        pass.set_index_buffer(prepared.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..prepared.index_count, 0, 0..1);
    }
}

impl PreparedMeshes {
    /// [`MeshScene::revision`] of the uploaded meshes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Scene origin the vertices are relative to: the camera target at preparation.
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// Rewrites only the meshes that changed when `scene` has the same mesh ids and sizes in
    /// the same order; returns `false` (nothing written) otherwise. Every change is encoded
    /// before any buffer is touched.
    pub fn update_meshes(
        &mut self,
        queue: &wgpu::Queue,
        scene: &MeshScene,
        max_error: f64,
    ) -> Result<bool, GpuError> {
        if self.source.len() != scene.meshes.len()
            || self.source.iter().zip(&scene.meshes).any(|(old, new)| {
                old.id != new.id
                    || old.positions.len() != new.positions.len()
                    || old.triangles.len() != new.triangles.len()
            })
        {
            return Ok(false);
        }
        let mut updates = Vec::new();
        let mut vertex_offset = 0usize;
        let mut index_offset = 0usize;
        for (index, (old, new)) in self.source.iter().zip(&scene.meshes).enumerate() {
            if old != new {
                let base = u32::try_from(vertex_offset).map_err(|_| GpuError::BufferLimit)?;
                let (vertices, indices) = encode_mesh(new, self.origin, max_error, base)?;
                updates.push((index, vertex_offset, index_offset, vertices, indices));
            }
            vertex_offset += new.positions.len();
            index_offset += new.triangles.len() * 3;
        }
        for (index, vertex_offset, index_offset, vertices, indices) in updates {
            if !vertices.is_empty() {
                queue.write_buffer(
                    &self.vertices,
                    (vertex_offset * VERTEX_BYTES) as u64,
                    &vertices,
                );
            }
            if !indices.is_empty() {
                let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
                queue.write_buffer(
                    &self.indices,
                    (index_offset * std::mem::size_of::<u32>()) as u64,
                    &bytes,
                );
            }
            if let (Some(slot), Some(mesh)) = (self.source.get_mut(index), scene.meshes.get(index))
            {
                slot.clone_from(mesh);
            }
        }
        self.revision = scene.revision;
        Ok(true)
    }

    /// Writes a new camera matrix for vertices relative to [`Self::origin`].
    pub fn update_camera(
        &self,
        queue: &wgpu::Queue,
        camera: OrbitCamera,
        aspect: f32,
        max_coordinate_error: f64,
    ) -> Result<(), GpuError> {
        let matrix = camera.matrix_relative(aspect, self.origin, max_coordinate_error)?;
        queue.write_buffer(&self.camera, 0, &matrix_bytes(&matrix));
        Ok(())
    }

    /// Whether `camera` can still be drawn against [`Self::origin`] within the error budget;
    /// otherwise prepare the scene again.
    pub fn can_reuse_origin(&self, camera: OrbitCamera, max_coordinate_error: f64) -> bool {
        camera_offset(camera.target, self.origin, max_coordinate_error).is_ok()
    }
}

fn matrix_bytes(matrix: &[f32; 16]) -> Vec<u8> {
    matrix.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Vertex bytes and global indices of one mesh.
fn encode_mesh(
    mesh: &Mesh3d,
    origin: [f64; 3],
    max_error: f64,
    base: u32,
) -> Result<(Vec<u8>, Vec<u32>), GpuError> {
    if mesh.color[3] != 255 {
        return Err(GpuError::Translucent(mesh.id));
    }
    let positions = relative_positions(&mesh.positions, origin, max_error).map_err(|source| {
        GpuError::Precision {
            mesh: mesh.id,
            source,
        }
    })?;
    // Colors are sRGB; lighting is computed in linear space (in f32, as uploaded).
    let color = mesh.color.map(|value| {
        let value = f32::from(value) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    let mut vertices = Vec::with_capacity(positions.len() * VERTEX_BYTES);
    for position in positions {
        for value in position.into_iter().chain(color) {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut indices = Vec::with_capacity(mesh.triangles.len() * 3);
    for triangle in &mesh.triangles {
        for index in triangle {
            if *index as usize >= mesh.positions.len() {
                return Err(GpuError::InvalidTriangle(mesh.id));
            }
            indices.push(base.checked_add(*index).ok_or(GpuError::BufferLimit)?);
        }
    }
    Ok((vertices, indices))
}
