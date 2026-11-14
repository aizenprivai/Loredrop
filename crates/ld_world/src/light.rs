//! Light levels: sunlight ("sky light") and light from blocks ("block light").
//!
//! Both are numbers from 0 (dark) to 15 (brightest), like Minecraft:
//!
//! * **Sky light** is 15 wherever the sky is visible. Straight down through
//!   clear blocks (air, glass) it stays 15; every other step — sideways, up,
//!   or through leaves/water — loses at least 1. So light pours down shafts,
//!   fades under overhangs, and caves are dark. The time of day doesn't change
//!   these numbers; the renderer dims sky light at night.
//! * **Block light** starts at a light-giving block (lava 15, torch 14…) and
//!   loses 1 per step in every direction.
//! * Opaque blocks stop light completely; leaves and water dim it
//!   ([`BlockDef::light_filter`](crate::BlockDef::light_filter)).
//!
//! ## How it's computed (Loredrop's approach)
//! Light is **not stored** with the world. When sections' meshes are rebuilt,
//! light is computed for a box reaching 15 blocks beyond them on every side
//! ([`LightRegion::around_sections`]) — enough for any light source that can
//! reach them — and flood-filled (breadth-first). One box serves a whole run
//! of stacked sections, which is ~2.5× cheaper than one box per section.
//! There is no incremental light update to get wrong, and nothing to save.
//! The price: block edits rebuild the sections within light range, and light
//! that would travel *out* of the box and back in is missed (rare, and only
//! more than 15 blocks from its source).

use crate::block::{BlockId, BlockRegistry};
use crate::column::ChunkColumn;
use crate::map::ChunkMap;
use crate::section::{self, Section};
use ld_core::coords::{BlockPos, ChunkPos};
use ld_core::world_size::{MAX_Y, MIN_Y, SECTIONS_PER_COLUMN};

/// Brightest light level.
pub const MAX_LIGHT: u8 = 15;

/// How far light can travel from a source (a level-15 source reaches 14 blocks).
pub const MARGIN: i32 = 15;

/// Filter value of blocks that stop light completely.
pub const OPAQUE: u8 = 15;

/// Light properties of every block type, indexed by [`BlockId`].
#[derive(Debug, Clone)]
pub struct LightTable {
    filter: Vec<u8>,
    emit: Vec<u8>,
}

impl LightTable {
    pub fn new(registry: &BlockRegistry) -> Self {
        let mut filter = Vec::with_capacity(registry.len());
        let mut emit = Vec::with_capacity(registry.len());
        for (_, def) in registry.iter() {
            filter.push(def.light_filter.min(MAX_LIGHT));
            emit.push(def.light.min(MAX_LIGHT));
        }
        Self { filter, emit }
    }

    /// How much light passing through this block loses (15 = blocks it).
    #[inline]
    pub fn filter(&self, id: BlockId) -> u8 {
        self.filter.get(id.0 as usize).copied().unwrap_or(OPAQUE)
    }

    /// Light given off by this block.
    #[inline]
    pub fn emit(&self, id: BlockId) -> u8 {
        self.emit.get(id.0 as usize).copied().unwrap_or(0)
    }

    /// Filter and emission packed as `filter << 4 | emit`.
    #[inline]
    pub fn packed(&self, id: BlockId) -> u8 {
        (self.filter(id) << 4) | self.emit(id)
    }

    /// True if swapping `a` for `b` can change light anywhere.
    pub fn changes_light(&self, a: BlockId, b: BlockId) -> bool {
        self.filter(a) != self.filter(b) || self.emit(a) != self.emit(b)
    }
}

/// Light arriving in a cell with filter `f` from a neighbour at level `from`.
/// Sky light at full strength going straight down through a clear block
/// doesn't fade; everything else loses at least 1.
#[inline]
fn step(from: u8, f: u8, sky_down: bool) -> u8 {
    if sky_down && from == MAX_LIGHT && f == 0 { MAX_LIGHT } else { from.saturating_sub(f.max(1)) }
}

