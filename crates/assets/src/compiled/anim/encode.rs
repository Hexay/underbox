//! Writing animations for fixtures: an `AnimationResourceData_t` and its decode key, and the
//! group, `.vanim_c` and sequence data around them. Static bones go in static segments, moving
//! positions as base + half deltas, moving rotations as 48-bit quaternions, scale and morphs
//! as full floats, split into frame blocks of `block` frames.

use super::layout::{self, ANGLE, MORPH, POSITION, SCALE};
use super::segment::{Decoder, encode_quaternion, half3_bytes};
use super::{Animation, Key, Rig};
use crate::compiled::kv::{int_value, list, obj, text};
use ::kv::Value;

pub use layout::sequences;

const DECODERS: [(Decoder, i64); 5] = [(Decoder::StaticFullVector3, 3), (Decoder::StaticQuaternion, 4), (Decoder::DeltaVector3, 3), (Decoder::AnimQuaternion, 4), (Decoder::FullFloat, 1)];

fn floats(v: &[f32]) -> impl Iterator<Item = u8> + '_ {
    v.iter().flat_map(|f| f.to_le_bytes())
}

/// Segments of one animation's frame blocks, appended to the shared `m_segmentArray`.
struct Segments<'a> {
    out: &'a mut Vec<Value>,
    bones: usize,
    controllers: usize,
}

impl Segments<'_> {
    /// Pushes a segment keying `targets` (bone or controller indices) of `channel`; its index.
    fn push(&mut self, channel: i64, decoder: Decoder, cardinality: i16, targets: &[usize], data: &[u8]) -> i64 {
        let index = DECODERS.iter().position(|(d, _)| *d == decoder).expect("writer decoder") as i16;
        let length = (8 + targets.len() * 2 + data.len()).min(i16::MAX as usize) as i16;
        let mut bytes: Vec<u8> = [index, cardinality, targets.len() as i16, length].iter().flat_map(|v| v.to_le_bytes()).collect();
        bytes.extend(targets.iter().flat_map(|&t| (layout::element(channel, t, self.bones) as i16).to_le_bytes()));
        bytes.extend_from_slice(data);
        let at = self.out.len() as i64;
        let masks = targets.iter().fold(0, |m, &t| m | layout::mask(t));
        let fields = [("m_nUniqueFrameIndex", int_value(at)), ("m_nLocalElementMasks", int_value(masks)), ("m_nLocalChannel", int_value(channel)), ("m_container", Value::Binary(bytes))];
        self.out.push(obj(fields.into()));
        at
    }
}

