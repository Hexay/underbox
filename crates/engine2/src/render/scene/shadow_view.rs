//! `CSceneSystem::AddShadowView`: managed's shadow mapper (ShadowMapper.Native.cs) asks for one per
//! sun cascade and local-light face while ours calls it from the view's lighting
//! (render/lighting/shadow.rs). Native queues a child view; ours draws it at once, before the
//! parent's layers: the parent's casters inside the shadow frustum, depth only, back faces culled,
//! managed's rasterizer bias, one instanced draw per mesh, scissored to the casters' bounds so
//! the next frame clears only that rect (render/dirty_clear.rs).

use super::shadow_cache::{self, Key};
use super::view;
use crate::frustum::camera::Camera;
use crate::render::{depth, dirty_clear};
use crate::render::draw::{self, Target, caster, model as model_draw};
use crate::render::lighting::Lighting;
use crate::render::texture;
use crate::scene::object;
use abi::structs::{NativeRect, RenderViewport};
use abi::{NativeRef, class};
use glam::{Mat4, Vec2, Vec3, Vec4Swizzles};
use render::RasterState;
use render::pass::Load;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// What one shadow view draws: `required` and `excluded` are `SceneObjectFlags` masks.
pub struct ShadowView {
    pub parent: view::Ref,
    pub frustum: NativeRef<class::CFrustum>,
    pub viewport: RenderViewport,
    pub target: texture::Ref,
    pub slice: u32,
    pub required: i64,
    pub excluded: i64,
    pub bias: (i32, f32),
}

/// The shadow camera's world → clip, as managed reads it back (`GetReverseZViewProjTranspose`).
fn camera(frustum: NativeRef<class::CFrustum>) -> Option<Camera> {
    objects::get::<_, Mutex<Camera>>(frustum).map(|c| *c.lock().unwrap())
}

fn view_proj(c: &Camera) -> Mat4 {
    let p = c.proj();
    if depth::SHADOW.is_reverse() { Camera::reverse_z(p) * c.view() } else { p * c.view() }
}

/// Where a view's casters can land in its map.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reach {
    Nothing,
    /// Inside this normalized-device-coordinate box (x and y, min and max).
    Ndc(Vec2, Vec2),
    /// Unbounded: a deformed caster, or a box reaching behind a perspective camera.
    Anywhere,
}

impl Reach {
    fn add(self, other: Reach) -> Reach {
        match (self, other) {
            (Reach::Nothing, r) | (r, Reach::Nothing) => r,
            (Reach::Ndc(a, b), Reach::Ndc(c, d)) => Reach::Ndc(a.min(c), b.max(d)),
            _ => Reach::Anywhere,
        }
    }

    /// The world box `lo..hi` seen through `view_proj`.
    fn of_box(view_proj: Mat4, lo: Vec3, hi: Vec3) -> Reach {
        (0..8).fold(Reach::Nothing, |r, i| {
            let clip = view_proj * Vec3::new([lo.x, hi.x][i & 1], [lo.y, hi.y][(i >> 1) & 1], [lo.z, hi.z][i >> 2]).extend(1.0);
            if clip.w <= f32::EPSILON {
                return Reach::Anywhere;
            }
            let ndc = clip.xy() / clip.w;
            r.add(Reach::Ndc(ndc, ndc))
        })
    }

    /// The texels of `vp` (`[x, y, w, h]`) this reach can cover, a texel wider on each side;
    /// `None` when that's none of them.
    fn texels(self, [x, y, w, h]: [u32; 4]) -> Option<[u32; 4]> {
        let (lo, hi) = match self {
            Reach::Nothing => return None,
            Reach::Anywhere => return (w > 0 && h > 0).then_some([x, y, w, h]),
            Reach::Ndc(lo, hi) => (lo, hi),
        };
        let (fw, fh) = (w as f32, h as f32);
        let clamp = |v: f32, size: f32| v.clamp(0.0, size) as u32;
        let x0 = clamp(((lo.x * 0.5 + 0.5) * fw).floor() - 1.0, fw);
        let x1 = clamp(((hi.x * 0.5 + 0.5) * fw).ceil() + 1.0, fw);
        let y0 = clamp(((0.5 - hi.y * 0.5) * fh).floor() - 1.0, fh);
        let y1 = clamp(((0.5 - lo.y * 0.5) * fh).ceil() + 1.0, fh);
        (x1 > x0 && y1 > y0).then_some([x + x0, y + y0, x1 - x0, y1 - y0])
    }
}

