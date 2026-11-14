//! The loaded world and the background pipeline that streams it in and out.
//!
//! ```text
//!   player moves ──► wanted columns (render distance + 1 ring)
//!        │
//!        ├─► missing column ──► [worker] generate ──► insert into ChunkMap
//!        │                                              │
//!        │        column + all 8 neighbours loaded ◄────┘
//!        │                     │
//!        │                     ▼
//!        ├─► dirty section ──► [worker] build mesh ──► upload queue ──► GPU
//!        │                                     (at most N uploads per frame)
//!        └─► column too far ──► unload + free its GPU meshes
//! ```
//!
//! * Generation needs the extra ring (render distance + 1) because a section's
//!   mesh depends on its neighbours (faces on the edge, ambient occlusion).
//! * Work is prioritised nearest-first, and the number of jobs in flight is
//!   capped so a fast-moving player doesn't queue thousands of stale jobs.
//! * Every section has a **version**. Editing a block bumps it; a mesh that
//!   finishes with an older version is thrown away, so stale meshes never
//!   overwrite newer ones.

use ahash::{AHashMap, AHashSet};
use crossbeam_channel::{Receiver, Sender, unbounded};
use ld_core::coords::{BlockPos, ChunkPos, SectionPos};
use ld_core::world_size::SECTIONS_PER_COLUMN;
use ld_mesh::{MeshTable, Neighborhood, SectionMesh, mesh_section};
use ld_physics::{Aabb, VoxelCollider};
use ld_world::column::section_y;
use ld_world::{BiomeRegistry, BlockId, BlockRegistry, ChunkColumn, ChunkMap};
use ld_worldgen::TerrainGenerator;
use std::collections::VecDeque;
use std::sync::Arc;

/// Where finished section meshes go. The game passes the GPU renderer;
/// tests and the headless benchmark pass a sink that just counts.
pub trait SectionSink {
    fn upload(&mut self, pos: SectionPos, mesh: &SectionMesh);
    fn remove(&mut self, pos: SectionPos);
}

impl SectionSink for ld_render::Renderer {
    fn upload(&mut self, pos: SectionPos, mesh: &SectionMesh) {
        self.upload_section(pos, mesh);
    }
    fn remove(&mut self, pos: SectionPos) {
        self.remove_section(pos);
    }
}

/// Per-column bookkeeping for meshing.
#[derive(Default)]
struct ColumnState {
    /// Bit `i` set = section `i` needs a (re)mesh.
    dirty: u32,
    /// Bit `i` set = section `i` currently has a mesh on the GPU.
    on_gpu: u32,
    /// Whether the column has been queued for its first full mesh.
    meshed_once: bool,
}

/// Counters for the debug overlay.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorldStats {
    pub columns_loaded: usize,
    pub generating: usize,
    pub meshing: usize,
    pub upload_queue: usize,
    pub world_bytes: usize,
    pub threads: usize,
    /// Columns within render distance that are fully meshed.
    pub columns_ready: usize,
    pub columns_wanted: usize,
}

pub struct World {
    pub registry: Arc<BlockRegistry>,
    pub biomes: Arc<BiomeRegistry>,
    /// The world seed (shown in the debug overlay).
    pub seed: u64,
    pub map: ChunkMap,
    generator: Arc<dyn TerrainGenerator>,
    mesh_table: Arc<MeshTable>,
    pool: rayon::ThreadPool,
    threads: usize,
    gen_tx: Sender<ChunkColumn>,
    gen_rx: Receiver<ChunkColumn>,
    mesh_tx: Sender<(SectionPos, u32, SectionMesh)>,
    mesh_rx: Receiver<(SectionPos, u32, SectionMesh)>,
    generating: AHashSet<ChunkPos>,
    /// Sections being meshed → the version they were started with.
    meshing: AHashMap<SectionPos, u32>,
    versions: AHashMap<SectionPos, u32>,
    columns: AHashMap<ChunkPos, ColumnState>,
    uploads: VecDeque<(SectionPos, u32, SectionMesh)>,
    /// Offsets within (render distance + 1), sorted nearest-first.
    offsets: Vec<(i32, i32)>,
    render_distance: i32,
    /// Columns that finished generating since last drained (for spawning animals).
    pub newly_loaded: Vec<ChunkPos>,
}

