//! World visibility (`world_visibility.vvis_c`): the voxel octree's clusters and the PVS rows
//! between them, as `IPVS` queries need. Layout and lookups after VRF's `VoxelVisibility`; see
//! research/world.md. Older layouts (region boxes, BSP) and octrees of two clusters or fewer
//! cull nothing, so they read as `None`.

use crate::Error;
use crate::values::{field_f32, field_floats, number};
use assets::container::block;
use kv::Object;

const MIN_CULLING_CLUSTERS: u32 = 2;
/// Queries grow by this so a box flush against a cell edge still reaches the far side.
const EPSILON: f32 = 1.0 / 32.0;
/// A box this big on more than one axis takes enclosed nodes' cluster lists whole.
const LARGE_BOX: f32 = 1024.0;
const SUB_GRID_1: [u32; 8] = [0, 2, 8, 10, 32, 34, 40, 42];
const SUB_GRID_2: [u32; 8] = [0, 1, 4, 5, 16, 17, 20, 21];

type V3 = [f32; 3];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Visibility {
    pub clusters: u32,
    pub row_bytes: u32,
    pub min: V3,
    pub max: V3,
    pub grid: f32,
    pub sky: u32,
    pub sun: u32,
    /// (first, second): leaf = first & 1, offset = first >> 1; region count = second & 0xFF,
    /// enclosed list = second >> 8 (0xFFFFFF: none).
    pub nodes: Vec<(u32, u32)>,
    /// cluster = bits 0..15, leaf = 16..40, mask index = 40..
    pub regions: Vec<u64>,
    pub enclosed_lists: Vec<(i32, i32)>,
    pub enclosed: Vec<u16>,
    /// 4×4×4 sub-cell occupancy of a leaf: cell (x, y, z) is bit x + 4y + 16z.
    pub masks: Vec<u64>,
    /// `row_bytes` per cluster, then the sky and sun rows at their cluster indices.
    pub rows: Vec<u8>,
}

/// The `{m_nOffset, m_nElementCount}` sub-blocks, in file order, with each element's size.
pub const BLOCKS: [(&str, usize); 6] =
    [("m_NodeBlock", 8), ("m_RegionBlock", 8), ("m_EnclosedClusterListBlock", 8), ("m_EnclosedClustersBlock", 2), ("m_MasksBlock", 8), ("m_nVisBlocks", 1)];

fn split(bytes: &[u8], size: usize) -> impl Iterator<Item = &[u8]> {
    bytes.chunks_exact(size)
}

fn u32_of(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}

impl Visibility {
    /// The octree in a `.vvis_c`, `None` when it has nothing that culls.
    pub fn parse(file: &[u8]) -> Result<Option<Visibility>, Error> {
        let data = crate::resource::data(file)?;
        let o = data.as_object().ok_or_else(|| Error::Format("vvis DATA isn't an object".into()))?;
        if o.get("m_nBaseClusterCount").is_none() {
            return Ok(None);
        }
        let vxvs = block(file, b"VXVS").map_err(Error::Container)?.ok_or_else(|| Error::Format("no VXVS block".into()))?;
        let mut parts = Vec::new();
        for (name, size) in BLOCKS {
            let b = o.get(name).and_then(|v| v.as_object()).ok_or_else(|| Error::Format(format!("no {name}")))?;
            let int = |k: &str| b.get(k).and_then(number).unwrap_or(0.0) as usize;
            let (at, n) = (int("m_nOffset"), int("m_nElementCount"));
            parts.push(vxvs.get(at..at + n * size).ok_or_else(|| Error::Format(format!("{name} past the VXVS block")))?);
        }
        let int = |k: &str| o.get(k).and_then(number).unwrap_or(0.0) as u32;
        let v = Visibility {
            clusters: int("m_nBaseClusterCount"),
            row_bytes: int("m_nPVSBytesPerCluster"),
            min: field_floats(o, "m_vMinBounds").unwrap_or_default(),
            max: field_floats(o, "m_vMaxBounds").unwrap_or_default(),
            grid: field_f32(o, "m_flGridSize").unwrap_or(0.0),
            sky: int("m_nSkyVisibilityCluster"),
            sun: int("m_nSunVisibilityCluster"),
            nodes: split(parts[0], 8).map(|b| (u32_of(b), u32_of(&b[4..]))).collect(),
            regions: split(parts[1], 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).collect(),
            enclosed_lists: split(parts[2], 8).map(|b| (u32_of(b) as i32, u32_of(&b[4..]) as i32)).collect(),
            enclosed: split(parts[3], 2).map(|b| u16::from_le_bytes(b.try_into().unwrap())).collect(),
            masks: split(parts[4], 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).collect(),
            rows: parts[5].to_vec(),
        };
        let culls = v.clusters > MIN_CULLING_CLUSTERS && v.row_bytes > 0 && !v.nodes.is_empty();
        Ok(culls.then_some(v))
    }

