//! Point and spot light shadows: one depth atlas of [`TILE`]² tiles, a spot light taking one tile
//! (a perspective over its outer cone) and a point light six (cube faces, in the order +X, -X,
//! +Y, -Y, +Z, -Z). A light's `ShadowMapIndex` is [`INDEX`] + its first tile (managed's renderer
//! writes indices into its own `ProjectedShadows` buffers instead, below [`INDEX`]); the stand-in reads the
//! tile's world → atlas (u, v, depth) matrix from [`MATRICES`] and samples [`TEXTURE`]. Lights
//! past the atlas's [`TILES`] go unshadowed.

use super::{Light, Lighting, kind};
use crate::render::draw::{self, Target, model as model_draw};
use crate::render::texture;
use crate::scene::{object, world};
use abi::structs::RenderViewport;
use glam::camera::rh;
use glam::{Mat4, Vec3, Vec4};
use render::pass::Load;
use std::sync::OnceLock;

/// Tile size in texels; the atlas is [`SIDE`] tiles a side.
pub const TILE: u32 = 512;
const SIDE: u32 = 4;
pub const TILES: usize = (SIDE * SIDE) as usize;
pub const TEXTURE: &str = "g_tUnderboxLocalShadow";
pub const BLOCK: &str = "UnderboxLocalShadows";
pub const MATRICES: &str = "g_matUnderboxLocalShadow";
/// The stand-in's `UNDERBOX_LOCAL_SHADOW_INDEX`.
pub const INDEX: u32 = 0x7FFF_0000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalShadows {
    /// World → atlas (u, v, depth), one per tile in use.
    pub tiles: Vec<Mat4>,
    /// Each binned light's first tile, parallel to `Lighting::lights`.
    pub first: Vec<Option<u32>>,
    pub map: texture::Ref,
}

fn map() -> texture::Ref {
    static MAP: OnceLock<texture::Ref> = OnceLock::new();
    // Image format 62: D32F.
    *MAP.get_or_init(|| texture::create_target("local shadows", TILE * SIDE, TILE * SIDE, 62))
}

/// The views a light's shadow renders through (clip space, forward-Z), one per tile.
fn views(l: &Light) -> Vec<Mat4> {
    let near = (l.radius * 0.01).max(1.0);
    let far = l.radius.max(near + 1.0);
    let face = |dir: Vec3, up: Vec3, fov: f32| rh::proj::directx::perspective(fov, 1.0, near, far) * rh::view::look_at_mat4(l.position, l.position + dir, up);
    match (l.kind, l.cone) {
        (kind::SPOT, Some((_, outer))) => {
            let dir = l.forward();
            let up = if dir.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
            vec![face(dir, up, (2.0 * outer).clamp(1.0, 170.0).to_radians())]
        }
        (kind::POINT, _) => {
            let fov = 90f32.to_radians();
            [(Vec3::X, Vec3::Z), (-Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z), (-Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X), (-Vec3::Z, Vec3::X)].map(|(d, u)| face(d, u, fov)).to_vec()
        }
        _ => Vec::new(),
    }
}

/// Clip space → tile `i`'s rect of the atlas in uv (y down), depth kept.
fn to_tile(i: u32) -> Mat4 {
    let s = 1.0 / SIDE as f32;
    let (x, y) = ((i % SIDE) as f32 * s, (i / SIDE) as f32 * s);
    Mat4::from_cols(Vec4::new(0.5 * s, 0.0, 0.0, 0.0), Vec4::new(0.0, -0.5 * s, 0.0, 0.0), Vec4::Z, Vec4::new(x + 0.5 * s, y + 0.5 * s, 0.0, 1.0))
}

fn viewport(i: u32) -> RenderViewport {
    let (x, y) = ((i % SIDE * TILE) as i32, (i / SIDE * TILE) as i32);
    RenderViewport { m_n_top_left_x: x, m_n_top_left_y: y, m_n_width: TILE as i32, m_n_height: TILE as i32, max_z: 1.0, ..Default::default() }
}

/// Render the shadow tiles of `lighting`'s shadow-casting point and spot lights and record them.
pub fn render(lighting: &mut Lighting, worlds: &[world::Ref]) {
    let mut out = LocalShadows { map: map(), first: vec![None; lighting.lights.len()], ..Default::default() };
    let mut cleared = false;
    for (n, l) in lighting.lights.iter().enumerate().filter(|(_, l)| l.shadows) {
        let views = views(l);
        if views.is_empty() || out.tiles.len() + views.len() > TILES {
            continue;
        }
        out.first[n] = Some(out.tiles.len() as u32);
        for view in views {
            let tile = out.tiles.len() as u32;
            let mut draws = Vec::new();
            for (r, obj) in object::refs_in_worlds(worlds) {
                let deform = model_draw::Deform::of(r);
                let s = obj.state.lock().unwrap();
                if s.rendering && obj.light.is_none() && deform.casts_shadow() {
                    draws.extend(model_draw::shadow_draws(&s, &deform, view));
                }
            }
            let target = Target { depth: out.map, viewport: viewport(tile), ..Default::default() };
            // The first tile's clear covers the whole atlas.
            draw::execute(&target, lighting, Load { color: None, depth: (!cleared).then_some(1.0) }, &draws);
            cleared = true;
            out.tiles.push(to_tile(tile) * view);
        }
    }
    if !out.tiles.is_empty() {
        lighting.local = Some(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn light(kind: u32, cone: Option<(f32, f32)>) -> Light {
        Light { kind, position: Vec3::new(0.0, 0.0, 100.0), rotation: Quat::from_rotation_arc(Vec3::X, -Vec3::Z), color: Vec3::ONE, radius: 300.0, quadratic: 1e-4, cone, fog: 1.0, shadows: true }
    }

    #[test]
    fn a_point_light_sees_every_direction_in_its_own_tiles() {
        let l = light(kind::POINT, None);
        let views = views(&l);
        assert_eq!(views.len(), 6);
        for (i, p) in [Vec3::new(50.0, 0.0, 100.0), Vec3::new(-50.0, 0.0, 100.0), Vec3::new(0.0, 50.0, 100.0), Vec3::new(0.0, -50.0, 100.0), Vec3::new(0.0, 0.0, 150.0), Vec3::new(0.0, 0.0, 50.0)].into_iter().enumerate() {
            let t = (to_tile(i as u32) * views[i]).project_point3(p);
            let (x0, y0) = ((i as u32 % SIDE) as f32 / SIDE as f32, (i as u32 / SIDE) as f32 / SIDE as f32);
            let centre = Vec3::new(x0 + 0.125, y0 + 0.125, t.z);
            assert!(t.distance(centre) < 1e-4 && (0.0..1.0).contains(&t.z), "face {i}: {t}");
        }
    }

    #[test]
    fn a_spot_light_looks_down_its_cone() {
        let l = light(kind::SPOT, Some((20.0, 30.0)));
        let views = views(&l);
        assert_eq!(views.len(), 1);
        let t = (to_tile(0) * views[0]).project_point3(Vec3::new(0.0, 0.0, 0.0));
        assert!(t.truncate().distance(glam::Vec2::splat(0.125)) < 1e-4, "{t}");
    }
}