/// Light of one section's 4 096 blocks, packed `sky << 4 | block` per byte,
/// in [`section::index`] order. Kept by the game for light queries
/// (creature spawning, shading creatures).
#[derive(Debug, Clone, PartialEq)]
pub enum SectionLight {
    /// Every block has the same packed value (e.g. open sky: `0xF0`).
    Uniform(u8),
    Data(Box<[u8; section::VOLUME]>),
}

impl SectionLight {
    /// Packed light of local block (x, y, z), each 0..16.
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u8 {
        match self {
            SectionLight::Uniform(v) => *v,
            SectionLight::Data(d) => d[section::index(x, y, z)],
        }
    }

    /// Approximate heap memory used.
    pub fn heap_bytes(&self) -> usize {
        match self {
            SectionLight::Uniform(_) => 0,
            SectionLight::Data(_) => section::VOLUME,
        }
    }
}

/// Packs a (sky, block) pair into one byte.
#[inline]
pub fn pack(sky: u8, block: u8) -> u8 {
    (sky << 4) | block
}

/// Splits a packed byte into (sky, block).
#[inline]
pub fn unpack(v: u8) -> (u8, u8) {
    (v >> 4, v & 15)
}

/// Light values for a box of blocks.
///
/// Coordinates are in whatever frame the region was built in: relative to
/// the section origin for [`around_section`](Self::around_section), world
/// coordinates for [`around_block`](Self::around_block).
#[derive(Debug, Clone)]
pub struct LightRegion {
    /// Coordinates of cell 0.
    origin: [i32; 3],
    size: [usize; 3],
    sky: Vec<u8>,
    block: Vec<u8>,
}

impl LightRegion {
    /// Flood-fills light in a box.
    ///
    /// * `filters` / `emits`: per cell, in x-fastest then z then y order.
    /// * `sky_in`: sky light arriving at the top of each column (index `z * sx + x`),
    ///   i.e. what's left after passing through everything above the box.
    pub fn compute(origin: [i32; 3], size: [usize; 3], filters: &[u8], emits: &[u8], sky_in: &[u8]) -> Self {
        let [sx, sy, sz] = size;
        let n = sx * sy * sz;
        debug_assert!(filters.len() == n && emits.len() == n && sky_in.len() == sx * sz);
        let layer = sx * sz;
        let mut sky = vec![0u8; n];
        // 1. Sunlight falling straight down each column.
        for z in 0..sz {
            for x in 0..sx {
                let mut l = sky_in[z * sx + x];
                for y in (0..sy).rev() {
                    if l == 0 {
                        break;
                    }
                    let i = y * layer + z * sx + x;
                    l = step(l, filters[i], true);
                    sky[i] = l;
                }
            }
        }
        // 2. Spread it from every cell that can light a neighbour sideways or
        //    upward (downward was handled by step 1).
        let mut queue = Vec::new();
        for y in 0..sy {
            for z in 0..sz {
                let row = y * layer + z * sx;
                for x in 0..sx {
                    let i = row + x;
                    let v = sky[i];
                    if v <= 1 {
                        continue;
                    }
                    let gives = |j: usize| step(v, filters[j], false) > sky[j];
                    if (x + 1 < sx && gives(i + 1))
                        || (x > 0 && gives(i - 1))
                        || (z + 1 < sz && gives(i + sx))
                        || (z > 0 && gives(i - sx))
                        || (y + 1 < sy && gives(i + layer))
                    {
                        queue.push(i as u32);
                    }
                }
            }
        }
        Self::flood(&mut sky, &mut queue, size, filters, true);
        // 3. Block light from every light-giving block.
        let mut block = vec![0u8; n];
        queue.clear();
        if emits.iter().any(|&e| e > 0) {
            for (i, &e) in emits.iter().enumerate() {
                if e > 0 {
                    block[i] = e;
                    if e > 1 {
                        queue.push(i as u32);
                    }
                }
            }
            Self::flood(&mut block, &mut queue, size, filters, false);
        }
        Self { origin, size, sky, block }
    }

