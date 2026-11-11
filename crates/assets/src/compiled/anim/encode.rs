//! Writing animations for fixtures: an `AnimationResourceData_t` and its decode key, and the
//! group, `.vanim_c` and sequence data around them. Static bones go in static segments, moving
//! positions as base + half deltas, moving rotations as 48-bit quaternions, scale and morphs
//! as full floats, split into frame blocks of `block` frames.

use super::segment::{Decoder, encode_quaternion, half3_bytes};
use super::{Animation, Key, Rig};
use crate::compiled::kv::{int_value, list, obj, resource, text};
use ::kv::Value;

const POSITION: i64 = 0;
const ANGLE: i64 = 1;
const SCALE: i64 = 2;
const MORPH: i64 = 3;
const DECODERS: [Decoder; 5] = [Decoder::StaticFullVector3, Decoder::StaticQuaternion, Decoder::DeltaVector3, Decoder::AnimQuaternion, Decoder::FullFloat];

fn container(decoder: Decoder, cardinality: i16, elements: &[usize], data: &[u8]) -> Vec<u8> {
    let index = DECODERS.iter().position(|d| *d == decoder).expect("writer decoder") as i16;
    let length = (8 + elements.len() * 2 + data.len()).min(i16::MAX as usize) as i16;
    let mut out: Vec<u8> = [index, cardinality, elements.len() as i16, length].iter().flat_map(|v| v.to_le_bytes()).collect();
    out.extend(elements.iter().flat_map(|&e| (e as i16).to_le_bytes()));
    out.extend_from_slice(data);
    out
}

fn segment(channel: i64, bytes: Vec<u8>) -> Value {
    obj(vec![("m_nUniqueFrameIndex", int_value(0)), ("m_container", Value::Binary(bytes)), ("m_nLocalChannel", int_value(channel))])
}

fn floats(v: &[f32]) -> impl Iterator<Item = u8> + '_ {
    v.iter().flat_map(|f| f.to_le_bytes())
}

/// The segments for frames `range` of `a`, appended to `out`; their indices.
fn block_segments(a: &Animation, base: &[Key], range: std::ops::RangeInclusive<usize>, out: &mut Vec<Value>) -> Vec<i64> {
    let frames = &a.frames[range.clone()];
    let bones = 0..base.len();
    let keyed = |f: &dyn Fn(&Key, &Key) -> bool| -> Vec<usize> { bones.clone().filter(|&b| a.frames.iter().any(|fr| f(&fr[b], &base[b]))).collect() };
    let moving = |b: usize, f: &dyn Fn(&Key, &Key) -> bool| frames.iter().any(|fr| f(&fr[b], &frames[0][b]));
    let pos_differs = |x: &Key, y: &Key| x.position != y.position;
    let rot_differs = |x: &Key, y: &Key| x.rotation != y.rotation;
    let mut segs = Vec::new();
    let mut push = |channel, bytes| {
        segs.push(out.len() as i64);
        out.push(segment(channel, bytes));
    };
    let (positions, rotations) = (keyed(&pos_differs), keyed(&rot_differs));
    let (still, moved): (Vec<usize>, Vec<usize>) = positions.iter().partition(|&&b| !moving(b, &pos_differs));
    if !still.is_empty() {
        push(POSITION, container(Decoder::StaticFullVector3, 3, &still, &still.iter().flat_map(|&b| floats(&frames[0][b].position).collect::<Vec<_>>()).collect::<Vec<_>>()));
    }
    if !moved.is_empty() {
        let mut data: Vec<u8> = moved.iter().flat_map(|&b| floats(&frames[0][b].position).collect::<Vec<_>>()).collect();
        for fr in frames {
            data.extend(moved.iter().flat_map(|&b| half3_bytes(std::array::from_fn(|i| fr[b].position[i] - frames[0][b].position[i]))));
        }
        push(POSITION, container(Decoder::DeltaVector3, 3, &moved, &data));
    }
    let (still, moved): (Vec<usize>, Vec<usize>) = rotations.iter().partition(|&&b| !moving(b, &rot_differs));
    if !still.is_empty() {
        push(ANGLE, container(Decoder::StaticQuaternion, 4, &still, &still.iter().flat_map(|&b| encode_quaternion(frames[0][b].rotation)).collect::<Vec<_>>()));
    }
    if !moved.is_empty() {
        let data: Vec<u8> = frames.iter().flat_map(|fr| moved.iter().flat_map(|&b| encode_quaternion(fr[b].rotation))).collect();
        push(ANGLE, container(Decoder::AnimQuaternion, 4, &moved, &data));
    }
    let scaled = keyed(&|x: &Key, y: &Key| x.scale != y.scale);
    if !scaled.is_empty() {
        let data: Vec<u8> = frames.iter().flat_map(|fr| scaled.iter().flat_map(|&b| fr[b].scale.to_le_bytes())).collect();
        push(SCALE, container(Decoder::FullFloat, 1, &scaled, &data));
    }
    if let Some(first) = a.morphs.first() {
        let all: Vec<usize> = (0..first.len()).collect();
        let data: Vec<u8> = a.morphs[range].iter().flat_map(|m| floats(m).collect::<Vec<_>>()).collect();
        push(MORPH, container(Decoder::FullFloat, 1, &all, &data));
    }
    segs
}

