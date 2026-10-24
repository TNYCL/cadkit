//! Render targets for [`MeshRenderer`] output: reusable textures and RGBA readback.

use std::sync::{Arc, Mutex};

use cadkit_scene::MeshScene;

use crate::{GpuError, MeshRenderer, OrbitCamera, PreparedMeshes, check, linear, scopes};

/// Description of the adapter an offscreen image came from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdapterSummary {
    /// Adapter name.
    pub name: String,
    /// Backend, for example `Vulkan`.
    pub backend: String,
    /// Device type, for example `DiscreteGpu` or `Cpu`.
    pub device_type: String,
    /// Driver name and version.
    pub driver: String,
}

impl AdapterSummary {
    /// Summary of `info`.
    pub fn new(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: info.driver.clone(),
        }
    }
}

/// A rendered `Rgba8UnormSrgb` texture and an `Rgba8Unorm` view of it for UI toolkits that
/// sample without conversion.
#[non_exhaustive]
pub struct RenderedTexture {
    /// The texture.
    pub texture: wgpu::Texture,
    /// An `Rgba8Unorm` view of [`Self::texture`].
    pub view: wgpu::TextureView,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

#[derive(Default)]
struct ColorPool {
    size: [u32; 2],
    available: Vec<RenderedTexture>,
}

/// Returns displayed textures to a [`RenderTargetCache`] once the UI no longer shows them.
#[derive(Clone, Default)]
pub struct RenderTextureRecycler {
    pool: Arc<Mutex<ColorPool>>,
}

impl RenderTextureRecycler {
    /// Offers `image` for reuse; kept when it has the current size and fewer than two wait.
    pub fn recycle(&self, image: RenderedTexture) {
        if let Ok(mut pool) = self.pool.lock()
            && pool.size == [image.width, image.height]
            && pool.available.len() < 2
        {
            pool.available.push(image);
        }
    }
}

/// Allocation counters of a [`RenderTargetCache`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderTargetStats {
    /// Color textures created.
    pub color_allocations: u64,
    /// Color textures reused from the recycler.
    pub color_reuses: u64,
    /// Depth textures created.
    pub depth_allocations: u64,
}

/// Keeps the depth texture until the size changes and reuses recycled color textures.
#[derive(Default)]
pub struct RenderTargetCache {
    background: Option<wgpu::Color>,
    recycler: RenderTextureRecycler,
    depth: Option<([u32; 2], wgpu::Texture)>,
    stats: RenderTargetStats,
}

impl RenderTargetCache {
    /// Sets the clear color from sRGB bytes (converted to the target's linear space).
    pub fn set_background_srgb(&mut self, rgb: [u8; 3]) {
        self.background = Some(wgpu::Color {
            r: linear(rgb[0]),
            g: linear(rgb[1]),
            b: linear(rgb[2]),
            a: 1.,
        });
    }

    /// A handle that returns displayed textures to this cache.
    pub fn recycler(&self) -> RenderTextureRecycler {
        self.recycler.clone()
    }

    /// Allocation counters.
    pub fn stats(&self) -> RenderTargetStats {
        self.stats
    }
}

/// Renders `prepared` into a new `width` x `height` texture and waits for the GPU.
pub async fn render_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &MeshRenderer,
    prepared: &PreparedMeshes,
    width: u32,
    height: u32,
) -> Result<RenderedTexture, GpuError> {
    render_texture_cached(
        device,
        queue,
        renderer,
        prepared,
        width,
        height,
        &mut RenderTargetCache::default(),
    )
    .await
}

/// Like [`render_texture`], reusing the depth texture and recycled color textures of
/// `targets`. The renderer must target `Rgba8UnormSrgb`.
pub async fn render_texture_cached(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &MeshRenderer,
    prepared: &PreparedMeshes,
    width: u32,
    height: u32,
    targets: &mut RenderTargetCache,
) -> Result<RenderedTexture, GpuError> {
    let max = device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > max || height > max {
        return Err(GpuError::TextureSize(width, height));
    }
    let (validation, allocation) = scopes(device);
    let texture = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cadkit offscreen attachment"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: if format == wgpu::TextureFormat::Rgba8UnormSrgb {
                &[wgpu::TextureFormat::Rgba8Unorm]
            } else {
                &[]
            },
        })
    };
    let size = [width, height];
    let available = {
        let mut pool = targets
            .recycler
            .pool
            .lock()
            .map_err(|_| GpuError::Device("texture pool lock poisoned".into()))?;
        if pool.size != size {
            pool.size = size;
            pool.available.clear();
        }
        pool.available.pop()
    };
    let output = if let Some(image) = available {
        targets.stats.color_reuses += 1;
        image.texture
    } else {
        targets.stats.color_allocations += 1;
        texture(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        )
    };
    let depth = match &targets.depth {
        Some((old, depth)) if *old == size => depth,
        _ => {
            targets.stats.depth_allocations += 1;
            let depth = texture(
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            &targets.depth.insert((size, depth)).1
        }
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("cadkit mesh commands"),
    });
    {
        let color = output.create_view(&Default::default());
        let depth = depth.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cadkit mesh pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(targets.background.unwrap_or(wgpu::Color {
                        r: 0.025,
                        g: 0.035,
                        b: 0.05,
                        a: 1.,
                    })),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        renderer.paint(&mut pass, prepared);
    }
    let submission = queue.submit([encoder.finish()]);
    // Bound the GPU work before the caller accepts another frame.
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(15)),
        })
        .map_err(|error| GpuError::Poll(error.to_string()))?;
    check(validation, allocation).await?;
    let view = output.create_view(&wgpu::TextureViewDescriptor {
        format: Some(wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    });
    Ok(RenderedTexture {
        texture: output,
        view,
        width,
        height,
    })
}

