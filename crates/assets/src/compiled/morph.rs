//! Morph sets (`MorphSetData_t`: a model's MRPH block or a `.vmorf_c`). Each morph target's
//! per-vertex deltas live in a texture atlas: rects of quantized texels, one bundle per kind
//! (position/speed, normal/wrinkle), placed on a `width`-wide grid whose cell `y * width + x`
//! is mesh vertex `y * width + x`. [`MorphSet::targets`] decodes them to sparse deltas.

use super::flex::{self, Controller, Rule};
use super::kv::{self, float, floats, int, int_value, list, obj, objects, resource, string, text};
use super::texture::{Image, Pixels};
use ::kv::{Object, Value};

pub const POSITION_SPEED: i64 = 1;
pub const NORMAL_WRINKLE: i64 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Bundle {
    /// Top-left in the atlas, 0..1.
    pub uv: [f32; 2],
    pub ranges: [f32; 4],
    pub offsets: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rect {
    /// Top-left on the vertex grid.
    pub dst: [i64; 2],
    /// Size in the atlas, 0..1.
    pub size: [f32; 2],
    /// One per `MorphSet::bundle_types` entry.
    pub bundles: Vec<Bundle>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MorphSet {
    pub width: usize,
    pub height: usize,
    pub bundle_types: Vec<i64>,
    /// (target name, its rects).
    pub morphs: Vec<(String, Vec<Rect>)>,
    /// Flex descriptor names: rule outputs, matched to targets by name.
    pub descriptors: Vec<String>,
    pub controllers: Vec<Controller>,
    pub rules: Vec<Rule>,
    /// The atlas `.vtex`.
    pub atlas: String,
}

/// A morph target's deltas: (vertex, position delta, normal delta), nonzero vertices only.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Target {
    pub name: String,
    pub deltas: Vec<(u32, [f32; 3], [f32; 3])>,
}

fn bundle_type(v: &Value) -> i64 {
    match v.as_str() {
        Some(s) if s.ends_with("POSITION_SPEED") => POSITION_SPEED,
        Some(s) if s.ends_with("NORMAL_WRINKLE") => NORMAL_WRINKLE,
        Some(_) => 0,
        None => kv::int_of(v).unwrap_or(0),
    }
}

pub fn read(o: &Object) -> MorphSet {
    let rect = |r: &Object| Rect {
        dst: [int(r, "m_nXLeftDst"), int(r, "m_nYTopDst")],
        size: [float(r, "m_flUWidthSrc"), float(r, "m_flVHeightSrc")],
        bundles: objects(r, "m_bundleDatas").map(|b| Bundle { uv: [float(b, "m_flULeftSrc"), float(b, "m_flVTopSrc")], ranges: kv::vec4(b, "m_ranges"), offsets: kv::vec4(b, "m_offsets") }).collect(),
    };
    MorphSet {
        width: int(o, "m_nWidth").max(0) as usize,
        height: int(o, "m_nHeight").max(0) as usize,
        bundle_types: kv::array(o, "m_bundleTypes").iter().map(bundle_type).collect(),
        morphs: objects(o, "m_morphDatas").filter(|m| !string(m, "m_name").is_empty()).map(|m| (string(m, "m_name"), objects(m, "m_morphRectDatas").map(rect).collect())).collect(),
        descriptors: objects(o, "m_FlexDesc").map(|d| string(d, "m_szFacs")).collect(),
        controllers: flex::controllers(o),
        rules: flex::rules(o),
        atlas: string(o, "m_pTextureAtlas"),
    }
}

impl MorphSet {
    /// Each target's deltas from the decoded atlas (mip 0, RGBA8).
    pub fn targets(&self, atlas: &Image) -> Vec<Target> {
        let Pixels::Rgba8(px) = &atlas.pixels else { return Vec::new() };
        let (tw, th) = (atlas.width as f32, atlas.height as f32);
        let cells = self.width * self.height;
        self.morphs
            .iter()
            .map(|(name, rects)| {
                let (mut pos, mut nrm) = (vec![[0.0f32; 3]; cells], vec![[0.0f32; 3]; cells]);
                for r in rects {
                    let (w, h) = ((r.size[0] * tw).round() as i64, (r.size[1] * th).round() as i64);
                    for (b, kind) in r.bundles.iter().zip(&self.bundle_types) {
                        let out = match *kind {
                            POSITION_SPEED => &mut pos,
                            NORMAL_WRINKLE => &mut nrm,
                            _ => continue,
                        };
                        let (u, v) = ((b.uv[0] * tw).round() as i64, (b.uv[1] * th).round() as i64);
                        for (row, col) in (0..h).flat_map(|y| (0..w).map(move |x| (y, x))) {
                            let (dy, dx) = (row + r.dst[1], col + r.dst[0]);
                            let texel = (v + row) * atlas.width as i64 + u + col;
                            // Rects that run past the grid are clipped, not wrapped.
                            if dx < 0 || dy < 0 || dx as usize >= self.width || dy as usize >= self.height || texel < 0 || (texel as usize + 1) * 4 > px.len() {
                                continue;
                            }
                            let t = &px[texel as usize * 4..texel as usize * 4 + 3];
                            out[dy as usize * self.width + dx as usize] = std::array::from_fn(|c| {
                                let value = f32::from(t[c]) / 255.0 * b.ranges[c] + b.offsets[c];
                                if value.abs() < b.ranges[c].abs() / 510.0 { 0.0 } else { value }
                            });
                        }
                    }
                }
                let deltas = (0..cells).filter(|&i| pos[i] != [0.0; 3] || nrm[i] != [0.0; 3]).map(|i| (i as u32, pos[i], nrm[i])).collect();
                Target { name: name.clone(), deltas }
            })
            .collect()
    }

    /// Each descriptor's weight for controller `values`.
    pub fn weights(&self, values: &[f32]) -> Vec<f32> {
        flex::evaluate(&self.rules, &self.controllers, &self.descriptors, values)
    }
}

/// Quantize `targets` for `vertices` vertices into a set and its RGBA8 atlas (width × height texels):
/// each target one rect over the whole grid, position rows then normal rows.
pub fn encode(targets: &[Target], vertices: usize, controllers: Vec<Controller>, rules: Vec<Rule>, atlas: &str) -> (MorphSet, usize, usize, Vec<u8>) {
    let width = vertices.clamp(1, 64);
    let height = vertices.div_ceil(width).max(1);
    let (tw, th) = (width, height * 2 * targets.len().max(1));
    let mut px = vec![0u8; tw * th * 4];
    let mut morphs = Vec::new();
    for (k, t) in targets.iter().enumerate() {
        let mut bundles = Vec::new();
        for (slot, pick) in [(0, 0usize), (1, 1)] {
            let value = |d: &(u32, [f32; 3], [f32; 3])| if pick == 0 { d.1 } else { d.2 };
            let lo: [f32; 3] = std::array::from_fn(|c| t.deltas.iter().map(|d| value(d)[c]).fold(0.0, f32::min));
            let hi: [f32; 3] = std::array::from_fn(|c| t.deltas.iter().map(|d| value(d)[c]).fold(0.0, f32::max));
            let range: [f32; 3] = std::array::from_fn(|c| hi[c] - lo[c]);
            let top = (k * 2 + slot) * height;
            let zero: [u8; 3] = std::array::from_fn(|c| if range[c] > 0.0 { (-lo[c] / range[c] * 255.0).round() as u8 } else { 0 });
            for i in 0..width * height {
                let at = ((top + i / width) * tw + i % width) * 4;
                px[at..at + 3].copy_from_slice(&zero);
            }
            for d in &t.deltas {
                let (i, v) = (d.0 as usize, value(d));
                let at = ((top + i / width) * tw + i % width) * 4;
                for c in 0..3 {
                    px[at + c] = if range[c] > 0.0 { ((v[c] - lo[c]) / range[c] * 255.0).round() as u8 } else { 0 };
                }
            }
            bundles.push(Bundle { uv: [0.0, top as f32 / th as f32], ranges: [range[0], range[1], range[2], 0.0], offsets: [lo[0], lo[1], lo[2], 0.0] });
        }
        morphs.push((t.name.clone(), vec![Rect { dst: [0, 0], size: [1.0, height as f32 / th as f32], bundles }]));
    }
    let descriptors = targets.iter().map(|t| t.name.clone()).collect();
    let set = MorphSet { width, height, bundle_types: vec![POSITION_SPEED, NORMAL_WRINKLE], morphs, descriptors, controllers, rules, atlas: atlas.to_owned() };
    (set, tw, th, px)
}

pub fn write(m: &MorphSet) -> Value {
    let bundle = |b: &Bundle| obj(vec![("m_flULeftSrc", Value::Double(b.uv[0].into())), ("m_flVTopSrc", Value::Double(b.uv[1].into())), ("m_ranges", floats(&b.ranges)), ("m_offsets", floats(&b.offsets))]);
    let rect = |r: &Rect| {
        obj(vec![
            ("m_nXLeftDst", int_value(r.dst[0])),
            ("m_nYTopDst", int_value(r.dst[1])),
            ("m_flUWidthSrc", Value::Double(r.size[0].into())),
            ("m_flVHeightSrc", Value::Double(r.size[1].into())),
            ("m_bundleDatas", list(r.bundles.iter().map(bundle))),
        ])
    };
    let kind = |k: &i64| text(if *k == POSITION_SPEED { "MORPH_BUNDLE_TYPE_POSITION_SPEED" } else { "MORPH_BUNDLE_TYPE_NORMAL_WRINKLE" });
    obj(vec![
        ("m_nWidth", int_value(m.width as i64)),
        ("m_nHeight", int_value(m.height as i64)),
        ("m_bundleTypes", list(m.bundle_types.iter().map(kind))),
        ("m_morphDatas", list(m.morphs.iter().map(|(n, rects)| obj(vec![("m_name", text(n)), ("m_morphRectDatas", list(rects.iter().map(rect)))])))),
        ("m_FlexDesc", list(m.descriptors.iter().map(|d| obj(vec![("m_szFacs", text(d))])))),
        ("m_FlexControllers", flex::write_controllers(&m.controllers)),
        ("m_FlexRules", flex::write_rules(&m.rules)),
        ("m_pTextureAtlas", resource(&m.atlas)),
    ])
}
