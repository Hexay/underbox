//! Swap chains (`SwapChainHandle_t`): a colour and a depth render target every view of a window
//! draws into, presented to the window's surface when there is one. Without a window (or before
//! the window lane registers a [`WindowSource`]) a swap chain is purely offscreen, so a client runs
//! headless and every presented frame can be read back and hashed (see [`frames`]).

use crate::render::{device, frames, texture};
use abi::slots::{g_p_render_device as dev, g_p_scene_system as scene};
use abi::structs::{RenderDeviceInfo_t, Vector2};
use abi::{PtrHandle, RawPtr, Registry, bind, handle};
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};

pub type Handle = PtrHandle<handle::SwapChainHandle_t>;

/// Turns managed's `SDL_Window*` into something wgpu can make a surface on; `None` keeps the swap
/// chain offscreen. The window crate installs one with [`set_window_source`].
pub type WindowSource = Box<dyn Fn(RawPtr<c_void>) -> Option<render::SurfaceTarget<'static>> + Send + Sync>;

static WINDOWS: OnceLock<WindowSource> = OnceLock::new();

pub fn set_window_source(source: WindowSource) {
    let _ = WINDOWS.set(source);
}

/// `SwapChainBuffer`: which of a swap chain's textures `GetSwapChainTexture` returns.
const BUFFER_DEPTH: i64 = 1;
/// `ImageFormat` of swap-chain depth: D32F (formats.rs).
const DEPTH_IMAGE_FORMAT: i32 = 62;

pub struct SwapChain {
    pub name: String,
    pub info: RenderDeviceInfo_t,
    pub color: texture::Ref,
    pub depth: texture::Ref,
    surface: Option<render::Surface>,
    pub presented: u64,
}

impl SwapChain {
    pub fn size(&self) -> (u32, u32) {
        let m = self.info.m_display_mode;
        (m.m_n_width.max(1) as u32, m.m_n_height.max(1) as u32)
    }

    fn image_format(&self) -> i32 {
        match self.surface.as_ref().map(render::Surface::format) {
            Some(render::wgpu::TextureFormat::Bgra8Unorm) => 12,
            _ => 0,
        }
    }

    /// (Re)create the targets at the current size.
    fn make_targets(&mut self) {
        let (w, h) = self.size();
        for t in [self.color, self.depth] {
            if !t.is_null() {
                crate::resource::destroy_handle(t);
            }
        }
        self.color = texture::create_target(&format!("{} color", self.name), w, h, self.image_format());
        self.depth = texture::create_target(&format!("{} depth", self.name), w, h, DEPTH_IMAGE_FORMAT);
    }
}

static CHAINS: LazyLock<RwLock<HashMap<usize, Arc<Mutex<SwapChain>>>>> = LazyLock::new(Default::default);
static NEXT: AtomicUsize = AtomicUsize::new(1);

pub fn get(h: Handle) -> Option<Arc<Mutex<SwapChain>>> {
    CHAINS.read().unwrap().get(&h.value).cloned()
}

/// Colour and depth targets of a swap chain.
pub fn targets(h: Handle) -> Option<(texture::Ref, texture::Ref)> {
    get(h).map(|c| {
        let c = c.lock().unwrap();
        (c.color, c.depth)
    })
}

pub fn create(window: RawPtr<c_void>, info: RenderDeviceInfo_t, name: Option<&CStr>) -> Handle {
    let Some(gpu) = device::gpu() else { return Handle::default() };
    let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "swapchain".into());
    let mut chain = SwapChain { name, info, color: Default::default(), depth: Default::default(), surface: None, presented: 0 };
    let (w, h) = chain.size();
    let target = if window.is_null() { None } else { WINDOWS.get().and_then(|source| source(window)) };
    if let Some(target) = target {
        match render::Surface::new(gpu, target, w, h, info.m_b_wait_for_v_sync != 0) {
            Ok(s) => chain.surface = Some(s),
            Err(e) => abi::rt::log::log(abi::rt::log::Level::Warn, &format!("swap chain '{}' stays offscreen: {e}", chain.name)),
        }
    }
    chain.make_targets();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    CHAINS.write().unwrap().insert(id, Arc::new(Mutex::new(chain)));
    Handle::new(id)
}

