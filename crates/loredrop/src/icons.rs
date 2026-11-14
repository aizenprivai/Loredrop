//! Item icons for the user interface.
//!
//! * Cube blocks are drawn as small 3D cubes in isometric view (top, left and
//!   right faces with Minecraft-style shading), like inventory icons in Minecraft.
//! * Plants, tools and materials use their flat 16×16 sprite.
//!
//! Icons are rendered once on the CPU at startup (32×32 pixels each) and
//! handed to egui as textures.

use ld_assets::TextureSet;
use ld_game::{ItemKind, ItemRegistry};
use ld_world::{BlockRegistry, Face, Shape, Tint};
use std::path::Path;

pub const SIZE: usize = 32;

/// An RGBA icon of `SIZE`×`SIZE` pixels.
pub type Icon = Vec<u8>;

/// Renders one icon per item, in item-ID order.
pub fn render_all(
    items: &ItemRegistry,
    blocks: &BlockRegistry,
    textures: &TextureSet,
    assets: &Path,
    tints: [[f32; 3]; 2],
) -> Vec<Icon> {
    items
        .iter()
        .map(|(_, def)| match def.kind {
            ItemKind::Block(id) => {
                let b = blocks.get(id);
                let tex = |f: Face| {
                    (texture(textures, &b.textures[f as usize]), tint_color(b.tints[f as usize], tints))
                };
                if b.shape == Shape::Cross {
                    let (t, tint) = tex(Face::North);
                    flat(&t, textures.size as usize, tint)
                } else {
                    cube(textures.size as usize, tex(Face::Up), tex(Face::North), tex(Face::East))
                }
            }
            _ => {
                let path = assets.join("textures").join("item").join(format!("{}.png", def.texture));
                match image::open(&path) {
                    Ok(img) => {
                        let img = img.to_rgba8();
                        let s = img.width().min(img.height()) as usize;
                        flat(&img.into_raw(), s, [1.0; 3])
                    }
                    Err(_) => missing(),
                }
            }
        })
        .collect()
}

fn texture(set: &TextureSet, name: &str) -> Vec<u8> {
    set.layers[set.layer_of(name) as usize].clone()
}

fn tint_color(t: Tint, tints: [[f32; 3]; 2]) -> [f32; 3] {
    match t {
        Tint::Grass => tints[0],
        Tint::Foliage => tints[1],
        _ => [1.0; 3],
    }
}

/// Samples texel (u, v in 0..1) from a square RGBA texture.
fn sample(tex: &[u8], size: usize, u: f32, v: f32) -> [u8; 4] {
    let x = ((u * size as f32) as usize).min(size - 1);
    let y = ((v * size as f32) as usize).min(size - 1);
    let i = (y * size + x) * 4;
    [tex[i], tex[i + 1], tex[i + 2], tex[i + 3]]
}

/// Isometric cube: each output pixel is mapped back onto one of three faces.
fn cube(
    size: usize,
    top: (Vec<u8>, [f32; 3]),
    left: (Vec<u8>, [f32; 3]),
    right: (Vec<u8>, [f32; 3]),
) -> Icon {
    let mut out = vec![0u8; SIZE * SIZE * 4];
    // Corner points of the cube's outline in the 32×32 icon.
    let (cx, top_y, mid_y, bot_y) = (16.0f32, 1.5f32, 9.0f32, 30.5f32);
    let (lx, rx) = (1.5f32, 30.5f32);
    let edge = mid_y - top_y; // vertical drop of a top edge
    let side = bot_y - (top_y + 2.0 * edge); // height of the side faces
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            // Solve point = origin + a·e1 + b·e2 for each face.
            let solve = |ox: f32, oy: f32, e1: (f32, f32), e2: (f32, f32)| {
                let det = e1.0 * e2.1 - e1.1 * e2.0;
                let (dx, dy) = (x - ox, y - oy);
                ((dx * e2.1 - dy * e2.0) / det, (e1.0 * dy - e1.1 * dx) / det)
            };
            let inside = |a: f32, b: f32| (0.0..1.0).contains(&a) && (0.0..1.0).contains(&b);
            let face = {
                let (a, b) = solve(cx, top_y, (rx - cx, edge), (lx - cx, edge));
                if inside(a, b) {
                    Some((&top, a, b, 1.0))
                } else {
                    let (a, b) = solve(lx, mid_y, (cx - lx, edge), (0.0, side));
                    if inside(a, b) {
                        Some((&left, a, b, 0.8))
                    } else {
                        let (a, b) = solve(cx, mid_y + edge, (rx - cx, -edge), (0.0, side));
                        inside(a, b).then_some((&right, a, b, 0.6))
                    }
                }
            };
            if let Some(((tex, tint), u, v, shade)) = face {
                let t = sample(tex, size, u, v);
                if t[3] > 0 {
                    let o = (py * SIZE + px) * 4;
                    for c in 0..3 {
                        out[o + c] = (t[c] as f32 * tint[c] * shade).min(255.0) as u8;
                    }
                    out[o + 3] = 255;
                }
            }
        }
    }
    out
}

/// A flat sprite scaled up to the icon size (nearest-neighbour).
fn flat(tex: &[u8], size: usize, tint: [f32; 3]) -> Icon {
    let mut out = vec![0u8; SIZE * SIZE * 4];
    for py in 0..SIZE {
        for px in 0..SIZE {
            let t = sample(tex, size, (px as f32 + 0.5) / SIZE as f32, (py as f32 + 0.5) / SIZE as f32);
            let o = (py * SIZE + px) * 4;
            for c in 0..3 {
                out[o + c] = (t[c] as f32 * tint[c]).min(255.0) as u8;
            }
            out[o + 3] = t[3];
        }
    }
    out
}

fn missing() -> Icon {
    (0..SIZE * SIZE)
        .flat_map(|i| {
            if (i % SIZE / 8 + i / SIZE / 8).is_multiple_of(2) { [248, 0, 248, 255] } else { [0, 0, 0, 255] }
        })
        .collect()
}
