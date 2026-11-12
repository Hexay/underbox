//! The translucent and depth-prepass layers over tests.rs's scene (a cube at the origin seen from
//! -X): blended materials draw after the opaque ones, back to front and without depth writes; a
//! prepass writes depth and no colour, and the opaque layer after it draws as it would alone.

use super::tests::{SIZE, read, scene};
use super::{camera, execute, field, layer, setup, view};
use crate::handles::Kind as ObjectKind;
use crate::mesh::material_ref::MaterialRef;
use crate::model::content::Content;
use crate::render::{device, material, swap_chain, texture};
use crate::scene::object::{self, IS_OPAQUE, IS_TRANSLUCENT, State};
use crate::scene::world;
use abi::StringToken;
use abi::class;
use abi::structs::Transform;
use glam::Vec4;
use render::srgb::{decode, encode};

/// A 16-unit cube at `x` on the camera's axis drawn with core's `unlit` (the stand-in), tinted
/// `tint`, alpha blended when `blend`.
fn cube(w: world::Ref, x: f32, [r, g, b, a]: [f32; 4], blend: bool) -> material::Ref {
    let mat = material::create_raw(Some(c"materials/test/layer.vmat"), Some(c"shaders/unlit.shader"), true);
    if blend {
        material::set_vector(mat, "F_TRANSLUCENT", crate::math::vec4(1.0, 0.0, 0.0, 0.0));
    }
    let state = State {
        model: crate::model::create_runtime("models/test/cube.vmdl", Content::default()),
        transform: Transform { position: crate::math::vec3(x, 0.0, 0.0), ..crate::math::identity() },
        tint: crate::math::vec4(r, g, b, a),
        material_override: MaterialRef::keep(mat),
        ..State::default()
    };
    let (o, _) = object::create_native::<class::CSceneObject, class::CSceneObject>(w, ObjectKind::SceneObject, state, None);
    let flags = object::with(o, |s| s.effective_flags());
    assert_eq!(flags & (IS_OPAQUE | IS_TRANSLUCENT), if blend { IS_TRANSLUCENT } else { IS_OPAQUE }, "flags follow the material");
    mat
}

fn center(cam: camera::Ref) -> [u8; 4] {
    let color = texture::create_target("layers", SIZE, SIZE, 0);
    assert!(camera::render_to_texture(cam, color, Default::default(), Default::default()));
    let px = read(color).texel(32, 32)[..4].try_into().unwrap();
    crate::resource::destroy_handle(color);
    px
}

#[test]
fn translucent_cubes_blend_back_to_front_over_the_opaque_one() {
    let Some(core) = crate::render::shading::tests::core_dir() else {
        eprintln!("skipped: no sbox-public checkout");
        return;
    };
    crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
    device::create().expect("a wgpu adapter");
    let (w, cam) = scene();
    let base = center(cam);
    // Nearest first: drawn in creation order, green would end up over red.
    let mats = [cube(w, -40.0, [1.0, 0.0, 0.0, 0.5], true), cube(w, -24.0, [0.0, 1.0, 0.0, 0.5], true)];
    let got = center(cam);
    let over = |c: u8, green: f32, red: f32| (encode(red * 0.5 + green * 0.25 + decode(f32::from(c) / 255.0) * 0.25) * 255.0).round();
    let want = [over(base[0], 0.0, 1.0), over(base[1], 1.0, 0.0), over(base[2], 0.0, 0.0)];
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((f32::from(*g) - w).abs() <= 6.0, "channel {i}: got {got:?}, want {want:?} over {base:?}");
    }
    mats.into_iter().for_each(crate::resource::destroy_handle);
}

/// `(LayerEnum, required flags)` per render layer, after a clear to black.
fn run_layers(w: world::Ref, cam: camera::Ref, layers: &[(i64, i64)]) -> (render::Frame, render::Frame) {
    let (camera, proj, viewport) = camera::camera(cam, SIZE, SIZE);
    let (color, depth) = (texture::create_target("prepass", SIZE, SIZE, 0), texture::create_target("prepass depth", SIZE, SIZE, 62));
    let desc = view::Desc {
        camera,
        proj,
        viewport,
        swap_chain: swap_chain::Handle::default(),
        color,
        depth,
        worlds: vec![w],
        render_tags: Vec::new(),
        exclude_tags: Vec::new(),
        post_process: false,
        ambient: Vec4::ZERO,
        environment: Default::default(),
        unique_id: 0,
        managed_camera_id: 0,
    };
    let v = view::create(desc);
    let clear = view::add_layer(v, layer::create(v, "Clear".into(), viewport, layer::Kind::Clear(setup::CLEAR_ALL), 0, 0), Default::default());
    layer::with(clear, |s| s.clear_color = crate::math::vec4(0.0, 0.0, 0.0, 1.0));
    for &(layer_type, required) in layers {
        let l = view::add_layer(v, layer::create(v, "Render".into(), viewport, layer::Kind::Render(StringToken::of("Forward")), required, 0), Default::default());
        field!(set i64, abi::slots::i_scene_layer::GET_LAYER_ENUM, l.addr(), layer_type);
    }
    execute::run(v);
    let out = (read(color), read(depth));
    view::destroy(v);
    [color, depth].into_iter().for_each(crate::resource::destroy_handle);
    out
}

#[test]
fn depth_prepass_writes_depth_only() {
    device::create().expect("a wgpu adapter");
    let (w, cam) = scene();
    let (prepass, opaque) = ((layer::layer_type::DEPTH_PREPASS, IS_OPAQUE), (layer::layer_type::OPAQUE, IS_OPAQUE));
    let d = |f: &render::Frame| f32::from_le_bytes(f.texel(32, 32).try_into().unwrap());

    let (color, depth) = run_layers(w, cam, &[prepass]);
    assert_eq!(color.texel(32, 32), [0, 0, 0, 255], "no colour from the prepass");
    assert!(d(&depth) < 1.0, "the cube's depth from the prepass");

    let (alone, alone_depth) = run_layers(w, cam, &[opaque]);
    let (after, after_depth) = run_layers(w, cam, &[prepass, opaque]);
    assert_ne!(alone.texel(32, 32), [0, 0, 0, 255], "the opaque layer draws the cube");
    assert_eq!(after.hash(), alone.hash(), "GreaterEqual passes the prepass's depth");
    assert_eq!(after_depth.hash(), alone_depth.hash());
    assert_eq!(d(&depth), d(&alone_depth), "the prepass writes the opaque layer's depth");
}
