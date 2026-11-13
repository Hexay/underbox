//! `GetWellKnownTexture`: native's fixed default textures, by `SceneSystemWellKnownTextureObjectID`.

use crate::render::texture::{self, TSPEC_CUBE_TEXTURE, TSPEC_TEXTURE_ARRAY, TSPEC_VOLUME_TEXTURE};
use abi::NativeRef;
use abi::structs::TextureCreationConfig_t;
use std::sync::OnceLock;

const RGBA8888: i32 = 0;
const R16F: i32 = 59;
const COUNT: usize = 13;
const WHITE: [u8; 4] = [255; 4];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const CLEAR: [u8; 4] = [0; 4];

/// (width, height, depth, flags, format, one texel or the whole image) of `id`.
fn spec(id: usize) -> (i16, i16, i16, u32, i32, Vec<u8>) {
    let rgba = |w, h, d, flags, texel: [u8; 4]| (w, h, d, flags, RGBA8888, texel.repeat(w as usize * h as usize * d as usize));
    // Native's 16-bit unorm one; R16 has no wgpu format without an optional feature, so a half 1.0.
    let one = |flags| (1, 1, 1, flags, R16F, vec![0x00, 0x3c]);
    match id {
        1 => rgba(1, 1, 1, 0, BLACK),
        2 => rgba(1, 1, 1, 0, CLEAR),
        3 => rgba(1, 1, 1, TSPEC_TEXTURE_ARRAY, WHITE),
        4 => (2, 2, 1, 0, RGBA8888, [BLACK, WHITE, WHITE, BLACK].concat()),
        5 => rgba(2, 2, 2, TSPEC_VOLUME_TEXTURE, CLEAR),
        7 => (1, 1, 1, TSPEC_CUBE_TEXTURE, RGBA8888, BLACK.repeat(6)),
        8 => (1, 1, 1, TSPEC_CUBE_TEXTURE | TSPEC_TEXTURE_ARRAY, RGBA8888, BLACK.repeat(6)),
        9 => rgba(1, 1, 1, TSPEC_TEXTURE_ARRAY, CLEAR),
        10 => rgba(2, 2, 2, TSPEC_VOLUME_TEXTURE, BLACK),
        11 => one(0),
        12 => one(TSPEC_TEXTURE_ARRAY),
        // White, and the light cookie sheet (0, 6) until cookies are sheeted.
        _ => rgba(1, 1, 1, 0, WHITE),
    }
}

/// The texture for `id`, made once; unknown ids get the white one.
pub fn texture(id: i64) -> texture::Ref {
    static MADE: [OnceLock<usize>; COUNT] = [const { OnceLock::new() }; COUNT];
    let id = usize::try_from(id).ok().filter(|&i| i < COUNT).unwrap_or(0);
    let addr = *MADE[id].get_or_init(|| {
        let (w, h, d, flags, format, mut data) = spec(id);
        let mut config = TextureCreationConfig_t::default();
        (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (w, h, d, 1);
        (config.m_n_flags, config.m_n_image_format) = (flags, format);
        let name = std::ffi::CString::new(format!("well-known {id}")).unwrap_or_default();
        texture::create(Some(&name), true, config, &mut data).addr()
    });
    NativeRef::from_raw(addr as *mut _)
}

#[cfg(test)]
mod tests {
    #[test]
    fn well_known_textures_hold_their_defaults() {
        let black = super::texture(1);
        let t = crate::render::texture::resolve(black).expect("a texture");
        assert_eq!(t.cpu_mip0().expect("rgba8").pixels, [0, 0, 0, 255]);
        assert_eq!(super::texture(1).addr(), black.addr(), "made once");
        let checker = crate::render::texture::resolve(super::texture(4)).unwrap().cpu_mip0().unwrap();
        assert_eq!((checker.width, checker.pixels[..8].to_vec()), (2, vec![0, 0, 0, 255, 255, 255, 255, 255]));
        assert_eq!(super::texture(99).addr(), super::texture(0).addr(), "unknown ids are white");
    }
}
