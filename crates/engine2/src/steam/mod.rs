//! Native's Steam surface (`globalSteam_*`, `ISteam*`, workshop) as a view over the installed
//! [`platform::Platform`]. Interfaces are opaque singletons; null means "not available", which
//! managed checks for (e.g. no Steam client on the offline platform).

mod dispatch;
mod game_server;

use abi::slots::{global_steam as global, i_steam_networking_sockets as sockets, steam_ugc_c_ugc_subscriptions as ugc};
use abi::{NativeClass, NativeRef, Registry, StrRet, bind, class};
use std::ffi::c_void;
use std::sync::OnceLock;

struct Interface;

/// The singleton for one interface, created on first use.
fn interface<C: NativeClass>(cell: &'static OnceLock<usize>) -> NativeRef<C> {
    let addr = *cell.get_or_init(|| objects::create::<C, _>(Interface).addr());
    NativeRef::from_raw(addr as *mut c_void)
}

pub fn register(r: &mut Registry) {
    game_server::register(r);
    dispatch::register(r);
    static SOCKETS: OnceLock<usize> = OnceLock::new();
    bind!(r, global::STEAM_NETWORKING_SOCKETS, || interface::<class::ISteamNetworkingSockets>(&SOCKETS));
    // FakeIP hides a server's address behind Steam's relays; without a relay there's nothing to hide behind.
    bind!(r, sockets::BEGIN_REQUEST_FAKE_IP, |_| {});
    bind!(r, sockets::START_AUTHENTICATION, |_| {});
    // No Steam client: managed skips user, app and relay-config features when these are null.
    // TODO: client-side interfaces come from the Platform trait with the Steam backend.
    bind!(r, global::STEAM_USER, NativeRef::null);
    bind!(r, global::STEAM_APPS, NativeRef::null);
    bind!(r, global::STEAM_NETWORKING_UTILS, NativeRef::null);
    // The workshop: none on the offline platform, so nothing subscribed or downloaded.
    bind!(r, ugc::INIT_WORKSHOP_FOR_GAME_SERVER, |_| true);
    bind!(r, ugc::GET_SUBSCRIBED_JSON, || StrRet::from(c"[]"));
    bind!(r, ugc::GET_DOWNLOADED_JSON, || StrRet::from(c"[]"));
}
