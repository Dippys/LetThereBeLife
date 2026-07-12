struct Camera {
    center: vec2<f32>,
    viewport: vec2<f32>,
    scale: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) position: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: u32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let world = input.position + corners[input.vertex_index] * input.size;
    let pixel = (world - camera.center) * camera.scale + camera.viewport * 0.5;
    let clip = vec2(pixel.x / camera.viewport.x * 2.0 - 1.0, 1.0 - pixel.y / camera.viewport.y * 2.0);
    let color = vec4<f32>(
        f32(input.color & 255u),
        f32((input.color >> 8u) & 255u),
        f32((input.color >> 16u) & 255u),
        f32((input.color >> 24u) & 255u),
    ) / 255.0;
    return VertexOutput(vec4(clip, 0.0, 1.0), color);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
