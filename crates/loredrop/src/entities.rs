//! Dropped items lying in the world, and building the triangles that show
//! them (plus the cracks on a block being mined).
//!
//! Item physics follow Minecraft: gravity 0.04 blocks/tick², air drag 0.98,
//! ground friction, a short pickup delay (10 ticks after a block drops it,
//! 40 after the player throws it), pickup within about one block, nearby
//! identical items merging, and despawning after 5 minutes.

use glam::{DVec3, Vec3};
use ld_game::{Inventory, ItemRegistry, ItemStack};
use ld_physics::{Aabb, VoxelCollider, move_and_collide};
use ld_render::EntityVertex;

/// How an item looks when dropped: a small cube, or a flat spinning sprite.
#[derive(Debug, Clone, Copy)]
pub enum ItemLook {
    /// Texture layers per face (East, West, Up, Down, South, North) and the
    /// packed tint colour per face.
    Cube {
        layers: [u32; 6],
        tints: [u32; 6],
    },
    Flat {
        layer: u32,
        tint: u32,
    },
}

pub struct ItemEntity {
    pub pos: DVec3,
    pub vel: DVec3,
    pub stack: ItemStack,
    pub age: u32,
    pub pickup_delay: u32,
    /// Random phase so items don't spin in sync.
    pub phase: f32,
}

const SIZE: f64 = 0.25;
const DESPAWN_TICKS: u32 = 6000;

#[derive(Default)]
pub struct ItemEntities {
    pub list: Vec<ItemEntity>,
    rng: u64,
}

impl ItemEntities {
    fn rand(&mut self) -> f64 {
        // xorshift64*: tiny and good enough for visual randomness.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Drops a stack from a broken block (popping out with a little random velocity).
    pub fn drop_from_block(&mut self, x: i32, y: i32, z: i32, stack: ItemStack) {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9_7F4A_7C15;
        }
        let pos = DVec3::new(
            x as f64 + 0.25 + self.rand() * 0.5,
            y as f64 + 0.25 + self.rand() * 0.5,
            z as f64 + 0.25 + self.rand() * 0.5,
        );
        let vel = DVec3::new((self.rand() - 0.5) * 0.2, 0.2, (self.rand() - 0.5) * 0.2);
        let phase = self.rand() as f32 * 6.28;
        self.list.push(ItemEntity { pos, vel, stack, age: 0, pickup_delay: 10, phase });
    }

    /// Throws a stack from the player's eyes in the look direction.
    pub fn throw(&mut self, eye: DVec3, look: DVec3, stack: ItemStack) {
        if self.rng == 0 {
            self.rng = 0x1234_5678_9ABC_DEF1;
        }
        let phase = self.rand() as f32 * 6.28;
        self.list.push(ItemEntity {
            pos: eye - DVec3::new(0.0, 0.3, 0.0),
            vel: look * 0.3 + DVec3::new(0.0, 0.1, 0.0),
            stack,
            age: 0,
            pickup_delay: 40,
            phase,
        });
    }

