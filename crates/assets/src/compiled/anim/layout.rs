//! The members real engine2 expects around animation data, in the order its compiled models
//! write them (AGRP, ANIM descriptions, ASEQ). Our reader needs few of them; real needs them all.

use super::{Animation, Rig};
use crate::compiled::kv::{floats, int_value, list, obj, resource, text};
use ::kv::Value;

pub const POSITION: i64 = 0;
pub const ANGLE: i64 = 1;
pub const MORPH: i64 = 2;
pub const SCALE: i64 = 3;

/// The decode-key element id of target `i` (bone or controller) of `channel`: real interleaves
/// positions (even) and angles (odd), then lists morphs, then scales (its citizen's key).
pub fn element(channel: i64, i: usize, bones: usize, morphs: usize) -> usize {
    match channel {
        POSITION => 2 * i,
        ANGLE => 2 * i + 1,
        MORPH => 2 * bones + i,
        _ => 2 * bones + morphs + i,
    }
}

/// `channel`'s index in [`decode_key`]'s `m_dataChannelArray` (a segment's `m_nLocalChannel`):
/// the constants' own, less one for Scale when there's no morph channel.
pub fn channel_index(channel: i64, morphs: usize) -> i64 {
    if channel == SCALE && morphs == 0 { SCALE - 1 } else { channel }
}

/// Target `i`'s bit in its channel's `m_nElementMaskArray`; a segment's `m_nLocalElementMasks`
/// ORs its elements' bits.
pub fn mask(i: usize) -> i64 {
    1 << (i % 31)
}

fn bools(names: &[&str], set: &[&str]) -> Value {
    obj(names.iter().map(|n| (*n, Value::Bool(set.contains(n)))).collect())
}

fn none() -> Value {
    list([])
}

pub fn decode_key(rig: &Rig) -> Value {
    let n = rig.bones.len();
    let bones = rig.bones.iter().map(|b| {
        let parent = b.parent.map_or(-1, |p| p as i64);
        obj(vec![("m_name", text(&b.name)), ("m_parent", int_value(parent)), ("m_pos", floats(&b.position)), ("m_quat", floats(&b.rotation)), ("m_qAlignment", floats(&[0.0; 4])), ("m_flags", int_value(64))])
    });
    let names: Vec<&str> = rig.bones.iter().map(|b| b.name.as_str()).collect();
    let flex: Vec<&str> = rig.flex.iter().map(String::as_str).collect();
    let channel = |class: &str, var: &str, ty: i64, ch: i64, names: &[&str]| {
        obj(vec![
            ("m_szChannelClass", text(class)),
            ("m_szVariableName", text(var)),
            ("m_nFlags", int_value(0)),
            ("m_nType", int_value(ty)),
            ("m_szGrouping", text("")),
            ("m_szDescription", text("")),
            ("m_szElementNameArray", list(names.iter().map(|n| text(n)))),
            ("m_nElementIndexArray", list((0..names.len()).map(|i| int_value(element(ch, i, n, flex.len()) as i64)))),
            ("m_nElementMaskArray", list((0..names.len()).map(|i| int_value(mask(i))))),
        ])
    };
    let mut channels = vec![channel("BoneChannel", "Position", 3, POSITION, &names), channel("BoneChannel", "Angle", 4, ANGLE, &names)];
    if !flex.is_empty() {
        channels.push(channel("MorphChannel", "data", 1, MORPH, &flex));
    }
    channels.push(channel("BoneChannel", "Scale", 1, SCALE, &names));
    obj(vec![
        ("m_name", text("")),
        ("m_boneArray", list(bones)),
        ("m_userArray", none()),
        ("m_morphArray", none()),
        ("m_nChannelElements", int_value((3 * n + flex.len()) as i64)),
        ("m_dataChannelArray", list(channels)),
    ])
}

/// An AGRP / `.vagrp_c` DATA: `key` over `anims` (`.vanim` paths; embedded groups list none).
pub fn group(key: Value, anims: &[&str]) -> Value {
    let anims = if anims.is_empty() { list([Value::Null]) } else { list(anims.iter().map(|p| resource(p))) };
    let retarget = obj(vec![("m_bEnableRetarget", int_value(0)), ("m_nRetargetStyle", int_value(0)), ("m_element", none()), ("m_chain", none()), ("m_replicated", none())]);
    obj(vec![
        ("m_nFlags", int_value(0)),
        ("m_name", text("")),
        ("m_localHAnimArray", anims),
        ("m_includedGroupArray", none()),
        ("m_directHSeqGroup", Value::Null),
        ("m_decodeKey", key),
        ("m_retarget", retarget),
        ("m_szScripts", none()),
    ])
}

fn fades() -> Value {
    obj(vec![("m_flFadeInTime", Value::Double(0.2f32.into())), ("m_flFadeOutTime", Value::Double(0.2f32.into()))])
}

