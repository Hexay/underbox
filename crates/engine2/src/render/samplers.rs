//! Sampler states managed describes (`CSamplerStateDesc`, from `SamplerState`): each distinct
//! description gets a bindless index (`FindOrCreateSamplerState`/`GetSamplerIndex`), and draws
//! get the wgpu sampler it describes, for the bindless sampler array and named sampler attributes.

use abi::structs::CSamplerStateDesc;
use render::wgpu::{self, AddressMode, FilterMode, SamplerBindingType};
use std::collections::HashMap;
use std::sync::Mutex;

/// Distinct descriptions in registration order: index `i` is handle `i + 1`.
static DESCS: Mutex<Vec<CSamplerStateDesc>> = Mutex::new(Vec::new());

type Bytes = [u8; size_of::<CSamplerStateDesc>()];

fn bytes(d: &CSamplerStateDesc) -> Bytes {
    // SAFETY: CSamplerStateDesc is a plain repr(C) struct of scalars.
    unsafe { std::mem::transmute_copy(d) }
}

/// The handle for `desc`: 1 + its bindless index.
pub fn find_or_create(desc: CSamplerStateDesc) -> usize {
    let mut descs = DESCS.lock().unwrap();
    let key = bytes(&desc);
    let i = descs.iter().position(|d| bytes(d) == key).unwrap_or_else(|| {
        descs.push(desc);
        descs.len() - 1
    });
    i + 1
}

/// The description at bindless index `index`.
pub fn desc(index: usize) -> Option<CSamplerStateDesc> {
    DESCS.lock().unwrap().get(index).copied()
}

/// Descriptions registered so far.
pub fn count() -> usize {
    DESCS.lock().unwrap().len()
}

fn address(mode: u8) -> AddressMode {
    match mode {
        1 | 4 => AddressMode::MirrorRepeat,
        2 => AddressMode::ClampToEdge,
        3 => AddressMode::ClampToBorder,
        _ => AddressMode::Repeat,
    }
}

/// `desc` as wgpu describes it, for a binding of `kind`. D3D11 filter bits (managed's
/// `FilterMode`): 0x01 linear mips, 0x04 linear magnification, 0x10 linear minification, 0x40
/// anisotropic. A non-filtering binding gets nearest filtering whatever the description says.
/// Border addressing needs `ADDRESS_MODE_CLAMP_TO_BORDER`; without it (`border_ok` false) it clamps
/// to the edge.
pub fn descriptor(desc: &CSamplerStateDesc, kind: SamplerBindingType, border_ok: bool) -> wgpu::SamplerDescriptor<'static> {
    let f = if kind == SamplerBindingType::NonFiltering { 0 } else { desc.m_n_filter_mode };
    let anisotropic = f & 0x40 != 0;
    let linear = |bit: u8| if anisotropic || f & bit != 0 { FilterMode::Linear } else { FilterMode::Nearest };
    // Anisotropy needs every filter linear.
    let clamp = if anisotropic { 1u16 << desc.m_n_aniso_exp.min(4) } else { 1 };
    let mode = |m: u8| match address(m) {
        AddressMode::ClampToBorder if !border_ok => AddressMode::ClampToEdge,
        a => a,
    };
    let modes = [mode(desc.m_n_address_u), mode(desc.m_n_address_v), mode(desc.m_n_address_w)];
    let border = modes.contains(&AddressMode::ClampToBorder).then(|| border_color(desc.m_n_border_color8_bit));
    wgpu::SamplerDescriptor {
        label: Some("sampler state"),
        address_mode_u: modes[0],
        address_mode_v: modes[1],
        address_mode_w: modes[2],
        mag_filter: linear(0x04),
        min_filter: linear(0x10),
        mipmap_filter: if anisotropic || f & 0x01 != 0 { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest },
        lod_min_clamp: f32::from(desc.m_n_min_lod),
        lod_max_clamp: f32::from(desc.m_n_max_lod.max(desc.m_n_min_lod)),
        anisotropy_clamp: clamp.max(1),
        border_color: border,
        ..Default::default()
    }
}

