// Sky shader: one full-screen triangle drawn behind everything.
//
// For each pixel we rebuild the view direction from the inverse camera
// matrix, then paint: a vertical gradient (horizon → zenith), a warm glow
// around the sun at sunrise/sunset, stars at night, a square sun and a
// square moon with phases. Colours come from ld_game::time (via the
// renderer); the shapes are Loredrop's own.

struct Camera {
    view_proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    fog: vec4<f32>,
    tints: array<vec4<f32>, 2>,
    inv_view_proj: mat4x4<f32>,
    // Linear colours. glow.w = glow strength.
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    glow: vec4<f32>,
    // xyz = direction toward the sun, w = star visibility.
    sun: vec4<f32>,
    // x = moon phase (0..7), y = daylight, z = 1 if the sky is drawn.
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> cam: Camera;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VertexOut {
    // A triangle covering the whole screen: (-1,-1), (3,-1), (-1,3).
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var out: VertexOut;
    // Depth 0 = infinitely far with reverse-Z, so terrain always covers it.
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.ndc = p;
    return out;
}

// Cheap hash of a 3D integer cell → 0..1.
fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.x + q.y) * q.z);
}

// Coordinates of `dir` on a square billboard facing `center` (unit vectors).
// Returns (u, v, facing) — u/v in units of the view cone, facing > 0 in front.
fn billboard(dir: vec3<f32>, center: vec3<f32>) -> vec3<f32> {
    // The sun travels around the Z axis, so Z is always "sideways" on it.
    let right = vec3<f32>(0.0, 0.0, 1.0);
    let up = normalize(cross(right, center));
    let f = dot(dir, center);
    return vec3<f32>(dot(dir, right) / f, dot(dir, up) / f, f);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Point on the near plane → direction from the camera (at the origin).
    let near = cam.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(near.xyz / near.w);
    let sun_dir = cam.sun.xyz;

    // Gradient: horizon colour near and below the horizon, zenith colour above.
    let up = dir.y;
    var c = mix(cam.sky_horizon.rgb, cam.sky_zenith.rgb, smoothstep(0.0, 0.55, up));
    // Below the horizon the "void" is darker, like Minecraft's lower sky.
    c = mix(c, cam.sky_horizon.rgb * 0.45, smoothstep(0.0, -0.35, up));

    // Sunrise/sunset glow: strongest toward the sun, along the horizon.
    let toward_sun = max(dot(normalize(vec3<f32>(dir.x, 0.0, dir.z) + vec3<f32>(0.0001, 0.0, 0.0)),
                             normalize(vec3<f32>(sun_dir.x, 0.0, sun_dir.z) + vec3<f32>(0.0001, 0.0, 0.0))), 0.0);
    let band = 1.0 - smoothstep(0.0, 0.45, abs(up - 0.05));
    let glow = cam.glow.w * pow(toward_sun, 3.0) * band;
    c = mix(c, cam.glow.rgb, clamp(glow, 0.0, 1.0) * 0.85);

    // Stars: a sparse grid of bright points on a cube around the camera.
    let stars = cam.sun.w;
    if (stars > 0.0 && up > -0.1) {
        // Project onto a cube, split into cells; a few cells hold one tiny
        // star at a random spot inside the cell.
        let m = max(abs(dir.x), max(abs(dir.y), abs(dir.z)));
        let q = dir / m * 160.0;
        let cell = floor(q);
        if (hash3(cell) > 0.993) {
            let center = vec3<f32>(hash3(cell + 1.0), hash3(cell + 2.0), hash3(cell + 3.0)) * 0.6 + 0.2;
            let d = length(q - cell - center);
            let twinkle = 0.5 + 0.5 * hash3(cell + 7.0);
            let point = 1.0 - smoothstep(0.18, 0.32, d);
            c += vec3<f32>(stars * twinkle * point * smoothstep(-0.1, 0.1, up));
        }
    }

    // The sun: a bright square with a soft halo.
    let s = billboard(dir, sun_dir);
    if (s.z > 0.0) {
        let d = max(abs(s.x), abs(s.y));
        let core = 1.0 - step(0.075, d);
        let halo = (1.0 - smoothstep(0.075, 0.35, d)) * 0.35;
        c = c + vec3<f32>(1.0, 0.92, 0.7) * halo + vec3<f32>(1.0, 0.97, 0.85) * core * 3.0;
    }

    // The moon: a pale square opposite the sun, lit according to its phase.
    let mdir = -sun_dir;
    let mb = billboard(dir, mdir);
    if (mb.z > 0.0) {
        let half_size = 0.055;
        if (max(abs(mb.x), abs(mb.y)) < half_size) {
            let x = mb.x / half_size; // −1..1 across the face
            let a = cam.misc.x / 8.0 * 6.2831853;
            // 0 = full; the terminator sweeps across through the month.
            var lit = true;
            if (a <= 3.1415927) {
                lit = x > -cos(a);
            } else {
                lit = x < cos(a);
            }
            // Faint grey craters (fixed pattern) on the lit part.
            let crater = hash3(floor(vec3<f32>(mb.xy / half_size * 4.0, 3.0))) > 0.7;
            var moon = vec3<f32>(0.85, 0.87, 0.95);
            if (crater) {
                moon *= 0.8;
            }
            if (lit) {
                c = moon;
            } else {
                c = c * 0.6; // the dark part hides stars
            }
        }
    }

    if (cam.fog.w > 0.5) {
        c = pow(c, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(c, 1.0);
}