    /// The (up to 6) neighbours of cell `i`, with "is this straight down".
    #[inline]
    fn neighbours(i: usize, size: [usize; 3]) -> impl Iterator<Item = (usize, bool)> {
        let [sx, sy, sz] = size;
        let layer = sx * sz;
        let (y, rem) = (i / layer, i % layer);
        let (z, x) = (rem / sx, rem % sx);
        [
            (x + 1 < sx).then(|| (i + 1, false)),
            (x > 0).then(|| (i - 1, false)),
            (z + 1 < sz).then(|| (i + sx, false)),
            (z > 0).then(|| (i - sx, false)),
            (y + 1 < sy).then(|| (i + layer, false)),
            (y > 0).then(|| (i - layer, true)),
        ]
        .into_iter()
        .flatten()
    }

    /// Breadth-first flood fill from the queued cells.
    fn flood(light: &mut [u8], queue: &mut Vec<u32>, size: [usize; 3], filters: &[u8], sky: bool) {
        let mut head = 0;
        while head < queue.len() {
            let i = queue[head] as usize;
            head += 1;
            let v = light[i];
            if v <= 1 {
                continue;
            }
            for (j, down) in Self::neighbours(i, size) {
                let nv = step(v, filters[j], sky && down);
                if nv > light[j] {
                    light[j] = nv;
                    queue.push(j as u32);
                }
            }
        }
    }

