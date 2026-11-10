//! `globalSdl` process-wide slots: subsystems, hints, errors, clipboard, properties, displays and
//! file dialogs.

use super::{text, win};
use abi::slots::global_sdl as sdl;
use abi::structs::{DisplayMode, Point, Rect};
use abi::{RawPtr, Registry, StrRet, bind};
use std::ffi::c_void;
use window::{clipboard, display, props, video};

fn bounds(display: u32, usable: bool, out: &mut Rect) -> bool {
    let b = display::bounds(display, usable);
    let [x, y, width, height] = b.unwrap_or_default();
    *out = Rect { x, y, width, height };
    b.is_some()
}

fn closest_mode(display: u32, w: i32, h: i32, hz: f32, high_density: bool, out: &mut DisplayMode) -> bool {
    // SAFETY: SDL_DisplayMode is plain data; all-zero is valid.
    let mut m: display::DisplayMode = unsafe { std::mem::zeroed() };
    let found = display::closest_fullscreen_mode(display, w, h, hz, high_density, &mut m);
    *out = DisplayMode {
        display: m.displayID.0,
        format: m.format.0 as u32,
        width: m.w,
        height: m.h,
        pixel_density: m.pixel_density,
        refresh_rate: m.refresh_rate,
        refresh_rate_numerator: m.refresh_rate_numerator,
        refresh_rate_denominator: m.refresh_rate_denominator,
        internal: m.internal as usize,
    };
    found
}

fn ptr(p: RawPtr<c_void>) -> *mut c_void {
    p.as_ptr()
}

pub fn register(r: &mut Registry) {
    bind!(r, sdl::INIT_SUB_SYSTEM, |f| video::init_subsystem(f as u32));
    bind!(r, sdl::QUIT_SUB_SYSTEM, |f| video::quit_subsystem(f as u32));
    bind!(r, sdl::SET_HINT, |n, v| video::set_hint(text(n), text(v)));
    bind!(r, sdl::GET_ERROR, || StrRet::from(window::error()));
    bind!(r, sdl::GET_CURRENT_VIDEO_DRIVER, || StrRet::from(video::current_driver()));
    bind!(r, sdl::DISABLE_SCREEN_SAVER, video::disable_screen_saver);
    bind!(r, sdl::ENABLE_SCREEN_SAVER, video::enable_screen_saver);
    // SAFETY: managed frees only what SDL handed it (clipboard text, display/mode arrays).
    bind!(r, sdl::FREE, |m| unsafe { window::free(ptr(m)) });

    bind!(r, sdl::HAS_CLIPBOARD_TEXT, clipboard::has_text);
    bind!(r, sdl::SET_CLIPBOARD_TEXT, |t| clipboard::set_text(text(t)));
    bind!(r, sdl::GET_CLIPBOARD_TEXT, || RawPtr::from_raw(clipboard::text().cast()));

    bind!(r, sdl::CREATE_PROPERTIES, props::create);
    bind!(r, sdl::DESTROY_PROPERTIES, props::destroy);
    bind!(r, sdl::GET_WINDOW_PROPERTIES, |p| props::of_window(win(p)));
    bind!(r, sdl::GET_POINTER_PROPERTY, |id, n, d| RawPtr::from_raw(props::get_pointer(id, text(n), ptr(d))));
    bind!(r, sdl::SET_STRING_PROPERTY, |id, n, v| props::set_string(id, text(n), text(v)));
    bind!(r, sdl::SET_NUMBER_PROPERTY, |id, n, v| props::set_number(id, text(n), v));
    bind!(r, sdl::SET_BOOLEAN_PROPERTY, |id, n, v| props::set_boolean(id, text(n), v));
    bind!(r, sdl::GET_BOOLEAN_PROPERTY, |id, n, d| props::get_boolean(id, text(n), d));

    bind!(r, sdl::GET_DISPLAYS, |n| RawPtr::from_raw(display::displays(n)));
    bind!(r, sdl::GET_PRIMARY_DISPLAY, display::primary);
    bind!(r, sdl::GET_DISPLAY_FOR_POINT, |p: &mut Point| display::for_point((p.x, p.y)));
    bind!(r, sdl::GET_DISPLAY_FOR_WINDOW, |p| display::for_window(win(p)));
    bind!(r, sdl::GET_DISPLAY_CONTENT_SCALE, display::content_scale);
    bind!(r, sdl::GET_DISPLAY_BOUNDS, |d, out| bounds(d, false, out));
    bind!(r, sdl::GET_DISPLAY_USABLE_BOUNDS, |d, out| bounds(d, true, out));
    bind!(r, sdl::GET_DESKTOP_DISPLAY_MODE, |d| RawPtr::from_raw(display::mode(d, true)));
    bind!(r, sdl::GET_CURRENT_DISPLAY_MODE, |d| RawPtr::from_raw(display::mode(d, false)));
    bind!(r, sdl::GET_FULLSCREEN_DISPLAY_MODES, |d, n| RawPtr::from_raw(display::fullscreen_modes(d, n)));
    bind!(r, sdl::GET_CLOSEST_FULLSCREEN_DISPLAY_MODE, closest_mode);

    // SAFETY (dialogs): managed passes an `SDL_DialogFileCallback` and userdata it keeps alive until
    // the callback runs, null or NUL-terminated paths, and `count` pinned filters.
    bind!(r, sdl::SHOW_OPEN_FOLDER_DIALOG_AT, |cb, ud, p, path| unsafe {
        window::dialog::open_folder(ptr(cb), ptr(ud), win(p), ptr(path).cast())
    });
    bind!(r, sdl::SHOW_OPEN_FILE_DIALOG, |cb, ud, p, filters, n, path, many| unsafe {
        window::dialog::open_file(ptr(cb), ptr(ud), win(p), ptr(filters), n, ptr(path).cast(), many)
    });
    bind!(r, sdl::SHOW_SAVE_FILE_DIALOG, |cb, ud, p, filters, n, path| unsafe {
        window::dialog::save_file(ptr(cb), ptr(ud), win(p), ptr(filters), n, ptr(path).cast())
    });
}
