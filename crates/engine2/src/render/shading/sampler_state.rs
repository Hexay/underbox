//! A vfx sampler's own state, from its annotation: `Filter( POINT ); AddressU( CLAMP ); ...`
//! (common_samplers.fxc). Without one a sampler is trilinear and wraps.

use render::wgpu::{self, AddressMode, FilterMode, MipmapFilterMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SamplerState {
    /// min, mag
    pub filter: [FilterMode; 2],
    pub mip: MipmapFilterMode,
    pub anisotropic: bool,
    pub address: [AddressMode; 3],
}

impl Default for SamplerState {
    fn default() -> Self {
        SamplerState { filter: [FilterMode::Linear; 2], mip: MipmapFilterMode::Linear, anisotropic: false, address: [AddressMode::Repeat; 3] }
    }
}

impl SamplerState {
    pub fn parse(filter: Option<&str>, address: [Option<&str>; 3]) -> SamplerState {
        let mut s = SamplerState::default();
        if let Some(f) = filter {
            s.set_filter(f.trim());
        }
        for (mode, a) in s.address.iter_mut().zip(address) {
            *mode = match a.map(str::trim) {
                Some("CLAMP") => AddressMode::ClampToEdge,
                Some("MIRROR" | "MIRROR_ONCE") => AddressMode::MirrorRepeat,
                Some("BORDER") => AddressMode::ClampToBorder,
                _ => AddressMode::Repeat,
            };
        }
        s
    }

    /// Shorthands, or D3D names read word by word: `MIN_MAG_LINEAR_MIP_POINT` sets min and mag
    /// linear, mip point.
    fn set_filter(&mut self, f: &str) {
        let f = ["COMPARISON_", "MINIMUM_", "MAXIMUM_"].iter().fold(f, |f, p| f.strip_prefix(p).unwrap_or(f));
        let (min_mag, mip) = match f {
            "POINT" => ("POINT", "POINT"),
            "BILINEAR" => ("LINEAR", "POINT"),
            "TRILINEAR" => ("LINEAR", "LINEAR"),
            _ if f.starts_with("ANISO") => {
                self.anisotropic = true;
                return;
            }
            _ => {
                let mut pending: Vec<&str> = Vec::new();
                for word in f.split('_') {
                    match word {
                        "MIN" | "MAG" | "MIP" => pending.push(word),
                        "POINT" | "LINEAR" => {
                            let mode = if word == "POINT" { FilterMode::Nearest } else { FilterMode::Linear };
                            for target in pending.drain(..) {
                                match target {
                                    "MIN" => self.filter[0] = mode,
                                    "MAG" => self.filter[1] = mode,
                                    _ => self.mip = if word == "POINT" { MipmapFilterMode::Nearest } else { MipmapFilterMode::Linear },
                                }
                            }
                        }
                        _ => return,
                    }
                }
                return;
            }
        };
        let mode = |w| if w == "POINT" { FilterMode::Nearest } else { FilterMode::Linear };
        self.filter = [mode(min_mag); 2];
        self.mip = if mip == "POINT" { MipmapFilterMode::Nearest } else { MipmapFilterMode::Linear };
    }

    /// Border addressing clamps to the edge on a device without `ADDRESS_MODE_CLAMP_TO_BORDER`.
    pub fn descriptor(&self, border_ok: bool) -> wgpu::SamplerDescriptor<'static> {
        let address = self.address.map(|a| if a == AddressMode::ClampToBorder && !border_ok { AddressMode::ClampToEdge } else { a });
        wgpu::SamplerDescriptor {
            label: Some("default"),
            address_mode_u: address[0],
            address_mode_v: address[1],
            address_mode_w: address[2],
            min_filter: self.filter[0],
            mag_filter: self.filter[1],
            mipmap_filter: self.mip,
            border_color: address.contains(&AddressMode::ClampToBorder).then_some(wgpu::SamplerBorderColor::TransparentBlack),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_shorthands_and_d3d_names() {
        let point = SamplerState::parse(Some(" POINT "), [Some("CLAMP"), Some("CLAMP"), None]);
        assert_eq!((point.filter, point.mip), ([FilterMode::Nearest; 2], MipmapFilterMode::Nearest));
        assert_eq!(point.address, [AddressMode::ClampToEdge, AddressMode::ClampToEdge, AddressMode::Repeat]);
        let bilinear = SamplerState::parse(Some("BILINEAR"), [None; 3]);
        assert_eq!((bilinear.filter, bilinear.mip), ([FilterMode::Linear; 2], MipmapFilterMode::Nearest));
        let d3d = SamplerState::parse(Some("COMPARISON_MIN_POINT_MAG_LINEAR_MIP_POINT"), [None; 3]);
        assert_eq!((d3d.filter, d3d.mip), ([FilterMode::Nearest, FilterMode::Linear], MipmapFilterMode::Nearest));
        assert!(SamplerState::parse(Some("ANISOTROPIC"), [None; 3]).anisotropic);
        assert_eq!(SamplerState::parse(Some("SOMETHING_ELSE"), [None; 3]), SamplerState::default());
    }
}
