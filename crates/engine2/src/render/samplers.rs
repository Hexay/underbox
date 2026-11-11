//! Sampler states managed describes (`CSamplerStateDesc`, from `SamplerState`): each distinct
//! description gets a bindless index (`FindOrCreateSamplerState`/`GetSamplerIndex`), and draws
//! get the wgpu sampler it describes, for the bindless sampler array and named sampler attributes.

use abi::structs::CSamplerStateDesc;
use render::wgpu::{self, AddressMode, FilterMode, SamplerBindingType};
use std::collections::HashMap;
use std::sync::Mutex;

/// Distinct descriptions in registration order: index `i` is handle `i + 1`.
static DESCS: Mutex<Vec<CSamplerStateDesc>> = Mutex::new(Vec::new());

fn bytes(d: &CSamplerStateDesc) -> Vec<u8> {
    // SAFETY: CSamplerStateDesc is a plain repr(C) struct of scalars.
    unsafe { std::slice::from_raw_parts((d as *const CSamplerStateDesc).cast::<u8>(), size_of::<CSamplerStateDesc>()) }.to_vec()
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
pub fn descriptor(desc: &CSamplerStateDesc, kind: SamplerBindingType) -> wgpu::SamplerDescriptor<'static> {
    let f = if kind == SamplerBindingType::NonFiltering { 0 } else { desc.m_n_filter_mode };
    let anisotropic = f & 0x40 != 0;
    let linear = |bit: u8| if anisotropic || f & bit != 0 { FilterMode::Linear } else { FilterMode::Nearest };
    // Anisotropy needs every filter linear.
    let clamp = if anisotropic { 1u16 << desc.m_n_aniso_exp.min(4) } else { 1 };
    let border = (address(desc.m_n_address_u) == AddressMode::ClampToBorder).then_some(wgpu::SamplerBorderColor::TransparentBlack);
    wgpu::SamplerDescriptor {
        label: Some("sampler state"),
        address_mode_u: address(desc.m_n_address_u),
        address_mode_v: address(desc.m_n_address_v),
        address_mode_w: address(desc.m_n_address_w),
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

/// The wgpu sampler `desc` describes for a binding of `kind`, created once.
pub fn sampler(gpu: &render::Gpu, desc: &CSamplerStateDesc, kind: SamplerBindingType) -> wgpu::Sampler {
    type Key = (Vec<u8>, SamplerBindingType);
    static CACHE: Mutex<Option<HashMap<Key, wgpu::Sampler>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    cache.get_or_insert_with(HashMap::new).entry((bytes(desc), kind)).or_insert_with(|| gpu.device.create_sampler(&descriptor(desc, kind))).clone()
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
        let point = descriptor(&desc(0x00, 2, 0), SamplerBindingType::Filtering);
        assert_eq!((point.min_filter, point.mag_filter, point.address_mode_u), (FilterMode::Nearest, FilterMode::Nearest, AddressMode::ClampToEdge));
        let bilinear = descriptor(&desc(0x14, 1, 0), SamplerBindingType::Filtering);
        assert_eq!((bilinear.min_filter, bilinear.mipmap_filter, bilinear.address_mode_v), (FilterMode::Linear, wgpu::MipmapFilterMode::Nearest, AddressMode::MirrorRepeat));
        let aniso = descriptor(&desc(0x55, 0, 3), SamplerBindingType::Filtering);
        assert_eq!((aniso.anisotropy_clamp, aniso.mipmap_filter), (8, wgpu::MipmapFilterMode::Linear));
        let forced = descriptor(&desc(0x55, 0, 3), SamplerBindingType::NonFiltering);
        assert_eq!((forced.min_filter, forced.anisotropy_clamp), (FilterMode::Nearest, 1));
    }

    #[test]
    fn equal_descriptions_share_an_index() {
        let a = find_or_create(desc(0x14, 2, 0));
        assert_eq!(find_or_create(desc(0x14, 2, 0)), a);
        assert_ne!(find_or_create(desc(0x15, 2, 0)), a);
        assert_eq!(super::desc(a - 1).map(|d| d.m_n_filter_mode), Some(0x14));
    }
}
