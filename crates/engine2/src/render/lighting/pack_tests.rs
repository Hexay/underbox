use super::*;
use crate::handles::Kind;
use crate::render::shading::layout::Value;
use crate::scene::light::Type;
use crate::scene::light_switches::MIXED_SHADOWS;
use crate::scene::world::{self, SceneWorld};
use glam::{Quat, Vec3, Vec4};
use std::sync::Arc;

fn light(baked: bool, bake_index: i32) -> Light {
    let pack = Pack { baked, bake_index, ..Pack::default() };
    Light { kind: super::super::kind::POINT, position: Vec3::ZERO, rotation: Quat::IDENTITY, color: Vec3::ONE, radius: 100.0, quadratic: 1e-4, cone: None, fog: 1.0, shadows: false, handle: None, shadow_index: None, pack }
}

#[test]
fn switches_set_the_gpu_light_flags() {
    let all = Pack::default();
    assert_eq!(all.flags, flags::VISIBLE | flags::DIFFUSE | flags::SPECULAR | flags::TRANSMISSIVE);
    let s = Switches { diffuse: false, transmissive: false, bake_index: 7, indexed: true, source_dim: [4.0, 2.0], modulators: [3, 2], ..Switches::default() };
    let p = Pack::of(&s, BAKED | MIXED_SHADOWS, 2, 3);
    assert_eq!(p.flags, flags::VISIBLE | flags::SPECULAR | flags::INDEXED | flags::NO_FOG_SHADOWS);
    assert_eq!((p.shape, p.baked, p.bake_index), (2, true, 7));
    let w = Light { pack: p, ..light(true, 7) }.gpu_light();
    assert_eq!((w[1], w[2]), (2, flags::VISIBLE | flags::SPECULAR | flags::INDEXED | flags::NO_FOG_SHADOWS));
    assert_eq!([f32::from_bits(w[27]), f32::from_bits(w[28])], [4.0, 2.0], "ShapeSize");
    assert_eq!([w[37], w[38]], [3, 2], "modulator range");
    assert_eq!(Pack::of(&Switches { bake_index: 7, ..Switches::default() }, 0, 0, 0).bake_index, -1, "an index without indexed lighting is none");
}

/// Dynamic lights first (the clusters' count), baked after; bake indices map into the buffer,
/// the sun's after the lights, and everything else at the black dummy.
#[test]
fn baked_lights_bin_after_dynamic_ones_and_map_by_bake_index() {
    let sun = Light { kind: super::super::kind::DIRECTIONAL, ..light(false, 9) };
    let mut l = Lighting { lights: vec![light(true, 4), light(false, -1), light(true, 0), light(false, 2)], sun: Some(sun), ..Default::default() };
    l.order_baked_last();
    assert_eq!(l.lights.iter().map(|x| x.pack.bake_index).collect::<Vec<_>>(), [-1, 2, 4, 0]);
    assert_eq!((l.dynamic_count(), l.baked_count()), (2, 2));
    let map = l.bake_mapping();
    assert_eq!([map[2], map[4], map[0], map[9]], [1, 2, 3, 4]);
    assert!(map.iter().enumerate().filter(|(i, _)| ![0, 2, 4, 9].contains(i)).all(|(_, &m)| m == 5));
    let binned = l.binned();
    assert_eq!((binned.len(), binned[5]), (6, [0; GPU_LIGHT_WORDS]));
    let toward = Vec3::new(f32::from_bits(binned[4][3]), f32::from_bits(binned[4][7]), f32::from_bits(binned[4][11]));
    assert!(toward.distance(-Vec3::X) < 1e-6, "a baked sun points at its source: {toward}");

    let view = crate::render::shading::view::View::default();
    let Some(Value::Vec(n)) = l.constant("ViewLightingConfigV2", "NumLights", &view) else { panic!("NumLights") };
    assert_eq!(n, Vec4::new(2.0, 2.0, 0.0, 0.0));
    let Some(Value::Array(m)) = l.constant("ViewLightingConfigV2", "BakedLightIndexMapping", &view) else { panic!("mapping") };
    assert!(m.len() == BAKE_INDICES && matches!(m[4], Value::Vec(v) if v.x == 2.0));
}

/// A light's slots reach `PackSceneLight`, which managed's own binner uploads as is.
#[test]
fn pack_scene_light_reports_switches_flags_and_bake_index() {
    crate::handles::fake_managed();
    let w: world::Ref = objects::create_shared(Arc::new(SceneWorld::default()));
    let mut scene = light::Light::new(Type::Spot);
    scene.switches = Switches { specular: false, bake_index: 12, indexed: true, ..Switches::default() };
    scene.flags = BAKED;
    let state = crate::scene::object::State::default();
    let (r, _) = object::create_native::<class::CSceneLightObject, class::CSceneLightObject>(w, Kind::SceneLightObject, state, Some(scene));
    let (mut out, mut index) = ([0u32; GPU_LIGHT_WORDS], -1i32);
    let ret = pack_scene_light(r.cast(), RawPtr::from_raw(out.as_mut_ptr().cast()), RawPtr::from_raw((&mut index as *mut i32).cast()));
    assert_eq!((ret as u32, index), (BAKED, 12));
    assert_eq!((out[0], out[2]), (super::super::kind::SPOT, flags::VISIBLE | flags::DIFFUSE | flags::TRANSMISSIVE | flags::INDEXED));
    assert_eq!(out[34], u32::MAX, "no shadow map");
    object::delete(r);
    assert_eq!(pack_scene_light(r.cast(), RawPtr::from_raw(std::ptr::null_mut()), RawPtr::from_raw(std::ptr::null_mut())), -1);
}