    /// Advances all items one tick; moves picked-up items into `inventory`.
    /// Returns how many stacks were (fully or partly) picked up.
    pub fn tick(
        &mut self,
        world: &impl VoxelCollider,
        player: &Aabb,
        inventory: &mut Inventory,
        items: &ItemRegistry,
    ) -> usize {
        let mut picked = 0;
        let pickup_zone =
            Aabb::new(player.min - DVec3::new(1.0, 0.5, 1.0), player.max + DVec3::new(1.0, 0.5, 1.0));
        for e in &mut self.list {
            e.age += 1;
            e.pickup_delay = e.pickup_delay.saturating_sub(1);
            e.vel.y -= 0.04;
            let aabb = Aabb::from_feet(e.pos, SIZE, SIZE);
            let r = move_and_collide(world, &aabb, e.vel);
            e.pos += r.moved;
            let on_ground = r.landed(e.vel);
            if r.hit_x {
                e.vel.x = 0.0;
            }
            if r.hit_y {
                e.vel.y = 0.0;
            }
            if r.hit_z {
                e.vel.z = 0.0;
            }
            let friction = if on_ground { 0.6 * 0.98 } else { 0.98 };
            e.vel.x *= friction;
            e.vel.z *= friction;
            e.vel.y *= 0.98;
            if e.pickup_delay == 0 && Aabb::from_feet(e.pos, SIZE, SIZE).intersects(&pickup_zone) {
                match inventory.add(e.stack, items) {
                    None => {
                        e.stack.count = 0;
                        picked += 1;
                    }
                    Some(rest) if rest.count < e.stack.count => {
                        e.stack = rest;
                        picked += 1;
                    }
                    Some(_) => {} // inventory full
                }
            }
        }
        // Merge identical items lying close together (every 4 ticks).
        if self.list.first().is_some_and(|e| e.age.is_multiple_of(4)) {
            for i in 0..self.list.len() {
                for j in i + 1..self.list.len() {
                    let (a, b) = (&self.list[i], &self.list[j]);
                    if a.stack.count > 0
                        && b.stack.count > 0
                        && a.stack.stacks_with(&b.stack)
                        // Minecraft: boxes enlarged by 0.5 horizontally must overlap.
                        && glam::DVec2::new(a.pos.x - b.pos.x, a.pos.z - b.pos.z).length() < 0.75
                        && (a.pos.y - b.pos.y).abs() < 0.5
                        && a.stack.count + b.stack.count <= items.get(a.stack.item).max_stack
                    {
                        let n = self.list[j].stack.count;
                        self.list[i].stack.count += n;
                        self.list[j].stack.count = 0;
                    }
                }
            }
        }
        self.list.retain(|e| e.stack.count > 0 && e.age < DESPAWN_TICKS && e.pos.y > -128.0);
        picked
    }

    /// Triangles for every item (spinning and bobbing), relative to `camera`.
    pub fn build_vertices(&self, camera: DVec3, time: f32, looks: &[ItemLook], out: &mut Vec<EntityVertex>) {
        for e in &self.list {
            let Some(look) = looks.get(e.stack.item.0 as usize) else { continue };
            let angle = time * 1.2 + e.phase;
            let bob = ((time * 2.5 + e.phase).sin() * 0.06 + 0.1) as f64;
            let center = (e.pos + DVec3::new(0.0, SIZE / 2.0 + bob, 0.0) - camera).as_vec3();
            // Bigger stacks show as 2–3 copies, like Minecraft.
            let copies = match e.stack.count {
                1 => 1,
                2..=16 => 2,
                _ => 3,
            };
            for c in 0..copies {
                let offset = Vec3::new(c as f32 * 0.06, c as f32 * 0.05, c as f32 * -0.04);
                match *look {
                    ItemLook::Cube { layers, tints } => {
                        cube(out, center + offset, 0.125, angle, layers, tints)
                    }
                    ItemLook::Flat { layer, tint } => sprite(out, center + offset, 0.25, angle, layer, tint),
                }
            }
        }
    }
}

/// Multiplies a packed sRGB colour by a brightness.
fn shade(color: u32, k: f32) -> u32 {
    let ch = |s: u32| (((color >> s) & 255) as f32 * k).min(255.0) as u32;
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

fn rotate_y(v: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    Vec3::new(v.x * c - v.z * s, v.y, v.x * s + v.z * c)
}

/// Pushes one quad (two triangles) with UVs (0,1)(1,1)(1,0)(0,0).
fn quad(out: &mut Vec<EntityVertex>, p: [Vec3; 4], layer: u32, color: u32) {
    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(EntityVertex { pos: p[i].to_array(), uv: uv[i], layer, color });
    }
}

/// A cube of half-size `h` rotated around Y.
pub fn cube(
    out: &mut Vec<EntityVertex>,
    center: Vec3,
    h: f32,
    angle: f32,
    layers: [u32; 6],
    tints: [u32; 6],
) {
    let corner = |x: f32, y: f32, z: f32| center + rotate_y(Vec3::new(x * h, y * h, z * h), angle);
    // (face index, 4 corners as bottom-left, bottom-right, top-right, top-left, brightness)
    let faces: [(usize, [Vec3; 4], f32); 6] = [
        (0, [corner(1., -1., 1.), corner(1., -1., -1.), corner(1., 1., -1.), corner(1., 1., 1.)], 0.6),
        (1, [corner(-1., -1., -1.), corner(-1., -1., 1.), corner(-1., 1., 1.), corner(-1., 1., -1.)], 0.6),
        (2, [corner(-1., 1., 1.), corner(1., 1., 1.), corner(1., 1., -1.), corner(-1., 1., -1.)], 1.0),
        (3, [corner(-1., -1., -1.), corner(1., -1., -1.), corner(1., -1., 1.), corner(-1., -1., 1.)], 0.5),
        (4, [corner(-1., -1., 1.), corner(1., -1., 1.), corner(1., 1., 1.), corner(-1., 1., 1.)], 0.8),
        (5, [corner(1., -1., -1.), corner(-1., -1., -1.), corner(-1., 1., -1.), corner(1., 1., -1.)], 0.8),
    ];
    for (f, p, k) in faces {
        quad(out, p, layers[f], shade(tints[f], k));
    }
}

