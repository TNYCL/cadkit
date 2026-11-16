use cadkit_gpu::{PlanRenderer, PlanTarget, wgpu};
use cadkit_scene::PlanFrame;

/// Draws the GPU part of a plan frame into a render pass the host application owns.
/// `frame_id` must increase for every painted canvas and frame.
pub fn paint_plan(
    renderer: &mut PlanRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &mut wgpu::RenderPass<'_>,
    frame: &PlanFrame,
    frame_id: u64,
) {
    // Viewport x, y, width, height in physical pixels, and physical pixels per logical pixel.
    let target = PlanTarget::new([0.0, 0.0, 1280.0, 800.0], 1.0);
    renderer.prepare(device, queue, frame_id, &frame.draws, target);
    renderer.paint(pass, frame_id);
    // frame.cpu and frame.labels are left to the host's CPU drawing path.
}