impl World {
    pub fn new(
        registry: Arc<BlockRegistry>,
        biomes: Arc<BiomeRegistry>,
        seed: u64,
        generator: Arc<dyn TerrainGenerator>,
        mesh_table: Arc<MeshTable>,
        worker_threads: u32,
    ) -> Self {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        let threads =
            if worker_threads == 0 { cores.saturating_sub(1).max(1) } else { worker_threads as usize };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("ld-worker-{i}"))
            .build()
            .expect("failed to start worker threads");
        log::info!("world: {threads} background worker thread(s)");
        let (gen_tx, gen_rx) = unbounded();
        let (mesh_tx, mesh_rx) = unbounded();
        Self {
            registry,
            biomes,
            seed,
            map: ChunkMap::new(),
            generator,
            mesh_table,
            pool,
            threads,
            gen_tx,
            gen_rx,
            mesh_tx,
            mesh_rx,
            generating: AHashSet::new(),
            meshing: AHashMap::new(),
            versions: AHashMap::new(),
            columns: AHashMap::new(),
            uploads: VecDeque::new(),
            offsets: Vec::new(),
            render_distance: -1,
            newly_loaded: Vec::new(),
        }
    }

    pub fn generator(&self) -> &dyn TerrainGenerator {
        self.generator.as_ref()
    }

    fn set_render_distance(&mut self, rd: i32) {
        if rd == self.render_distance {
            return;
        }
        self.render_distance = rd;
        let r = rd + 1;
        let mut offsets: Vec<(i32, i32)> = (-r..=r).flat_map(|x| (-r..=r).map(move |z| (x, z))).collect();
        offsets.sort_by_key(|&(x, z)| x * x + z * z);
        self.offsets = offsets;
    }

    /// Advances the streaming pipeline. Call once per frame.
    pub fn update(
        &mut self,
        center: ChunkPos,
        render_distance: u32,
        uploads_per_frame: u32,
        renderer: &mut impl SectionSink,
    ) {
        self.set_render_distance(render_distance as i32);
        let rd = self.render_distance;

        // 1. Collect finished terrain.
        while let Ok(col) = self.gen_rx.try_recv() {
            let pos = col.pos;
            self.generating.remove(&pos);
            if pos.chebyshev_distance(center) > rd + 1 {
                continue; // player moved away meanwhile
            }
            self.map.insert(col);
            self.columns.entry(pos).or_default();
            self.newly_loaded.push(pos);
        }

        // 2. Unload far columns; drop meshes of columns outside render distance.
        let mut to_unload = Vec::new();
        for (&pos, state) in &mut self.columns {
            let d = pos.chebyshev_distance(center);
            if d > rd {
                for i in 0..SECTIONS_PER_COLUMN as usize {
                    if state.on_gpu & (1 << i) != 0 {
                        renderer.remove(SectionPos { x: pos.x, y: section_y(i), z: pos.z });
                    }
                }
                state.on_gpu = 0;
                state.dirty = 0;
                state.meshed_once = false;
            }
            if d > rd + 1 {
                to_unload.push(pos);
            }
        }
        for pos in to_unload {
            self.map.remove(pos);
            self.columns.remove(&pos);
        }

        // 3. Queue generation, nearest first.
        let max_gen = self.threads * 2;
        for &(dx, dz) in &self.offsets {
            if self.generating.len() >= max_gen {
                break;
            }
            let pos = ChunkPos::new(center.x + dx, center.z + dz);
            if self.map.contains(pos) || self.generating.contains(&pos) {
                continue;
            }
            self.generating.insert(pos);
            let (g, tx) = (self.generator.clone(), self.gen_tx.clone());
            self.pool.spawn(move || {
                let _ = tx.send(g.generate(pos));
            });
        }

        // 4. Queue meshing for columns whose neighbours are all loaded.
        let max_mesh = self.threads * 4;
        for &(dx, dz) in &self.offsets {
            if self.meshing.len() >= max_mesh {
                break;
            }
            if dx.abs().max(dz.abs()) > rd {
                continue;
            }
            let pos = ChunkPos::new(center.x + dx, center.z + dz);
            let Some(neigh) = self.neighbourhood(pos) else { continue };
            let state = self.columns.entry(pos).or_default();
            if !state.meshed_once {
                state.meshed_once = true;
                state.dirty = (1 << SECTIONS_PER_COLUMN) - 1;
            }
            while state.dirty != 0 && self.meshing.len() < max_mesh {
                let i = state.dirty.trailing_zeros() as usize;
                state.dirty &= !(1 << i);
                let spos = SectionPos { x: pos.x, y: section_y(i), z: pos.z };
                if neigh[1][1].as_ref().is_some_and(|c| c.section(i).is_empty()) {
                    // Nothing to draw: remove any old mesh without a job.
                    if state.on_gpu & (1 << i) != 0 {
                        renderer.remove(spos);
                        state.on_gpu &= !(1 << i);
                    }
                    continue;
                }
                let version = *self.versions.get(&spos).unwrap_or(&0);
                if self.meshing.get(&spos) == Some(&version) {
                    continue; // identical job already running
                }
                self.meshing.insert(spos, version);
                let (table, tx, cols) = (self.mesh_table.clone(), self.mesh_tx.clone(), neigh.clone());
                self.pool.spawn(move || {
                    let refs: [[Option<&ChunkColumn>; 3]; 3] =
                        std::array::from_fn(|z| std::array::from_fn(|x| cols[z][x].as_deref()));
                    let mesh = mesh_section(&Neighborhood::from_columns(refs, i), &table);
                    let _ = tx.send((spos, version, mesh));
                });
            }
        }

        // 5. Collect finished meshes and upload a limited number.
        while let Ok((spos, version, mesh)) = self.mesh_rx.try_recv() {
            if self.meshing.get(&spos) == Some(&version) {
                self.meshing.remove(&spos);
            }
            self.uploads.push_back((spos, version, mesh));
        }
        let mut uploaded = 0;
        while uploaded < uploads_per_frame {
            let Some((spos, version, mesh)) = self.uploads.pop_front() else { break };
            let cpos = ChunkPos::new(spos.x, spos.z);
            let current = *self.versions.get(&spos).unwrap_or(&0);
            let Some(state) = self.columns.get_mut(&cpos) else { continue }; // unloaded
            if version != current || cpos.chebyshev_distance(center) > rd || !state.meshed_once {
                continue; // stale or no longer wanted
            }
            let bit = 1 << (spos.y - section_y(0)) as u32;
            if mesh.is_empty() {
                renderer.remove(spos);
                state.on_gpu &= !bit;
            } else {
                renderer.upload(spos, &mesh);
                state.on_gpu |= bit;
                uploaded += 1;
            }
        }
    }

    /// The 3×3 columns around `pos` if all are loaded (`[dz+1][dx+1]`).
    fn neighbourhood(&self, pos: ChunkPos) -> Option<[[Option<Arc<ChunkColumn>>; 3]; 3]> {
        let mut out: [[Option<Arc<ChunkColumn>>; 3]; 3] = Default::default();
        for dz in -1..=1 {
            for dx in -1..=1 {
                let c = self.map.get(ChunkPos::new(pos.x + dx, pos.z + dz))?;
                out[(dz + 1) as usize][(dx + 1) as usize] = Some(c.clone());
            }
        }
        Some(out)
    }

    /// Changes a block and schedules the affected meshes for rebuilding.
    pub fn set_block(&mut self, pos: BlockPos, id: BlockId) {
        for s in self.map.set_block(pos, id) {
            *self.versions.entry(s).or_insert(0) += 1;
            if let Some(state) = self.columns.get_mut(&ChunkPos::new(s.x, s.z)) {
                let i = s.y - section_y(0);
                if (0..SECTIONS_PER_COLUMN).contains(&i) && state.meshed_once {
                    state.dirty |= 1 << i;
                }
            }
        }
    }

    /// Generates and inserts the columns within `radius` of `center`
    /// immediately on this thread (tests and tools; the game uses `update`).
    #[allow(dead_code)]
    pub fn load_now(&mut self, center: ChunkPos, radius: i32) {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = ChunkPos::new(center.x + dx, center.z + dz);
                if !self.map.contains(pos) {
                    self.map.insert(self.generator.generate(pos));
                    self.columns.entry(pos).or_default();
                }
            }
        }
    }

    /// True when the column at `pos` is loaded (physics may run there).
    pub fn is_loaded(&self, pos: ChunkPos) -> bool {
        self.map.contains(pos)
    }

    pub fn stats(&self, center: ChunkPos) -> WorldStats {
        let rd = self.render_distance.max(0);
        let wanted = ((2 * rd + 1) * (2 * rd + 1)) as usize;
        let ready = self
            .columns
            .iter()
            .filter(|(p, s)| p.chebyshev_distance(center) <= rd && s.meshed_once && s.dirty == 0)
            .count();
        WorldStats {
            columns_loaded: self.map.len(),
            generating: self.generating.len(),
            meshing: self.meshing.len(),
            upload_queue: self.uploads.len(),
            world_bytes: self.map.heap_bytes(),
            threads: self.threads,
            columns_ready: ready,
            columns_wanted: wanted,
        }
    }

    /// True once everything in render distance is generated, meshed and uploaded.
    pub fn is_settled(&self, center: ChunkPos) -> bool {
        let s = self.stats(center);
        s.generating == 0 && s.meshing == 0 && s.upload_queue == 0 && s.columns_ready == s.columns_wanted
    }
}

