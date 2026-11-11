//! `ISteamMatchmaking`, `ISteamNetworkingMessages` and `ISteamNetworkingUtils` over the local user's
//! [`platform::Lobbies`], [`platform::PeerMessages`] and [`platform::NetUtils`]. Each accessor is null
//! without its service (offline, or a dedicated server), which managed checks: no lobbies, no lobby
//! P2P transport, and no relay bootstrap or fake-lag settings. The methods answer as an idle, empty
//! service if called anyway.

use super::client::local;
use super::interface;
use abi::slots::{global_steam as global, i_steam_matchmaking as mm, i_steam_networking_messages as msgs, i_steam_networking_utils as nu};
use abi::structs::SteamNetConnectionRealTimeStatus_t as Status;
use abi::{NativeClass, NativeRef, RawPtr, Registry, StrRet, bind};
use platform::{Lobbies, NetUtils, PeerMessages, SteamId};
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_void};
use std::sync::OnceLock;

/// `EResult::k_EResultNoConnection`: what sending to a peer gives with no network session.
const NO_CONNECTION: i32 = 3;

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn lobbies<R: Default>(f: impl FnOnce(&dyn Lobbies) -> R) -> R {
    local().and_then(|c| c.lobbies()).map(f).unwrap_or_default()
}

fn peers<R>(none: R, f: impl FnOnce(&dyn PeerMessages) -> R) -> R {
    local().and_then(|c| c.peers()).map_or(none, f)
}

fn utils<R: Default>(f: impl FnOnce(&dyn NetUtils) -> R) -> R {
    local().and_then(|c| c.net_utils()).map(f).unwrap_or_default()
}

/// The interface singleton when its service exists, else null.
fn offered<C: NativeClass>(cell: &'static OnceLock<usize>, available: bool) -> NativeRef<C> {
    if available { interface(cell) } else { NativeRef::null() }
}

/// `s` NUL-terminated into a `cap`-byte managed buffer, cut to fit.
fn write_c(dst: RawPtr<c_void>, cap: i32, s: &str) {
    // SAFETY: managed passes a buffer of `cap` bytes.
    let dst = unsafe { dst.cast::<u8>().as_mut_slice(cap.max(0) as usize) };
    let Some(room) = dst.len().checked_sub(1) else { return };
    let n = s.len().min(room);
    dst[..n].copy_from_slice(&s.as_bytes()[..n]);
    dst[n] = 0;
}

thread_local! {
    /// `GetLobbyData`'s answer, valid until the next call on this thread (as Steam's own is).
    static LOBBY_DATA: RefCell<CString> = RefCell::default();
}

fn lobby_data(lobby: u64, key: Option<&CStr>) -> RawPtr<c_void> {
    let value = lobbies(|l| l.data(SteamId(lobby), &text(key)));
    LOBBY_DATA.with(|d| {
        *d.borrow_mut() = CString::new(value).unwrap_or_default();
        RawPtr::from_raw(d.borrow().as_ptr().cast_mut().cast())
    })
}

fn data_by_index(lobby: u64, index: i32, key: RawPtr<c_void>, key_cap: i32, value: RawPtr<c_void>, value_cap: i32) -> bool {
    let Some((k, v)) = lobbies(|l| l.data_by_index(SteamId(lobby), index)) else { return false };
    write_c(key, key_cap, &k);
    write_c(value, value_cap, &v);
    true
}

fn session_state(user: u64, end_reason: RawPtr<c_void>, debug: RawPtr<c_void>) -> i32 {
    let s = peers(Default::default(), |p| p.session_state(SteamId(user)));
    // SAFETY: managed passes an int and a 256-byte buffer.
    unsafe { end_reason.cast::<i32>().as_mut_slice(1) }.iter_mut().for_each(|r| *r = s.end_reason);
    write_c(debug, 256, &s.debug);
    s.state
}

fn receive(channel: i32, out: RawPtr<c_void>, max: i32) -> i32 {
    // SAFETY: managed passes an array of `max` message pointers.
    let out = unsafe { out.cast::<*mut c_void>().as_mut_slice(max.max(0) as usize) };
    peers(0, |p| p.receive(channel, out))
}

fn status(user: u64) -> Status {
    let s = peers(Default::default(), |p| p.status(SteamId(user)));
    Status {
        m_e_state: s.state,
        m_n_ping: s.ping,
        m_fl_connection_quality_local: s.quality_local,
        m_fl_connection_quality_remote: s.quality_remote,
        m_fl_out_packets_per_sec: s.out_packets_per_sec,
        m_fl_out_bytes_per_sec: s.out_bytes_per_sec,
        m_fl_in_packets_per_sec: s.in_packets_per_sec,
        m_fl_in_bytes_per_sec: s.in_bytes_per_sec,
        m_n_send_rate_bytes_per_second: s.send_rate_bytes_per_sec,
        m_cb_pending_unreliable: s.pending_unreliable,
        m_cb_pending_reliable: s.pending_reliable,
        m_cb_sent_unacked_reliable: s.sent_unacked_reliable,
        m_usec_queue_time: s.queue_time_usec,
        m_usec_max_jitter: s.max_jitter_usec,
        ..Default::default()
    }
}

