//! Core's Fog.hlsl through the stand-in with each input family: gradient, cubemap, sky tint, sun
//! lobe and volumetric froxels. Skips without a GPU or an sbox-public checkout.

use super::tests::standin_cube;
use crate::render::fog::{self, ViewFog};
use crate::render::draw::{self, Target};
use crate::render::lighting::shadow::SunShadow;
use crate::render::lighting::{Light, Lighting, kind};
use render::pass::Load;
use crate::render::{bindless, fog_volume, scene::camera, texture};
use abi::NativeRef;
use abi::structs::{SceneVolumetricFogParameters, SceneVolumetricFogVolume, TextureCreationConfig_t};
use glam::{Mat4, Quat, Vec3, Vec4};
use std::sync::Arc;

const WHITE: [f32; 4] = [1.0; 4];
const IDENTITY_SLOTS: [usize; 4] = [44, 49, 54, 59];

/// The texel at the middle of the frame, where the cube's -X face is.
fn middle(lighting: &Lighting) -> Option<[u8; 3]> {
    standin_cube(WHITE, lighting).map(|f| f.texel(32, 32)[..3].try_into().unwrap())
}

/// A texel on the cube's top face whose ray is unambiguously mostly +X; the middle's is a tie with -Z.
fn toward_x(lighting: &Lighting) -> Option<[u8; 3]> {
    standin_cube(WHITE, lighting).map(|f| f.texel(32, 26)[..3].try_into().unwrap())
}

fn with_view_fog(view_fog: ViewFog) -> Lighting {
    Lighting { ambient: Vec4::ONE, view_fog: Some(Arc::new(view_fog)), ..Default::default() }
}

/// `FogConstants` opaque over any distance; `edit` fills the fields a test is about (by float index).
fn fog_block(edit: impl FnOnce(&mut [f32; 76])) -> Arc<[u8]> {
    let mut f = [0f32; 76];
    // Weight, opacity, ramp over 0..1 units, exponent, overall opacity, distance contribution.
    (f[0], f[1], f[4], f[5], f[6], f[15], f[20]) = (1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0);
    for i in IDENTITY_SLOTS {
        f[i] = 1.0;
    }
    edit(&mut f);
    f.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>().into()
}

/// A 4×4 cube whose faces are +X red, -Y yellow and black elsewhere. Sizes above 1 keep seamless
/// filtering from blending neighbouring faces into a texel away from the edges.
fn cube_texture() -> texture::Ref {
    // Gotcha: a texture made before the device has no GPU side and binds the default.
    crate::render::device::create();
    let mut faces = [[0u8, 0, 0, 255]; 6];
    (faces[0], faces[3]) = ([200, 0, 0, 255], [200, 200, 0, 255]);
    let mut data: Vec<u8> = faces.iter().flat_map(|f| f.repeat(16)).collect();
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (4, 4, 1, 1);
    config.m_n_flags = texture::TSPEC_CUBE_TEXTURE;
    texture::create(Some(c"fog cube"), true, config, &mut data)
}

#[test]
fn gradient_fog_is_full_below_its_start_height_and_gone_above_its_end() {
    let lighting = |start: f32, end: f32| {
        let gradient = fog::gradient(Vec4::new(0.0, 1.0, start, end), Vec4::new(1.0, 1.0, 1.0, 0.0), Vec4::new(0.0, 0.0, 1.0, 0.0));
        with_view_fog(ViewFog { gradient: Some(gradient), ..Default::default() })
    };
    let Some(fogged) = middle(&lighting(1000.0, 1001.0)) else { return };
    assert_eq!(fogged, [0, 0, 255], "the fog colour");
    let clear = middle(&lighting(-20.0, -10.0)).unwrap();
    assert_eq!(clear[0], clear[2], "above the fog: {clear:?}");
    assert!(clear[0] > 100, "above the fog: {clear:?}");
}

