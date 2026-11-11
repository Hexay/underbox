//! `CSceneLightObject`: what managed sets on a light and reads back. Server-side it's state only;
//! the renderer (M2) shades with it.

use super::object::{self, State};
use super::world;
use crate::handles::Kind;
use crate::math::{from_rot, from_vec, to_rot, to_vec, vec3};
use crate::render::texture;
use crate::resource;
use abi::slots::{c_scene_light_object as l, g_p_scene_system as system};
use abi::structs::{Rotation, Vector3};
use abi::{HandleId, NativeRef, Registry, bind, class};

type Ref = NativeRef<class::CSceneLightObject>;

/// managed's `ShadowLightType` (the values `GetLightType` reports).
#[derive(Debug, Clone, Copy)]
pub enum Type {
    Point = 1,
    Directional = 2,
    Spot = 3,
    // TODO: confirm; ShadowLightType has no ortho value, so this is our own.
    Ortho = 4,
}

/// Read by the renderer's light collection (render/lighting).
#[derive(Clone)]
pub struct Light {
    pub(crate) kind: Type,
    pub(crate) direction: Rotation,
    pub(crate) color: Vector3,
    bounce_color: Vector3,
    pub(crate) radius: f32,
    /// Inner and outer spot cone half-angles, degrees.
    pub(crate) theta: f32,
    pub(crate) phi: f32,
    fall_off: f32,
    pub(crate) shadows: bool,
    pub(crate) shadow_resolution: i32,
    shadow_width: i32,
    shadow_height: i32,
    pub(crate) shadow_cascades: i32,
    cascade_split_ratio: f32,
    cascade_distance_scale: f32,
    constant_attn: f32,
    pub(crate) linear_attn: f32,
    pub(crate) quadratic_attn: f32,
    pub(crate) fog_strength: f32,
    fog_mode: i32,
    flags: u32,
    shape: i64,
    /// An owned texture handle (see `cookie` in register).
    cookie: texture::Ref,
}

impl Light {
    fn new(kind: Type) -> Self {
        Self {
            kind,
            direction: crate::math::identity().rotation,
            color: vec3(1.0, 1.0, 1.0),
            bounce_color: vec3(0.0, 0.0, 0.0),
            radius: 200.0,
            theta: 0.0,
            phi: 0.0,
            fall_off: 1.0,
            shadows: false,
            shadow_resolution: 0,
            shadow_width: 0,
            shadow_height: 0,
            shadow_cascades: 1,
            cascade_split_ratio: 0.0,
            cascade_distance_scale: 1.0,
            constant_attn: 0.0,
            linear_attn: 0.0,
            quadratic_attn: 1.0,
            fog_strength: 1.0,
            fog_mode: 0,
            flags: 0,
            shape: 0,
            cookie: texture::Ref::NULL,
        }
    }
}

impl Drop for Light {
    fn drop(&mut self) {
        resource::destroy_handle(self.cookie);
    }
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&mut Light) -> R) -> R {
    let Some(o) = object::get(r.cast()) else { return R::default() };
    o.light.as_ref().map(|l| f(&mut l.lock().unwrap())).unwrap_or_default()
}

fn create(w: world::Ref, light: Light) -> HandleId<class::CSceneLightObject> {
    object::create_with::<class::CSceneLightObject, _>(w, Kind::SceneLightObject, State::default(), Some(light))
}

/// `GetWorldDirection`: native's points at the light, so it is minus the light's forward axis (+X
/// of its rotation); managed negates it back (`SceneMirror`, `ShadowMapper.FromNative`).
fn toward_light(direction: Rotation) -> Vector3 {
    from_vec(-(to_rot(direction) * physics::Vector::X))
}

/// A directional light shining along `direction` (+X when zero).
fn directional(direction: Vector3) -> Light {
    let forward = to_vec(direction).normalize_or(physics::Vector::X);
    Light { direction: from_rot(physics::Rotation::from_rotation_arc(physics::Vector::X, forward)), ..Light::new(Type::Directional) }
}

/// Bind a getter/setter pair to a `Light` field.
macro_rules! field {
    ($r:expr, $get:ident, $set:ident, $field:ident) => {
        bind!($r, l::$get, |r| with(r, |x| Some(x.$field)).unwrap_or_default());
        bind!($r, l::$set, |r, v| with(r, |x| x.$field = v));
    };
}

