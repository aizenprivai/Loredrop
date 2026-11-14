//! Pathfinding for creatures: A* search over the block grid.
//!
//! A position is *standable* when the creature's feet and head fit in
//! non-solid blocks and the block below is solid (or water, which they swim
//! in). From each position a creature can walk to a neighbour on the same
//! level, step up one block (by jumping), or drop down up to 3 blocks
//! (further drops would hurt — and they're avoided like in Minecraft).
//!
//! The search is capped (a few hundred nodes) so a far or unreachable goal
//! can't stall the game; in that case the path leads to the reachable spot
//! closest to the goal.

use ld_core::coords::BlockPos;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// What the pathfinder needs to know about the world.
pub trait PathWorld {
    /// True if creatures can't walk through this block.
    fn is_solid(&self, p: BlockPos) -> bool;
    /// True for water (creatures can stand/swim in it).
    fn is_water(&self, p: BlockPos) -> bool;
    /// True for blocks to avoid stepping into (lava, cactus).
    fn is_dangerous(&self, p: BlockPos) -> bool;
}

const MAX_NODES: usize = 600;
const MAX_DROP: i32 = 3;

/// Whether a creature `height` blocks tall can stand with its feet at `p`.
pub fn standable(w: &impl PathWorld, p: BlockPos, height: i32) -> bool {
    for dy in 0..height {
        let b = BlockPos::new(p.x, p.y + dy, p.z);
        if w.is_solid(b) || w.is_dangerous(b) {
            return false;
        }
    }
    let below = BlockPos::new(p.x, p.y - 1, p.z);
    (w.is_solid(below) && !w.is_dangerous(below)) || w.is_water(p) || w.is_water(below)
}

/// Finds a path from `start` to (or toward) `goal`. Returns the list of
/// positions to walk through, excluding `start`.
pub fn find_path(w: &impl PathWorld, start: BlockPos, goal: BlockPos, height: i32) -> Vec<BlockPos> {
    let h = |p: BlockPos| (p.x - goal.x).abs() + (p.y - goal.y).abs() + (p.z - goal.z).abs();
    let mut open = BinaryHeap::new();
    let mut came: HashMap<BlockPos, BlockPos> = HashMap::new();
    let mut cost: HashMap<BlockPos, i32> = HashMap::new();
    cost.insert(start, 0);
    open.push(Reverse((h(start), 0, (start.x, start.y, start.z))));
    let mut best = (h(start), start);
    let mut expanded = 0;
    while let Some(Reverse((_, g, (x, y, z)))) = open.pop() {
        let cur = BlockPos::new(x, y, z);
        if cur == goal {
            best = (0, cur);
            break;
        }
        if g > *cost.get(&cur).unwrap_or(&i32::MAX) {
            continue; // stale entry
        }
        expanded += 1;
        if expanded > MAX_NODES {
            break;
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let mut candidates = Vec::with_capacity(2);
            let flat = BlockPos::new(x + dx, y, z + dz);
            if standable(w, flat, height) {
                candidates.push((flat, 1));
            } else if !w.is_solid(BlockPos::new(x, y + height, z))
                && standable(w, BlockPos::new(x + dx, y + 1, z + dz), height)
            {
                candidates.push((BlockPos::new(x + dx, y + 1, z + dz), 2)); // jump up a step
            } else if !w.is_solid(flat) && !w.is_solid(BlockPos::new(flat.x, flat.y + 1, flat.z)) {
                for drop in 1..=MAX_DROP {
                    let p = BlockPos::new(x + dx, y - drop, z + dz);
                    if standable(w, p, height) {
                        candidates.push((p, 1 + drop));
                        break;
                    }
                    if w.is_solid(p) {
                        break;
                    }
                }
            }
            for (next, step) in candidates {
                let ng = g + step;
                if ng < *cost.get(&next).unwrap_or(&i32::MAX) {
                    cost.insert(next, ng);
                    came.insert(next, cur);
                    let hn = h(next);
                    if hn < best.0 {
                        best = (hn, next);
                    }
                    open.push(Reverse((ng + hn, ng, (next.x, next.y, next.z))));
                }
            }
        }
    }
    // Walk back from the best node reached.
    let mut path = Vec::new();
    let mut p = best.1;
    while p != start {
        path.push(p);
        match came.get(&p) {
            Some(&prev) => p = prev,
            None => break,
        }
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct Grid {
        solid: HashSet<BlockPos>,
    }
    impl PathWorld for Grid {
        fn is_solid(&self, p: BlockPos) -> bool {
            p.y < 0 || self.solid.contains(&p)
        }
        fn is_water(&self, _: BlockPos) -> bool {
            false
        }
        fn is_dangerous(&self, _: BlockPos) -> bool {
            false
        }
    }

    #[test]
    fn walks_around_a_wall() {
        let mut solid = HashSet::new();
        for z in -3..=3 {
            for y in 0..3 {
                solid.insert(BlockPos::new(2, y, z));
            }
        }
        let g = Grid { solid };
        let path = find_path(&g, BlockPos::new(0, 0, 0), BlockPos::new(4, 0, 0), 2);
        assert_eq!(*path.last().unwrap(), BlockPos::new(4, 0, 0));
        assert!(path.iter().all(|p| p.x != 2 || p.z.abs() > 3), "goes around, not through");
    }

    #[test]
    fn climbs_one_block_steps_but_not_two() {
        let mut solid = HashSet::new();
        solid.insert(BlockPos::new(1, 0, 0)); // a 1-high step
        let g = Grid { solid: solid.clone() };
        let path = find_path(&g, BlockPos::new(0, 0, 0), BlockPos::new(1, 1, 0), 2);
        assert_eq!(path, vec![BlockPos::new(1, 1, 0)]);
        // A 2-high wall all around a pit: unreachable, path ends as close as possible.
        let mut walls = HashSet::new();
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                if x.abs() == 2 || z.abs() == 2 {
                    walls.insert(BlockPos::new(x, 0, z));
                    walls.insert(BlockPos::new(x, 1, z));
                }
            }
        }
        let g = Grid { solid: walls };
        let path = find_path(&g, BlockPos::new(0, 0, 0), BlockPos::new(10, 0, 0), 2);
        assert!(path.iter().all(|p| p.x.abs() < 2 && p.z.abs() < 2), "can't escape a 2-high wall");
    }

    #[test]
    fn drops_down_safely() {
        // Standing on a pillar at y=3; ground is at y=0 (3-block drop is fine).
        let mut solid = HashSet::new();
        for y in 0..3 {
            solid.insert(BlockPos::new(0, y, 0));
        }
        let g = Grid { solid };
        let path = find_path(&g, BlockPos::new(0, 3, 0), BlockPos::new(3, 0, 0), 2);
        assert_eq!(*path.last().unwrap(), BlockPos::new(3, 0, 0));
    }
}