    #[inline]
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (lx, ly, lz) = (x - self.origin[0], y - self.origin[1], z - self.origin[2]);
        let [sx, sy, sz] = self.size;
        if lx < 0 || ly < 0 || lz < 0 || lx as usize >= sx || ly as usize >= sy || lz as usize >= sz {
            return None;
        }
        Some((ly as usize * sz + lz as usize) * sx + lx as usize)
    }

    /// (sky, block) light at a cell, or `None` outside the region.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<(u8, u8)> {
        self.index(x, y, z).map(|i| (self.sky[i], self.block[i]))
    }

    /// Light for the box around section `section_index` (0..24) of the centre
    /// column. See [`around_sections`](Self::around_sections).
    pub fn around_section(
        columns: [[Option<&ChunkColumn>; 3]; 3],
        section_index: usize,
        table: &LightTable,
    ) -> Self {
        Self::around_sections(columns, section_index, section_index, table)
    }

    /// Light for the box around sections `lo..=hi` (0..24) of the centre
    /// column. `columns[dz+1][dx+1]` is the column at offset (dx, dz); missing
    /// columns count as solid rock. Coordinates are relative to section
    /// `lo`'s origin; use [`view`](Self::view) to look at one section.
    pub fn around_sections(
        columns: [[Option<&ChunkColumn>; 3]; 3],
        lo: usize,
        hi: usize,
        table: &LightTable,
    ) -> Self {
        let base_y = MIN_Y + lo as i32 * 16;
        let side = (16 + 2 * MARGIN) as usize;
        let height = (hi - lo + 1) * 16 + 2 * MARGIN as usize;
        let size = [side, height, side];
        let origin = [-MARGIN, -MARGIN, -MARGIN];
        // World y range [y0, y1).
        let (y0, y1) = (base_y - MARGIN, MIN_Y + (hi as i32 + 1) * 16 + MARGIN);
        let mut filters = vec![OPAQUE; side * height * side];
        let mut emits = vec![0u8; side * height * side];
        let mut sky_in = vec![0u8; side * side];
        let mut buf = Box::new([0u8; section::VOLUME]);
        for (zi, row) in columns.iter().enumerate() {
            for (xi, col) in row.iter().enumerate() {
                let Some(col) = col else { continue }; // stays opaque, no sky
                // Region cells (relative to the section origin) this column covers.
                let (ox, oz) = ((xi as i32 - 1) * 16, (zi as i32 - 1) * 16);
                let (x0, x1) = ((-MARGIN).max(ox), (16 + MARGIN).min(ox + 16));
                let (z0, z1) = ((-MARGIN).max(oz), (16 + MARGIN).min(oz + 16));
                let (lx0, lx1) = ((x0 - ox) as usize, (x1 - ox) as usize);
                let (lz0, lz1) = ((z0 - oz) as usize, (z1 - oz) as usize);
                let levels = sky_entering_area(col, lx0..lx1, lz0..lz1, y1, table, &mut buf);
                for lz in lz0..lz1 {
                    for lx in lx0..lx1 {
                        let (cx, cz) =
                            ((lx as i32 + ox + MARGIN) as usize, (lz as i32 + oz + MARGIN) as usize);
                        sky_in[cz * side + cx] = levels[lz * 16 + lx];
                    }
                }
                // Decode each overlapping section once, then copy the needed part.
                for s in lo as i32 - 1..=hi as i32 + 1 {
                    let (sy0, sy1) = ((MIN_Y + s * 16).max(y0), (MIN_Y + (s + 1) * 16).min(y1));
                    if s < 0 || sy0 >= sy1 {
                        continue; // below the world: stays opaque
                    }
                    if s >= SECTIONS_PER_COLUMN {
                        buf.fill(0); // above the world: open air
                    } else {
                        col.section(s as usize).decode_with(|id| table.packed(id), &mut buf);
                    }
                    for wy in sy0..sy1 {
                        let ly = (wy - MIN_Y - s * 16) as usize;
                        let ry = (wy - y0) as usize;
                        for rz in z0..z1 {
                            let lz = (rz - oz) as usize;
                            let row_base = (ry * side + (rz + MARGIN) as usize) * side;
                            for rx in x0..x1 {
                                let v = buf[section::index((rx - ox) as usize, ly, lz)];
                                let i = row_base + (rx + MARGIN) as usize;
                                filters[i] = v >> 4;
                                emits[i] = v & 15;
                            }
                        }
                    }
                }
            }
        }
        Self::compute(origin, size, &filters, &emits, &sky_in)
    }

    /// Light for the box of `radius` blocks around `center`, read from the
    /// loaded world (unloaded columns count as solid rock). Coordinates are
    /// world coordinates. Slower than the mesher's path; meant for occasional
    /// gameplay queries where the light cache has nothing.
    pub fn around_block(map: &ChunkMap, center: BlockPos, radius: i32, table: &LightTable) -> Self {
        let side = (2 * radius + 1) as usize;
        let size = [side; 3];
        let origin = [center.x - radius, center.y - radius, center.z - radius];
        let (y0, y1) = (center.y - radius, center.y + radius + 1);
        let mut filters = vec![OPAQUE; side * side * side];
        let mut emits = vec![0u8; side * side * side];
        let mut sky_in = vec![0u8; side * side];
        for cz in 0..side {
            for cx in 0..side {
                let (wx, wz) = (origin[0] + cx as i32, origin[2] + cz as i32);
                let Some(col) = map.get(ChunkPos::new(wx.div_euclid(16), wz.div_euclid(16))) else {
                    continue;
                };
                let (lx, lz) = (wx.rem_euclid(16) as usize, wz.rem_euclid(16) as usize);
                sky_in[cz * side + cx] = sky_entering(col, lx, lz, y1, table);
                fill_column(col, lx, lz, y0, y1, table, |ry, f, e| {
                    let i = (ry * side + cz) * side + cx;
                    filters[i] = f;
                    emits[i] = e;
                });
            }
        }
        Self::compute(origin, size, &filters, &emits, &sky_in)
    }

    /// This region seen from a section `dy` blocks above the region's frame
    /// origin (e.g. `16 * (k - lo)` for section `k` of
    /// [`around_sections`](Self::around_sections)).
    pub fn view(&self, dy: i32) -> LightView<'_> {
        LightView { region: self, dy }
    }

    /// The light of the 16³ section whose origin is `(x, y, z)` in this
    /// region's frame, compressed to [`SectionLight::Uniform`] when possible.
    pub fn section_light(&self, x: i32, y: i32, z: i32) -> SectionLight {
        let mut data = Box::new([0u8; section::VOLUME]);
        for ly in 0..16 {
            for lz in 0..16 {
                for lx in 0..16 {
                    let (s, b) = self.get(x + lx as i32, y + ly as i32, z + lz as i32).unwrap_or((0, 0));
                    data[section::index(lx, ly, lz)] = pack(s, b);
                }
            }
        }
        let first = data[0];
        if data.iter().all(|&v| v == first) { SectionLight::Uniform(first) } else { SectionLight::Data(data) }
    }
}

/// A [`LightRegion`] in one section's coordinates (the section is 0..16).
#[derive(Debug, Clone, Copy)]
pub struct LightView<'a> {
    region: &'a LightRegion,
    dy: i32,
}