/// RGBA pixels read back from the GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OffscreenImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major sRGB RGBA, 4 bytes per pixel, no padding.
    pub rgba: Vec<u8>,
    /// The adapter that drew the image.
    pub adapter: AdapterSummary,
}

/// Opens the default high-performance adapter, renders `scene` into a 1..=4096 pixel image
/// and reads it back. For export and device checks; interactive views keep textures on the
/// GPU with [`render_texture_cached`].
pub async fn render_offscreen(
    scene: &MeshScene,
    camera: OrbitCamera,
    width: u32,
    height: u32,
) -> Result<OffscreenImage, GpuError> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(GpuError::TextureSize(width, height));
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        })
        .await
        .map_err(|error| GpuError::AdapterUnavailable(error.to_string()))?;
    let summary = AdapterSummary::new(&adapter.get_info());
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("cadkit offscreen"),
            ..Default::default()
        })
        .await
        .map_err(|error| GpuError::DeviceUnavailable(error.to_string()))?;
    let errors = Arc::new(Mutex::new(Vec::<GpuError>::new()));
    let sink = Arc::clone(&errors);
    device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
        if let Ok(mut errors) = sink.lock() {
            errors.push(GpuError::Device(error.to_string()));
        }
    }));
    let sink = Arc::clone(&errors);
    device.set_device_lost_callback(move |reason, message| {
        if let Ok(mut errors) = sink.lock() {
            errors.push(GpuError::Device(format!("{reason:?}: {message}")));
        }
    });
    let rgba = render_pixels(&device, &queue, scene, camera, width, height).await?;
    if let Some(error) = errors
        .lock()
        .map_err(|_| GpuError::Device("error channel poisoned".into()))?
        .pop()
    {
        return Err(error);
    }
    Ok(OffscreenImage {
        width,
        height,
        rgba,
        adapter: summary,
    })
}

/// Renders `scene` on an existing device and reads back RGBA; opens no adapter. The size is
/// limited by the device's texture limit.
pub async fn render_offscreen_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &MeshScene,
    camera: OrbitCamera,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, GpuError> {
    render_pixels(device, queue, scene, camera, width, height).await
}

async fn render_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &MeshScene,
    camera: OrbitCamera,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, GpuError> {
    let (validation, allocation) = scopes(device);
    let renderer = MeshRenderer::new(device, wgpu::TextureFormat::Rgba8UnormSrgb).await?;
    let prepared = renderer
        .prepare(device, scene, camera, width as f32 / height as f32, 1e-4)
        .await?;
    let rendered = render_texture(device, queue, &renderer, &prepared, width, height).await?;
    let rgba = read_texture(device, queue, &rendered.texture, width, height).await;
    check(validation, allocation).await?;
    rgba
}

/// Copies a 4-byte-per-pixel texture into RGBA rows without padding.
pub(crate) async fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, GpuError> {
    let row = width
        .checked_mul(4)
        .ok_or(GpuError::TextureSize(width, height))?;
    let stride = row.div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cadkit readback"),
        size: u64::from(stride) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("cadkit readback commands"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(15)),
        })
        .map_err(|error| GpuError::Poll(error.to_string()))?;
    receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|error| GpuError::Readback(error.to_string()))?
        .map_err(|error| GpuError::Readback(error.to_string()))?;
    let mapped = readback
        .slice(..)
        .get_mapped_range()
        .map_err(|error| GpuError::Readback(error.to_string()))?;
    let row = row as usize;
    let mut rgba = Vec::with_capacity(row * height as usize);
    for line in mapped.chunks_exact(stride as usize) {
        rgba.extend_from_slice(line.get(..row).unwrap_or_default());
    }
    drop(mapped);
    readback.unmap();
    Ok(rgba)
}
