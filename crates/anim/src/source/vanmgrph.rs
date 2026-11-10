//! AnimGraph1 sources (`.vanmgrph`, KV3 `CAnimationGraph`): the parameter list, which is what
//! managed's legacy `SetAnimParameter`/`GetFloat`... read and write.
// TODO: evaluate the node graph (CStateMachineAnimNode, CBlendAnimNode...); it needs clip data
// from FBX sources we can't import yet, so legacy graphs hold parameters only.

use super::{kv_bool, kv_f32, kv_vec3};
use crate::params::{ParamDef, Value};
use glam::Quat;
use kv::Value as Kv;

#[derive(Debug)]
pub enum Error {
    Parse(kv::Error),
    NotAGraph,
}

pub fn parameters(src: &str) -> Result<Vec<ParamDef>, Error> {
    let (_, root) = kv::kv3::parse(src).map_err(Error::Parse)?;
    let root = root.as_object().filter(|o| o.str("_class") == Some("CAnimationGraph")).ok_or(Error::NotAGraph)?;
    let list = root.get("m_pParameterList").and_then(Kv::as_object).and_then(|l| l.get("m_Parameters")).and_then(Kv::as_array).unwrap_or_default();
    Ok(list.iter().filter_map(Kv::as_object).filter_map(param).collect())
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
}