/// `MaximumOpacity` 0.5 of blue over the white cube: half of each, read back.
#[test]
fn gradient_fog_opacity_caps_the_blend() {
    let gradient = fog::gradient(Vec4::new(0.0, 1.0, 1000.0, 1001.0), Vec4::new(0.5, 1.0, 1.0, 0.0), Vec4::new(0.0, 0.0, 1.0, 0.0));
    let Some(half) = middle(&with_view_fog(ViewFog { gradient: Some(gradient), ..Default::default() })) else { return };
    assert!(half[0].abs_diff(128) <= 1 && half[1].abs_diff(128) <= 1 && half[2] == 255, "half blue: {half:?}");
}

#[test]
fn cubemap_fog_samples_along_the_ray_through_its_transform() {
    let t = cube_texture();
    let lighting = |transform: Mat4| with_view_fog(ViewFog { cube: Some(fog::cube(Vec4::new(0.0, 1.0, 1.0, 1.0), Vec4::ZERO, transform, t, 4)), ..Default::default() });
    let Some(plain) = toward_x(&lighting(Mat4::IDENTITY)) else { return };
    assert!(plain[0] > 60 && plain[1] < 5 && plain[2] < 5, "+X face: {plain:?}");
    // A sky turned +90 about Z shows, along world +X, what it holds at its own -Y.
    let turned = toward_x(&lighting(Mat4::from_rotation_z(90f32.to_radians()))).unwrap();
    assert!(turned[0] > 60 && turned[0] == turned[1] && turned[2] < 5, "-Y face: {turned:?}");
    crate::resource::destroy_handle(t);
}

#[test]
fn no_gradient_or_cube_leaves_the_surface_alone() {
    let Some(clear) = middle(&with_view_fog(ViewFog::default())) else { return };
    assert!(clear.iter().all(|c| *c > 100) && clear[0] == clear[2], "white under ambient: {clear:?}");
}

#[test]
fn sky_tint_multiplies_the_fog_colour_by_the_bindless_cube() {
    let t = cube_texture();
    let index = bindless::index(t, false);
    let block = fog_block(|f| {
        (f[12], f[13], f[14]) = (1.0, 1.0, 1.0);
        f[22] = 1.0;
        f[40] = f32::from_bits(index as u32);
    });
    let Some(tinted) = toward_x(&Lighting { ambient: Vec4::ONE, fog: Some(block), ..Default::default() }) else { return };
    assert!(tinted[0] > 60 && tinted[1] < 5 && tinted[2] < 5, "+X face: {tinted:?}");
}

fn green_sun(toward: Vec3) -> Light {
    let rotation = Quat::from_rotation_arc(Vec3::X, -toward);
    Light { kind: kind::DIRECTIONAL, position: Vec3::ZERO, rotation, color: Vec3::Y, radius: 0.0, quadratic: 0.0, cone: None, fog: 1.0, shadows: false }
}

/// Black distance fog with a tight lobe: green toward the sun, nothing away from it.
#[test]
fn sun_lobe_brightens_fog_toward_the_sun() {
    let lighting = |toward: Vec3| {
        let block = fog_block(|f| (f[28], f[29], f[33]) = (1.0, 100.0, 1.0));
        Lighting { ambient: Vec4::ONE, fog: Some(block), sun: Some(green_sun(toward)), ..Default::default() }
    };
    let ray = (Vec3::ZERO - Vec3::new(-30.0, 0.0, 30.0)).normalize();
    let Some(toward) = middle(&lighting(ray)) else { return };
    assert!(toward[1] > 200 && toward[0] == 0, "facing the sun: {toward:?}");
    let away = middle(&lighting(-ray)).unwrap();
    assert!(away[1] < 5, "facing away: {away:?}");
}