pub fn register(r: &mut Registry) {
    static MATCHMAKING: OnceLock<usize> = OnceLock::new();
    static MESSAGES: OnceLock<usize> = OnceLock::new();
    static UTILS: OnceLock<usize> = OnceLock::new();
    bind!(r, global::STEAM_MATCHMAKING, || offered(&MATCHMAKING, lobbies(|_| true)));
    bind!(r, global::STEAM_NETWORKING_MESSAGES, || offered(&MESSAGES, peers(false, |_| true)));
    bind!(r, global::STEAM_NETWORKING_UTILS, || offered(&UTILS, utils(|_| true)));

    bind!(r, mm::LEAVE_LOBBY, |_, id| lobbies(|l| l.leave(SteamId(id))));
    bind!(r, mm::GET_NUM_LOBBY_MEMBERS, |_, id| lobbies(|l| l.member_count(SteamId(id))));
    bind!(r, mm::GET_LOBBY_MEMBER_BY_INDEX, |_, id, i| lobbies(|l| l.member(SteamId(id), i).0));
    bind!(r, mm::GET_LOBBY_DATA, |_, id, key| lobby_data(id, key));
    bind!(r, mm::SET_LOBBY_DATA, |_, id, k, v| lobbies(|l| l.set_data(SteamId(id), &text(k), &text(v))));
    bind!(r, mm::GET_LOBBY_DATA_COUNT, |_, id| lobbies(|l| l.data_count(SteamId(id))));
    bind!(r, mm::DELETE_LOBBY_DATA, |_, id, k| {
        lobbies(|l| l.delete_data(SteamId(id), &text(k)));
    });
    bind!(r, mm::GET_LOBBY_DATA_BY_INDEX, |_, id, i, k, kn, v, vn| data_by_index(id, i, k, kn, v, vn));

    // k_ESteamNetworkingConnectionState_None without a session.
    bind!(r, msgs::GET_SESSION_STATE, |_, id, reason, debug| session_state(id, reason, debug));
    bind!(r, msgs::SEND_MESSAGE_TO_USER, |_, id, data, len, flags, channel| {
        // SAFETY: managed passes its buffer and that buffer's length.
        let data = unsafe { data.cast::<u8>().as_slice(len.max(0) as usize) };
        peers(NO_CONNECTION, |p| p.send(SteamId(id), data, flags, channel))
    });
    bind!(r, msgs::RECEIVE_MESSAGES_ON_CHANNEL, |_, channel, out, max| receive(channel, out, max));
    bind!(r, msgs::ACCEPT_SESSION_WITH_USER, |_, id| peers(false, |p| p.accept(SteamId(id))));
    bind!(r, msgs::CLOSE_CHANNEL_WITH_USER, |_, id, channel| peers(false, |p| p.close(SteamId(id), channel)));
    bind!(r, msgs::RELEASE_MESSAGE, |_, m| {
        if !m.is_null() {
            // SAFETY: managed releases each message it received, once.
            peers((), |p| unsafe { p.release(m.as_ptr()) })
        }
    });
    bind!(r, msgs::GET_CONNECTION_INFO, |_, id| status(id));

    bind!(r, nu::SET_CONFIG, |_, key, v| {
        utils(|u| u.set_config_int(key as i32, v));
    });
    bind!(r, nu::SET_CONFIG_1, |_, key, v| {
        utils(|u| u.set_config_str(key as i32, &text(v)));
    });
    bind!(r, nu::INITIALIZE_RELAY_NETWORK, |_| utils(|u| u.init_relay_network()));
    bind!(r, nu::GET_LOCAL_PING_LOCATION_STRING, |_| StrRet::from(utils(|u| u.local_ping_location())));
    // -1: no estimate.
    bind!(r, nu::ESTIMATE_PING_FROM_LOCATION_STRING, |_, loc| local().and_then(|c| c.net_utils()).map_or(-1, |u| u.estimate_ping(&text(loc))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_c_strings_cut_to_fit() {
        let mut buf = [0xffu8; 4];
        write_c(RawPtr::from_raw(buf.as_mut_ptr().cast()), 4, "abcdef");
        assert_eq!(&buf, b"abc\0");
        write_c(RawPtr::from_raw(buf.as_mut_ptr().cast()), 0, "x");
        assert_eq!(&buf, b"abc\0", "no room, nothing written");
    }

    #[test]
    fn offline_has_no_lobbies_or_peers() {
        assert_eq!(lobbies(|l| l.member_count(SteamId(1))), 0);
        let data = lobby_data(1, Some(c"k"));
        // SAFETY: lobby_data returns a NUL-terminated string valid until the next call.
        assert_eq!(unsafe { CStr::from_ptr(data.as_ptr().cast()) }, c"");
        let mut out = [std::ptr::null_mut::<c_void>(); 2];
        assert_eq!(receive(0, RawPtr::from_raw(out.as_mut_ptr().cast()), 2), 0);
        assert_eq!(status(1).m_e_state, 0);
    }
}
