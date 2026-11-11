//! `Glue.Networking`: Steam Networking Sockets for managed's SteamNetwork transport. IP traffic is
//! GameNetworkingSockets' (the `gns` crate); Steam-id connections and listen sockets go over the
//! platform's `ISteamNetworkingMessages` (`p2p`) when it has one. Messages are
//! `SteamNetworkingMessage_t`s either way, which managed reads in place and releases through
//! `StmNtwrkngSckts_ReleaseMessage`.

mod events;
mod p2p;
mod transport;

use abi::slots::{glue_networking as glue, i_steam_networking_sockets as sockets};
use abi::structs::{HSteamListenSocket, HSteamNetConnection, HSteamNetPollGroup, SteamNetConnectionRealTimeStatus_t};
use abi::{RawPtr, Registry, StrRet, bind};
use p2p::{P2p, message};
use std::ffi::c_void;
use transport::{SteamP2p, of};

/// The library, started on first use; false (logged once) if it can't start.
fn ready() -> bool {
    static STARTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *STARTED.get_or_init(|| match gns::init() {
        Ok(()) => {
            // Without Steam there's no certificate authority to vouch for either end.
            gns::set_config_int(gns::config::IP_ALLOW_WITHOUT_AUTH, 1);
            true
        }
        Err(e) => {
            log_error(&format!("networking: {e}"));
            false
        }
    })
}

fn log_error(msg: &str) {
    abi::rt::log::log(abi::rt::log::Level::Error, msg);
}

fn listen(h: u32) -> HSteamListenSocket {
    HSteamListenSocket { id: h }
}

fn connection(h: u32) -> HSteamNetConnection {
    HSteamNetConnection { id: h }
}

/// Managed's `IntPtr[]` of message pointers, `len` long.
fn messages<'a>(p: RawPtr<c_void>, len: i32) -> &'a mut [*mut c_void] {
    if p.is_null() || len <= 0 {
        return &mut [];
    }
    // SAFETY: managed passes a pinned array of `len` message pointers.
    unsafe { std::slice::from_raw_parts_mut(p.as_ptr().cast(), len as usize) }
}

fn run_callbacks() {
    // Changes are collected first: managed may call back into a transport while handling one.
    let mut changes = if ready() { gns::run_callbacks() } else { vec![] };
    changes.extend(SteamP2p.pump());
    changes.iter().for_each(events::dispatch);
}

/// Our own Steam id as `SteamNetworkingIdentity::ToString` writes it, when Steam-id traffic works.
fn steam_identity() -> Option<String> {
    transport::wire()?;
    let p = platform::get();
    let id = if crate::app::dict::current_config().dedicated_server { p.server_id() } else { p.client().user() };
    Some(format!("steamid:{}", id.0))
}

fn connect_steam_id(id: u64, vport: i32) -> u32 {
    if transport::wire().is_none() {
        log_error(&format!(
            "networking: can't connect to Steam id {id}: Steam-id connections need the Steam platform \
             (a --features steam build, UNDERBOX_PLATFORM=steam, Steam running); connect by IP instead"
        ));
        return 0;
    }
    SteamP2p.connect(id, vport)
}

/// `ESteamNetworkingAvailability::CannotTry`: no relay network or certificate authority offline.
const CANNOT_TRY: i64 = -102;

