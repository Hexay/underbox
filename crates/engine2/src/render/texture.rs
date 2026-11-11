//! Textures (`ITexture` handles over `CTextureBase`): the description, a CPU copy of the pixels
//! managed uploads, and, when a client device exists, the GPU texture (see device.rs).

use crate::render::{device, formats, image};
use crate::resource::{self, ByName, Resource};
use abi::slots::{g_p_render_device as device_slots, glue_resources as glue};
use abi::structs::{CTextureDesc, Color32, Rect3D, TextureCreationConfig_t};
use abi::{Guid, NativeRef, RawPtr, Registry, bind, class};
use std::ffi::CStr;
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::CTextureBase>;

/// `RuntimeTextureSpecificationFlags.TSPEC_RENDER_TARGET`.
const TSPEC_RENDER_TARGET: u32 = 0x0001;

pub struct Texture {
    pub name: String,
    config: TextureCreationConfig_t,
    // Boxed: GetTextureDesc hands managed a pointer that must outlive this call.
    desc: Box<CTextureDesc>,
    data: Mutex<Vec<u8>>,
    pub gpu: Option<render::Texture>,
    error: bool,
    guid: Guid,
}

impl Resource for Texture {
    fn is_error(&self) -> bool {
        self.error
    }
    fn guid(&self) -> Guid {
        self.guid
    }
}

impl Texture {
    pub fn is_render_target(&self) -> bool {
        self.config.m_n_flags & TSPEC_RENDER_TARGET != 0
    }

    /// `ImageFormat` managed created it with.
    pub fn image_format(&self) -> i32 {
        self.config.m_n_image_format
    }

    /// Mip 0 of the CPU copy, for uncompressed formats the copy covers.
    pub fn cpu_mip0(&self) -> Option<render::Frame> {
        let Some(image::Layout::Pixel { bytes }) = image::layout(self.config.m_n_image_format.into()) else { return None };
        let (w, h) = (self.config.m_n_width.max(1) as u32, self.config.m_n_height.max(1) as u32);
        let len = (w * h * bytes) as usize;
        let data = self.data.lock().unwrap();
        (data.len() >= len).then(|| render::Frame { width: w, height: h, texel_size: bytes, pixels: data[..len].to_vec() })
    }
}

static LOADED: ByName<Texture> = ByName::new();

