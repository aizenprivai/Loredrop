// Chunk shader: draws packed section meshes (see ld_mesh::PackedVertex).

struct Camera {
    view_proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    // x = fog start, y = fog end, z = 1 if fog on, w = 1 if the shader must gamma-encode.
    fog: vec4<f32>,
    // Linear-space tint colours: [0] grass, [1] foliage. (Biome colours in Phase 4.)
    tints: array<vec4<f32>, 2>,
};
@group(0) @binding(0) var<uniform> cam: Camera;

@group(1) @binding(0) var block_textures: texture_2d_array<f32>;
@group(1) @binding(1) var block_sampler: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) rel_pos: vec3<f32>,
    @location(2) light: f32,
    @location(3) @interpolate(flat) layer: u32,
    @location(4) @interpolate(flat) tint: u32,
};

// Minecraft-style fixed face shading: top brightest, bottom darkest.
fn face_shade(face: u32) -> f32 {
    switch face {
        case 0u, 1u: { return 0.6; }   // east / west
        case 2u: { return 1.0; }       // up
        case 3u: { return 0.5; }       // down
        default: { return 0.8; }       // south / north
    }
}

// Texture coordinates derived from position, oriented so textures are upright
// when viewed from outside each face. Integer positions + repeat sampling =
// textures tile across merged (greedy) quads.
fn face_uv(face: u32, p: vec3<f32>) -> vec2<f32> {
    switch face {
        case 0u: { return vec2<f32>(-p.z, -p.y); } // east  (+X)
        case 1u: { return vec2<f32>(p.z, -p.y); }  // west  (−X)
        case 2u: { return vec2<f32>(p.x, p.z); }   // up
        case 3u: { return vec2<f32>(p.x, -p.z); }  // down
        case 4u: { return vec2<f32>(p.x, -p.y); }  // south (+Z)
        default: { return vec2<f32>(-p.x, -p.y); } // north (−Z)
    }
}

@vertex
fn vs_main(@location(0) packed: vec2<u32>, @location(1) origin: vec4<f32>) -> VertexOut {
    let a = packed.x;
    let b = packed.y;
    let local = vec3<f32>(f32(a & 31u), f32((a >> 5u) & 31u), f32((a >> 10u) & 31u));
    let face = (a >> 15u) & 7u;
    let ao = (a >> 18u) & 3u;

    var out: VertexOut;
    // `origin` = section origin − camera position (computed in f64 on the CPU).
    let rel = local + origin.xyz;
    out.clip = cam.view_proj * vec4<f32>(rel, 1.0);
    out.rel_pos = rel;
    out.uv = face_uv(face, local);
    let ao_levels = array<f32, 4>(0.4, 0.6, 0.8, 1.0);
    out.light = face_shade(face) * ao_levels[ao];
    out.layer = b & 0xFFFFu;
    out.tint = (b >> 16u) & 3u;
    return out;
}

fn shade(in: VertexOut, texel: vec4<f32>) -> vec4<f32> {
    var c = texel.rgb;
    if (in.tint > 0u) {
        c = c * cam.tints[in.tint - 1u].rgb;
    }
    c = c * in.light;
    if (cam.fog.z > 0.5) {
        let d = length(in.rel_pos.xz);
        c = mix(c, cam.fog_color.rgb, smoothstep(cam.fog.x, cam.fog.y, d));
    }
    if (cam.fog.w > 0.5) {
        c = pow(c, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_opaque(in: VertexOut) -> @location(0) vec4<f32> {
    return shade(in, textureSample(block_textures, block_sampler, in.uv, in.layer));
}

@fragment
fn fs_cutout(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(block_textures, block_sampler, in.uv, in.layer);
    if (texel.a < 0.5) {
        discard; // see-through pixel (holes in leaves, clear glass)
    }
    return shade(in, texel);
}
