// Chunk shader: draws packed section meshes (see ld_mesh::PackedVertex).

struct Camera {
    view_proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    // x = fog start, y = fog end, z = 1 if fog on, w = 1 if the shader must gamma-encode.
    fog: vec4<f32>,
    // Unused since Phase 4 (biome colours are per vertex); kept for layout stability.
    tints: array<vec4<f32>, 2>,
    // Phase 8 sky data (see sky.wgsl). Used here: misc.y = daylight, misc.w = brightness setting.
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
    @location(2) light: f32,
    @location(3) @interpolate(flat) layer: u32,
    @location(4) color: vec3<f32>,
    // Smooth-lit colour of the light hitting this vertex (sky and block light).
    @location(5) lit: vec3<f32>,
};

// Light level 0..15 → brightness 0..1. Minecraft-like curve (dim levels
// fall off quickly), lifted toward a straight line by the brightness
// setting (misc.w: 0 = moody, 1 = bright), with a small floor so the
// darkest caves aren't pure black.
fn light_curve(level: f32) -> f32 {
    let f = level / 15.0;
    let moody = f / (4.0 - 3.0 * f);
    let bright = 1.0 - (1.0 - f) * (1.0 - f);
    return mix(moody, bright, cam.misc.w * 0.6) * 0.96 + 0.04;
}

// Colour of light reaching a surface: sunlight (dimmed at night by
// daylight = misc.y) or warm block light (torches, lava), whichever is brighter.
fn light_color(sky_level: f32, block_level: f32) -> vec3<f32> {
    let sky = light_curve(sky_level) * cam.misc.y;
    let blk = light_curve(block_level) * select(0.0, 1.0, block_level > 0.0);
    return max(vec3<f32>(sky), blk * vec3<f32>(1.0, 0.88, 0.72));
}

// Minecraft-style fixed face shading: top brightest, bottom darkest.
fn face_shade(face: u32) -> f32 {
    switch face {
        case 0u, 1u: { return 0.6; }   // east / west
        case 2u: { return 1.0; }       // up
        case 3u: { return 0.5; }       // down
        case 6u, 7u: { return 0.9; }   // plant panes
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
        case 5u: { return vec2<f32>(-p.x, -p.y); } // north (−Z)
        default: { return vec2<f32>(p.x, -p.y); }  // plant panes: across the diagonal
    }
}

// sRGB 0..1 → linear (approximate gamma 2.2, matching the rest of the pipeline).
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

@vertex
fn vs_main(@location(0) packed: vec3<u32>, @location(1) origin: vec4<f32>) -> VertexOut {
    let a = packed.x;
    let b = packed.y;
    let c = packed.z;
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
    out.lit = light_color(f32((b >> 22u) & 15u), f32((b >> 18u) & 15u));
    // Biome colour (white for untinted faces), blended per column on the CPU.
    let rgb = vec3<f32>(f32((c >> 16u) & 255u), f32((c >> 8u) & 255u), f32(c & 255u)) / 255.0;
    out.color = to_linear(rgb);
    return out;
}

fn shade(in: VertexOut, texel: vec4<f32>) -> vec4<f32> {
    var c = texel.rgb * in.color * in.light * in.lit;
    if (cam.fog.z > 0.5) {
        let d = length(in.rel_pos.xz);
        c = mix(c, cam.fog_color.rgb, smoothstep(cam.fog.x, cam.fog.y, d));
    }
    if (cam.fog.w > 0.5) {
        c = pow(c, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(c, texel.a);
}

@fragment
fn fs_opaque(in: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(in, textureSample(block_textures, block_sampler, in.uv, in.layer));
    return vec4<f32>(c.rgb, 1.0);
}

@fragment
fn fs_cutout(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(block_textures, block_sampler, in.uv, in.layer);
    if (texel.a < 0.5) {
        discard; // see-through pixel (holes in leaves, clear glass)
    }
    let c = shade(in, texel);
    return vec4<f32>(c.rgb, 1.0);
}

@fragment
fn fs_translucent(in: VertexOut) -> @location(0) vec4<f32> {
    // Alpha comes from the texture (water ≈ 70 % opaque) and is blended.
    return shade(in, textureSample(block_textures, block_sampler, in.uv, in.layer));
}