    fn row(&self, cluster: u32) -> Option<&[u8]> {
        let at = cluster as usize * self.row_bytes as usize;
        self.rows.get(at..at + self.row_bytes as usize)
    }

    fn region(&self, i: usize) -> Option<(u32, u64)> {
        let r = *self.regions.get(i)?;
        Some(((r & 0x7FFF) as u32, *self.masks.get((r >> 40) as usize)?))
    }

    /// The leaf holding `p` and its bounds.
    fn leaf(&self, p: V3) -> Option<(usize, V3, V3)> {
        let (mut lo, mut hi) = (self.min, self.max);
        if (0..3).any(|a| p[a] < lo[a] || p[a] > hi[a]) {
            return None;
        }
        let mut i = 0;
        while self.nodes.get(i)?.0 & 1 == 0 {
            let octant = halve(p, &mut lo, &mut hi);
            i = (self.nodes[i].0 >> 1) as usize + octant;
        }
        Some((i, lo, hi))
    }

    /// The regions of `p`'s leaf whose sub-cells hold it.
    fn regions_at(&self, p: V3) -> Vec<u32> {
        let Some((leaf, mut lo, mut hi)) = self.leaf(p) else { return Vec::new() };
        let mask = 1u64 << (SUB_GRID_1[halve(p, &mut lo, &mut hi)] + SUB_GRID_2[halve(p, &mut lo, &mut hi)]);
        let (first, second) = self.nodes[leaf];
        let start = (first >> 1) as usize;
        (start..start + (second & 0xFF) as usize).filter_map(|i| self.region(i)).filter(|&(_, m)| m & mask != 0).map(|(c, _)| c).collect()
    }

    /// `p`'s cluster, else the first within half a grid cell of it.
    pub fn cluster_at(&self, p: V3) -> Option<u32> {
        if let Some(&c) = self.regions_at(p).first() {
            return Some(c);
        }
        let h = self.grid * 0.5;
        self.clusters_in_box([p[0] - h, p[1] - h, p[2] - h], [p[0] + h, p[1] + h, p[2] + h]).into_iter().next()
    }

    /// The clusters visible from `p`: its regions' rows ORed; every cluster when none claims it.
    pub fn pvs_at(&self, p: V3) -> Vec<u8> {
        let mut out: Option<Vec<u8>> = None;
        for row in self.regions_at(p).into_iter().filter(|&c| c < self.clusters).filter_map(|c| self.row(c)) {
            match &mut out {
                Some(acc) => acc.iter_mut().zip(row).for_each(|(a, b)| *a |= b),
                None => out = Some(row.to_vec()),
            }
        }
        out.unwrap_or_else(|| vec![0xFF; self.row_bytes as usize])
    }

    /// Every cluster the box overlaps; cluster 0 (in every row) where the octree can't answer.
    pub fn clusters_in_box(&self, a: V3, b: V3) -> Vec<u32> {
        let lo: V3 = std::array::from_fn(|i| a[i].min(b[i]) - EPSILON);
        let hi: V3 = std::array::from_fn(|i| a[i].max(b[i]) + EPSILON);
        if (0..3).all(|i| lo[i] <= self.min[i] && hi[i] >= self.max[i]) {
            return vec![0];
        }
        let large = (0..3).filter(|&i| hi[i] - lo[i] >= LARGE_BOX).count() > 1;
        let mut q = BoxQuery { vis: self, lo, hi, large, out: Vec::new() };
        q.visit(0, self.min, self.max);
        q.out.sort_unstable();
        q.out.dedup();
        q.out
    }

    fn sees(row: &[u8], cluster: u32) -> bool {
        row.get(cluster as usize / 8).is_some_and(|b| b & (1 << (cluster % 8)) != 0)
    }

    /// Some source sees `p` (a point in no cluster counts as seen).
    pub fn point_visible(&self, sources: &[V3], p: V3) -> bool {
        let Some(c) = self.cluster_at(p) else { return true };
        sources.iter().any(|&s| Self::sees(&self.pvs_at(s), c))
    }

    pub fn box_visible(&self, sources: &[V3], lo: V3, hi: V3) -> bool {
        let clusters = self.clusters_in_box(lo, hi);
        sources.iter().any(|&s| {
            let row = self.pvs_at(s);
            clusters.iter().any(|&c| Self::sees(&row, c))
        })
    }

    /// The sky (or, `sun`, sunlight) reaches `p`'s cluster; true when there's no such row.
    pub fn sky_visible(&self, p: V3, sun: bool) -> bool {
        let Some(row) = self.row(if sun { self.sun } else { self.sky }) else { return true };
        self.cluster_at(p).is_none_or(|c| Self::sees(row, c))
    }
}

/// Step into the octant of `p` (x 1, y 2, z 4 set when past the middle), narrowing the bounds.
fn halve(p: V3, lo: &mut V3, hi: &mut V3) -> usize {
    let mut octant = 0;
    for a in 0..3 {
        let mid = (lo[a] + hi[a]) * 0.5;
        if mid < p[a] {
            octant |= 1 << a;
            lo[a] = mid;
        } else {
            hi[a] = mid;
        }
    }
    octant
}