/// The parent's shadow casters inside `c`, with the view's flag masks, and where they can land.
/// Each caster's object inputs go into `key`.
fn casters(v: &ShadowView, c: &Camera, view_proj: Mat4, mut key: Option<&mut Key>) -> (Vec<draw::Draw>, Reach) {
    let worlds = view::with(v.parent, |_, s| s.worlds.clone());
    let mut draws = Vec::new();
    let mut reach = Reach::Nothing;
    for (r, obj) in object::refs_in_worlds(&worlds) {
        let deform = model_draw::Deform::of(r);
        let s = obj.state.lock().unwrap();
        let flags = s.flags;
        let wanted = flags & v.required == v.required && flags & v.excluded == 0;
        if !(s.rendering && obj.light.is_none() && deform.casts_shadow() && wanted) {
            continue;
        }
        let b = s.world_bounds();
        let (lo, hi) = (crate::math::v3(b.mins), crate::math::v3(b.maxs));
        if !c.intersects_box(lo, hi) {
            continue;
        }
        let these = model_draw::shadow_draws(&s, &deform, view_proj);
        if these.is_empty() {
            continue;
        }
        // Managed may set an object's bounds; the model's own, moved, back them up.
        reach = reach.add(match deform.rigid() {
            true => {
                let (mlo, mhi) = crate::math::moved_box(model_draw::matrix(&s.transform), crate::model::bounds(s.model));
                Reach::of_box(view_proj, lo.min(mlo), hi.max(mhi))
            }
            false => Reach::Anywhere,
        });
        if let Some(k) = key.as_deref_mut() {
            k.write_usize(r.addr());
            deform.hash_inputs(k);
            k.mat(&model_draw::matrix(&s.transform));
            k.floats(&model_draw::tint(s.tint));
            (s.model.addr(), s.mesh_groups, s.lod, flags).hash(k);
            s.material_override.is_some().hash(k);
            if let Some(m) = &s.material_override {
                k.pin(m);
            }
        }
        draws.extend(these);
    }
    (draws, reach)
}

/// [`draw`] with the cache as `UNDERBOX_SHADOW_CACHE` sets it.
pub fn render(v: &ShadowView) -> bool {
    draw(v, shadow_cache::enabled())
}

