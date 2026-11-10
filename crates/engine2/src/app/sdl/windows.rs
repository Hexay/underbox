//! `globalSdl` window slots: creation, state, size and position, surfaces and Facepunch's
//! window extensions. Implementations live in the `window` crate.

use super::{text, win, win_ret};
use abi::slots::global_sdl as sdl;
use abi::{RawPtr, Registry, bind};
use window::{ext, surface, windows as w};

/// Writes an SDL (a, b) pair into managed's two out parameters; zeroes on failure.
pub(super) fn out_pair(pair: Option<(i32, i32)>, a: &mut i32, b: &mut i32) -> bool {
    let (x, y) = pair.unwrap_or_default();
    (*a, *b) = (x, y);
    pair.is_some()
}

pub fn register(r: &mut Registry) {
    bind!(r, sdl::CREATE_WINDOW, |t, wd, h, f| win_ret(w::create(text(t), wd, h, f as u64)));
    bind!(r, sdl::CREATE_STARTUP_WINDOW, |t, wd, h, f| win_ret(ext::create_startup_window(text(t), wd, h, f as u64)));
    bind!(r, sdl::CREATE_WINDOW_WITH_PROPERTIES, |p| win_ret(w::create_with_properties(p)));
    bind!(r, sdl::CREATE_POPUP_WINDOW, |p, x, y, wd, h, f| win_ret(w::create_popup(win(p), x, y, wd, h, f as u64)));
    bind!(r, sdl::DESTROY_WINDOW, |p| w::destroy(win(p)));
    bind!(r, sdl::SET_WINDOW_TITLE, |p, t| w::set_title(win(p), text(t)));
    bind!(r, sdl::SHOW_WINDOW, |p| w::show(win(p)));
    bind!(r, sdl::HIDE_WINDOW, |p| w::hide(win(p)));
    bind!(r, sdl::MINIMIZE_WINDOW, |p| w::minimize(win(p)));
    bind!(r, sdl::MAXIMIZE_WINDOW, |p| w::maximize(win(p)));
    bind!(r, sdl::RESTORE_WINDOW, |p| w::restore(win(p)));
    bind!(r, sdl::RAISE_WINDOW, |p| w::raise(win(p)));
    bind!(r, sdl::SYNC_WINDOW, |p| w::sync(win(p)));
    bind!(r, sdl::GET_WINDOW_FLAGS, |p| w::flags(win(p)) as i64);
    bind!(r, sdl::GET_WINDOW_POSITION, |p, x, y| out_pair(w::position(win(p)), x, y));
    bind!(r, sdl::GET_WINDOW_SIZE, |p, x, y| out_pair(w::size(win(p)), x, y));
    bind!(r, sdl::GET_WINDOW_SIZE_IN_PIXELS, |p, x, y| out_pair(w::size_in_pixels(win(p)), x, y));
    bind!(r, sdl::GET_WINDOW_PIXEL_DENSITY, |p| w::pixel_density(win(p)));
    bind!(r, sdl::GET_WINDOW_DISPLAY_SCALE, |p| w::display_scale(win(p)));
    bind!(r, sdl::SET_WINDOW_MINIMUM_SIZE, |p, x, y| w::set_minimum_size(win(p), x, y));
    bind!(r, sdl::SET_WINDOW_MAXIMUM_SIZE, |p, x, y| w::set_maximum_size(win(p), x, y));
    bind!(r, sdl::SET_WINDOW_SIZE, |p, x, y| w::set_size(win(p), x, y));
    bind!(r, sdl::SET_WINDOW_POSITION, |p, x, y| w::set_position(win(p), x, y));
    bind!(r, sdl::SET_WINDOW_ALWAYS_ON_TOP, |p, v| w::set_always_on_top(win(p), v));
    bind!(r, sdl::SET_WINDOW_OPACITY, |p, v| w::set_opacity(win(p), v));
    bind!(r, sdl::SET_WINDOW_ASPECT_RATIO, |p, a, b| w::set_aspect_ratio(win(p), a, b));
    bind!(r, sdl::SET_WINDOW_PARENT, |p, parent| w::set_parent(win(p), win(parent)));
    bind!(r, sdl::SET_WINDOW_MODAL, |p, v| w::set_modal(win(p), v));
    bind!(r, sdl::FLASH_WINDOW, |p, op| w::flash(win(p), op as i32));
    bind!(r, sdl::SET_WINDOW_BORDERED, |p, v| w::set_bordered(win(p), v));
    bind!(r, sdl::SET_WINDOW_RESIZABLE, |p, v| w::set_resizable(win(p), v));
    bind!(r, sdl::SET_WINDOW_FULLSCREEN, |p, v| w::set_fullscreen(win(p), v));
    // SAFETY: managed passes null or a pointer to its own Sdl.DisplayMode, same layout as SDL's.
    bind!(r, sdl::SET_WINDOW_FULLSCREEN_MODE, |p, m: RawPtr<_>| unsafe {
        window::display::set_window_fullscreen_mode(win(p), m.as_ptr())
    });
    // SAFETY: managed passes an `SDL_HitTest` function pointer it keeps alive (static unmanaged).
    bind!(r, sdl::SET_WINDOW_HIT_TEST, |p, cb: RawPtr<_>, ud: RawPtr<_>| unsafe {
        w::set_hit_test(win(p), cb.as_ptr(), ud.as_ptr())
    });

    // SAFETY: managed passes pixels of `height * pitch` bytes, pinned for the surface's life.
    bind!(r, sdl::CREATE_SURFACE_FROM, |wd, h, fmt, px: RawPtr<_>, pitch| {
        RawPtr::from_raw(unsafe { surface::create_from(wd, h, fmt, px.as_ptr(), pitch) }.as_ptr())
    });
    // SAFETY: managed only passes surfaces CreateSurfaceFrom returned.
    bind!(r, sdl::DESTROY_SURFACE, |s: RawPtr<_>| unsafe { surface::destroy(surface::Surface::from_ptr(s.as_ptr())) });
    // SAFETY: as above.
    bind!(r, sdl::SET_WINDOW_ICON, |p, s: RawPtr<_>| unsafe {
        surface::set_window_icon(win(p), surface::Surface::from_ptr(s.as_ptr()))
    });
    bind!(r, sdl::WINDOW_HAS_SURFACE, |p| surface::window_has_surface(win(p)));
    bind!(r, sdl::DESTROY_WINDOW_SURFACE, |p| surface::destroy_window_surface(win(p)));
    bind!(r, sdl::FILL_WINDOW_COLOR, |p, rr, g, b, a| surface::fill_window(win(p), [rr, g, b, a]));
    // SAFETY: managed passes a pinned framebuffer of `height * pitch` bytes.
    bind!(r, sdl::PRESENT_WINDOW_PIXELS, |p, wd, h, px: RawPtr<_>, pitch| unsafe {
        surface::present_pixels(win(p), wd, h, px.as_ptr(), pitch)
    });

    bind!(r, sdl::SET_WINDOW_EXTERNAL_DROP_TARGET, |p, cbs: RawPtr<_>| ext::set_external_drop_target(win(p), cbs.as_ptr()));
    bind!(r, sdl::BEGIN_EXTERNAL_DRAG, |p, files, t| ext::begin_external_drag(win(p), text(files), text(t)));
    bind!(r, sdl::SET_WINDOW_MOUSE_TRANSPARENT, |p, v| ext::set_mouse_transparent(win(p), v));
    bind!(r, sdl::SET_WINDOW_CAN_MAXIMIZE, |p, v| ext::set_can_maximize(win(p), v));
    bind!(r, sdl::SET_WINDOW_HAS_SHADOW, |p, v| ext::set_has_shadow(win(p), v));
    bind!(r, sdl::SET_WINDOW_ROUNDED_CORNERS, |p, v| ext::set_rounded_corners(win(p), v));
    bind!(r, sdl::SEND_WINDOW_TO_BACK, |p| ext::send_to_back(win(p)));
    bind!(r, sdl::RAISE_WINDOW_WITHOUT_FOCUS, |p| ext::raise_without_focus(win(p)));
    bind!(r, sdl::SET_WINDOW_IN_TASKBAR, |p, v| ext::set_in_taskbar(win(p), v));
    bind!(r, sdl::SET_WINDOW_CAN_CLOSE, |p, v| ext::set_can_close(win(p), v));
    bind!(r, sdl::SET_WINDOW_TASKBAR_OVERLAY_ICON, |p, wd, h, px: RawPtr<_>, d| {
        ext::set_taskbar_overlay_icon(win(p), wd, h, px.as_ptr(), text(d))
    });
}