struct BoxQuery<'a> {
    vis: &'a Visibility,
    lo: V3,
    hi: V3,
    large: bool,
    out: Vec<u32>,
}

impl BoxQuery<'_> {
    fn visit(&mut self, i: usize, lo: V3, hi: V3) {
        let Some(&(first, second)) = self.vis.nodes.get(i) else { return };
        let list = (second >> 8) as usize;
        if self.large && list != 0xFF_FFFF {
            let Some(&(at, n)) = self.vis.enclosed_lists.get(list) else { return };
            let ids = self.vis.enclosed.iter().skip(at.max(0) as usize).take(n.max(0) as usize);
            self.out.extend(ids.map(|&c| u32::from(c)));
            return;
        }
        if first & 1 != 0 {
            let mask = self.box_mask(lo, hi);
            let start = (first >> 1) as usize;
            let hits = (start..start + (second & 0xFF) as usize).filter_map(|r| self.vis.region(r)).filter(|&(_, m)| m & mask != 0);
            let hits: Vec<u32> = hits.map(|(c, _)| c).collect();
            self.out.extend(hits);
            return;
        }
        let mid: V3 = std::array::from_fn(|a| (lo[a] + hi[a]) * 0.5);
        for octant in 0..8 {
            let high = |a: usize| octant & (1 << a) != 0;
            let reach = |a: usize| if high(a) { self.lo[a] <= hi[a] && mid[a] <= self.hi[a] } else { lo[a] <= self.hi[a] && self.lo[a] <= mid[a] };
            if (0..3).all(reach) {
                let child_lo = std::array::from_fn(|a| if high(a) { mid[a] } else { lo[a] });
                let child_hi = std::array::from_fn(|a| if high(a) { hi[a] } else { mid[a] });
                self.visit((first >> 1) as usize + octant, child_lo, child_hi);
            }
        }
    }

    /// The leaf's sub-cells the box reaches (cells are half open).
    fn box_mask(&self, lo: V3, hi: V3) -> u64 {
        let cell = (hi[0] - lo[0]) * 0.25;
        if cell <= 0.0 {
            return 0;
        }
        let mut mask = 0;
        for bit in 0..64 {
            let at = [bit % 4, (bit / 4) % 4, bit / 16];
            let inside = (0..3).all(|a| {
                let low = lo[a] + cell * at[a] as f32;
                low < self.hi[a] && self.lo[a] < low + cell
            });
            if inside {
                mask |= 1u64 << bit;
            }
        }
        mask
    }
}

/// The `.vvis_c` holding `v`: KV3 DATA naming each sub-block's place in the VXVS block.
pub fn write(v: &Visibility) -> Vec<u8> {
    let mut vxvs = Vec::new();
    let mut fields: Vec<(&str, kv::Value)> = Vec::new();
    let parts: [(Vec<u8>, usize); 6] = [
        (v.nodes.iter().flat_map(|(a, b)| [a.to_le_bytes(), b.to_le_bytes()].concat()).collect(), v.nodes.len()),
        (v.regions.iter().flat_map(|r| r.to_le_bytes()).collect(), v.regions.len()),
        (v.enclosed_lists.iter().flat_map(|(a, b)| [a.to_le_bytes(), b.to_le_bytes()].concat()).collect(), v.enclosed_lists.len()),
        (v.enclosed.iter().flat_map(|c| c.to_le_bytes()).collect(), v.enclosed.len()),
        (v.masks.iter().flat_map(|m| m.to_le_bytes()).collect(), v.masks.len()),
        (v.rows.clone(), v.rows.len()),
    ];
    for ((name, _), (bytes, count)) in BLOCKS.iter().zip(parts) {
        let at = kv::Value::Int(vxvs.len() as i64);
        fields.push((name, obj(vec![("m_nOffset", at), ("m_nElementCount", kv::Value::Int(count as i64))])));
        vxvs.extend(bytes);
    }
    let vec3 = |p: V3| kv::Value::Array(p.iter().map(|&x| kv::Value::Double(x.into())).collect());
    let int = |n: u32| kv::Value::Int(n.into());
    fields.extend([
        ("m_nBaseClusterCount", int(v.clusters)),
        ("m_nPVSBytesPerCluster", int(v.row_bytes)),
        ("m_vMinBounds", vec3(v.min)),
        ("m_vMaxBounds", vec3(v.max)),
        ("m_flGridSize", kv::Value::Double(v.grid.into())),
        ("m_nSkyVisibilityCluster", int(v.sky)),
        ("m_nSunVisibilityCluster", int(v.sun)),
    ]);
    let mut file = crate::resource::write(&obj(fields), assets::kv3bin::Compression::None);
    let mut c = assets::container::Container::read(&file).expect("just written");
    c.blocks.push((*b"VXVS", vxvs));
    file = c.write();
    file
}

fn obj(fields: Vec<(&str, kv::Value)>) -> kv::Value {
    kv::Value::Object(Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}