impl LightView<'_> {
    /// (sky, block) light at a cell in section coordinates.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<(u8, u8)> {
        self.region.get(x, y + self.dy, z)
    }

    /// The section's own light (its 0..16 cube), compressed when uniform.
    pub fn section_light(&self) -> SectionLight {
        self.region.section_light(0, self.dy, 0)
    }
}

/// Sky light that reaches world height `top` (from above) in one block
/// column, after passing through everything above it.
pub fn sky_entering(col: &ChunkColumn, lx: usize, lz: usize, top: i32, table: &LightTable) -> u8 {
    let mut l = MAX_LIGHT;
    let mut y = MAX_Y - 1;
    while y >= top && l > 0 {
        let s = ((y - MIN_Y) / 16) as usize;
        match col.section(s) {
            // A whole clear section: sunlight passes straight through.
            Section::Uniform(id) if table.filter(*id) == 0 && l == MAX_LIGHT => {
                y = MIN_Y + s as i32 * 16 - 1;
                continue;
            }
            _ => {
                l = step(l, table.filter(col.get(lx, y, lz)), true);
                y -= 1;
            }
        }
    }
    if y < MIN_Y - 1 { 0 } else { l }
}

/// [`sky_entering`] for a rectangle of block columns at once (`[lz * 16 + lx]`),
/// decoding each section once instead of reading blocks one by one.
fn sky_entering_area(
    col: &ChunkColumn,
    xs: std::ops::Range<usize>,
    zs: std::ops::Range<usize>,
    top: i32,
    table: &LightTable,
    buf: &mut [u8; section::VOLUME],
) -> [u8; 256] {
    let mut level = [MAX_LIGHT; 256];
    if top >= MAX_Y {
        return level;
    }
    let lowest_section = ((top.max(MIN_Y) - MIN_Y) / 16) as usize;
    for s in (lowest_section..SECTIONS_PER_COLUMN as usize).rev() {
        // Rows of this section that lie above the box (world y >= top).
        let first_row = (top - (MIN_Y + s as i32 * 16)).clamp(0, 16) as usize;
        match col.section(s) {
            Section::Uniform(id) if table.filter(*id) == 0 => {
                // Clear section: full light passes; weaker light fades 1 per row.
                for lz in zs.clone() {
                    for lx in xs.clone() {
                        let l = &mut level[lz * 16 + lx];
                        if *l < MAX_LIGHT {
                            *l = l.saturating_sub((16 - first_row) as u8);
                        }
                    }
                }
            }
            sec => {
                sec.decode_with(|id| table.filter(id), buf);
                for ly in (first_row..16).rev() {
                    for lz in zs.clone() {
                        for lx in xs.clone() {
                            let l = &mut level[lz * 16 + lx];
                            *l = step(*l, buf[section::index(lx, ly, lz)], true);
                        }
                    }
                }
            }
        }
        if zs.clone().all(|lz| xs.clone().all(|lx| level[lz * 16 + lx] == 0)) {
            break; // everything below is in shadow
        }
    }
    level
}

