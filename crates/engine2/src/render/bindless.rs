//! Managed's bindless texture indices (`GetTextureViewIndex`) and the arrays they index
//! (`common/classes/Bindless.hlsl`). Every texture gets a linear/sRGB pair: the linear view at an
//! odd index and its sRGB view one above (`Bindless::GetTexture2DSrgb` adds 1). Index 0 is never
//! handed out, so a zeroed constant samples the default texture.

use crate::render::texture;
use crate::resource::Resource as _;
use render::wgpu;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Textures by pair, and each texture's pair by address.
#[derive(Default)]
struct Table {
    pairs: Vec<texture::Ref>,
    by_addr: HashMap<usize, u32>,
}

static TABLE: LazyLock<Mutex<Table>> = LazyLock::new(Default::default);

/// `RENDER_SRGB`, the colour space that names a pair's sRGB view.
pub const SRGB: u8 = 1;

/// The index of `t`'s linear (or `srgb`) view: stable for the texture's lifetime; 0 for null, which
/// shaders read as "no texture" (Decals.hlsl tests `> 0`; -1 masked to 16 bits indexed far out of
/// the array and crashed lavapipe).
pub fn index(t: texture::Ref, srgb: bool) -> i32 {
    if t.is_null() {
        return 0;
    }
    let mut table = TABLE.lock().unwrap();
    let Table { pairs, by_addr } = &mut *table;
    let pair = *by_addr.entry(t.addr()).or_insert_with(|| {
        pairs.push(t);
        pairs.len() as u32 - 1
    });
    (pair * 2 + 1 + u32::from(srgb)) as i32
}

/// The texture an index names and whether it is the sRGB view (the handle may be dead).
pub fn at(index: u32) -> Option<(texture::Ref, bool)> {
    let i = index.checked_sub(1)?;
    TABLE.lock().unwrap().pairs.get((i / 2) as usize).map(|t| (*t, i % 2 == 1))
}

/// One past the highest index handed out.
pub fn len() -> u32 {
    TABLE.lock().unwrap().pairs.len() as u32 * 2 + 1
}

/// Every texture with its linear index, in the order indices were handed out.
pub fn entries() -> Vec<(u32, texture::Ref)> {
    TABLE.lock().unwrap().pairs.iter().enumerate().map(|(pair, t)| (pair as u32 * 2 + 1, *t)).collect()
}

/// The view for one element of a bindless array declared with `dimension` and `sample_type`: the
/// texture `index` names when it is alive and has that shape, else `None` (the caller's default).
/// A texture in `targets` gets `None` too: wgpu drops the whole submit when a pass samples its own
/// attachment, even unread in an array (the depth prepass's normals target, walker).
pub fn view(index: u32, dimension: wgpu::TextureViewDimension, sample_type: wgpu::TextureSampleType, targets: &[texture::Ref]) -> Option<wgpu::TextureView> {
    let (t, srgb) = at(index)?;
    let t = texture::resolve(t).filter(|t| !t.is_error())?;
    if targets.iter().filter_map(|r| texture::resolve(*r)).any(|r| std::sync::Arc::ptr_eq(&r, &t)) {
        return None;
    }
    let gpu = t.gpu.as_ref()?;
    if gpu.dimension() != dimension || !crate::render::shading::bind::sample_type_fits(gpu.desc.format, sample_type) {
        return None;
    }
    Some(gpu.sample_view(srgb).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_start_at_one_with_srgb_above_linear() {
        let a = texture::create_target("bindless a", 1, 1, 0);
        let b = texture::create_target("bindless b", 1, 1, 0);
        let (la, sa, lb) = (index(a, false), index(a, true), index(b, false));
        assert!(la >= 1 && la % 2 == 1, "{la}");
        assert_eq!(sa, la + 1);
        assert_eq!(index(a, false), la, "stable");
        assert_ne!(lb, la);
        assert_eq!(at(la as u32).map(|(t, s)| (t.addr(), s)), Some((a.addr(), false)));
        assert_eq!(at(sa as u32).map(|(t, s)| (t.addr(), s)), Some((a.addr(), true)));
        assert!(at(0).is_none());
        assert_eq!(index(texture::Ref::default(), false), 0);
    }
}
