//! Compiled animation (animgraph 1). An `AnimationResourceData_t` (a model's ANIM block, an
//! animation group's ANIM block, or a `.vanim_c` DATA) holds animations as frame blocks of
//! compressed segments ([`segment`]); a group's `m_decodeKey` says which bone or flex controller
//! each segment element is. Sequences (ASEQ) rename and reflag animations. Layout and sources:
//! research/resources.md.

pub mod blend;
pub mod encode;
pub mod segment;

use super::kv::{self, int, objects, string, strings};
use super::model::Bone;
use super::{Error, format};
use ::kv::{Object, Value};
use segment::{Decoder, Value as Element};

/// One bone at one frame, parent-relative.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub position: [f32; 3],
    /// xyzw.
    pub rotation: [f32; 4],
    pub scale: f32,
}

impl Key {
    pub const IDENTITY: Key = Key { position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], scale: 1.0 };

    pub fn bind(b: &Bone) -> Key {
        Key { position: b.position, rotation: b.rotation, scale: 1.0 }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Animation {
    pub name: String,
    pub fps: f32,
    pub looping: bool,
    pub delta: bool,
    pub hidden: bool,
    /// `frames[f][bone]` over every rig bone: unkeyed bones hold the bind pose (identity in a delta).
    pub frames: Vec<Vec<Key>>,
    /// `morphs[f][controller]`; empty when nothing keys a morph.
    pub morphs: Vec<Vec<f32>>,
    pub events: Vec<Event>,
}

/// An `m_eventArray` entry: `AE_FOOTSTEP`, `AE_CL_PLAYSOUND`, `AE_GENERIC_EVENT`... with its
/// KV3 payload (what managed's `SceneModel.OnAnimationEvent` reads).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub frame: i64,
    pub cycle: f32,
    pub name: String,
    pub data: Value,
}

fn events(desc: &Object) -> Vec<Event> {
    let event = |e: &Object| Event {
        frame: int(e, "m_nFrame"),
        cycle: kv::float(e, "m_flCycle"),
        name: string(e, "m_sEventName"),
        data: e.get("m_EventData").cloned().unwrap_or(Value::Null),
    };
    objects(desc, "m_eventArray").map(event).filter(|e| !e.name.is_empty()).collect()
}

/// What animations bind against: the model's bones and flex controller names.
#[derive(Clone, Copy)]
pub struct Rig<'a> {
    pub bones: &'a [Bone],
    pub flex: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attr {
    Position,
    Angle,
    Scale,
    Morph,
    Other,
}

/// A decode-key channel: its attribute and, per bone or controller, its element index.
struct Channel {
    attr: Attr,
    elements: Vec<i64>,
}

fn channel(c: &Object, rig: &Rig) -> Channel {
    let (class, var) = (string(c, "m_szChannelClass"), string(c, "m_szVariableName"));
    let attr = match (class.as_str(), var.as_str()) {
        ("MorphChannel", _) | ("", "data") => Attr::Morph,
        ("BoneChannel" | "", "Position") => Attr::Position,
        ("BoneChannel" | "", "Angle") => Attr::Angle,
        ("BoneChannel" | "", "Scale") => Attr::Scale,
        _ => Attr::Other,
    };
    let targets: Vec<&str> = match attr {
        Attr::Morph => rig.flex.iter().map(String::as_str).collect(),
        Attr::Other => Vec::new(),
        _ => rig.bones.iter().map(|b| b.name.as_str()).collect(),
    };
    let mut elements = vec![-1; targets.len()];
    for (name, index) in strings(c, "m_szElementNameArray").iter().zip(kv::ints(c, "m_nElementIndexArray")) {
        if let Some(t) = targets.iter().position(|t| t.eq_ignore_ascii_case(name)) {
            elements[t] = index;
        }
    }
    Channel { attr, elements }
}

/// A segment: (position in the segment, bone or controller) for each target it keys.
struct Segment {
    decoder: Decoder,
    attr: Attr,
    count: usize,
    wanted: Vec<(usize, usize)>,
    data: Vec<u8>,
}

/// The `m_container` header: decoder, cardinality, element count, length, then the element ids.
fn segment(s: &Object, decoders: &[Option<Decoder>], channels: &[Channel]) -> Option<Segment> {
    let c = match s.get("m_container") {
        Some(Value::Binary(b)) => b.clone(),
        _ => kv::ints(s, "m_container").into_iter().map(|b| b as u8).collect(),
    };
    let i16_at = |i: usize| c.get(i * 2..i * 2 + 2).map(|b| i16::from_le_bytes([b[0], b[1]]));
    let decoder = (*decoders.get(usize::try_from(i16_at(0)?).ok()?)?)?;
    let count = usize::try_from(i16_at(2)?).ok()?;
    let ids: Vec<i16> = (0..count).map(|i| i16_at(4 + i)).collect::<Option<_>>()?;
    let ch = channels.get(usize::try_from(int(s, "m_nLocalChannel")).ok()?)?;
    let wanted = ch.elements.iter().enumerate().filter_map(|(target, &e)| Some((ids.iter().position(|&id| i64::from(id) == e && e >= 0)?, target))).collect();
    Some(Segment { decoder, attr: ch.attr, count, wanted, data: c[8 + count * 2..].to_vec() })
}

fn set(key: &mut Key, attr: Attr, v: Element) {
    match (attr, v) {
        (Attr::Position, Element::Vec3(p)) => key.position = p,
        (Attr::Angle, Element::Quat(q)) => key.rotation = q,
        (Attr::Scale, Element::Float(s)) => key.scale = s,
        _ => {}
    }
}