/// The nearest of wgpu's fixed border colours to an RGBA8 one (red in the low byte).
fn border_color(rgba: u32) -> wgpu::SamplerBorderColor {
    match rgba {
        0xFFFF_FFFF => wgpu::SamplerBorderColor::OpaqueWhite,
        c if c >> 24 == 0 => wgpu::SamplerBorderColor::TransparentBlack,
        _ => wgpu::SamplerBorderColor::OpaqueBlack,
    }
}

/// The wgpu sampler `desc` describes for a binding of `kind`, created once.
pub fn sampler(gpu: &render::Gpu, desc: &CSamplerStateDesc, kind: SamplerBindingType) -> wgpu::Sampler {
    type Key = (Bytes, SamplerBindingType);
    static CACHE: Mutex<Option<HashMap<Key, wgpu::Sampler>>> = Mutex::new(None);
    let border_ok = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
    let mut cache = CACHE.lock().unwrap();
    cache.get_or_insert_with(HashMap::new).entry((bytes(desc), kind)).or_insert_with(|| gpu.device.create_sampler(&descriptor(desc, kind, border_ok))).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(filter: u8, address: u8, aniso_exp: u8) -> CSamplerStateDesc {
        let mut d = CSamplerStateDesc::default();
        (d.m_n_filter_mode, d.m_n_address_u, d.m_n_address_v, d.m_n_address_w, d.m_n_aniso_exp, d.m_n_max_lod) = (filter, address, address, address, aniso_exp, 15);
        d
    }

    #[test]
    fn managed_filter_and_address_modes() {
        let point = descriptor(&desc(0x00, 2, 0), SamplerBindingType::Filtering, true);
        assert_eq!((point.min_filter, point.mag_filter, point.address_mode_u), (FilterMode::Nearest, FilterMode::Nearest, AddressMode::ClampToEdge));
        let bilinear = descriptor(&desc(0x14, 1, 0), SamplerBindingType::Filtering, true);
        assert_eq!((bilinear.min_filter, bilinear.mipmap_filter, bilinear.address_mode_v), (FilterMode::Linear, wgpu::MipmapFilterMode::Nearest, AddressMode::MirrorRepeat));
        let aniso = descriptor(&desc(0x55, 0, 3), SamplerBindingType::Filtering, true);
        assert_eq!((aniso.anisotropy_clamp, aniso.mipmap_filter), (8, wgpu::MipmapFilterMode::Linear));
        let forced = descriptor(&desc(0x55, 0, 3), SamplerBindingType::NonFiltering, true);
        assert_eq!((forced.min_filter, forced.anisotropy_clamp), (FilterMode::Nearest, 1));
    }

    /// Grubs' terrain material: a border-addressed sampler state on a device without the feature
    /// made an invalid sampler, so the whole material fell back to the placeholder.
    #[test]
    fn border_addressing_follows_the_device() {
        let mut d = desc(0x14, 3, 0);
        d.m_n_border_color8_bit = 0xFFFF_FFFF;
        let with = descriptor(&d, SamplerBindingType::Filtering, true);
        assert_eq!((with.address_mode_u, with.border_color), (AddressMode::ClampToBorder, Some(wgpu::SamplerBorderColor::OpaqueWhite)));
        let without = descriptor(&d, SamplerBindingType::Filtering, false);
        assert_eq!((without.address_mode_w, without.border_color), (AddressMode::ClampToEdge, None));
        let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
        assert!(gpu.checked(|| sampler(gpu, &d, SamplerBindingType::Filtering)).is_ok(), "valid on this device either way");
    }

    #[test]
    fn equal_descriptions_share_an_index() {
        let a = find_or_create(desc(0x14, 2, 0));
        assert_eq!(find_or_create(desc(0x14, 2, 0)), a);
        assert_ne!(find_or_create(desc(0x15, 2, 0)), a);
        assert_eq!(super::desc(a - 1).map(|d| d.m_n_filter_mode), Some(0x14));
    }
}
