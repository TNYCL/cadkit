//! Renderer for [`cadkit_scene::PlanFrame`] chunks.

use std::collections::HashMap;
use std::ops::Range;

use cadkit_scene::ChunkDraw;
use cadkit_scene::layout::INSTANCE_BYTES;

/// A chunk buffer not drawn for this many prepared frames is released.
const EVICT_FRAMES: u64 = 600;
/// Per-chunk uniform: origin, scale, pixels per point and viewport as eight f32.
const UNIFORM_BYTES: u64 = 32;
/// Prepared frames kept per renderer; several canvases may share one renderer.
const KEEP_PREPARED: u64 = 16;

/// Where a frame is drawn, in physical pixels of the render target.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PlanTarget {
    /// Canvas center, in physical pixels (y down). Chunk offsets are added to it in f64.
    pub center_px: [f64; 2],
    /// Physical pixels per logical pixel; stroke widths are logical.
    pub pixels_per_point: f32,
    /// Viewport of the draw: left, top, width and height in physical pixels.
    pub viewport_px: [f32; 4],
}

impl PlanTarget {
    /// A target whose canvas fills `viewport_px` (left, top, width, height).
    pub fn new(viewport_px: [f32; 4], pixels_per_point: f32) -> Self {
        let [left, top, width, height] = viewport_px;
        Self {
            center_px: [
                f64::from(left) + f64::from(width) / 2.,
                f64::from(top) + f64::from(height) / 2.,
            ],
            pixels_per_point,
            viewport_px,
        }
    }

    /// Sets [`Self::center_px`].
    #[must_use]
    pub fn with_center_px(mut self, center_px: [f64; 2]) -> Self {
        self.center_px = center_px;
        self
    }
}

struct GpuChunk {
    generation: u64,
    buffer: wgpu::Buffer,
    used: u64,
}

struct Prepared {
    bind_group: wgpu::BindGroup,
    /// Chunk slot, dynamic uniform offset and instance ranges.
    draws: Vec<(u64, u32, Vec<Range<u32>>)>,
}

/// Draws [`ChunkDraw`]s; see the [crate documentation](crate).
///
/// Uploads happen in [`Self::prepare`]; [`Self::paint`] only records draw calls, so it can
/// run inside a render pass that a UI toolkit owns.
pub struct PlanRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    stride: u64,
    chunks: HashMap<u64, GpuChunk>,
    frame: u64,
    prepared: HashMap<u64, Prepared>,
}

impl PlanRenderer {
    /// Creates the pipeline for color targets of `format`, single-sampled, blending
    /// premultiplied alpha.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cadkit plan shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("plan.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadkit plan chunk"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cadkit plan pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadkit plan"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Uint32x2,
                        3 => Unorm8x4,
                        4 => Float32,
                        5 => Uint32,
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        Self {
            pipeline,
            layout,
            stride: UNIFORM_BYTES.div_ceil(alignment) * alignment,
            chunks: HashMap::new(),
            frame: 0,
            prepared: HashMap::new(),
        }
    }

    /// Releases every chunk buffer and prepared frame; the pipeline stays.
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.prepared.clear();
    }

    /// Number of chunk buffers currently held.
    pub fn chunk_buffers(&self) -> usize {
        self.chunks.len()
    }

    /// Uploads changed chunks and the per-chunk positions of `draws` and stores them under
    /// `id`, which [`Self::paint`] takes. Use a new, increasing `id` per painted canvas and
    /// frame; the last 16 are kept. Chunk buffers not drawn for 600 prepared frames are
    /// released.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        draws: &[ChunkDraw],
        target: PlanTarget,
    ) {
        self.frame += 1;
        let frame = self.frame;
        let stride = usize::try_from(self.stride).unwrap_or(usize::MAX);
        let mut uniforms = vec![0u8; draws.len().max(1).saturating_mul(stride)];
        let mut prepared = Vec::with_capacity(draws.len());
        for (index, draw) in draws.iter().enumerate() {
            let fresh = self
                .chunks
                .get(&draw.slot)
                .is_some_and(|chunk| chunk.generation == draw.generation);
            if !fresh {
                let size = draw.bytes.len() as u64;
                match self.chunks.get_mut(&draw.slot) {
                    Some(chunk) if chunk.buffer.size() >= size => {
                        queue.write_buffer(&chunk.buffer, 0, &draw.bytes);
                        chunk.generation = draw.generation;
                    }
                    _ => {
                        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("cadkit plan chunk instances"),
                            // Headroom so a growing chunk is not reallocated on every edit.
                            size: (size + size / 4).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        queue.write_buffer(&buffer, 0, &draw.bytes);
                        self.chunks.insert(
                            draw.slot,
                            GpuChunk {
                                generation: draw.generation,
                                buffer,
                                used: frame,
                            },
                        );
                    }
                }
            }
            if let Some(chunk) = self.chunks.get_mut(&draw.slot) {
                chunk.used = frame;
            }
            let [left, top, width, height] = target.viewport_px;
            let values = [
                (draw.offset[0] + target.center_px[0]) as f32,
                (draw.offset[1] + target.center_px[1]) as f32,
                draw.scale as f32,
                target.pixels_per_point,
                left,
                top,
                width,
                height,
            ];
            let base = index.saturating_mul(stride);
            if let Some(slot) = uniforms.get_mut(base..base.saturating_add(32)) {
                for (bytes, value) in slot.as_chunks_mut::<4>().0.iter_mut().zip(values) {
                    *bytes = value.to_le_bytes();
                }
            }
            prepared.push((
                draw.slot,
                u32::try_from(index as u64 * self.stride).unwrap_or(u32::MAX),
                draw.ranges.clone(),
            ));
        }
        self.chunks
            .retain(|_, chunk| frame.saturating_sub(chunk.used) < EVICT_FRAMES);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadkit plan chunk positions"),
            size: uniforms.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, &uniforms);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadkit plan chunk positions"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(UNIFORM_BYTES),
                }),
            }],
        });
        self.prepared
            .retain(|other, _| id.saturating_sub(*other) < KEEP_PREPARED);
        self.prepared.insert(
            id,
            Prepared {
                bind_group,
                draws: prepared,
            },
        );
    }

    /// Records the draws prepared under `id`; nothing when there are none.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>, id: u64) {
        let Some(prepared) = self.prepared.get(&id) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        for (slot, offset, ranges) in &prepared.draws {
            let Some(chunk) = self.chunks.get(slot) else {
                continue;
            };
            pass.set_bind_group(0, &prepared.bind_group, &[*offset]);
            pass.set_vertex_buffer(0, chunk.buffer.slice(..));
            for range in ranges {
                pass.draw(0..6, range.clone());
            }
        }
    }
}