pub fn register(r: &mut Registry) {
    bind!(r, glue::RUN_CALLBACKS, run_callbacks);
    bind!(r, glue::SET_DEBUG_FUNCTION, |level, f| {
        if ready() {
            // SAFETY: managed passes an unmanaged `(int type, char* msg)` function pointer or null.
            let f = (!f.is_null()).then(|| unsafe { std::mem::transmute::<*mut c_void, gns::ffi::DebugFn>(f.as_ptr()) });
            gns::set_debug_output(level, f);
        }
    });
    bind!(r, glue::GET_AUTHENTICATION_STATUS, |_| CANNOT_TRY);
    bind!(r, glue::GET_RELAY_NETWORK_STATUS, |_| CANNOT_TRY);

    // FakeIP (hide_address) needs Steam's relays; offline the real address is used.
    bind!(r, glue::CREATE_IP_BASED_SOCKET, |port, _hide| listen(if ready() { gns::listen_ip(port as u16) } else { 0 }));
    bind!(r, glue::CREATE_SOCKET, |vport| listen(match transport::wire() {
        Some(_) => SteamP2p.listen(vport),
        None if ready() => gns::listen_p2p(vport),
        None => 0,
    }));
    bind!(r, glue::CLOSE_SOCKET, |s| {
        if P2p::is_listen_socket(s.id) { SteamP2p.close_listen(s.id) } else { gns::close_listen(s.id) };
    });
    bind!(r, glue::GET_SOCKET_ADDRESS, |s| StrRet::from(if P2p::is_listen_socket(s.id) { steam_identity() } else { gns::listen_address(s.id) }));
    bind!(r, glue::GET_IDENTITY, || StrRet::from(steam_identity().or_else(|| if ready() { gns::identity() } else { None })));
    bind!(r, glue::BEGIN_ASYNC_REQUEST_FAKE_IP, || {});

    bind!(r, glue::CONNECT_TO_IP_ADDRESS, |a| {
        let a = a.map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
        connection(if ready() { gns::connect_ip(&a) } else { 0 })
    });
    bind!(r, glue::CONNECT_TO_STEAM_ID, |id, vport| connection(connect_steam_id(id, vport)));
    bind!(r, glue::ACCEPT_CONNECTION, |c| {
        of(c.id).accept(c.id);
    });
    bind!(r, glue::CLOSE_CONNECTION, |c, reason, debug| {
        of(c.id).close(c.id, reason, debug, true);
    });
    bind!(r, glue::GET_CONNECTION_STATE, |c| of(c.id).state(c.id));
    bind!(r, glue::GET_CONNECTION_DESCRIPTION, |c| StrRet::from(of(c.id).description(c.id)));
    bind!(r, glue::GET_CONNECTION_STEAM_ID, |c| of(c.id).steam_id(c.id));

    bind!(r, glue::CREATE_POLL_GROUP, || HSteamNetPollGroup { id: if ready() { gns::create_poll_group() } else { 0 } });
    bind!(r, glue::DESTROY_POLL_GROUP, |pg| {
        gns::destroy_poll_group(pg.id);
    });
    bind!(r, glue::SET_POLL_GROUP, |c, pg| {
        of(c.id).set_poll_group(c.id, pg.id);
    });
    bind!(r, glue::GET_POLL_GROUP_MESSAGES, |pg, out, max| {
        let out = messages(out, max);
        let n = gns::receive_poll_group(pg.id, out);
        (n + SteamP2p.receive_poll_group(pg.id, &mut out[n..])) as i32
    });
    bind!(r, glue::GET_CONNECTION_MESSAGES, |c, out, max| of(c.id).receive(c.id, messages(out, max)) as i32);

    bind!(r, glue::ALLOCATE_MESSAGE, |c, size, flags| RawPtr::from_raw(of(c.id).allocate_message(c.id, size, flags)));
    bind!(r, glue::ALLOCATE_MESSAGE_WITH_MANAGED_BUFFER, |c, data, size, flags| {
        let m = of(c.id).allocate_message(c.id, size, flags);
        if !m.is_null() && !data.is_null() && size > 0 {
            // SAFETY: managed's buffer holds `size` bytes; the message was allocated with that size.
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), message::data(m).cast(), size as usize) };
        }
        RawPtr::from_raw(m)
    });
    // SAFETY: managed passes a message from AllocateMessage.
    bind!(r, glue::GET_MESSAGE_DATA_BUFFER, |m| RawPtr::from_raw(if m.is_null() { m.as_ptr() } else { unsafe { message::data(m.as_ptr()) } }));
    bind!(r, glue::SEND_MESSAGES, |msgs, n| {
        // SAFETY: managed's batch holds `n` freshly allocated messages, which sending consumes.
        unsafe { transport::send_messages(messages(msgs, n)) };
    });
    bind!(r, glue::SEND_MESSAGE, |c, data, size, flags| {
        // SAFETY: managed's buffer holds `size` bytes.
        let bytes = if data.is_null() || size <= 0 { &[][..] } else { unsafe { data.cast::<u8>().as_slice(size as usize) } };
        of(c.id).send(c.id, bytes, flags) as i64
    });
    // P2P reliable data goes out as it is sent; only GNS batches.
    bind!(r, glue::FLUSH_MESSAGES_ON_CONNECTION, |c| {
        if !SteamP2p::owns(c.id) {
            gns::flush(c.id);
        }
    });
    // Messages always carry their own release function, ours or the library's.
    bind!(r, glue::SET_FREE_DATA_CALLBACK, |_| {});
    bind!(r, sockets::RELEASE_MESSAGE, |_, m| {
        if !m.is_null() {
            // SAFETY: managed releases each received message exactly once.
            unsafe { message::release_any(m.as_ptr()) }
        }
    });
    bind!(r, sockets::GET_CONNECTION_INFO, |_, c| {
        let mut s = SteamNetConnectionRealTimeStatus_t::default();
        of(c.id).realtime_status(c.id, &mut s);
        s
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_id_connects_fail_cleanly_offline() {
        assert!(transport::wire().is_none(), "tests run on the offline platform");
        assert_eq!(connect_steam_id(90071992549575139, 77), 0);
        assert_eq!(steam_identity(), None);
        assert_eq!(SteamP2p.pump(), vec![]);
    }
}