/// The segments for frames `range` of `a`; their indices.
fn block_segments(a: &Animation, base: &[Key], range: std::ops::RangeInclusive<usize>, out: &mut Segments) -> Vec<i64> {
    let frames = &a.frames[range.clone()];
    let bones = 0..base.len();
    let keyed = |f: &dyn Fn(&Key, &Key) -> bool| -> Vec<usize> { bones.clone().filter(|&b| a.frames.iter().any(|fr| f(&fr[b], &base[b]))).collect() };
    let moving = |b: usize, f: &dyn Fn(&Key, &Key) -> bool| frames.iter().any(|fr| f(&fr[b], &frames[0][b]));
    let pos_differs = |x: &Key, y: &Key| x.position != y.position;
    let rot_differs = |x: &Key, y: &Key| x.rotation != y.rotation;
    let mut segs = Vec::new();
    let (positions, rotations) = (keyed(&pos_differs), keyed(&rot_differs));
    let (still, moved): (Vec<usize>, Vec<usize>) = positions.iter().partition(|&&b| !moving(b, &pos_differs));
    if !still.is_empty() {
        let data: Vec<u8> = still.iter().flat_map(|&b| floats(&frames[0][b].position).collect::<Vec<_>>()).collect();
        segs.push(out.push(POSITION, Decoder::StaticFullVector3, 3, &still, &data));
    }
    if !moved.is_empty() {
        let mut data: Vec<u8> = moved.iter().flat_map(|&b| floats(&frames[0][b].position).collect::<Vec<_>>()).collect();
        for fr in frames {
            data.extend(moved.iter().flat_map(|&b| half3_bytes(std::array::from_fn(|i| fr[b].position[i] - frames[0][b].position[i]))));
        }
        segs.push(out.push(POSITION, Decoder::DeltaVector3, 3, &moved, &data));
    }
    let (still, moved): (Vec<usize>, Vec<usize>) = rotations.iter().partition(|&&b| !moving(b, &rot_differs));
    if !still.is_empty() {
        let data: Vec<u8> = still.iter().flat_map(|&b| encode_quaternion(frames[0][b].rotation)).collect();
        segs.push(out.push(ANGLE, Decoder::StaticQuaternion, 4, &still, &data));
    }
    if !moved.is_empty() {
        let data: Vec<u8> = frames.iter().flat_map(|fr| moved.iter().flat_map(|&b| encode_quaternion(fr[b].rotation))).collect();
        segs.push(out.push(ANGLE, Decoder::AnimQuaternion, 4, &moved, &data));
    }
    let scaled = keyed(&|x: &Key, y: &Key| x.scale != y.scale);
    if !scaled.is_empty() {
        let data: Vec<u8> = frames.iter().flat_map(|fr| scaled.iter().flat_map(|&b| fr[b].scale.to_le_bytes())).collect();
        segs.push(out.push(SCALE, Decoder::FullFloat, 1, &scaled, &data));
    }
    // Without controllers the decode key has no morph channel, and real ignores the animation ("mismatched anim").
    if let Some(first) = a.morphs.first().filter(|_| out.controllers > 0) {
        let all: Vec<usize> = (0..first.len()).collect();
        let data: Vec<u8> = a.morphs[range].iter().flat_map(|m| floats(m).collect::<Vec<_>>()).collect();
        segs.push(out.push(MORPH, Decoder::FullFloat, 1, &all, &data));
    }
    segs
}

/// (`AnimationResourceData_t`, `m_decodeKey`) for `anims` over `rig`.
pub fn write(anims: &[Animation], rig: &Rig, block: usize) -> (Value, Value) {
    let mut segments = Vec::new();
    let mut out = Segments { out: &mut segments, bones: rig.bones.len(), controllers: rig.flex.len() };
    let descs: Vec<Value> = anims
        .iter()
        .map(|a| {
            let base: Vec<Key> = rig.bones.iter().map(|b| if a.delta { Key::IDENTITY } else { Key::bind(b) }).collect();
            let blocks: Vec<Value> = (0..a.frames.len())
                .step_by(block.max(1))
                .map(|s| {
                    let end = (s + block.max(1) - 1).min(a.frames.len() - 1);
                    let segs = block_segments(a, &base, s..=end, &mut out);
                    obj(vec![("m_nStartFrame", int_value(s as i64)), ("m_nEndFrame", int_value(end as i64)), ("m_segmentIndexArray", list(segs.into_iter().map(int_value)))])
                })
                .collect();
            layout::desc(a, blocks, rig.bones.len())
        })
        .collect();
    let decoders = DECODERS.iter().map(|(d, ty)| obj(vec![("m_szName", text(d.name())), ("m_nVersion", int_value(0)), ("m_nType", int_value(*ty))]));
    let count = segments.len() as i64;
    let fields = [("m_name", text("")), ("m_animArray", list(descs)), ("m_decoderArray", list(decoders)), ("m_nMaxUniqueFrameIndex", int_value(count)), ("m_segmentArray", list(segments))];
    (obj(fields.into()), layout::decode_key(rig))
}

/// An animation group: DATA with `key` and `anims` (`.vanim` resource paths), plus `anim` as its ANIM block.
pub fn group_file(key: &Value, anim: Option<&Value>, anims: &[&str]) -> Vec<u8> {
    let mut blocks = vec![(*b"DATA", crate::compiled::kv3(&layout::group(key.clone(), anims)))];
    blocks.extend(anim.map(|a| (*b"ANIM", crate::compiled::kv3(a))));
    crate::compiled::write(0, anims, blocks)
}

/// A `.vanim_c`: `data` as its DATA.
pub fn anim_file(data: &Value) -> Vec<u8> {
    crate::compiled::write(0, &[], vec![(*b"DATA", crate::compiled::kv3(data))])
}
