//! The HTML surface (`ISteamHTMLSurface`), which stays unavailable even with the Steam backend: see
//! docs/steam.md (HTML surface). The accessor returns null, so managed's WebSurface logs
//! "unavailable" and returns. The methods answer as an idle surface if called anyway.

use abi::slots::{global_steam as global, i_steam_html_surface as html};
use abi::{NativeRef, Registry, bind};

pub fn register(r: &mut Registry) {
    bind!(r, global::STEAM_HTML_SURFACE, NativeRef::null);
    bind!(r, html::INIT, |_| false);
    bind!(r, html::SHUTDOWN, |_| true);
    // k_uAPICallInvalid: no browser is coming.
    bind!(r, html::CREATE_BROWSER, |_, _, _| 0);
    bind!(r, html::REMOVE_BROWSER, |_, _| {});
    bind!(r, html::LOAD_URL, |_, _, _, _| {});
    bind!(r, html::ADD_HEADER, |_, _, _, _| {});
    bind!(r, html::EXECUTE_JAVASCRIPT, |_, _, _| {});
    bind!(r, html::SET_SIZE, |_, _, _, _| {});
    bind!(r, html::GET_LINK_AT_POSITION, |_, _, _, _| {});
    bind!(r, html::SET_HORIZONTAL_SCROLL, |_, _, _| {});
    bind!(r, html::SET_VERTICAL_SCROLL, |_, _, _| {});
    bind!(r, html::SET_KEY_FOCUS, |_, _, _| {});
    bind!(r, html::ALLOW_START_REQUEST, |_, _, _| {});
    bind!(r, html::JS_DIALOG_RESPONSE, |_, _, _| {});
    bind!(r, html::SET_BACKGROUND_MODE, |_, _, _| {});
    bind!(r, html::SET_DPI_SCALING_FACTOR, |_, _, _| {});
    bind!(r, html::KEY_DOWN, |_, _, _, _, _| {});
    bind!(r, html::KEY_UP, |_, _, _, _| {});
    bind!(r, html::KEY_CHAR, |_, _, _, _| {});
    bind!(r, html::MOUSE_UP, |_, _, _| {});
    bind!(r, html::MOUSE_DOWN, |_, _, _| {});
    bind!(r, html::MOUSE_DOUBLE_CLICK, |_, _, _| {});
    bind!(r, html::MOUSE_MOVE, |_, _, _, _| {});
    bind!(r, html::MOUSE_WHEEL, |_, _, _| {});
    bind!(r, html::SET_COOKIE, |_, _, _, _, _, _, _, _| {});
}