pub fn register(r: &mut Registry) {
    bind!(r, system::CREATE_POINT_LIGHT, |w| create(w, Light::new(Type::Point)));
    bind!(r, system::CREATE_SPOT_LIGHT, |w| create(w, Light::new(Type::Spot)));
    bind!(r, system::CREATE_ORTHO_LIGHT, |w| create(w, Light::new(Type::Ortho)));
    bind!(r, system::CREATE_DIRECTIONAL_LIGHT, |w, direction| create(w, directional(direction)));

    bind!(r, l::GET_LIGHT_TYPE, |r| with(r, |x| x.kind as i32));
    bind!(r, l::SET_WORLD_POSITION, |r, p| object::with(r.cast(), |st| st.transform.position = p));
    bind!(r, l::GET_WORLD_POSITION, |r| object::with(r.cast(), |st| st.transform.position));
    bind!(r, l::SET_WORLD_DIRECTION, |r, q| with(r, |x| x.direction = q));
    bind!(r, l::GET_WORLD_DIRECTION, |r| with(r, |x| Some(toward_light(x.direction))).unwrap_or_default());
    field!(r, GET_COLOR, SET_COLOR, color);
    bind!(r, l::SET_BOUNCE_COLOR, |r, c| with(r, |x| x.bounce_color = c));
    field!(r, GET_RADIUS, SET_RADIUS, radius);
    field!(r, GET_THETA, SET_THETA, theta);
    field!(r, GET_PHI, SET_PHI, phi);
    field!(r, GET_FALL_OFF, SET_FALL_OFF, fall_off);
    field!(r, GET_SHADOWS, SET_SHADOWS, shadows);
    field!(r, GET_SHADOW_TEXTURE_RESOLUTION, SET_SHADOW_TEXTURE_RESOLUTION, shadow_resolution);
    field!(r, GET_SHADOW_TEXTURE_WIDTH, SET_SHADOW_TEXTURE_WIDTH, shadow_width);
    field!(r, GET_SHADOW_TEXTURE_HEIGHT, SET_SHADOW_TEXTURE_HEIGHT, shadow_height);
    field!(r, GET_SHADOW_CASCADES, SET_SHADOW_CASCADES, shadow_cascades);
    field!(r, GET_SHADOW_CASCADE_SPLIT_RATIO, SET_SHADOW_CASCADE_SPLIT_RATIO, cascade_split_ratio);
    field!(r, GET_CASCADE_DISTANCE_SCALE, SET_CASCADE_DISTANCE_SCALE, cascade_distance_scale);
    field!(r, GET_CONSTANT_ATTN, SET_CONSTANT_ATTN, constant_attn);
    field!(r, GET_LINEAR_ATTN, SET_LINEAR_ATTN, linear_attn);
    field!(r, GET_QUADRATIC_ATTN, SET_QUADRATIC_ATTN, quadratic_attn);
    field!(r, GET_FOG_CONTRIBUTION_STENGTH, SET_FOG_CONTRIBUTION_STENGTH, fog_strength);
    field!(r, GET_FOG_LIGHTING_MODE, SET_FOG_LIGHTING_MODE, fog_mode);
    field!(r, GET_LIGHT_FLAGS, SET_LIGHT_FLAGS, flags);
    field!(r, GET_LIGHT_SHAPE, SET_LIGHT_SHAPE, shape);
    // Managed owns what getters return (Texture.FromNative), so the light keeps and hands out copies.
    bind!(r, l::SET_LIGHT_COOKIE, |r, t| {
        let copy = resource::copy_handle::<_, texture::Texture>(t);
        let old = with(r, |x| Some(std::mem::replace(&mut x.cookie, copy)));
        old.into_iter().for_each(resource::destroy_handle);
    });
    bind!(r, l::GET_LIGHT_COOKIE, |r| {
        let cookie = with(r, |x| Some(x.cookie)).unwrap_or_default();
        resource::copy_handle::<_, texture::Texture>(cookie)
    });
    // TODO: the renderer's per-light switches and baked-lighting indices (M2).
    bind!(r, l::SET_RENDER_DIFFUSE, |_, _| {});
    bind!(r, l::SET_RENDER_SPECULAR, |_, _| {});
    bind!(r, l::SET_RENDER_TRANSMISSIVE, |_, _| {});
    bind!(r, l::SET_LIGHT_MODULATOR_RANGE, |_, _, _| {});
    bind!(r, l::SET_BAKE_LIGHT_INDEX, |_, _| {});
    bind!(r, l::SET_BAKE_LIGHT_INDEX_SCALE, |_, _| {});
    bind!(r, l::SET_USES_INDEXED_BAKED_LIGHTING, |_, _| {});
    bind!(r, l::SET_LIGHT_SOURCE_SIZE0, |_, _| {});
    bind!(r, l::SET_LIGHT_SOURCE_SIZE1, |_, _| {});
    bind!(r, l::SET_LIGHT_SOURCE_DIM0, |_, _| {});
    bind!(r, l::SET_LIGHT_SOURCE_DIM1, |_, _| {});
}
