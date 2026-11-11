//! Attachments and hitbox sets, which compiled models keep in their meshes' data
//! (`m_attachments`, `m_hitboxsets`). Entries may be wrapped as `{ key, value }` map pairs.

use super::kv::{self, float, floats, int, int_value, list, obj, objects, string, text};
use ::kv::{Object, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Influence {
    pub bone: String,
    pub offset: [f32; 3],
    /// xyzw.
    pub rotation: [f32; 4],
    pub weight: f32,
    pub root: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub influences: Vec<Influence>,
    pub ignore_rotation: bool,
}

/// `CHitBox`: `shape` 0 box (`min`/`max`), 1 sphere (`min` the centre), 2 capsule (`min`/`max` the ends).
#[derive(Debug, Clone, PartialEq)]
pub struct Hitbox {
    pub name: String,
    pub surface: String,
    pub bone: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub radius: f32,
    pub shape: i64,
    pub translation_only: bool,
    pub group: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HitboxSet {
    pub name: String,
    pub hitboxes: Vec<Hitbox>,
}

fn value(o: &Object) -> &Object {
    o.get("value").and_then(Value::as_object).unwrap_or(o)
}

pub fn attachments(mesh: &Object) -> Vec<Attachment> {
    objects(mesh, "m_attachments")
        .map(value)
        .map(|a| {
            let (names, offsets, rotations) = (kv::strings(a, "m_influenceNames"), kv::array(a, "m_vInfluenceOffsets"), kv::array(a, "m_vInfluenceRotations"));
            let (weights, roots) = (kv::array(a, "m_influenceWeights"), kv::array(a, "m_bInfluenceRootTransform"));
            let count = usize::try_from(kv::int_or(a, "m_nInfluences", names.len() as i64)).unwrap_or(0).min(names.len());
            let influences = (0..count)
                .map(|i| Influence {
                    bone: names[i].clone(),
                    offset: kv::floats_of(offsets.get(i)),
                    rotation: rotations.get(i).map_or([0.0, 0.0, 0.0, 1.0], |r| kv::floats_of(Some(r))),
                    weight: weights.get(i).and_then(Value::as_f64).map_or(1.0, |w| w as f32),
                    root: roots.get(i).and_then(kv::int_of).is_some_and(|r| r != 0),
                })
                .collect();
            Attachment { name: string(a, "m_name"), influences, ignore_rotation: kv::bool(a, "m_bIgnoreRotation") }
        })
        .collect()
}

pub fn hitbox_sets(mesh: &Object) -> Vec<HitboxSet> {
    objects(mesh, "m_hitboxsets")
        .map(value)
        .map(|s| {
            let key = if s.get("m_HitBoxes").is_some() { "m_HitBoxes" } else { "m_hitboxes" };
            let hitboxes = objects(s, key)
                .map(|h| Hitbox {
                    name: string(h, "m_name"),
                    surface: string(h, "m_sSurfaceProperty"),
                    bone: string(h, "m_sBoneName"),
                    min: kv::vec3(h, "m_vMinBounds"),
                    max: kv::vec3(h, "m_vMaxBounds"),
                    radius: float(h, "m_flShapeRadius"),
                    shape: int(h, "m_nShapeType"),
                    translation_only: kv::bool(h, "m_bTranslationOnly"),
                    group: int(h, "m_nGroupId"),
                })
                .collect();
            HitboxSet { name: string(s, "m_name"), hitboxes }
        })
        .collect()
}

fn entry(name: &str, v: Value) -> Value {
    obj(vec![("key", text(name)), ("value", v)])
}

/// `m_attachments` and `m_hitboxsets` members for a mesh's data.
pub fn write(attachments: &[Attachment], sets: &[HitboxSet]) -> Vec<(&'static str, Value)> {
    let att = attachments.iter().map(|a| {
        let i = &a.influences;
        let v = obj(vec![
            ("m_name", text(&a.name)),
            ("m_bIgnoreRotation", Value::Bool(a.ignore_rotation)),
            ("m_nInfluences", int_value(i.len() as i64)),
            ("m_influenceNames", list(i.iter().map(|x| text(&x.bone)))),
            ("m_vInfluenceOffsets", list(i.iter().map(|x| floats(&x.offset)))),
            ("m_vInfluenceRotations", list(i.iter().map(|x| floats(&x.rotation)))),
            ("m_influenceWeights", floats(&i.iter().map(|x| x.weight).collect::<Vec<_>>())),
            ("m_bInfluenceRootTransform", list(i.iter().map(|x| Value::Bool(x.root)))),
        ]);
        entry(&a.name, v)
    });
    let hitbox = |h: &Hitbox| {
        obj(vec![
            ("m_name", text(&h.name)),
            ("m_sSurfaceProperty", text(&h.surface)),
            ("m_sBoneName", text(&h.bone)),
            ("m_vMinBounds", floats(&h.min)),
            ("m_vMaxBounds", floats(&h.max)),
            ("m_flShapeRadius", Value::Double(h.radius.into())),
            ("m_nShapeType", int_value(h.shape)),
            ("m_bTranslationOnly", Value::Bool(h.translation_only)),
            ("m_nGroupId", int_value(h.group)),
        ])
    };
    let sets = sets.iter().map(|s| entry(&s.name, obj(vec![("m_name", text(&s.name)), ("m_HitBoxes", list(s.hitboxes.iter().map(hitbox)))])));
    vec![("m_attachments", list(att)), ("m_hitboxsets", list(sets))]
}