fn update(h: Handle, info: RenderDeviceInfo_t) -> bool {
    let (Some(gpu), Some(chain)) = (device::gpu(), get(h)) else { return false };
    let mut c = chain.lock().unwrap();
    c.info = info;
    let (w, hgt) = c.size();
    if let Some(s) = c.surface.as_mut() {
        s.resize(gpu, w, hgt, info.m_b_wait_for_v_sync != 0);
    }
    c.make_targets();
    true
}

fn destroy(h: Handle) {
    if let Some(chain) = CHAINS.write().unwrap().remove(&h.value) {
        let c = chain.lock().unwrap();
        crate::resource::destroy_handle(c.color);
        crate::resource::destroy_handle(c.depth);
    }
}

/// Show the frame: copy it to the window when there is one, and offer it to the frame capture.
pub fn present(h: Handle) -> bool {
    let (Some(gpu), Some(chain)) = (device::gpu(), get(h)) else { return false };
    let mut c = chain.lock().unwrap();
    c.presented += 1;
    let color = texture::resolve(c.color);
    if let Some(tex) = color.as_ref().and_then(|t| t.gpu.as_ref()) {
        frames::on_present(gpu, &c.name, c.presented, tex);
        if let Some(s) = &c.surface {
            return s.present(gpu, tex);
        }
    }
    true
}

pub fn register(r: &mut Registry) {
    bind!(r, dev::CREATE_SWAP_CHAIN, create);
    bind!(r, dev::UPDATE_SWAP_CHAIN, update);
    bind!(r, dev::DESTROY_SWAP_CHAIN, destroy);
    bind!(r, dev::PRESENT, present);
    bind!(r, dev::GET_SWAP_CHAIN_INFO, |h| get(h).map(|c| c.lock().unwrap().info).unwrap_or_default());
    bind!(r, dev::GET_SWAP_CHAIN_TEXTURE, |h, buffer| targets(h).map(|(c, d)| if buffer == BUFFER_DEPTH { d } else { c }).unwrap_or_default());
    bind!(r, dev::CAN_RENDER_TO_SWAP_CHAIN, |h| get(h).is_some());
    bind!(r, dev::SET_SWAP_CHAIN_IS_MAIN_WINDOW, |h, main| {
        if let Some(c) = get(h) {
            c.lock().unwrap().info.m_b_is_main_window = u8::from(main);
        }
    });
    bind!(r, dev::GET_BACKBUFFER_DIMENSIONS, |h| {
        let (w, hgt) = get(h).map_or((0, 0), |c| c.lock().unwrap().size());
        Vector2 { vec: abi::structs::System_Numerics_Vector2 { x: w as f32, y: hgt as f32 } }
    });
    // TODO: GPU timestamps; managed shows the frame time only when this answers true.
    bind!(r, dev::GET_GPU_FRAME_TIME_MS, |_, ms, frame| {
        (*ms, *frame) = (0.0, 0);
        false
    });
    bind!(r, scene::SET_MAIN_SWAP_CHAIN, |h| *MAIN.lock().unwrap() = h);
    bind!(r, scene::GET_MAIN_SWAP_CHAIN_MULTISAMPLE_TYPE, || {
        let main = *MAIN.lock().unwrap();
        get(main).map_or(0, |c| c.lock().unwrap().info.m_n_multisample_type as i64)
    });
}

static MAIN: Mutex<Handle> = Mutex::new(Handle::new(0));

/// The swap chain the scene system renders the main views to.
pub fn main() -> Handle {
    *MAIN.lock().unwrap()
}