/// The texture at a resource path. Nothing loads compiled textures yet (TODO: assets subsystem),
/// so every path but the ones we generate (texture_generated.rs) is native's error texture: a
/// valid handle that reports `IsError`.
pub fn load(path: Option<&CStr>, guid: Guid) -> Ref {
    let name = path.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let texture = LOADED.find_or_insert(&name, || {
        if let Some(c) = crate::compiled::texture::load(&name) {
            let gpu = gpu_texture(&name, &c.config, &c.data);
            return Texture { desc: Box::new(desc_of(&c.config)), config: c.config, name: name.clone(), data: Mutex::new(c.data), gpu, error: false, guid };
        }
        let generated = super::texture_generated::texture(&name);
        let error = generated.is_none();
        let (w, h, data) = generated.unwrap_or((1, 1, vec![255, 0, 255, 255]));
        let mut config = TextureCreationConfig_t::default();
        (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (w as _, h as _, 1, 1);
        let gpu = gpu_texture(&name, &config, &data);
        Texture { desc: Box::new(desc_of(&config)), config, name: name.clone(), data: Mutex::new(data), gpu, error, guid }
    });
    resource::new_handle(texture)
}

pub fn resolve(t: Ref) -> Option<Arc<Texture>> {
    resource::resolve::<_, Texture>(t)
}

fn desc_of(c: &TextureCreationConfig_t) -> CTextureDesc {
    let mut d = CTextureDesc::default();
    d.m_n_width = c.m_n_width;
    d.m_n_height = c.m_n_height;
    d.m_n_depth = c.m_n_depth;
    d.m_n_num_mip_levels = c.m_n_num_mip_levels;
    d.m_n_decode_flags = c.m_n_decode_flags;
    d.m_n_image_format = c.m_n_image_format;
    d.m_n_flags = c.m_n_flags;
    d.m_n_display_rect_width = c.m_n_display_rect_width;
    d.m_n_display_rect_height = c.m_n_display_rect_height;
    d.m_n_motion_vectors_max_distance_in_pixels = c.m_n_motion_vectors_max_distance_in_pixels;
    d
}

fn mem_size(c: &TextureCreationConfig_t) -> i32 {
    let mips = c.m_n_num_mip_levels.max(1) as u32;
    image::mem_required(c.m_n_width as i32, c.m_n_height as i32, c.m_n_depth as i32, Some(mips), c.m_n_image_format as i64)
}

/// The GPU side of a 2D texture, when a device exists and the format maps. Volumes, arrays and
/// cubes stay CPU-only for now (TODO).
fn gpu_texture(name: &str, c: &TextureCreationConfig_t, data: &[u8]) -> Option<render::Texture> {
    let gpu = device::gpu()?;
    let format = formats::to_wgpu(c.m_n_image_format)?;
    if c.m_n_depth > 1 || c.m_n_width <= 0 || c.m_n_height <= 0 {
        return None;
    }
    let desc = render::TextureDesc {
        label: name.into(),
        width: c.m_n_width as u32,
        height: c.m_n_height as u32,
        format,
        mips: c.m_n_num_mip_levels.max(1) as u32,
        render_target: c.m_n_flags & TSPEC_RENDER_TARGET != 0 || format.is_depth_stencil_format(),
    };
    let texture = render::Texture::new(gpu, desc);
    if texture.texel_size().is_some_and(|t| data.len() == (t * texture.desc.width * texture.desc.height) as usize) {
        texture.upload(gpu, data);
    }
    Some(texture)
}

pub(crate) fn create(name: Option<&CStr>, _anonymous: bool, config: TextureCreationConfig_t, data: &mut [u8]) -> Ref {
    let name = name.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let texture = Texture {
        gpu: gpu_texture(&name, &config, data),
        name,
        desc: Box::new(desc_of(&config)),
        config,
        data: Mutex::new(data.to_vec()),
        error: false,
        guid: Guid::default(),
    };
    resource::new_handle(Arc::new(texture))
}

/// A render target of `image_format` (an `ImageFormat`; depth formats make depth targets).
pub fn create_target(name: &str, width: u32, height: u32, image_format: i32) -> Ref {
    let mut config = TextureCreationConfig_t::default();
    let clamp = |v: u32| v.clamp(1, i16::MAX as u32) as i16;
    (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (clamp(width), clamp(height), 1, 1);
    config.m_n_image_format = image_format;
    config.m_n_flags = TSPEC_RENDER_TARGET;
    let name = std::ffi::CString::new(name).unwrap_or_default();
    create(Some(&name), true, config, &mut [])
}

fn on<R: Default>(t: Ref, f: impl FnOnce(&Texture) -> R) -> R {
    resolve(t).map(|t| f(&t)).unwrap_or_default()
}

/// Fill the whole texture with one colour: on the GPU when it has a side there, and always in the
/// CPU copy of 4-byte formats so reads without a device agree.
pub fn clear(t: Ref, c: Color32) {
    on(t, |t| {
        if let (Some(gpu), Some(tex)) = (device::gpu(), &t.gpu)
            && !tex.desc.format.is_depth_stencil_format()
        {
            let unit = |v: u8| f64::from(v) / 255.0;
            tex.clear(gpu, render::wgpu::Color { r: unit(c.r), g: unit(c.g), b: unit(c.b), a: unit(c.a) }, 1.0);
        }
        let mut d = t.data.lock().unwrap();
        if t.config.m_n_image_format == 0 && d.len() % 4 == 0 {
            for px in d.chunks_mut(4) {
                px.copy_from_slice(&[c.r, c.g, c.b, c.a]);
            }
        }
    });
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::c_texture_base, Texture);
    bind!(r, device_slots::FIND_OR_CREATE_TEXTURE2, create);
    bind!(r, glue::GET_TEXTURE, load);
    bind!(r, device_slots::FIND_OR_CREATE_FILE_TEXTURE, |path, _flags| load(path, Guid::default()));
    bind!(r, device_slots::GET_TEXTURE_DESC, |t| on(t, |t| RawPtr::from_raw(&*t.desc as *const _ as *mut _)));
    bind!(r, device_slots::GET_ON_DISK_TEXTURE_DESC, |t| on(t, |t| RawPtr::from_raw(&*t.desc as *const _ as *mut _)));
    bind!(r, device_slots::GET_TEXTURE_MULTISAMPLE_TYPE, |t| on(t, |t| t.config.m_n_multisample_type as i64));
    bind!(r, device_slots::COMPUTE_TEXTURE_MEMORY_SIZE, |t| on(t, |t| mem_size(&t.config) as u32));
    bind!(r, device_slots::IS_TEXTURE_RENDER_TARGET, |t| on(t, Texture::is_render_target));
    bind!(r, device_slots::CLEAR_TEXTURE, clear);
    // Sequences are sprite-sheet animations, read from compiled textures (none load yet).
    bind!(r, device_slots::GET_SEQUENCE_COUNT, |_| 0);
    bind!(r, device_slots::GET_TEXTURE_LAST_USED, |_| 0);
    bind!(r, device_slots::MARK_TEXTURE_USED, |_, _| {});
    bind!(r, device_slots::GET_NUM_TEXTURE_LOADS_IN_FLIGHT, || 0);
    bind!(r, device_slots::UN_THROTTLE_TEXTURE_STREAMING_FOR_N_FRAMES, |_| {});
    bind!(r, device_slots::GET_TEXTURE_POOL_STATS, |a, b, c| {
        (*a, *b, *c) = (0, 0, 0);
    });
    bind!(r, device_slots::ASYNC_SET_TEXTURE_DATA2, set_data);
}

/// Replace the texels in `rect` (only whole-texture uploads are kept for now).
fn set_data(t: Ref, bytes: &mut [u8], _rect: Rect3D) {
    on(t, |t| {
        let mut d = t.data.lock().unwrap();
        if bytes.len() == d.len() {
            d.copy_from_slice(bytes);
            if let (Some(gpu), Some(tex)) = (device::gpu(), &t.gpu) {
                tex.upload(gpu, bytes);
            }
        }
    });
}
