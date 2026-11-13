//! AnimGraph1 sources (`.vanmgrph`, KV3 `CAnimationGraph`): the parameter list, which is what
//! managed's legacy `SetAnimParameter`/`GetFloat`... read and write, and the sequence nodes' tag
//! spans (`crate::tag_spans`).
// TODO: evaluate the node graph (state machines, blends); it needs clips from FBX sources (vmdl.rs).

use super::{kv_bool, kv_f32, kv_vec3};
use crate::params::{ParamDef, Value};
use crate::tag_spans::{SequenceTags, TagSpan};
use glam::Quat;
use kv::Value as Kv;

#[derive(Debug)]
pub enum Error {
    Parse(kv::Error),
    NotAGraph,
}

#[derive(Debug, Default)]
pub struct Graph1Source {
    pub params: Vec<ParamDef>,
    pub tags: Vec<SequenceTags>,
}

pub fn read(src: &str) -> Result<Graph1Source, Error> {
    let (_, root) = kv::kv3::parse(src).map_err(Error::Parse)?;
    let root = root.as_object().filter(|o| o.str("_class") == Some("CAnimationGraph")).ok_or(Error::NotAGraph)?;
    let list = root.get("m_pParameterList").and_then(Kv::as_object).and_then(|l| l.get("m_Parameters")).and_then(Kv::as_array).unwrap_or_default();
    let params = list.iter().filter_map(Kv::as_object).filter_map(param).collect();
    Ok(Graph1Source { params, tags: tag_spans(root) })
}

pub fn parameters(src: &str) -> Result<Vec<ParamDef>, Error> {
    read(src).map(|g| g.params)
}

/// An `AnimTagID`: a bare number or `{ m_id = n }`.
fn tag_id(v: Option<&Kv>) -> Option<i64> {
    let v = v?;
    v.as_f64().or_else(|| v.as_object()?.get("m_id")?.as_f64()).map(|n| n as i64)
}

fn each_object<'a>(v: &'a Kv, out: &mut Vec<&'a kv::Object>) {
    match v {
        Kv::Object(o) => {
            out.push(o);
            o.0.iter().for_each(|(_, v)| each_object(v, out));
        }
        Kv::Array(a) => a.iter().for_each(|v| each_object(v, out)),
        _ => {}
    }
}

/// Tag names come from `m_pTagManager.m_tags` by `m_tagID`; spans from every `CSequenceAnimNode`
/// wherever the node manager nests it.
fn tag_spans(root: &kv::Object) -> Vec<SequenceTags> {
    let manager = root.get("m_pTagManager").and_then(Kv::as_object);
    let defs = manager.and_then(|m| m.get("m_tags")).and_then(Kv::as_array).unwrap_or_default();
    let names: Vec<(i64, String)> = defs.iter().filter_map(Kv::as_object).filter_map(|t| Some((tag_id(t.get("m_tagID"))?, t.str("m_name")?.to_owned()))).collect();
    let span = |s: &kv::Object| {
        let id = tag_id(s.get("m_id"))?;
        let tag = names.iter().find(|(n, _)| *n == id)?.1.clone();
        Some(TagSpan { tag, start: kv_f32(s, "m_fStartCycle"), duration: kv_f32(s, "m_fDuration") })
    };
    let mut nodes = Vec::new();
    root.0.iter().for_each(|(_, v)| each_object(v, &mut nodes));
    let mut out: Vec<SequenceTags> = Vec::new();
    for n in nodes.into_iter().filter(|n| n.str("_class") == Some("CSequenceAnimNode")) {
        let spans: Vec<TagSpan> = n.get("m_tagSpans").and_then(Kv::as_array).unwrap_or_default().iter().filter_map(Kv::as_object).filter_map(span).collect();
        let Some(sequence) = n.str("m_sequenceName").filter(|_| !spans.is_empty()) else { continue };
        match out.iter_mut().find(|s| s.sequence.eq_ignore_ascii_case(sequence)) {
            Some(s) => s.spans.extend(spans),
            None => out.push(SequenceTags { sequence: sequence.to_owned(), spans }),
        }
    }
    out
}