fn animation(desc: &Object, segments: &[Option<Segment>], rig: &Rig) -> Animation {
    let flags = kv::object(desc, "m_flags");
    let delta = kv::bool(flags, "m_bDelta") || kv::bool(flags, "m_bAnimGraphAdditive");
    let p = desc.get("m_pData").and_then(Value::as_object).unwrap_or(desc);
    let frame_count = usize::try_from(kv::int_or(p, "m_nFrames", int(desc, "numframes"))).unwrap_or(0);
    let blocks: Vec<(usize, usize, Vec<i64>)> = objects(p, "m_frameblockArray")
        .map(|b| (int(b, "m_nStartFrame").max(0) as usize, int(b, "m_nEndFrame").max(0) as usize, kv::ints(b, "m_segmentIndexArray")))
        .collect();
    let base: Vec<Key> = rig.bones.iter().map(|b| if delta { Key::IDENTITY } else { Key::bind(b) }).collect();
    let (mut frames, mut morphs, mut morphed) = (Vec::with_capacity(frame_count), Vec::with_capacity(frame_count), false);
    for f in 0..frame_count {
        let (mut keys, mut weights) = (base.clone(), vec![0.0; rig.flex.len()]);
        for (start, _, segs) in blocks.iter().filter(|(s, e, _)| (*s..=*e).contains(&f)) {
            for s in segs.iter().filter_map(|&i| segments.get(usize::try_from(i).ok()?)?.as_ref()) {
                morphed |= s.attr == Attr::Morph && !s.wanted.is_empty();
                for &(pos, target) in &s.wanted {
                    let Some(v) = s.decoder.read(&s.data, s.count, f - start, pos) else { continue };
                    match (s.attr, v) {
                        (Attr::Morph, Element::Float(m)) => weights.get_mut(target).into_iter().for_each(|w| *w = m),
                        (attr, v) => keys.get_mut(target).into_iter().for_each(|k| set(k, attr, v)),
                    }
                }
            }
        }
        frames.push(keys);
        morphs.push(weights);
    }
    Animation {
        name: string(desc, "m_name"),
        fps: kv::float(desc, "fps"),
        looping: kv::bool(flags, "m_bLooping"),
        delta,
        hidden: kv::bool(flags, "m_bHidden"),
        frames,
        morphs: if morphed { morphs } else { Vec::new() },
        events: events(desc),
    }
}

/// Every animation in `data` (an `AnimationResourceData_t`), decoded with `key` (`m_decodeKey`).
pub fn read(data: &Object, key: &Object, rig: &Rig) -> Vec<Animation> {
    let decoders: Vec<Option<Decoder>> = objects(data, "m_decoderArray").map(|d| Decoder::from_name(&string(d, "m_szName"))).collect();
    let channels: Vec<Channel> = objects(key, "m_dataChannelArray").map(|c| channel(c, rig)).collect();
    let segments: Vec<Option<Segment>> = objects(data, "m_segmentArray").map(|s| segment(s, &decoders, &channels)).collect();
    objects(data, "m_animArray").map(|a| animation(a, &segments, rig)).collect()
}

/// Sequences (`m_localS1SeqDescArray`) over `anims`: each plays the animation (or earlier
/// sequence) its fetch references name, blended per [`blend`], under its own name and flags;
/// animations no sequence took keep their own.
pub fn sequences(seq: &Object, anims: Vec<Animation>) -> Vec<Animation> {
    let names = strings(seq, "m_localSequenceNameArray");
    let mut out: Vec<Animation> = Vec::new();
    for desc in objects(seq, "m_localS1SeqDescArray") {
        let fetch = kv::object(desc, "m_fetch");
        let refs = kv::ints(fetch, "m_localReferenceArray");
        let find = |i: i64| {
            let target = names.get(usize::try_from(i).ok()?)?;
            anims.iter().chain(out.iter()).find(|a| a.name.eq_ignore_ascii_case(target))
        };
        let parts: Vec<(&Animation, f32)> = blend::weights(fetch, seq).into_iter().filter_map(|(r, w)| Some((find(*refs.get(r)?)?, w))).collect();
        let Some(mut a) = blend::blend(&parts) else { continue };
        let flags = kv::object(desc, "m_flags");
        a.name = string(desc, "m_sName");
        (a.looping, a.hidden) = (kv::bool(flags, "m_bLooping"), kv::bool(flags, "m_bHidden"));
        a.delta |= kv::bool(flags, "m_bLegacyDelta");
        out.push(a);
    }
    let rest: Vec<Animation> = anims.into_iter().filter(|a| !out.iter().any(|s| s.name.eq_ignore_ascii_case(&a.name))).collect();
    out.extend(rest);
    out
}

fn kv3_object(bytes: &[u8]) -> Result<Object, Error> {
    match crate::kv3bin::read(bytes)? {
        Value::Object(o) => Ok(o),
        _ => Err(format("animation data isn't an object")),
    }
}

/// An animation group (`.vagrp_c`): its decode key, and animations from its own ANIM block or
/// from the `.vanim_c` files `m_localHAnimArray` names (read through `load`).
pub fn read_group(file: &[u8], rig: &Rig, load: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<Animation>, Error> {
    let group = kv3_object(super::required(file, b"DATA", "DATA")?)?;
    let key = kv::object(&group, "m_decodeKey");
    if let Some(anim) = crate::container::block(file, b"ANIM")? {
        return Ok(read(&kv3_object(anim)?, key, rig));
    }
    let mut out = Vec::new();
    for path in strings(&group, "m_localHAnimArray").iter().filter(|p| !p.is_empty()) {
        let Some(bytes) = load(&super::model::compiled_name(path)) else { continue };
        out.extend(read(&kv3_object(super::required(&bytes, b"DATA", "DATA")?)?, key, rig));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