fn volume(strength: f32) -> SceneVolumetricFogVolume {
    let mut v = SceneVolumetricFogVolume::default();
    (v.m_f_strength, v.m_f_exponent, v.m_mat_world_to_volume) = (strength, 0.0, camera::matrix(Mat4::IDENTITY));
    (v.m_v_min.vec.x, v.m_v_min.vec.y, v.m_v_min.vec.z) = (-200.0, -200.0, -200.0);
    (v.m_v_max.vec.x, v.m_v_max.vec.y, v.m_v_max.vec.z) = (200.0, 200.0, 200.0);
    v.m_v_color.vec.x = 1.0;
    v
}

/// A red medium around camera and cube dims the cube and adds the ambient's in-scattering.
#[test]
fn volumetric_fog_attenuates_and_scatters() {
    // A probe draw first: it creates the device the froxel pass needs, and skips without one.
    let Some(clear) = middle(&with_view_fog(ViewFog::default())) else { return };
    let (world, empty): (crate::scene::world::Ref, crate::scene::world::Ref) = (NativeRef::from_raw(0xF06 as *mut _), NativeRef::from_raw(0xF08 as *mut _));
    fog_volume::add(world.addr(), 1, volume(4.0));
    let view_proj = render::Camera::perspective(90f32.to_radians(), 1.0, 1.0, Vec3::new(-30.0, 0.0, 30.0), Vec3::ZERO).view_proj;
    let mut params = SceneVolumetricFogParameters::default();
    (params.m_b_fog_enabled, params.m_fl_scattering, params.m_fl_draw_distance, params.m_fl_indirect_strength) = (1, 1.0, 1000.0, 1.0);
    let ambient = Lighting { ambient: Vec4::ONE, ..Default::default() };
    assert!(fog_volume::render(&[empty], view_proj, &ambient, &params, 0).is_none(), "no volumes, no grid");
    let frustum = fog_volume::render(&[world], view_proj, &ambient, &params, 0).expect("froxels");
    let near = fog_volume::froxel(&frustum, [48, 28, 2]);
    assert!(near[3] < 0.9 && near[0] > near[1], "a slice past the camera scatters red: {near:?}");
    let fogged = middle(&Lighting { fog_volume: Some(frustum), ..with_view_fog(ViewFog::default()) }).unwrap();
    assert!(i32::from(fogged[1]) + 40 < i32::from(clear[1]), "attenuated: {fogged:?} from {clear:?}");
    assert!(i32::from(fogged[0]) > i32::from(fogged[1]) + 40, "red in-scattering: {fogged:?}");
    fog_volume::remove(world.addr(), 1);
    assert!(fog_volume::render(&[world], view_proj, &ambient, &params, 0).is_none(), "removed");
}

/// A spot at the eye lights the medium inside its cone only: the centre column scatters with
/// the spot aimed down the view, and not with it aimed away.
#[test]
fn spot_lights_scatter_inside_their_cone() {
    let Some(_) = middle(&with_view_fog(ViewFog::default())) else { return };
    let eye = Vec3::new(-30.0, 0.0, 30.0);
    let view_proj = render::Camera::perspective(90f32.to_radians(), 1.0, 1.0, eye, Vec3::ZERO).view_proj;
    let settings = || fog_volume::Settings { draw_distance: 1000.0, fade_in: (-2.0, -1.0), anisotropy: 0.0, scattering: 1.0 };
    let volumes = [fog_volume::Volume::native(&volume(4.0))];
    let scatter = |aim: Vec3| {
        let spot = fog_volume::FogLight { position: eye, radius: 500.0, color: Vec3::splat(4.0), spot: Some((aim, 0.95, 0.9)) };
        let sources = fog_volume::Sources { lights: vec![spot], ..Default::default() };
        let grid = fog_volume::march(&volumes, view_proj, &sources, &settings(), 0).expect("froxels");
        fog_volume::froxel(&grid, [48, 28, 2])
    };
    let ahead = (Vec3::ZERO - eye).normalize();
    let lit = scatter(ahead);
    assert!(lit[0] > 0.01 && lit[1] == 0.0, "inside the cone, the red medium scatters: {lit:?}");
    let dark = scatter(-ahead);
    assert!(dark[0] == 0.0, "outside the cone: {dark:?}");
}

