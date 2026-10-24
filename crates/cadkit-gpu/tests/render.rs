//! Rendering checks on a real adapter. They run only with `CADKIT_GPU_TESTS=1` (CI uses
//! Mesa lavapipe); otherwise they return immediately. Assertions are structural (pixel
//! colors at known positions) so they hold across conformant drivers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use cadkit_geom::{Arc, Circle, Curve, LineSegment, Point2};
use cadkit_gpu::{
    GpuError, MeshRenderer, OrbitCamera, PlanRenderer, PlanTarget, block_on, render_offscreen_with,
    wgpu,
};
use cadkit_scene::{
    GroupId, Mesh3d, MeshScene, ObjectId, Plan2d, Plan2dOptions, PlanItem, PlanView,
};

const W: u32 = 200;
const H: u32 = 100;

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    std::env::var_os("CADKIT_GPU_TESTS")?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("CADKIT_GPU_TESTS is set but no adapter is available");
    let info = adapter.get_info();
    eprintln!(
        "adapter: {} ({:?}, {})",
        info.name, info.backend, info.driver
    );
    Some(block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap())
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Curve {
    Curve::Line(LineSegment {
        start: Point2::new(x0, y0),
        end: Point2::new(x1, y1),
    })
}

/// Draws `plan` into a white `Rgba8Unorm` target (premultiplied blending, as UI toolkits do).
fn draw_plan(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    plan: &mut Plan2d,
    view: &PlanView,
    overlay: &BTreeSet<ObjectId>,
) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = PlanRenderer::new(device, format);
    plan.prepare(view, |_| true);
    let frame = plan.frame(view, |_| true, overlay, true);
    assert!(
        frame.cpu.is_subset(overlay),
        "unexpected CPU objects {:?}",
        frame.cpu
    );
    renderer.prepare(
        device,
        queue,
        1,
        &frame.draws,
        PlanTarget::new([0., 0., W as f32, H as f32], 1.),
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let target = texture.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
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
    read(device, queue, &texture)
}

fn read(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let stride = (W * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride * H),
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
                rows_per_image: Some(H),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    mapped
        .chunks(stride as usize)
        .flat_map(|row| row[..(W * 4) as usize].to_vec())
        .collect()
}

fn pixel(image: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    image[i..i + 4].try_into().unwrap()
}

#[test]
fn plan_strokes_land_on_their_pixels() {
    let Some((device, queue)) = gpu() else { return };
    let mut plan = Plan2d::new(Plan2dOptions::new());
    let black = [0, 0, 0, 255];
    // Canvas: 200 x 100 px, 10 px per unit, model (0, 0) at the canvas center (100, 50).
    plan.upsert(
        ObjectId(1),
        PlanItem::new(GroupId(0), vec![line(-8., 2., 8., 2.)], black, 3.),
    );
    plan.upsert(
        ObjectId(2),
        PlanItem::new(
            GroupId(0),
            vec![Curve::Circle(Circle {
                center: Point2::new(5., -2.),
                radius: 1.5,
            })],
            [255, 0, 0, 255],
            2.,
        ),
    );
    plan.upsert(
        ObjectId(3),
        PlanItem::new(
            GroupId(0),
            vec![Curve::Arc(Arc {
                center: Point2::new(-5., -2.),
                radius: 2.,
                start_angle: 0.,
                sweep_angle: std::f64::consts::PI,
            })],
            [0, 0, 255, 128],
            2.,
        ),
    );
    let view = PlanView::new(Point2::new(0., 0.), 10., [W as f64, H as f64], W as f64);
    let image = draw_plan(&device, &queue, &mut plan, &view, &BTreeSet::new());
    // The line at model y = 2 is pixel row 30 (y down), from x = 20 to 180.
    assert_eq!(pixel(&image, 100, 30), [0, 0, 0, 255]);
    assert_eq!(pixel(&image, 100, 36), [255, 255, 255, 255]);
    assert_eq!(pixel(&image, 10, 30), [255, 255, 255, 255]);
    // Circle ring at (150, 70) with radius 15 px: on the ring red, at the center background.
    assert_eq!(pixel(&image, 164, 70), [255, 0, 0, 255]);
    assert_eq!(pixel(&image, 150, 70), [255, 255, 255, 255]);
    // Upper half arc around (50, 70), radius 20 px, half-transparent blue over white.
    let top = pixel(&image, 50, 50);
    assert!(top[2] == 255 && (120..=135).contains(&top[0]), "{top:?}");
    assert_eq!(pixel(&image, 50, 90), [255, 255, 255, 255]);
    // The overlay cuts the line out of the GPU draw; the caller draws it instead.
    let image = draw_plan(
        &device,
        &queue,
        &mut plan,
        &view,
        &BTreeSet::from([ObjectId(1)]),
    );
    assert_eq!(pixel(&image, 100, 30), [255, 255, 255, 255]);
    assert_eq!(pixel(&image, 164, 70), [255, 0, 0, 255]);
}

