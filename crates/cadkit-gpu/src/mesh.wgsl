// Lit, depth-tested triangle meshes; flat normals from screen-space derivatives.
struct Camera { view_projection: mat4x4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}
@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> VertexOutput {
    var result: VertexOutput;
    result.clip = camera.view_projection * vec4(position, 1.0);
    result.position = position;
    result.color = color;
    return result;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(cross(dpdx(input.position), dpdy(input.position)));
    let light = normalize(vec3(0.4, -0.6, 0.8));
    let shade = 0.32 + 0.68 * abs(dot(normal, light));
    return vec4(input.color.rgb * shade, 1.0);
}