/// Under one history key a still view settles on exactly what it marched alone; a light switched
/// on shows up partly at first (blended with the dark history), then converges.
#[test]
fn froxels_blend_with_their_history() {
    let Some(_) = middle(&with_view_fog(ViewFog::default())) else { return };
    let eye = Vec3::new(-30.0, 0.0, 30.0);
    let view_proj = render::Camera::perspective(90f32.to_radians(), 1.0, 1.0, eye, Vec3::ZERO).view_proj;
    let settings = fog_volume::Settings { draw_distance: 1000.0, fade_in: (-2.0, -1.0), anisotropy: 0.0, scattering: 1.0 };
    let volumes = [fog_volume::Volume::native(&volume(4.0))];
    let lit = |ambient: f32| fog_volume::Sources { ambient: Vec3::splat(ambient), ..Default::default() };
    let at = |sources: &fog_volume::Sources, key: usize| fog_volume::froxel(&fog_volume::march(&volumes, view_proj, sources, &settings, key).expect("froxels"), [48, 28, 4]);
    let alone = at(&lit(1.0), 0);
    const KEY: usize = 0xF06_0001;
    assert_eq!(at(&lit(0.0), KEY)[0], 0.0, "dark first frame");
    let first = at(&lit(1.0), KEY);
    assert!(first[0] > 0.0 && first[0] < alone[0] * 0.5, "partly lit after one frame: {first:?} of {alone:?}");
    let settled = (0..60).fold(first, |_, _| at(&lit(1.0), KEY));
    // History is stored at half precision.
    assert!(settled.iter().zip(alone).all(|(a, b)| (a - b).abs() <= b.abs() * 2e-3), "converged: {settled:?} to {alone:?}");
}

/// Every point maps to the shadow map's centre at depth 0.5: a map cleared to 0.2 (an occluder
/// in front) shadows the sun's in-scattering, one cleared to 1 scatters as no map does.
#[test]
fn sun_shadow_map_gates_in_scattering() {
    let Some(_) = middle(&with_view_fog(ViewFog::default())) else { return };
    let eye = Vec3::new(-30.0, 0.0, 30.0);
    let view_proj = render::Camera::perspective(90f32.to_radians(), 1.0, 1.0, eye, Vec3::ZERO).view_proj;
    let settings = fog_volume::Settings { draw_distance: 1000.0, fade_in: (-2.0, -1.0), anisotropy: 0.0, scattering: 1.0 };
    let volumes = [fog_volume::Volume::native(&volume(4.0))];
    let map = texture::create_target("fog shadow", 16, 16, 62);
    let world_to_shadow = Mat4::from_cols(Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::new(0.5, 0.5, 0.5, 1.0));
    let scatter = |shadow: Option<SunShadow>| {
        let sources = fog_volume::Sources { sun: Some(((eye - Vec3::ZERO).normalize(), Vec3::splat(4.0))), shadow, ..Default::default() };
        fog_volume::froxel(&fog_volume::march(&volumes, view_proj, &sources, &settings, 0).expect("froxels"), [48, 28, 2])
    };
    let cleared_to = |depth: f32| {
        let target = Target { depth: map, ..Default::default() };
        draw::execute(&target, &Lighting::default(), Load { color: None, depth: Some(depth) }, &[]);
        Some(SunShadow { world_to_shadow, map })
    };
    let open = scatter(None);
    assert!(open[0] > 0.01, "the sun scatters without a map: {open:?}");
    assert_eq!(scatter(cleared_to(1.0)), open, "nothing in front");
    let shadowed = scatter(cleared_to(0.2));
    assert!(shadowed[0] == 0.0 && shadowed[3] == open[3], "no sun, same density: {shadowed:?}");
    crate::resource::destroy_handle(map);
}