/// Draw `v` into its target slice: cleared, then every caster, scissored to where they can land.
/// With `cache`, nothing when the slice still holds that render (shadow_cache.rs). Returns
/// whether it drew.
pub fn draw(v: &ShadowView, cache: bool) -> bool {
    let Some(c) = camera(v.frustum) else { return false };
    let Some(t) = texture::resolve(v.target) else { return false };
    let Some(g) = &t.gpu else { return false };
    let view_proj = view_proj(&c);
    let state = RasterState { cull: Some(render::wgpu::Face::Back), depth_compare: depth::SHADOW.compare(), depth_bias: depth::SHADOW.bias(v.bias.0, v.bias.1), ..Default::default() };
    let mut key = cache.then(shadow_cache::key);
    let (draws, reach) = casters(v, &c, view_proj, key.as_mut());
    let [x, y, w, h] = draw::viewport_rect(g, &v.viewport);
    let (w, h) = (w.min(g.desc.width.saturating_sub(x)), h.min(g.desc.height.saturating_sub(y)));
    let scissor = reach.texels([x, y, w, h]);
    let draws: Vec<_> = match scissor {
        Some(_) => draws.into_iter().map(|d| draw::Draw { state: RasterState { topology: d.state.topology, ..state }, ..d }).collect(),
        None => Vec::new(),
    };
    let draws = caster::instanced(draws, view_proj);
    if let Some(k) = key.as_mut() {
        k.mat(&view_proj);
        let vp = &v.viewport;
        (vp.m_n_top_left_x, vp.m_n_top_left_y, vp.m_n_width, vp.m_n_height, [x, y, w, h], scissor, state, g.desc.format).hash(k);
        k.floats(&[vp.min_z, vp.max_z]);
        caster::program().is_some().hash(k);
        if let Some(p) = caster::program() {
            k.pin(&p);
        }
        draws.iter().for_each(|d| k.draw(d));
    }
    if let Some(k) = key.take() {
        match shadow_cache::holds(&t, v.slice, k) {
            Ok(()) => {
                crate::render::stats::drew(&draws);
                return false;
            }
            Err(k) => key = Some(k),
        }
    }
    let before = g.writes();
    let prepared = dirty_clear::prepare(&t, v.slice, render::wgpu::Color::TRANSPARENT, depth::SHADOW.clear());
    let passes = u64::from(prepared.full || !draws.is_empty());
    let scissor_rect = scissor.map(|[x, y, w, h]| NativeRect { x: x as i32, y: y as i32, w: w as i32, h: h as i32 });
    let target = Target { depth: v.target, depth_slice: v.slice, viewport: v.viewport, scissor: scissor_rect, ..Default::default() };
    let load = Load { color: None, depth: prepared.full.then(|| depth::SHADOW.clear()) };
    draw::execute(&target, &Lighting::default(), load, &draws);
    let ours = dirty_clear::finish(&t, v.slice, prepared, passes, scissor);
    if let Some(k) = key {
        shadow_cache::rendered(&t, v.slice, before, ours, k);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::Reach;
    use glam::{Mat4, Vec2, Vec3};

    #[test]
    fn reach_covers_the_box_a_texel_wider_with_y_down() {
        let r = Reach::Ndc(Vec2::new(-0.5, 0.0), Vec2::new(0.0, 0.5));
        assert_eq!(r.texels([0, 0, 100, 100]), Some([24, 24, 27, 27]));
        assert_eq!(r.texels([10, 20, 100, 100]), Some([34, 44, 27, 27]));
    }

    #[test]
    fn reach_clamps_to_the_viewport_and_drops_what_misses_it() {
        assert_eq!(Reach::Ndc(Vec2::splat(-3.0), Vec2::splat(3.0)).texels([0, 0, 64, 64]), Some([0, 0, 64, 64]));
        assert_eq!(Reach::Ndc(Vec2::splat(2.0), Vec2::splat(3.0)).texels([0, 0, 64, 64]), None);
        assert_eq!(Reach::Nothing.texels([0, 0, 64, 64]), None);
        assert_eq!(Reach::Anywhere.texels([8, 8, 32, 16]), Some([8, 8, 32, 16]));
    }

    #[test]
    fn reach_unions_and_gives_up_behind_the_camera() {
        let a = Reach::Ndc(Vec2::ZERO, Vec2::ONE);
        assert_eq!(a.add(Reach::Ndc(Vec2::splat(-1.0), Vec2::ZERO)), Reach::Ndc(Vec2::splat(-1.0), Vec2::ONE));
        assert_eq!(a.add(Reach::Nothing), a);
        assert_eq!(a.add(Reach::Anywhere), Reach::Anywhere);
        let persp = Mat4::from_cols_array_2d(&[[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 0.0]]);
        assert_eq!(Reach::of_box(persp, Vec3::new(-1.0, -1.0, -5.0), Vec3::new(1.0, 1.0, 5.0)), Reach::Anywhere);
        assert!(matches!(Reach::of_box(Mat4::IDENTITY, Vec3::splat(-0.5), Vec3::splat(0.5)), Reach::Ndc(lo, hi) if lo == Vec2::splat(-0.5) && hi == Vec2::splat(0.5)));
    }
}
