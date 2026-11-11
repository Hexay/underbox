//! Native's Steam surface (`globalSteam_*`, `ISteam*`, workshop) as a view over the installed
//! [`platform::Platform`]. Interfaces are opaque singletons; null means "not available", which
//! managed checks for (e.g. no Steam client on the offline platform).

mod client;
mod dispatch;
#[cfg(not(feature = "steam"))]
mod flat_api;
mod game_server;
mod inventory;
mod lobby;
mod server_list;
mod ugc;
mod ugc_query;
mod unavailable;

use abi::slots::{global_steam as global, i_steam_networking_sockets as sockets};
use abi::{NativeClass, NativeRef, Registry, bind, class};
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
    client::register(r);
    ugc::register(r);
    lobby::register(r);
    inventory::register(r);
    server_list::register(r);
    unavailable::register(r);
    static SOCKETS: OnceLock<usize> = OnceLock::new();
    bind!(r, global::STEAM_NETWORKING_SOCKETS, || interface::<class::ISteamNetworkingSockets>(&SOCKETS));
    // FakeIP hides a server's address behind Steam's relays; without a relay there's nothing to hide behind.
    bind!(r, sockets::BEGIN_REQUEST_FAKE_IP, |_| {});
    bind!(r, sockets::START_AUTHENTICATION, |_| {});
}
