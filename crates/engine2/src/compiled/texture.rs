//! Compiled textures (`.vtex_c`) decoded on the CPU into what `render::texture::create` takes:
//! RGBA8888, or RGBA16161616F for HDR formats, with cube faces and array slices as depth layers.
//! Every stored mip is decoded, so minified samples filter like real's do.

use abi::structs::TextureCreationConfig_t;
use assets::compiled::texture::{self as vtex, Pixels};

/// `ImageFormat` values the decoded pixels are in.
const RGBA8888: i32 = 0;
const RGBA16161616F: i32 = 24;

pub struct Decoded {
    pub config: TextureCreationConfig_t,
    /// Mip 0, every layer.
    pub data: Vec<u8>,
    /// Mips 1 and up, each level's layers back to back (`render::Texture::upload_smaller_mips`).
    pub smaller: Vec<u8>,
    pub sheet: Vec<vtex::sheet::Sequence>,
}

/// The `.vtex_c` resource path for `path` (`materials/x.vtex`, `_c` optional).
pub fn compiled_path(path: &str) -> String {
    let path = path.strip_suffix("_c").unwrap_or(path);
    if path.ends_with(".vtex") { format!("{path}_c") } else { format!("{path}.vtex_c") }
}

/// Every surface of mip `level` decoded and stacked, with its size and `ImageFormat`.
fn decode_level(t: &vtex::Texture, level: usize, layers: usize) -> Option<(Vec<u8>, usize, usize, i32)> {
    let mut data = Vec::new();
    let mut format = RGBA8888;
    let (mut width, mut height) = (0, 0);
    for i in 0..layers {
        let image = vtex::decode(t, level, i / t.faces(), i % t.faces())?;
        (width, height) = (image.width, image.height);
        match image.pixels {
            Pixels::Rgba8(p) => data.extend(p),
            Pixels::Rgba16F(p) => {
                format = RGBA16161616F;
                data.extend(p.iter().flat_map(|h| h.to_le_bytes()));
            }
        }
    }
    Some((data, width, height, format))
}

/// The `ImageFormat` of a block format the GPU samples as stored.
fn block_format(format: vtex::Format) -> Option<i32> {
    Some(match format {
        vtex::Format::DXT1 => 13,
        vtex::Format::DXT5 => 15,
        vtex::Format::ATI2N => 36,
        vtex::Format::ATI1N => 37,
        vtex::Format::BC6H => 85,
        vtex::Format::BC7 => 86,
        _ => return None,
    })
}

/// A block-compressed texture's mips as stored, as real engine2 uploads them, when the device
/// samples BC. wgpu wants whole blocks at mip 0, and VRF pads 2 or 3 layers to 4, so those decode.
/// TODO: wgpu has no RGB-only BC1, so a 3-colour block's index 3 samples alpha 0 where real's reads 1;
/// only blended BC1 materials would show it (decoding instead costs testgrid's exact texels).
fn stored(t: &vtex::Texture) -> Option<Decoded> {
    let format = block_format(t.format)?;
    let (width, height, depth) = t.level_size(0);
    let layers = depth * t.faces();
    let bc = crate::render::device::gpu().is_some_and(|g| g.supports_bc());
    if !bc || t.flags & vtex::flags::VOLUME != 0 || !width.is_multiple_of(4) || !height.is_multiple_of(4) || (2..4).contains(&layers) {
        return None;
    }
    let level = |l: usize| (0..layers).map(|i| t.surface(l, i / t.faces(), i % t.faces())).collect::<Option<Vec<_>>>().map(|s| s.concat());
    let data = level(0)?;
    let smaller = (1..t.levels.len()).map(level).collect::<Option<Vec<_>>>()?.concat();
    let config = config(t, width, height, layers, t.levels.len() as i16, format);
    Some(Decoded { config, data, smaller, sheet: t.sheet.clone() })
}

fn config(t: &vtex::Texture, width: usize, height: usize, layers: usize, mips: i16, format: i32) -> TextureCreationConfig_t {
    let image_file = t.format.is_image_file();
    let mut config = TextureCreationConfig_t::default();
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (width as i16, height as i16, layers as i16, mips);
    config.m_n_image_format = format;
    // As managed's builders describe them: a cube's depth counts cubes, not faces.
    if t.is_cube() && !image_file {
        config.m_n_depth = (layers / 6) as i16;
        config.m_n_flags |= crate::render::texture::TSPEC_CUBE_TEXTURE;
    }
    if layers > 1 && !t.is_cube() && t.flags & vtex::flags::VOLUME == 0 {
        config.m_n_flags |= crate::render::texture::TSPEC_TEXTURE_ARRAY;
    }
    if let Some((w, h)) = t.display {
        (config.m_n_display_rect_width, config.m_n_display_rect_height) = (w as i16, h as i16);
    }
    config
}

/// The texture's mips as the GPU takes them: stored block data where the device samples it,
/// else decoded; `None` for formats we don't decode. A level that fails to decode ends the chain there.
pub fn decode(t: &vtex::Texture) -> Option<Decoded> {
    if let Some(stored) = stored(t) {
        return Some(stored);
    }
    let (_, _, depth) = t.level_size(0);
    let image_file = t.format.is_image_file();
    let layers = if image_file { 1 } else { depth * t.faces() };
    let (data, width, height, format) = decode_level(t, 0, layers)?;
    let mut smaller = Vec::new();
    let mut mips = 1;
    // Volumes shrink in depth per mip, which the stacked-layer layout can't hold.
    let chain = if image_file || t.flags & vtex::flags::VOLUME != 0 { 1 } else { t.levels.len() };
    for level in 1..chain {
        let expected = ((width >> level).max(1), (height >> level).max(1));
        match decode_level(t, level, layers) {
            Some((d, w, h, f)) if (w, h) == expected && f == format => smaller.extend(d),
            _ => break,
        }
        mips += 1;
    }
    Some(Decoded { config: config(t, width, height, layers, mips, format), data, smaller, sheet: t.sheet.clone() })
}

/// Image files a material source may name instead of a `.vtex`.
const IMAGE_EXTENSIONS: [&str; 6] = ["tga", "png", "jpg", "jpeg", "psd", "exr"];

/// For an image a material source names (`dir/x.tga`), the compiled texture the resource compiler
/// wrote beside it: `dir/x_tga_<settings hash>.generated.vtex_c`.
pub fn generated_sibling(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", &path));
    let (stem, ext) = file.rsplit_once('.')?;
    if !IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
        return None;
    }
    let prefix = format!("{stem}_{ext}_").to_ascii_lowercase();
    crate::fs::search_roots().into_iter().find_map(|root| {
        let names = std::fs::read_dir(root.join(dir)).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned());
        let name = names.filter(|n| n.to_ascii_lowercase().starts_with(&prefix) && n.to_ascii_lowercase().ends_with(".generated.vtex_c")).min()?;
        Some(if dir.is_empty() { name } else { format!("{dir}/{name}") })
    })
}

/// The compiled texture at `path` (or its generated sibling), decoded; `None` without a readable `.vtex_c`.
pub fn load(path: &str) -> Option<Decoded> {
    let file = crate::fs::read(&compiled_path(path), "GAME").or_else(|| crate::fs::read(&generated_sibling(path)?, "GAME"))?;
    let t = vtex::read(&file).inspect_err(|e| abi::rt::log::warn(&format!("{path}: {e:?}"))).ok()?;
    let decoded = decode(&t);
    if decoded.is_none() {
        abi::rt::log::warn(&format!("{path}: texture format {} isn't decoded", t.format.0));
    }
    decoded
}
