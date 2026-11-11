//! What the material editor asks of a shader: `GetPropertiesJson` (managed's `Shader.ShaderSchema`),
//! `IsUiVariableActive` (visibility rules in active.rs), and `CVfxCombo`'s name and group.

use super::{active, shader, string};
use abi::slots::{c_vfx as vfx, c_vfx_combo as combo};
use abi::{Registry, StrRet, bind};
use serde_json::{Value, json};
use std::collections::HashMap;
use shaders::VfxShader;
use shaders::annotation::Annotated;

/// A material feature as `CVfxCombo` describes it.
pub(super) struct ComboInfo {
    pub name: String,
    pub group: String,
    pub min: u32,
    pub max: u32,
    pub labels: Vec<String>,
}

pub(super) fn combos(s: &VfxShader) -> Vec<ComboInfo> {
    s.features
        .iter()
        .map(|f| ComboInfo {
            name: f.name.clone(),
            group: f.group.clone().unwrap_or_default(),
            min: f.min,
            max: f.max,
            labels: (f.min..=f.max).map(|v| f.labels.iter().find(|(i, _)| *i == v).map(|(_, l)| l.clone()).unwrap_or_default()).collect(),
        })
        .collect()
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

/// Up to four comma-separated numbers from `args`, from `skip` on, zero-filled.
fn vec4(args: &str, skip: usize, take: usize) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (o, v) in out.iter_mut().zip(args.split(',').skip(skip).take(take.min(4))) {
        *o = v.trim().trim_end_matches('f').parse().unwrap_or(0.0);
    }
    out
}

fn annotated(a: &Annotated) -> Value {
    let get = |k: &str| a.get(k).map(unquote).unwrap_or_default();
    let range = a.get("Range").unwrap_or("");
    let half = range.split(',').count() / 2;
    json!({
        "Name": a.name,
        "Type": a.decl.split_whitespace().next().unwrap_or(""),
        "AttributeName": get("Attribute"),
        "SourceType": if a.get("Attribute").is_some() { "Attribute" } else { "Variable" },
        "UiType": get("UiType"),
        "UiGroup": get("UiGroup"),
        "UiStep": a.get("UiStep").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0),
        "FloatMin": vec4(range, 0, half),
        "FloatMax": vec4(range, half, half),
        "FloatDefault": vec4(a.get("Default").unwrap_or(""), 0, 4),
        "IntDefault": vec4(a.get("Default").unwrap_or(""), 0, 4),
    })
}

/// `CreateInputTexture2D( Name, Srgb, 8, "", "_color", "Group,10/10", Default3( ... ) )`.
fn input(args: &[String]) -> Option<Value> {
    let (kind, rest) = args.split_first()?;
    let arg = |i: usize| rest.get(i).map(|s| unquote(s)).unwrap_or_default();
    Some(json!({
        "Name": arg(0),
        "Type": kind.strip_prefix("CreateInput").unwrap_or(kind),
        "SourceType": "InputTexture",
        "UiGroup": arg(5),
        "TextureEnding": arg(4),
        "DefaultInputTexture": rest.get(6).map(|d| d.trim().to_string()).unwrap_or_default(),
    }))
}

/// Every variable the editor shows, first declaration of each name wins.
fn variables(s: &VfxShader) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut push = |v: Value| {
        if !out.iter().any(|o| o["Name"] == v["Name"]) {
            out.push(v);
        }
    };
    for p in &s.programs {
        p.decls.inputs.iter().filter_map(|i| input(i)).for_each(&mut push);
        let ui = |a: &&Annotated| ["Attribute", "UiType", "UiGroup"].iter().any(|k| a.get(k).is_some());
        p.annotations.iter().filter(ui).map(annotated).for_each(&mut push);
    }
    out
}

pub(super) fn properties_json(s: &VfxShader) -> String {
    let combos: Vec<Value> = combos(s)
        .into_iter()
        .enumerate()
        .map(|(i, c)| json!({"Name": c.name, "Group": c.group, "Min": c.min, "Max": c.max, "Index": i, "Values": c.labels}))
        .collect();
    json!({ "Variables": variables(s), "Combos": combos }).to_string()
}

/// As hshader.def: `feature` exists and `value` is in its range; with it at `value` and every
/// other feature at its minimum, `variable` is one the editor lists and shows.
pub(super) fn ui_variable_active(s: &VfxShader, variable: &str, feature: &str, value: i32) -> bool {
    let Some(f) = s.feature(feature) else { return false };
    if value < f.min as i32 || value > f.max as i32 {
        return false;
    }
    let mut values: HashMap<&str, i64> = s.features.iter().map(|f| (f.name.as_str(), f.min as i64)).collect();
    values.insert(f.name.as_str(), value as i64);
    variables(s).iter().any(|v| v["Name"] == variable) && active::shown(s, &values).contains(variable)
}

pub fn register(r: &mut Registry) {
    bind!(r, vfx::GET_PROPERTIES_JSON, |h| StrRet::from(shader(h).map(|s| properties_json(&s))));
    bind!(r, vfx::IS_UI_VARIABLE_ACTIVE, |h, variable, feature, value| {
        shader(h).is_some_and(|s| ui_variable_active(&s, &string(variable), &string(feature), value))
    });
    bind!(r, combo::GET_NAME, |c| StrRet::from(objects::get::<_, ComboInfo>(c).map(|c| c.name.clone())));
    bind!(r, combo::GET_GROUP, |c| StrRet::from(objects::get::<_, ComboInfo>(c).map(|c| c.group.clone())));
}
