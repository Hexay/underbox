//! `Glue.Networking`: Steam Networking Sockets for managed's SteamNetwork transport, served by
//! GameNetworkingSockets (the `gns` crate). Messages are the library's own
//! `SteamNetworkingMessage_t`s, which managed reads in place and releases through
//! `StmNtwrkngSckts_ReleaseMessage`.

mod events;

use abi::slots::{glue_networking as glue, i_steam_networking_sockets as sockets};
use abi::structs::{HSteamListenSocket, HSteamNetConnection, HSteamNetPollGroup, SteamNetConnectionRealTimeStatus_t};
use abi::{RawPtr, Registry, StrRet, bind};
use std::ffi::c_void;

/// The library, started on first use; false (logged once) if it can't start.
fn ready() -> bool {
    static STARTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *STARTED.get_or_init(|| match gns::init() {
        Ok(()) => {
            // Without Steam there's no certificate authority to vouch for either end.
            // TODO: the Steam platform backend serves these slots from steam_api's sockets instead.
            gns::set_config_int(gns::config::IP_ALLOW_WITHOUT_AUTH, 1);
            true
        }
        Err(e) => {
            abi::rt::log::log(abi::rt::log::Level::Error, &format!("networking: {e}"));
            false
        }
    })
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
    if ready() {
        // Changes are collected first: managed may call back into the library while handling one.
        for s in gns::run_callbacks() {
            events::dispatch(&s);
        }
    }
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
    bind!(r, glue::CREATE_SOCKET, |vport| listen(if ready() { gns::listen_p2p(vport) } else { 0 }));
    bind!(r, glue::CLOSE_SOCKET, |s| {
        gns::close_listen(s.id);
    });
    bind!(r, glue::GET_SOCKET_ADDRESS, |s| StrRet::from(gns::listen_address(s.id)));
    bind!(r, glue::GET_IDENTITY, || StrRet::from(if ready() { gns::identity() } else { None }));
    bind!(r, glue::BEGIN_ASYNC_REQUEST_FAKE_IP, || {});

    bind!(r, glue::CONNECT_TO_IP_ADDRESS, |a| {
        let a = a.map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
        connection(if ready() { gns::connect_ip(&a) } else { 0 })
    });
    // SteamID connections go through Steam's P2P signalling, which only the Steam backend has.
    bind!(r, glue::CONNECT_TO_STEAM_ID, |_, _| connection(0));
    bind!(r, glue::ACCEPT_CONNECTION, |c| {
        gns::accept(c.id);
    });
    bind!(r, glue::CLOSE_CONNECTION, |c, reason, debug| {
        gns::close(c.id, reason, debug, true);
    });
    bind!(r, glue::GET_CONNECTION_STATE, |c| gns::connection_state(c.id));
    bind!(r, glue::GET_CONNECTION_DESCRIPTION, |c| StrRet::from(gns::connection_description(c.id)));
    bind!(r, glue::GET_CONNECTION_STEAM_ID, |c| gns::connection_steam_id(c.id));

    bind!(r, glue::CREATE_POLL_GROUP, || HSteamNetPollGroup { id: if ready() { gns::create_poll_group() } else { 0 } });
    bind!(r, glue::DESTROY_POLL_GROUP, |pg| {
        gns::destroy_poll_group(pg.id);
    });
    bind!(r, glue::SET_POLL_GROUP, |c, pg| {
        gns::set_poll_group(c.id, pg.id);
    });
    bind!(r, glue::GET_POLL_GROUP_MESSAGES, |pg, out, max| gns::receive_poll_group(pg.id, messages(out, max)) as i32);
    bind!(r, glue::GET_CONNECTION_MESSAGES, |c, out, max| gns::receive_connection(c.id, messages(out, max)) as i32);

    bind!(r, glue::ALLOCATE_MESSAGE, |c, size, flags| RawPtr::from_raw(gns::allocate_message(c.id, size, flags)));
    bind!(r, glue::ALLOCATE_MESSAGE_WITH_MANAGED_BUFFER, |c, data, size, flags| {
        let m = gns::allocate_message(c.id, size, flags);
        if !m.is_null() && !data.is_null() && size > 0 {
            // SAFETY: managed's buffer holds `size` bytes; the message was allocated with that size.
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), gns::message_data(m).cast(), size as usize) };
        }
        RawPtr::from_raw(m)
    });
    // SAFETY: managed passes a message from AllocateMessage.
    bind!(r, glue::GET_MESSAGE_DATA_BUFFER, |m| RawPtr::from_raw(if m.is_null() { m.as_ptr() } else { unsafe { gns::message_data(m.as_ptr()) } }));
    bind!(r, glue::SEND_MESSAGES, |msgs, n| {
        // SAFETY: managed's batch holds `n` freshly allocated messages, which sending consumes.
        unsafe { gns::send_messages(messages(msgs, n)) };
    });
    bind!(r, glue::SEND_MESSAGE, |c, data, size, flags| {
        // SAFETY: managed's buffer holds `size` bytes.
        let bytes = if data.is_null() || size <= 0 { &[][..] } else { unsafe { data.cast::<u8>().as_slice(size as usize) } };
        gns::send(c.id, bytes, flags) as i64
    });
    bind!(r, glue::FLUSH_MESSAGES_ON_CONNECTION, |c| {
        gns::flush(c.id);
    });
    // Messages are always the library's, freed by its own release function.
    bind!(r, glue::SET_FREE_DATA_CALLBACK, |_| {});
    bind!(r, sockets::RELEASE_MESSAGE, |_, m| {
        if !m.is_null() {
            // SAFETY: managed releases each received message exactly once.
            unsafe { gns::release(m.as_ptr()) }
        }
    });
    bind!(r, sockets::GET_CONNECTION_INFO, |_, c| {
        let mut s = SteamNetConnectionRealTimeStatus_t::default();
        gns::realtime_status(c.id, &mut s);
        s
    });
}
