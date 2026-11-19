use super::*;
use crate::math::vec4;
use crate::mesh::render_mesh::{RenderMesh, TRIANGLES};
use crate::model::content::{ALL_LODS, Content, MaterialGroup, ModelMesh};
use crate::render::material;
use crate::scene::object::{IS_OPAQUE, IS_TRANSLUCENT, State};

/// A material, with `vectors` as its params (features, user attributes).
fn mat(name: &str, vectors: &[(&str, f32)]) -> Arc<MaterialRef> {
    let h = material::create_raw(std::ffi::CString::new(name).ok().as_deref(), Some(c"shaders/complex.shader"), true);
    vectors.iter().for_each(|(k, v)| material::set_vector(h, k, vec4(*v, 0.0, 0.0, 0.0)));
    let own = MaterialRef::keep(h).unwrap();
    crate::resource::destroy_handle(h);
    own
}

struct Fixture {
    skin: Arc<MaterialRef>,
    cloth: Arc<MaterialRef>,
    alt_skin: Arc<MaterialRef>,
    model: Arc<Model>,
    handle: crate::model::Ref,
}

/// A model drawing `skin` (user attribute `skin` 1) and `cloth`, with an `alt` group swapping the
/// skin; parts `head` (groups 0, 1) and `feet` (group 2).
fn fixture() -> Fixture {
    let (skin, cloth, alt_skin) = (mat("skin", &[("skin", 1.0)]), mat("cloth", &[]), mat("alt_skin", &[]));
    let mesh = |m: &Arc<MaterialRef>, group_mask| ModelMesh { mesh: RenderMesh::new("m", Some(m.clone()), TRIANGLES), group_mask, lod_mask: ALL_LODS };
    let content = Content {
        meshes: vec![mesh(&skin, 1), mesh(&cloth, 4)],
        groups: vec!["head_@0".into(), "head_@1".into(), "feet".into()],
        material_groups: vec![MaterialGroup { name: "default".into(), materials: vec![skin.clone(), cloth.clone()] }, MaterialGroup { name: "alt".into(), materials: vec![alt_skin.clone(), cloth.clone()] }],
        ..Default::default()
    };
    let handle = crate::model::create_runtime("models/test/overrides.vmdl", content);
    Fixture { skin, cloth, alt_skin, model: crate::model::get(handle).unwrap(), handle }
}

fn is(a: Option<Arc<MaterialRef>>, b: &Arc<MaterialRef>) -> bool {
    a.is_some_and(|a| Arc::ptr_eq(&a, b))
}

#[test]
fn groups_attributes_and_indices_replace_materials() {
    let f = fixture();
    let (red, blue) = (mat("red", &[]), mat("blue", &[]));
    let mut o = Overrides::default();
    assert!(is(o.resolve(&f.model, Some(&f.skin)), &f.skin) && o.resolve(&f.model, None).is_none());

    o.group = Some(StringToken::of("ALT"));
    assert!(is(o.resolve(&f.model, Some(&f.skin)), &f.alt_skin), "the skin swaps slot for slot");
    assert!(is(o.resolve(&f.model, Some(&f.cloth)), &f.cloth));

    o.set_by_attribute(Some(red.clone()), StringToken::of("skin"), 1);
    assert!(is(o.resolve(&f.model, Some(&f.skin)), &red), "the default skin's attribute matches under another skin");
    assert!(is(o.resolve(&f.model, Some(&f.cloth)), &f.cloth));
    o.set_by_attribute(Some(blue.clone()), StringToken::of("skin"), 2);
    assert!(is(o.resolve(&f.model, Some(&f.skin)), &red), "value 2 matches nothing");

    assert_eq!(crate::model::parts::materials(&f.model).len(), 3, "Model.Materials: default skin, then alt's");
    o.set_by_index(1, Some(blue.clone()));
    assert!(is(o.resolve(&f.model, Some(&f.cloth)), &blue), "index 1 is cloth");
    o.set_by_index(1, None);
    assert!(is(o.resolve(&f.model, Some(&f.cloth)), &f.cloth));

    o.set_by_attribute(None, StringToken::of("skin"), 1);
    assert!(is(o.resolve(&f.model, Some(&f.skin)), &f.alt_skin), "a null material stops the replacement");
    o.set_by_index(0, Some(blue.clone()));
    o.clear();
    assert!(is(o.resolve(&f.model, Some(&f.skin)), &f.alt_skin), "clearing keeps the skin");
    crate::resource::destroy_handle(f.handle);
}

/// What draws decides surface flags: a translucent replacement makes the object translucent,
/// and a whole-object override beats per-material ones.
#[test]
fn overrides_reach_the_draws() {
    let f = fixture();
    let glass = mat("glass", &[("F_TRANSLUCENT", 1.0)]);
    let mut s = State { model: f.handle, ..State::default() };
    assert_eq!(s.effective_flags() & (IS_OPAQUE | IS_TRANSLUCENT), IS_OPAQUE);
    s.materials.set_by_attribute(Some(glass.clone()), StringToken::of("skin"), 1);
    assert_eq!(s.effective_flags() & (IS_OPAQUE | IS_TRANSLUCENT), IS_OPAQUE | IS_TRANSLUCENT);
    s.mesh_groups = crate::model::parts::with_body_group(&f.model, s.mesh_groups, StringToken::of("Head"), 1).unwrap();
    assert_eq!(s.mesh_groups & 0b111, 0b110, "head switched to its second choice: the skin mesh is hidden");
    assert_eq!(s.effective_flags() & (IS_OPAQUE | IS_TRANSLUCENT), IS_OPAQUE);
    assert_eq!(crate::model::parts::with_body_group(&f.model, 0b111, StringToken::of("feet"), 1), None, "feet has one choice");

    s.mesh_groups = u64::MAX;
    s.material_override = Some(f.cloth.clone());
    assert!(is(s.material_for(&f.model, Some(&f.skin)), &f.cloth));
    assert_eq!(s.effective_flags() & (IS_OPAQUE | IS_TRANSLUCENT), IS_OPAQUE);
    crate::resource::destroy_handle(f.handle);
}