/// (`AnimationResourceData_t`, `m_decodeKey`) for `anims` over `rig`.
pub fn write(anims: &[Animation], rig: &Rig, block: usize) -> (Value, Value) {
    let mut segments = Vec::new();
    let descs: Vec<Value> = anims
        .iter()
        .map(|a| {
            let base: Vec<Key> = rig.bones.iter().map(|b| if a.delta { Key::IDENTITY } else { Key::bind(b) }).collect();
            let starts = (0..a.frames.len()).step_by(block.max(1));
            let blocks: Vec<Value> = starts
                .map(|s| {
                    let end = (s + block.max(1) - 1).min(a.frames.len() - 1);
                    let segs = block_segments(a, &base, s..=end, &mut segments);
                    obj(vec![("m_nStartFrame", int_value(s as i64)), ("m_nEndFrame", int_value(end as i64)), ("m_segmentIndexArray", list(segs.into_iter().map(int_value)))])
                })
                .collect();
            let flags = obj(vec![("m_bLooping", Value::Bool(a.looping)), ("m_bDelta", Value::Bool(a.delta)), ("m_bHidden", Value::Bool(a.hidden))]);
            let data = obj(vec![("m_nFrames", int_value(a.frames.len() as i64)), ("m_frameblockArray", list(blocks))]);
            obj(vec![("m_name", text(&a.name)), ("fps", Value::Double(a.fps.into())), ("m_flags", flags), ("m_pData", data), ("m_movementArray", list([]))])
        })
        .collect();
    let decoders = DECODERS.iter().map(|d| obj(vec![("m_szName", text(d.name())), ("m_nVersion", int_value(0)), ("m_nType", int_value(0))]));
    let data = obj(vec![("m_name", text("")), ("m_animArray", list(descs)), ("m_decoderArray", list(decoders)), ("m_segmentArray", list(segments))]);
    let bones: Vec<&str> = rig.bones.iter().map(|b| b.name.as_str()).collect();
    let flex: Vec<&str> = rig.flex.iter().map(String::as_str).collect();
    let ch = |class: &str, var: &str, names: &[&str]| {
        obj(vec![
            ("m_szChannelClass", text(class)),
            ("m_szVariableName", text(var)),
            ("m_szElementNameArray", list(names.iter().map(|n| text(n)))),
            ("m_nElementIndexArray", list((0..names.len()).map(|i| int_value(i as i64)))),
        ])
    };
    let channels = [ch("BoneChannel", "Position", &bones), ch("BoneChannel", "Angle", &bones), ch("BoneChannel", "Scale", &bones), ch("MorphChannel", "data", &flex)];
    (data, obj(vec![("m_dataChannelArray", list(channels)), ("m_userArray", list([]))]))
}

/// An animation group: DATA with `key` and `anims` (`.vanim` resource paths), plus `anim` as its ANIM block.
pub fn group_file(key: &Value, anim: Option<&Value>, anims: &[&str]) -> Vec<u8> {
    let data = obj(vec![("m_name", text("")), ("m_localHAnimArray", list(anims.iter().map(|p| resource(p)))), ("m_decodeKey", key.clone())]);
    let mut blocks = vec![(*b"DATA", crate::compiled::kv3(&data))];
    blocks.extend(anim.map(|a| (*b"ANIM", crate::compiled::kv3(a))));
    crate::compiled::write(0, anims, blocks)
}

/// A `.vanim_c`: `data` as its DATA.
pub fn anim_file(data: &Value) -> Vec<u8> {
    crate::compiled::write(0, &[], vec![(*b"DATA", crate::compiled::kv3(data))])
}

/// A sequence group: each (name, animation it plays, looping, hidden).
pub fn sequences(seqs: &[(&str, &str, bool, bool)]) -> Value {
    let mut names: Vec<&str> = Vec::new();
    let descs: Vec<Value> = seqs
        .iter()
        .map(|&(name, target, looping, hidden)| {
            let i = names.iter().position(|n| *n == target).unwrap_or_else(|| {
                names.push(target);
                names.len() - 1
            });
            let fetch = obj(vec![("m_localReferenceArray", list([int_value(i as i64)]))]);
            obj(vec![("m_sName", text(name)), ("m_flags", obj(vec![("m_bLooping", Value::Bool(looping)), ("m_bHidden", Value::Bool(hidden))])), ("m_fetch", fetch)])
        })
        .collect();
    obj(vec![("m_localSequenceNameArray", list(names.iter().map(|n| text(n)))), ("m_localS1SeqDescArray", list(descs))])
}