/// A flat square sprite of half-size `h`, rotated around Y (visible from both sides).
fn sprite(out: &mut Vec<EntityVertex>, center: Vec3, h: f32, angle: f32, layer: u32, tint: u32) {
    let corner = |x: f32, y: f32| center + rotate_y(Vec3::new(x * h, y * h, 0.0), angle);
    quad(out, [corner(-1., -1.), corner(1., -1.), corner(1., 1.), corner(-1., 1.)], layer, tint);
}

/// The cracks shown on a block being mined: its six faces, slightly enlarged,
/// using the crack texture for `stage` (0–9).
pub fn crack_vertices(out: &mut Vec<EntityVertex>, block_min: DVec3, camera: DVec3, layer: u32) {
    let center = (block_min + DVec3::splat(0.5) - camera).as_vec3();
    cube(out, center, 0.502, 0.0, [layer; 6], [0xFF_FFFF; 6]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ld_core::coords::BlockPos;

    struct Floor;
    impl VoxelCollider for Floor {
        fn collision_boxes(&self, p: BlockPos, out: &mut Vec<Aabb>) {
            if p.y < 0 {
                out.push(Aabb::block(p.x, p.y, p.z));
            }
        }
    }

    fn items() -> ItemRegistry {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
        let blocks = ld_world::BlockRegistry::from_toml(
            &std::fs::read_to_string(format!("{dir}blocks.toml")).unwrap(),
        )
        .unwrap();
        ItemRegistry::new(&blocks, &std::fs::read_to_string(format!("{dir}items.toml")).unwrap()).unwrap()
    }

    #[test]
    fn dropped_items_fall_merge_and_get_picked_up() {
        let items = items();
        let dirt = items.id("dirt").unwrap();
        let mut ents = ItemEntities::default();
        ents.drop_from_block(10, 3, 10, ItemStack::new(dirt, 3));
        ents.drop_from_block(10, 3, 10, ItemStack::new(dirt, 2));
        let far_player = Aabb::from_feet(DVec3::new(100.0, 0.0, 100.0), 0.6, 1.8);
        let mut inv = Inventory::default();
        for _ in 0..60 {
            ents.tick(&Floor, &far_player, &mut inv, &items);
        }
        assert_eq!(ents.list.len(), 1, "two nearby dirt drops merged");
        assert!(ents.list[0].pos.y.abs() < 1e-6, "landed on the floor");
        assert_eq!(ents.list[0].stack.count, 5);
        // Walk up to wherever the item came to rest.
        let at = ents.list[0].pos;
        let near = Aabb::from_feet(DVec3::new(at.x + 0.8, 0.0, at.z), 0.6, 1.8);
        ents.tick(&Floor, &near, &mut inv, &items);
        assert!(ents.list.is_empty());
        assert_eq!(inv.count_of(dirt), 5);
    }

    #[test]
    fn thrown_items_wait_before_pickup() {
        let items = items();
        let mut ents = ItemEntities::default();
        ents.throw(DVec3::new(0.5, 1.62, 0.5), DVec3::Z, ItemStack::new(items.id("stick").unwrap(), 1));
        let player = Aabb::from_feet(DVec3::new(0.5, 0.0, 0.5), 0.6, 1.8);
        let mut inv = Inventory::default();
        for _ in 0..5 {
            ents.tick(&Floor, &player, &mut inv, &items);
        }
        assert_eq!(ents.list.len(), 1, "not picked up straight away");
    }
}
