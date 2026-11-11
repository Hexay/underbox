//! Compiled textures (`.vtex_c`) decoded on the CPU into what `render::texture::create` takes:
//! mip 0 as RGBA8888, or RGBA16161616F for HDR formats, with cube faces and array slices as
//! depth layers. Only mip 0 is kept because the GPU upload takes exactly one level.

use abi::structs::TextureCreationConfig_t;
use assets::compiled::texture::{self as vtex, Pixels};

/// `ImageFormat` values the decoded pixels are in.
const RGBA8888: i32 = 0;
const RGBA16161616F: i32 = 24;

pub struct Decoded {
    pub config: TextureCreationConfig_t,
    pub data: Vec<u8>,
}

/// The `.vtex_c` resource path for `path` (`materials/x.vtex`, `_c` optional).
pub fn compiled_path(path: &str) -> String {
    let path = path.strip_suffix("_c").unwrap_or(path);
    if path.ends_with(".vtex") { format!("{path}_c") } else { format!("{path}.vtex_c") }
}

/// Every surface of mip 0 decoded and stacked; `None` for formats we don't decode.
pub fn decode(t: &vtex::Texture) -> Option<Decoded> {
    let (_, _, depth) = t.level_size(0);
    let layers = if t.format.is_image_file() { 1 } else { depth * t.faces() };
    let mut data = Vec::new();
    let mut format = RGBA8888;
    let (mut width, mut height) = (0, 0);
    for i in 0..layers {
        let image = vtex::decode(t, 0, i / t.faces(), i % t.faces())?;
        (width, height) = (image.width, image.height);
        match image.pixels {
            Pixels::Rgba8(p) => data.extend(p),
            Pixels::Rgba16F(p) => {
                format = RGBA16161616F;
                data.extend(p.iter().flat_map(|h| h.to_le_bytes()));
            }
        }
    }
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (width as i16, height as i16, layers as i16, 1);
    config.m_n_image_format = format;
    // As managed's builders describe them: a cube's depth counts cubes, not faces.
    if t.is_cube() && !t.format.is_image_file() {
        config.m_n_depth = (layers / 6) as i16;
        config.m_n_flags |= crate::render::texture::TSPEC_CUBE_TEXTURE;
    }
    if layers > 1 && !t.is_cube() && t.flags & vtex::flags::VOLUME == 0 {
        config.m_n_flags |= crate::render::texture::TSPEC_TEXTURE_ARRAY;
    }
    if let Some((w, h)) = t.display {
        (config.m_n_display_rect_width, config.m_n_display_rect_height) = (w as i16, h as i16);
    }
    Some(Decoded { config, data })
}

/// The compiled texture at `path`, decoded; `None` without a readable `.vtex_c`.
pub fn load(path: &str) -> Option<Decoded> {
    let file = crate::fs::read(&compiled_path(path), "GAME")?;
    let t = vtex::read(&file).inspect_err(|e| abi::rt::log::warn(&format!("{path}: {e:?}"))).ok()?;
    let decoded = decode(&t);
    if decoded.is_none() {
        abi::rt::log::warn(&format!("{path}: texture format {} isn't decoded", t.format.0));
    }
    decoded
}