/// Calls `put(relative y, filter, emit)` for world heights `y0..y1` of one
/// block column. Below the world is solid; above it is open air.
fn fill_column(
    col: &ChunkColumn,
    lx: usize,
    lz: usize,
    y0: i32,
    y1: i32,
    table: &LightTable,
    mut put: impl FnMut(usize, u8, u8),
) {
    let mut y = y0;
    while y < y1 {
        let ry = (y - y0) as usize;
        if y < MIN_Y {
            put(ry, OPAQUE, 0);
            y += 1;
            continue;
        }
        if y >= MAX_Y {
            put(ry, 0, 0);
            y += 1;
            continue;
        }
        let s = ((y - MIN_Y) / 16) as usize;
        let section_end = (MIN_Y + (s as i32 + 1) * 16).min(y1);
        match col.section(s.min(SECTIONS_PER_COLUMN as usize - 1)) {
            Section::Uniform(id) => {
                let (f, e) = (table.filter(*id), table.emit(*id));
                for yy in y..section_end {
                    put((yy - y0) as usize, f, e);
                }
            }
            _ => {
                for yy in y..section_end {
                    let id = col.get(lx, yy, lz);
                    put((yy - y0) as usize, table.filter(id), table.emit(id));
                }
            }
        }
        y = section_end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCKS: &str = r#"
        [[block]]
        name = "stone"
        layer = "opaque"
        textures = { all = "stone" }
        [[block]]
        name = "leaves"
        layer = "cutout"
        light_filter = 1
        textures = { all = "leaves" }
        [[block]]
        name = "lava"
        layer = "opaque"
        light = 15
        textures = { all = "lava" }
        [[block]]
        name = "glass"
        layer = "cutout"
        textures = { all = "glass" }
    "#;

    fn setup() -> (BlockRegistry, LightTable) {
        let reg = BlockRegistry::from_toml(BLOCKS).unwrap();
        let t = LightTable::new(&reg);
        (reg, t)
    }

    /// A 3×3 neighbourhood of identical columns built by `f(column, world x, y, z)`.
    fn world(f: impl Fn(&mut ChunkColumn, i32, i32, i32)) -> Vec<ChunkColumn> {
        let mut cols = Vec::new();
        for dz in -1..=1 {
            for dx in -1..=1 {
                let mut c = ChunkColumn::new(ChunkPos::new(dx, dz));
                for lz in 0..16 {
                    for lx in 0..16 {
                        for y in MIN_Y..MAX_Y {
                            f(&mut c, dx * 16 + lx, y, dz * 16 + lz);
                        }
                    }
                }
                c.compact();
                cols.push(c);
            }
        }
        cols
    }

    fn region(cols: &[ChunkColumn], section_index: usize, t: &LightTable) -> LightRegion {
        let refs: [[Option<&ChunkColumn>; 3]; 3] =
            std::array::from_fn(|z| std::array::from_fn(|x| Some(&cols[z * 3 + x])));
        LightRegion::around_section(refs, section_index, t)
    }

    /// Section index containing world y.
    fn sec(y: i32) -> usize {
        ((y - MIN_Y) / 16) as usize
    }

    #[test]
    fn open_sky_is_15_and_rock_is_dark() {
        let (reg, t) = setup();
        let stone = reg.id("stone").unwrap();
        // Ground: solid up to y = 63.
        let cols = world(|c, x, y, z| {
            if y < 64 {
                c.set(x.rem_euclid(16) as usize, y, z.rem_euclid(16) as usize, stone);
            }
        });
        let r = region(&cols, sec(64), &t); // section 64..80
        assert_eq!(r.get(5, 0, 5), Some((15, 0)), "on the ground");
        assert_eq!(r.get(5, 10, 5), Some((15, 0)), "in the air");
        assert_eq!(r.get(5, -1, 5), Some((0, 0)), "inside the ground");
        let deep = region(&cols, sec(0), &t);
        assert_eq!(deep.get(8, 8, 8), Some((0, 0)));
    }

    #[test]
    fn light_fades_under_a_roof_and_pours_down_a_shaft() {
        let (reg, t) = setup();
        let stone = reg.id("stone").unwrap();
        // Floor at y=63, a stone roof at y=68 covering x,z in 0..16 except a
        // 1×1 hole at (8, 8).
        let cols = world(|c, x, y, z| {
            let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            let under_roof = (0..16).contains(&x) && (0..16).contains(&z);
            if y < 64 || (y == 68 && under_roof && !(x == 8 && z == 8)) {
                c.set(lx, y, lz, stone);
            }
        });
        let r = region(&cols, sec(64), &t);
        // Below the hole: full sunlight straight down.
        assert_eq!(r.get(8, 0, 8).unwrap().0, 15);
        // One step sideways from the shaft: 14, then 13…
        assert_eq!(r.get(9, 0, 8).unwrap().0, 14);
        assert_eq!(r.get(10, 0, 8).unwrap().0, 13);
        // Near the roof's edge, light from outside comes in.
        assert_eq!(r.get(15, 0, 0).unwrap().0, 14, "next to the uncovered side (x = 16)");
    }

    #[test]
    fn leaves_dim_sunlight() {
        let (reg, t) = setup();
        let (stone, leaves) = (reg.id("stone").unwrap(), reg.id("leaves").unwrap());
        let cols = world(|c, x, y, z| {
            let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            if y < 64 {
                c.set(lx, y, lz, stone);
            } else if (66..69).contains(&y) {
                c.set(lx, y, lz, leaves); // a 3-thick leaf canopy everywhere
            }
        });
        let r = region(&cols, sec(64), &t);
        assert_eq!(r.get(5, 6, 5).unwrap().0, 15, "above the canopy");
        // Each leaf block costs 1 (15 → 12 after three), and below the canopy
        // light keeps fading by 1 per block: only full-strength sky light
        // falls without loss.
        let below = r.get(5, 1, 5).unwrap().0;
        assert!((10..=12).contains(&below), "under 3 leaves: {below}");
    }

    #[test]
    fn lava_lights_its_surroundings() {
        let (reg, t) = setup();
        let (stone, lava) = (reg.id("stone").unwrap(), reg.id("lava").unwrap());
        // Solid rock with a 5×5×5 air pocket and lava in its middle, deep underground.
        let cols = world(|c, x, y, z| {
            let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            let pocket = (6..11).contains(&x) && (6..11).contains(&z) && (2..7).contains(&y);
            if x == 8 && z == 8 && y == 4 {
                c.set(lx, y, lz, lava);
            } else if !pocket {
                c.set(lx, y, lz, stone);
            }
        });
        let r = region(&cols, sec(4), &t); // section y 0..16
        assert_eq!(r.get(8, 4, 8), Some((0, 15)), "the lava itself");
        assert_eq!(r.get(9, 4, 8), Some((0, 14)));
        assert_eq!(r.get(10, 4, 10), Some((0, 11)), "4 steps away");
        assert_eq!(r.get(12, 4, 8), Some((0, 0)), "behind the rock wall");
    }

    #[test]
    fn region_from_the_map_matches_the_mesher_path() {
        let (reg, t) = setup();
        let stone = reg.id("stone").unwrap();
        let cols = world(|c, x, y, z| {
            let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            if y < 64 || (y == 68 && (0..16).contains(&x) && (0..16).contains(&z) && !(x == 8 && z == 8)) {
                c.set(lx, y, lz, stone);
            }
        });
        let mut map = ChunkMap::new();
        for c in cols.clone() {
            map.insert(c);
        }
        let section = region(&cols, sec(64), &t);
        let point = LightRegion::around_block(&map, BlockPos::new(10, 64, 8), 15, &t);
        assert_eq!(point.get(10, 64, 8), section.get(10, 0, 8));
    }

    #[test]
    fn one_box_for_several_sections_matches_one_box_each() {
        let (reg, t) = setup();
        let (stone, lava) = (reg.id("stone").unwrap(), reg.id("lava").unwrap());
        let cols = world(|c, x, y, z| {
            let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            if (y < 64 && !(y > 40 && x == 8)) || (y == 70 && x > 3) {
                c.set(lx, y, lz, stone);
            }
            if (x, y, z) == (8, 41, 8) {
                c.set(lx, y, lz, lava);
            }
        });
        let refs: [[Option<&ChunkColumn>; 3]; 3] =
            std::array::from_fn(|z| std::array::from_fn(|x| Some(&cols[z * 3 + x])));
        let (lo, hi) = (sec(32), sec(80));
        let big = LightRegion::around_sections(refs, lo, hi, &t);
        for k in lo..=hi {
            let single = LightRegion::around_section(refs, k, &t);
            let view = big.view(16 * (k - lo) as i32);
            for (x, y, z) in [(8, 5, 8), (8, 9, 3), (2, 0, 15), (9, 15, 8), (0, 8, 0)] {
                assert_eq!(view.get(x, y, z), single.get(x, y, z), "section {k} at {x},{y},{z}");
            }
            assert_eq!(view.section_light(), single.section_light(0, 0, 0), "section {k}");
        }
    }

    #[test]
    fn section_light_compresses_uniform_sky() {
        let (_, t) = setup();
        let cols = world(|_, _, _, _| {});
        let r = region(&cols, sec(200), &t);
        assert_eq!(r.section_light(0, 0, 0), SectionLight::Uniform(pack(15, 0)));
        assert_eq!(unpack(pack(9, 4)), (9, 4));
    }
}
