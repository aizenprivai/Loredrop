//! Small maths helpers. `no_std` has no `f32::sin` etc., so these forward to
//! the `libm` crate (pure Rust, identical results on every platform).

/// Sine of an angle in radians.
#[inline]
pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

/// Cosine of an angle in radians.
#[inline]
pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

/// Tangent of an angle in radians.
#[inline]
pub fn tan(x: f32) -> f32 {
    libm::tanf(x)
}

/// Square root. On the calculator this is the FPU's own `vsqrt` instruction
/// (about 14 cycles); `libm::sqrtf` there is a slow software routine, and
/// the renderer takes one square root per pixel.
#[inline]
pub fn sqrt(x: f32) -> f32 {
    #[cfg(target_arch = "arm")]
    {
        let r: f32;
        // SAFETY: a single FPU instruction on registers, no memory access.
        unsafe {
            core::arch::asm!("vsqrt.f32 {0}, {1}", out(sreg) r, in(sreg) x, options(pure, nomem, nostack));
        }
        r
    }
    #[cfg(not(target_arch = "arm"))]
    {
        libm::sqrtf(x)
    }
}

/// Largest integer value not greater than `x`.
#[inline]
pub fn floor(x: f32) -> f32 {
    libm::floorf(x)
}

/// `floor(x)` as an integer (fast path that avoids a libm call).
#[inline]
pub fn ifloor(x: f32) -> i32 {
    let i = x as i32;
    if (i as f32) > x { i - 1 } else { i }
}

/// Clamps `x` to `lo..=hi`.
#[inline]
pub fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Hermite smoothstep: 0 below `e0`, 1 above `e1`, smooth in between.
#[inline]
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Absolute value.
#[inline]
pub fn abs(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

/// π.
pub const PI: f32 = core::f32::consts::PI;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ifloor_matches_floor() {
        for i in -400..400 {
            let x = i as f32 * 0.137;
            assert_eq!(ifloor(x), floor(x) as i32, "x = {x}");
        }
    }
}
