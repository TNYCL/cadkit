//! Renders the first model of a DWG, DXF or DGN file to a PNG with the GPU plan renderer.
//!
//! `cargo run -p cadkit-gpu --example render_png -- plan.dxf plan.png [width] [height]`
//!
//! Lines, arcs, circles and flattened curves are drawn; the bridge report lists what was
//! not (text, hatches, dashes drawn solid...). Chunks that exceed the f32 error budget at
//! this zoom would go to a CPU path; this example has none and reports them instead.
#![allow(clippy::print_stdout)]

use std::collections::BTreeSet;
use std::io::Write;

use cadkit_geom::Point2;
use cadkit_gpu::{PlanRenderer, PlanTarget, block_on, wgpu};
use cadkit_scene::document::{DocumentPlanOptions, plan_document};
use cadkit_scene::{Plan2d, Plan2dOptions, PlanView};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(input), Some(output)) = (args.get(1), args.get(2)) else {
        return Err("usage: render_png <drawing> <output.png> [width] [height]".into());
    };
    let width: u32 = args.get(3).map_or(Ok(1600), |w| w.parse())?;
    let height: u32 = args.get(4).map_or(Ok(1000), |h| h.parse())?;

    let doc = cadkit::open(input)?;
    let options = DocumentPlanOptions::new();
    let planned = plan_document(&doc, &options);
    println!("{:?}", planned.report);
    let bbox = doc.bbox(options.model).ok_or("the model has no extent")?;
    let mut plan = Plan2d::new(Plan2dOptions::new());
    for (id, item) in planned.items {
        plan.upsert(id, item);
    }
    let (w, h) = (bbox.max.x - bbox.min.x, bbox.max.y - bbox.min.y);
    let zoom = 0.95 * (f64::from(width) / w.max(1e-9)).min(f64::from(height) / h.max(1e-9));
    let center = Point2::new(
        (bbox.min.x + bbox.max.x) / 2.,
        (bbox.min.y + bbox.max.y) / 2.,
    );
    let view = PlanView::new(
        center,
        zoom,
        [f64::from(width), f64::from(height)],
        f64::from(width.max(height)),
    );
    plan.prepare(&view, |_| true);
    let frame = plan.frame(&view, |_| true, &BTreeSet::new(), true);
    if !frame.cpu.is_empty() {
        println!(
            "{} objects need a CPU path and are not drawn",
            frame.cpu.len()
        );
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    println!("adapter: {}", adapter.get_info().name);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("render_png"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = PlanRenderer::new(&device, format);
    renderer.prepare(
        &device,
        &queue,
        1,
        &frame.draws,
        PlanTarget::new([0., 0., width as f32, height as f32], 1.),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let target = texture.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render_png"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        renderer.paint(&mut pass, 1);
    }
    queue.submit([encoder.finish()]);
    let rgba = read(&device, &queue, &texture, width, height)?;
    std::fs::write(output, png(width, height, &rgba)?)?;
    println!("wrote {output}");
    Ok(())
}

fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let row = width as usize * 4;
    let stride = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    })?;
    receiver.recv()??;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let mut rgba = Vec::with_capacity(row * height as usize);
    for line in mapped.chunks_exact(stride as usize) {
        rgba.extend_from_slice(line.get(..row).ok_or("short row")?);
    }
    Ok(rgba)
}

/// A minimal RGBA8 PNG: signature, IHDR, one zlib IDAT and IEND.
fn png(width: u32, height: u32, rgba: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks_exact(width as usize * 4) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zlib.write_all(&raw)?;
    let data = zlib.finish()?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    for (kind, body) in [(*b"IHDR", header), (*b"IDAT", data), (*b"IEND", Vec::new())] {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(&kind);
        out.extend_from_slice(&body);
        let crc = crc32(out.get(start..).unwrap_or_default());
        out.extend_from_slice(&crc.to_be_bytes());
    }
    Ok(out)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