/// An `m_animArray` entry for `a` over `bones` bones, its frame blocks `blocks`.
pub fn desc(a: &Animation, blocks: Vec<Value>, bones: usize) -> Value {
    let set: Vec<&str> = [(a.looping, "m_bLooping"), (a.hidden, "m_bHidden"), (a.delta, "m_bDelta"), (true, "m_bModelDoc")].into_iter().filter_map(|(on, n)| on.then_some(n)).collect();
    let flags = bools(&["m_bLooping", "m_bAllZeros", "m_bHidden", "m_bDelta", "m_bLegacyWorldspace", "m_bModelDoc", "m_bDisableInterpolation"], &set);
    let bits = || list((0..bones.max(bones.div_ceil(8))).map(|byte| int_value((0..8).filter(|bit| byte * 8 + bit < bones).fold(0, |m, bit| m | 1 << bit))));
    let usage = obj(vec![
        ("m_boneArray", none()),
        ("m_morphArray", none()),
        ("m_userArray", none()),
        ("m_bHasRotationBitArray", bits()),
        ("m_bHasMovementBitArray", bits()),
        ("m_bHasMorphBitArray", none()),
        ("m_bHasUserBitArray", none()),
    ]);
    let data = obj(vec![("m_fileName", text("")), ("m_nFrames", int_value(a.frames.len() as i64)), ("m_nFramesPerBlock", int_value(0)), ("m_frameblockArray", list(blocks)), ("m_usageDifferences", usage)]);
    let events = a.events.iter().map(|e| {
        let fields = [("m_nFrame", int_value(e.frame)), ("m_nEndFrame", int_value(e.end_frame)), ("m_flCycle", Value::Double(e.cycle.into())), ("m_sEventName", text(&e.name)), ("m_EventData", e.data.clone())];
        obj(fields.into())
    });
    let zeros = || list((0..bones).map(|_| floats(&[0.0; 3])));
    obj(vec![
        ("m_name", text(&a.name)),
        ("m_flags", flags),
        ("fps", Value::Double(a.fps.into())),
        ("m_pData", data),
        ("m_movementArray", none()),
        ("m_eventArray", list(events)),
        ("m_activityArray", none()),
        ("m_hierarchyArray", none()),
        ("framestalltime", Value::Double(0.0)),
        ("m_vecRootMin", floats(&[0.0; 3])),
        ("m_vecRootMax", floats(&[0.0; 3])),
        ("m_vecBoneWorldMin", zeros()),
        ("m_vecBoneWorldMax", zeros()),
        ("m_sequenceParams", fades()),
    ])
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
            let set: Vec<&str> = [(looping, "m_bLooping"), (hidden, "m_bHidden"), (true, "m_bModelDoc")].into_iter().filter_map(|(on, n)| on.then_some(n)).collect();
            let flags = ["m_bLooping", "m_bSnap", "m_bAutoplay", "m_bPost", "m_bHidden", "m_bMulti", "m_bLegacyDelta", "m_bLegacyWorldspace", "m_bLegacyCyclepose", "m_bLegacyRealtime", "m_bModelDoc"];
            let fetch = obj(vec![
                ("m_flags", bools(&["m_bRealtime", "m_bCylepose", "m_b0D", "m_b1D", "m_b2D", "m_b2D_TRI"], &["m_b0D"])),
                ("m_localReferenceArray", list([int_value(i as i64)])),
                ("m_nGroupSize", list([int_value(1), int_value(1)])),
                ("m_nLocalPose", list([int_value(-1), int_value(-1)])),
                ("m_poseKeyArray0", floats(&[0.0])),
                ("m_poseKeyArray1", floats(&[0.0])),
                ("m_nLocalCyclePoseParameter", int_value(-1)),
                ("m_bCalculatePoseParameters", Value::Bool(false)),
            ]);
            obj(vec![
                ("m_sName", text(name)),
                ("m_flags", bools(&flags, &set)),
                ("m_fetch", fetch),
                ("m_nLocalWeightlist", int_value(0)),
                ("m_autoLayerArray", none()),
                ("m_IKLockArray", none()),
                ("m_transition", fades()),
                ("m_SequenceKeys", Value::Null),
                ("m_keyValueText", text("")),
                ("m_activityArray", none()),
                ("m_footMotion", none()),
            ])
        })
        .collect();
    let weights = obj(vec![("m_sName", text("default")), ("m_nLocalBoneArray", none()), ("m_flBoneWeightArray", none()), ("m_flDefaultMorphCtrlWeight", Value::Double(1.0)), ("m_morphCtrlWeightArray", none())]);
    obj(vec![
        ("m_sName", text("")),
        ("m_nFlags", int_value(0)),
        ("m_localSequenceNameArray", list(names.iter().map(|n| text(n)))),
        ("m_localS1SeqDescArray", list(descs)),
        ("m_localMultiSeqDescArray", none()),
        ("m_localSynthAnimDescArray", none()),
        ("m_localCmdSeqDescArray", none()),
        ("m_localBoneMaskArray", list([weights])),
        ("m_localBoneNameArray", none()),
        ("m_localNodeName", text("")),
        ("m_localPoseParamArray", none()),
        ("m_keyValues", obj(vec![])),
        ("m_localIKAutoplayLockArray", none()),
    ])
}
