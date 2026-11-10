//! Textures (`ITexture` handles over `CTextureBase`). With the empty device a texture is its
//! description plus a CPU copy of the pixels managed uploads; the GPU side comes with M2.

use crate::render::image;
use crate::resource::{self, ByName, Resource};
use abi::slots::{g_p_render_device as device, glue_resources as glue};
use abi::structs::{CTextureDesc, Rect3D, TextureCreationConfig_t};
use abi::{Guid, NativeRef, RawPtr, Registry, bind, class};
use std::ffi::CStr;
use std::sync::{Arc, Mutex};

pub type Ref = NativeRef<class::CTextureBase>;

pub struct Texture {
    #[allow(dead_code)]
    name: String,
    config: TextureCreationConfig_t,
    // Boxed: GetTextureDesc hands managed a pointer that must outlive this call.
    desc: Box<CTextureDesc>,
    data: Mutex<Vec<u8>>,
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

static LOADED: ByName<Texture> = ByName::new();

/// The texture at a resource path. Nothing loads compiled textures yet (TODO: assets subsystem),
/// so every path is native's error texture: a valid handle that reports `IsError`.
pub fn load(path: Option<&CStr>, guid: Guid) -> Ref {
    let name = path.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let texture = LOADED.find_or_insert(&name, || {
        let mut config = TextureCreationConfig_t::default();
        (config.m_n_width, config.m_n_height, config.m_n_depth, config.m_n_num_mip_levels) = (1, 1, 1, 1);
        Texture { desc: Box::new(desc_of(&config)), config, name: name.clone(), data: Mutex::new(vec![255, 0, 255, 255]), error: true, guid }
    });
    resource::new_handle(texture)
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

fn create(name: Option<&CStr>, _anonymous: bool, config: TextureCreationConfig_t, data: &mut [u8]) -> Ref {
    let bytes = data.to_vec();
    let texture = Texture {
        name: name.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        desc: Box::new(desc_of(&config)),
        config,
        data: Mutex::new(bytes),
        error: false,
        guid: Guid::default(),
    };
    resource::new_handle(Arc::new(texture))
}

fn on<R: Default>(t: Ref, f: impl FnOnce(&Texture) -> R) -> R {
    resource::resolve::<_, Texture>(t).map(|t| f(&t)).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::c_texture_base, Texture);
    bind!(r, device::FIND_OR_CREATE_TEXTURE2, create);
    bind!(r, glue::GET_TEXTURE, load);
    bind!(r, device::FIND_OR_CREATE_FILE_TEXTURE, |path, _flags| load(path, Guid::default()));
    bind!(r, device::GET_TEXTURE_DESC, |t| on(t, |t| RawPtr::from_raw(&*t.desc as *const _ as *mut _)));
    bind!(r, device::GET_ON_DISK_TEXTURE_DESC, |t| on(t, |t| RawPtr::from_raw(&*t.desc as *const _ as *mut _)));
    bind!(r, device::GET_TEXTURE_MULTISAMPLE_TYPE, |t| on(t, |t| t.config.m_n_multisample_type as i64));
    bind!(r, device::COMPUTE_TEXTURE_MEMORY_SIZE, |t| on(t, |t| mem_size(&t.config) as u32));
    bind!(r, device::IS_TEXTURE_RENDER_TARGET, |_| false);
    // Sequences are sprite-sheet animations, read from compiled textures (none load yet).
    bind!(r, device::GET_SEQUENCE_COUNT, |_| 0);
    bind!(r, device::GET_TEXTURE_LAST_USED, |_| 0);
    bind!(r, device::MARK_TEXTURE_USED, |_, _| {});
    bind!(r, device::GET_NUM_TEXTURE_LOADS_IN_FLIGHT, || 0);
    bind!(r, device::UN_THROTTLE_TEXTURE_STREAMING_FOR_N_FRAMES, |_| {});
    bind!(r, device::GET_TEXTURE_POOL_STATS, |a, b, c| {
        (*a, *b, *c) = (0, 0, 0);
    });
    bind!(r, device::ASYNC_SET_TEXTURE_DATA2, set_data);
}

/// Replace the texels in `rect` (only whole-texture uploads are kept for now).
fn set_data(t: Ref, bytes: &mut [u8], _rect: Rect3D) {
    on(t, |t| {
        let mut d = t.data.lock().unwrap();
        if bytes.len() == d.len() {
            d.copy_from_slice(bytes);
        }
    });
}