#[test]
fn hairlines_fade_instead_of_thinning() {
    let Some((device, queue)) = gpu() else { return };
    let mut plan = Plan2d::new(Plan2dOptions::new());
    plan.upsert(
        ObjectId(1),
        PlanItem::new(
            GroupId(0),
            vec![line(-8., 0.05, 8., 0.05)],
            [0, 0, 0, 255],
            0.5,
        ),
    );
    let view = PlanView::new(Point2::new(0., 0.), 10., [W as f64, H as f64], W as f64);
    let image = draw_plan(&device, &queue, &mut plan, &view, &BTreeSet::new());
    // A 0.5 px line through the middle of pixel row 49 covers it at about half opacity.
    let value = pixel(&image, 100, 49)[0];
    assert!((100..=155).contains(&value), "{value}");
}

fn quad(id: u64, z: f64, color: [u8; 4]) -> Mesh3d {
    Mesh3d::new(
        ObjectId(id),
        GroupId(0),
        vec![[-1., -1., z], [1., -1., z], [1., 1., z], [-1., 1., z]],
        vec![[0, 1, 2], [0, 2, 3]],
        color,
    )
}

#[test]
fn meshes_render_lit_and_depth_tested() {
    let Some((device, queue)) = gpu() else { return };
    // Looking down from above at a 2 x 2 square on z = 0 and a red one at z = 0.5 on top.
    let camera = OrbitCamera::new([0., 0., 0.], 0., 1.5, 10., 0.6);
    let scene = MeshScene::new(
        1,
        vec![
            quad(1, 0., [0, 200, 0, 255]),
            quad(2, 0.5, [200, 0, 0, 255]),
        ],
    );
    let image = block_on(render_offscreen_with(&device, &queue, &scene, camera, W, H)).unwrap();
    let center = pixel(&image, W / 2, H / 2);
    assert!(center[0] > 100 && center[1] < 20, "{center:?}");
    let corner = pixel(&image, 2, 2);
    assert!(
        corner[0] < 80 && corner[1] < 80 && corner[2] < 80,
        "{corner:?}"
    );
}

#[test]
fn mesh_errors_are_typed() {
    let Some((device, queue)) = gpu() else { return };
    let camera = OrbitCamera::new([0., 0., 0.], 0., 1.0, 10., 0.6);
    let render = |scene: &MeshScene, camera| {
        block_on(render_offscreen_with(&device, &queue, scene, camera, W, H))
    };
    assert_eq!(
        render(
            &MeshScene::new(1, vec![quad(7, 0., [9, 9, 9, 128])]),
            camera
        ),
        Err(GpuError::Translucent(ObjectId(7)))
    );
    let mut broken = quad(8, 0., [9, 9, 9, 255]);
    broken.triangles.push([0, 1, 9]);
    assert_eq!(
        render(&MeshScene::new(1, vec![broken]), camera),
        Err(GpuError::InvalidTriangle(ObjectId(8)))
    );
    assert_eq!(
        render(&MeshScene::new(1, vec![]), camera),
        Err(GpuError::EmptyScene)
    );
    assert_eq!(
        render(
            &MeshScene::new(1, vec![quad(1, 0., [9, 9, 9, 255])]).with_complete(false),
            camera
        ),
        Err(GpuError::IncompleteScene)
    );
    let bad = OrbitCamera::new([0., 0., 0.], 0., 2.0, 10., 0.6);
    assert_eq!(
        render(&MeshScene::new(1, vec![quad(1, 0., [9, 9, 9, 255])]), bad),
        Err(GpuError::InvalidCamera)
    );
    let renderer = block_on(MeshRenderer::new(
        &device,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ))
    .unwrap();
    let far = MeshScene::new(1, vec![quad(3, 1e9 + 0.3, [9, 9, 9, 255])]);
    assert!(matches!(
        block_on(renderer.prepare(&device, &far, camera, 2., 1e-4)),
        Err(GpuError::Precision {
            mesh: ObjectId(3),
            ..
        })
    ));
}
