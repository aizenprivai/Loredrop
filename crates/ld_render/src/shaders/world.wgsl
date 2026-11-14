// World shader (Phase 2 test version).
//
// Vertices are stored relative to their mesh's origin; `mesh.offset` is that
// origin *minus the camera position*, computed on the CPU in f64. Doing the
// subtraction in double precision before converting to f32 is what keeps
// geometry rock-steady even millions of blocks from spawn
// ("camera-relative rendering").

struct Camera {
    view_proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    // x = fog start distance, y = fog end distance, z = 1.0 if fog enabled,
    // w = 1.0 if the frame isn't sRGB and we must gamma-encode ourselves.
    fog: vec4<f32>,
};
@group(0) @binding(0) var<uniform> cam: Camera;

struct Mesh {
    offset: vec4<f32>,
};
@group(1) @binding(0) var<uniform> mesh: Mesh;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) rel_pos: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> VertexOut {
    var out: VertexOut;
    let rel = pos + mesh.offset.xyz;
    out.clip = cam.view_proj * vec4<f32>(rel, 1.0);
    // Vertex colours are authored in sRGB; lighting/fog maths happens in linear.
    out.color = pow(color.rgb, vec3<f32>(2.2)) * color.a; // alpha channel = face shade
    out.rel_pos = rel;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var c = in.color;
    if (cam.fog.z > 0.5) {
        // Cylindrical fog (horizontal distance only), like Minecraft: looking
        // straight up or down doesn't fog nearby blocks.
        let d = length(in.rel_pos.xz);
        let t = smoothstep(cam.fog.x, cam.fog.y, d);
        c = mix(c, cam.fog_color.rgb, t);
    }
    if (cam.fog.w > 0.5) {
        c = pow(c, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(c, 1.0);
}
