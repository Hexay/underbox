//! `EngineGlobal` slots only s&box's tools and launcher apps call: desktop ↔ window coordinates
//! for panel windows (PanelWindowDesktop) and the panel app's engine (PanelAppSystem).

use abi::slots::global;
use abi::{RawPtr, Registry, bind};
use std::ffi::c_void;

/// Moves `(x, y)` by the window's desktop position, `sign` 1 into desktop space, -1 out of it.
/// A window SDL doesn't know leaves the point as it was.
fn offset(position: Option<(i32, i32)>, x: &mut i32, y: &mut i32, sign: i32) {
    if let Some((wx, wy)) = position {
        *x += sign * wx;
        *y += sign * wy;
    }
}

fn position(window: RawPtr<c_void>) -> Option<(i32, i32)> {
    if window.is_null() {
        return None;
    }
    window::windows::position(window::Window::from_ptr(window.as_ptr()))
}

pub fn register(r: &mut Registry) {
    // The handle is the SDL window (SurfaceWindow.Handle); SDL's window coordinates are desktop points.
    bind!(r, global::PLAT_WINDOW_TO_SCREEN_COORDS, |w, x, y| offset(position(w), x, y, 1));
    bind!(r, global::PLAT_SCREEN_TO_WINDOW_COORDS, |w, x, y| offset(position(w), x, y, -1));
    // The panel app's windows draw and present themselves (PanelWindows.FrameAll); what native
    // adds is its render device, which managed's Graphics.Initialize has already asked for.
    bind!(r, global::SOURCE_ENGINE_PANEL_APP_INIT, |_, _draws_scenes| {
        crate::render::device::report_to_managed();
        true
    });
    bind!(r, global::SOURCE_ENGINE_PANEL_APP_FRAME, || {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_shift_by_the_window_position() {
        let (mut x, mut y) = (5, 7);
        offset(Some((100, 200)), &mut x, &mut y, 1);
        assert_eq!((x, y), (105, 207));
        offset(Some((100, 200)), &mut x, &mut y, -1);
        assert_eq!((x, y), (5, 7));
        offset(None, &mut x, &mut y, 1);
        assert_eq!((x, y), (5, 7));
        assert_eq!(position(RawPtr::NULL), None);
    }
}
