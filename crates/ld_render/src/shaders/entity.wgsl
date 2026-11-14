// Entity shader: free-form textured quads (dropped items, block-breaking
// cracks; creatures from Phase 7). Positions are already camera-relative,
// computed in f64 on the CPU. Textures come from the block texture array.

struct Camera {
    view_proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    // x = fog start, y = fog end, z = 1 if fog on, w = 1 if the shader must gamma-encode.
    fog: vec4<f32>,
    // Unused since Phase 4 (biome colours are per vertex); kept for layout stability.
    tints: array<vec4<f32>, 2>,
    // Phase 8 sky data (see sky.wgsl); only misc.y (daylight) is used here.
    inv_view_proj: mat4x4<f32>,
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    glow: vec4<f32>,
    sun: vec4<f32>,
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var block_textures: texture_2d_array<f32>;
@group(1) @binding(1) var block_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) rel_pos: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) @interpolate(flat) layer: u32,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32, @location(3) color: u32) -> VertexOut {
    var out: VertexOut;
    out.clip = cam.view_proj * vec4<f32>(pos, 1.0);
    out.uv = uv;
    out.rel_pos = pos;
    // Packed sRGB colour multiplier (tint × shading) → linear.
    let rgb = vec3<f32>(f32((color >> 16u) & 255u), f32((color >> 8u) & 255u), f32(color & 255u)) / 255.0;
    out.color = pow(rgb, vec3<f32>(2.2));
    out.layer = layer;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(block_textures, block_sampler, in.uv, in.layer);
    if (texel.a < 0.5) {
        discard;
    }
    var c = texel.rgb * in.color * cam.misc.y;
    if (cam.fog.z > 0.5) {
        c = mix(c, cam.fog_color.rgb, smoothstep(cam.fog.x, cam.fog.y, length(in.rel_pos.xz)));
    }
    if (cam.fog.w > 0.5) {
        c = pow(c, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(c, 1.0);
}
