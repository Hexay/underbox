//! The animation side of ModelDoc `.vmdl` sources: attachments, hitbox sets, animation names and
//! the anim graph, following `Prefab` nodes into `.vmdl_prefab` files.
// TODO: bones and clip keys live in the FBX/DMX files `RenderMeshFile`/`AnimFile` name; until we
// import those, skeletons and clips come only from runtime builders.

use super::{angles, kv_bool, kv_f32, kv_vec3};
use crate::model::{Attachment, Hitbox, HitboxSet, HitboxShape, Influence, ModelAnim};
use crate::xform::Xform;
use kv::{Object, Value};

/// Prefabs may nest; this bounds a cycle.
const MAX_PREFAB_DEPTH: usize = 8;

#[derive(Debug)]
pub enum Error {
    Parse(kv::Error),
    NoRootNode,
}

/// Read the animation data of a `.vmdl`. `prefab` loads a content path's text.
pub fn read(src: &str, prefab: &dyn Fn(&str) -> Option<String>) -> Result<ModelAnim, Error> {
    let mut m = ModelAnim::default();
    visit_root(src, prefab, &mut m, 0)?;
    Ok(m)
}

fn visit_root(src: &str, prefab: &dyn Fn(&str) -> Option<String>, m: &mut ModelAnim, depth: usize) -> Result<(), Error> {
    let (_, root) = kv::kv3::parse(src).map_err(Error::Parse)?;
    let node = root.as_object().and_then(|o| o.get("rootNode")).and_then(Value::as_object).ok_or(Error::NoRootNode)?;
    visit(node, prefab, m, depth);
    Ok(())
}

fn children(node: &Object) -> impl Iterator<Item = &Object> {
    node.get("children").and_then(Value::as_array).unwrap_or_default().iter().filter_map(Value::as_object)
}

fn visit(node: &Object, prefab: &dyn Fn(&str) -> Option<String>, m: &mut ModelAnim, depth: usize) {
    if let Some(graph) = node.str("anim_graph_name").filter(|g| !g.is_empty()) {
        m.graph = Some(graph.to_owned());
    }
    match node.str("_class").unwrap_or("") {
        "Prefab" if depth < MAX_PREFAB_DEPTH => {
            if let Some(text) = node.str("target_file").and_then(prefab) {
                // A broken prefab only loses its own nodes.
                let _ = visit_root(&text, prefab, m, depth + 1);
            }
            return;
        }
        "Attachment" => return m.attachments.push(attachment(node)),
        "HitboxSet" => {
            let hitboxes = children(node).filter_map(hitbox).collect();
            return m.hitbox_sets.push(HitboxSet { name: node.str("name").unwrap_or_default().to_owned(), hitboxes });
        }
        "AnimFile" | "EmptyAnim" => {
            if let Some(name) = node.str("name") {
                m.clip_names.push(name.to_owned());
            }
            return;
        }
        _ => {}
    }
    for child in children(node) {
        visit(child, prefab, m, depth);
    }
}

fn influence(node: &Object) -> Influence {
    let offset = Xform::new(kv_vec3(node, "relative_origin"), angles(kv_vec3(node, "relative_angles")));
    Influence { bone: node.str("parent_bone").unwrap_or_default().to_owned(), offset, weight: node.get("weight").map_or(1.0, |_| kv_f32(node, "weight")) }
}

fn attachment(node: &Object) -> Attachment {
    let own = influence(node);
    let extra: Vec<Influence> = children(node).filter(|c| c.str("_class") == Some("AttachmentInfluence")).map(influence).collect();
    let influences = if own.weight > 0.0 || extra.is_empty() { std::iter::once(own).chain(extra).collect() } else { extra };
    Attachment { name: node.str("name").unwrap_or_default().to_owned(), influences, ignore_rotation: kv_bool(node, "ignore_rotation") }
}

fn hitbox(node: &Object) -> Option<Hitbox> {
    let shape = match node.str("_class")? {
        "HitboxCapsule" => HitboxShape::Capsule { a: kv_vec3(node, "point0"), b: kv_vec3(node, "point1"), radius: kv_f32(node, "radius") },
        "HitboxSphere" => HitboxShape::Sphere { center: kv_vec3(node, "center"), radius: kv_f32(node, "radius") },
        "HitboxBox" => HitboxShape::Box { min: kv_vec3(node, "mins"), max: kv_vec3(node, "maxs") },
        _ => return None,
    };
    Some(Hitbox {
        name: node.str("name").unwrap_or_default().to_owned(),
        bone: node.str("parent_bone").unwrap_or_default().to_owned(),
        surface: node.str("surface_property").unwrap_or_default().to_owned(),
        tags: node.str("tags").unwrap_or_default().split_whitespace().map(str::to_owned).collect(),
        shape,
        translation_only: kv_bool(node, "translation_only"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r#"{ rootNode = { _class = "RootNode" anim_graph_name = "models/x.vanmgrph" children = [
        { _class = "AttachmentList" children = [ { _class = "Prefab" target_file = "att.vmdl_prefab" } ] },
        { _class = "AnimationList" children = [ { _class = "AnimFile" name = "idle" source_filename = "x@idle.fbx" } ] },
        { _class = "HitboxSetList" children = [ { _class = "HitboxSet" name = "default" children = [
            { _class = "HitboxCapsule" name = "HB_head" parent_bone = "head" surface_property = "flesh" tags = "head  upper"
              radius = 5.0 point0 = [ 0.0, 0.0, 0.0 ] point1 = [ 0.0, 0.0, 8.0 ] } ] } ] },
    ] } }"#;

    const PREFAB: &str = r#"{ rootNode = { _class = "RootNode" children = [ { _class = "AttachmentList" children = [
        { _class = "Folder" children = [ { _class = "Attachment" name = "eyes" parent_bone = "head"
            relative_origin = [ 3.0, 0.0, 1.0 ] relative_angles = [ 0.0, 90.0, 0.0 ] weight = 1.0 ignore_rotation = false } ] },
        { _class = "Attachment" name = "hold" parent_bone = "spine" weight = 0.0 children = [
            { _class = "AttachmentInfluence" parent_bone = "hand_L" weight = 1.0 },
            { _class = "AttachmentInfluence" parent_bone = "hand_R" weight = 1.0 } ] },
        { _class = "Prefab" target_file = "att.vmdl_prefab" },
    ] } ] } }"#;

    #[test]
    fn reads_lists_through_prefabs() {
        let loads = std::cell::Cell::new(0);
        let prefab = |p: &str| {
            loads.set(loads.get() + 1);
            (p == "att.vmdl_prefab").then(|| PREFAB.to_owned())
        };
        let m = read(MODEL, &prefab).unwrap();
        assert_eq!(m.graph.as_deref(), Some("models/x.vanmgrph"));
        assert_eq!(m.clip_names, ["idle"]);
        assert_eq!(loads.get(), MAX_PREFAB_DEPTH, "a self-including prefab stops at the depth bound");
        let eyes = m.attachment("eyes").unwrap();
        assert_eq!(eyes.influences[0].bone, "head");
        assert!(eyes.influences[0].offset.pos.abs_diff_eq(glam::Vec3::new(3.0, 0.0, 1.0), 1e-6));
        let hold = m.attachment("hold").unwrap();
        assert_eq!(hold.influences.iter().map(|i| i.bone.as_str()).collect::<Vec<_>>(), ["hand_L", "hand_R"]);
        let hb = &m.hitbox_sets[0].hitboxes[0];
        assert_eq!((hb.bone.as_str(), hb.tags.len(), hb.shape.kind()), ("head", 2, 2));
    }
}