/// Lets physics collide with the loaded world. Unloaded columns count as
/// solid walls, so the player can't fall into terrain that hasn't arrived yet.
impl VoxelCollider for World {
    fn collision_boxes(&self, pos: BlockPos, out: &mut Vec<Aabb>) {
        match self.map.block(pos) {
            Some(id) if self.registry.get(id).solid => out.push(Aabb::block(pos.x, pos.y, pos.z)),
            Some(_) => {}
            None => out.push(Aabb::block(pos.x, pos.y, pos.z)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ld_worldgen::TestTerrain;
    use std::collections::HashMap;

    /// Records what would be on the GPU.
    #[derive(Default)]
    struct FakeGpu {
        sections: HashMap<SectionPos, usize>,
        uploads: usize,
    }
    impl SectionSink for FakeGpu {
        fn upload(&mut self, pos: SectionPos, mesh: &SectionMesh) {
            self.sections.insert(pos, mesh.quad_count());
            self.uploads += 1;
        }
        fn remove(&mut self, pos: SectionPos) {
            self.sections.remove(&pos);
        }
    }

    pub(crate) fn test_biomes() -> Arc<BiomeRegistry> {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/biomes.toml"))
                .unwrap();
        Arc::new(BiomeRegistry::from_toml(&text).unwrap())
    }

    fn world() -> World {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/blocks.toml"))
                .unwrap();
        let reg = Arc::new(BlockRegistry::from_toml(&text).unwrap());
        let generator = Arc::new(TestTerrain::new(1, &reg).unwrap());
        let table = Arc::new(MeshTable::new(&reg, None, |_| 1));
        World::new(reg, test_biomes(), 1, generator, table, 2)
    }

    /// Runs the pipeline until everything within render distance is on the "GPU".
    fn settle(w: &mut World, center: ChunkPos, rd: u32, gpu: &mut FakeGpu) {
        for _ in 0..20_000 {
            w.update(center, rd, 64, gpu);
            if w.is_settled(center) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        panic!("world never settled: {:?}", w.stats(center));
    }

    #[test]
    fn loads_render_distance_plus_border_and_meshes_inner_area() {
        let mut w = world();
        let mut gpu = FakeGpu::default();
        let c = ChunkPos::new(0, 0);
        settle(&mut w, c, 3, &mut gpu);
        assert_eq!(w.map.len(), 9 * 9, "render distance 3 + 1 ring = 9×9 columns");
        assert!(!gpu.sections.is_empty());
        assert!(
            gpu.sections.keys().all(|s| ChunkPos::new(s.x, s.z).chebyshev_distance(c) <= 3),
            "nothing meshed outside render distance"
        );
        // Every column within render distance has at least its surface meshed.
        for x in -3..=3 {
            for z in -3..=3 {
                assert!(gpu.sections.keys().any(|s| s.x == x && s.z == z), "column {x},{z} has no mesh");
            }
        }
    }

    #[test]
    fn moving_unloads_far_columns_and_frees_their_meshes() {
        let mut w = world();
        let mut gpu = FakeGpu::default();
        settle(&mut w, ChunkPos::new(0, 0), 2, &mut gpu);
        let far = ChunkPos::new(20, 0);
        settle(&mut w, far, 2, &mut gpu);
        assert!(!w.map.contains(ChunkPos::new(0, 0)), "old area unloaded");
        assert!(
            gpu.sections.keys().all(|s| ChunkPos::new(s.x, s.z).chebyshev_distance(far) <= 2),
            "old meshes freed"
        );
        assert_eq!(w.map.len(), 7 * 7);
    }

    #[test]
    fn editing_a_block_on_a_chunk_border_remeshes_both_sides() {
        let mut w = world();
        let mut gpu = FakeGpu::default();
        let c = ChunkPos::new(0, 0);
        settle(&mut w, c, 2, &mut gpu);
        // Dig a hole right at the x=0/x=-1 column border, at the surface.
        let h = w.generator().surface_height(0, 5);
        let before = gpu.uploads;
        w.set_block(BlockPos::new(0, h, 5), BlockId::AIR);
        settle(&mut w, c, 2, &mut gpu);
        let remeshed = gpu.uploads - before;
        assert!(remeshed >= 2, "own section and the neighbour across the border were rebuilt ({remeshed})");
        assert_eq!(w.map.block(BlockPos::new(0, h, 5)), Some(BlockId::AIR));
    }
}