fn param(p: &kv::Object) -> Option<ParamDef> {
    let default = match p.str("_class")? {
        "CBoolAnimParameter" => Value::Bool(kv_bool(p, "m_bDefaultValue")),
        "CFloatAnimParameter" => Value::Float(kv_f32(p, "m_fDefaultValue")),
        "CIntAnimParameter" => Value::Int(kv_f32(p, "m_defaultValue") as i32),
        "CEnumAnimParameter" => Value::Enum(kv_f32(p, "m_defaultValue") as u8),
        "CVectorAnimParameter" => Value::Vector(kv_vec3(p, "m_defaultValue")),
        "CQuaternionAnimParameter" => {
            let q = p.get("m_defaultValue").and_then(Kv::as_array).unwrap_or_default();
            let c = |i: usize| q.get(i).and_then(Kv::as_f64).unwrap_or(if i == 3 { 1.0 } else { 0.0 }) as f32;
            Value::Rotation(Quat::from_xyzw(c(0), c(1), c(2), c(3)).normalize())
        }
        _ => return None,
    };
    let (min, max) = match &default {
        Value::Float(_) => (kv_f32(p, "m_fMinValue"), kv_f32(p, "m_fMaxValue")),
        _ => (kv_f32(p, "m_minValue"), kv_f32(p, "m_maxValue")),
    };
    let options = p.get("m_enumOptions").and_then(Kv::as_array).unwrap_or_default().iter().filter_map(Kv::as_str).map(str::to_owned).collect();
    Some(ParamDef { name: p.str("m_name")?.to_owned(), default, min, max, options, auto_reset: kv_bool(p, "m_bAutoReset") })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAPH: &str = r#"<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:animgraph2:version{0f7898b8-5471-45c4-9867-cd9c46bcfdb5} -->
{
    _class = "CAnimationGraph"
    m_pParameterList = { _class = "CAnimParameterList" m_Parameters = [
        { _class = "CVectorAnimParameter" m_name = "aim_head" m_bAutoReset = false m_defaultValue = [ 100.0, 0.0, 0.0 ] },
        { _class = "CFloatAnimParameter" m_name = "aim_head_weight" m_fDefaultValue = 1.0 m_fMinValue = 0.0 m_fMaxValue = 1.0 },
        { _class = "CBoolAnimParameter" m_name = "b_jump" m_bAutoReset = true m_bDefaultValue = false },
        { _class = "CEnumAnimParameter" m_name = "holdtype" m_defaultValue = 0 m_enumOptions = [ "none", "pistol" ] },
        { _class = "CIntAnimParameter" m_name = "hit_bone" m_defaultValue = 20 m_minValue = 0 m_maxValue = 127 },
        { _class = "CQuaternionAnimParameter" m_name = "aim_rot" m_defaultValue = [ 0.0, 0.0, 0.0, 1.0 ] },
    ] }
}"#;

    #[test]
    fn reads_every_parameter_kind() {
        let p = parameters(GRAPH).unwrap();
        let names: Vec<_> = p.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["aim_head", "aim_head_weight", "b_jump", "holdtype", "hit_bone", "aim_rot"]);
        assert_eq!(p[0].default, Value::Vector(glam::Vec3::new(100.0, 0.0, 0.0)));
        assert_eq!((p[1].min, p[1].max), (0.0, 1.0));
        assert!(p[2].auto_reset);
        assert_eq!(p[3].options, ["none", "pistol"]);
        assert_eq!((p[4].default.clone(), p[4].max), (Value::Int(20), 127.0));
        assert_eq!(p[5].default, Value::Rotation(Quat::IDENTITY));
    }

    const TAGGED: &str = r#"<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:animgraph2:version{0f7898b8-5471-45c4-9867-cd9c46bcfdb5} -->
{
    _class = "CAnimationGraph"
    m_pTagManager = { _class = "CAnimTagManager" m_tags = [
        { _class = "CStringAnimTag" m_name = "footplant" m_tagID = { m_id = 11 } },
        { _class = "CTaskStatusAnimTag" m_name = "attack_hit" m_tagID = 12 },
    ] }
    m_pNodeManager = { _class = "CAnimNodeManager" m_nodes = [
        { key = { m_id = 1 } value = { _class = "CSequenceAnimNode" m_sequenceName = "walk" m_tagSpans = [
            { _class = "CAnimTagSpan" m_id = { m_id = 11 } m_fStartCycle = 0.25 m_fDuration = 0.1 },
            { _class = "CAnimTagSpan" m_id = { m_id = 99 } m_fStartCycle = 0.0 m_fDuration = 1.0 },
        ] } },
        { key = { m_id = 2 } value = { _class = "CSequenceAnimNode" m_sequenceName = "attack" m_tagSpans = [
            { _class = "CAnimTagSpan" m_id = 12 m_fStartCycle = 0.5 m_fDuration = 0.2 },
        ] } },
        { key = { m_id = 3 } value = { _class = "CSequenceAnimNode" m_sequenceName = "idle" m_tagSpans = [] } },
    ] }
}"#;

    #[test]
    fn reads_sequence_tag_spans_by_tag_id() {
        let g = read(TAGGED).unwrap();
        let span = |tag: &str, start: f32, duration: f32| TagSpan { tag: tag.into(), start, duration };
        assert_eq!(g.tags, [
            SequenceTags { sequence: "walk".into(), spans: vec![span("footplant", 0.25, 0.1)] },
            SequenceTags { sequence: "attack".into(), spans: vec![span("attack_hit", 0.5, 0.2)] },
        ], "unknown tag IDs and span-less nodes dropped");
    }
}
