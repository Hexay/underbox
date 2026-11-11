//! Lobbies, peer messages and networking utilities over the Steam client.

use super::ffi;
use super::pump::field;
use super::strings::{c, owned, written};
use crate::{Lobbies, NetUtils, PeerMessages, RealTimeStatus, SessionState, SteamId, SteamResult};
use std::ffi::c_void;
use std::mem::offset_of;
use steamworks::sys;

macro_rules! mm {
    ($f:ident $(, $arg:expr)*) => {
        // SAFETY: the client is initialised while it hands this service out, and each argument is valid for the call.
        unsafe { sys::$f(sys::SteamAPI_SteamMatchmaking_v009() $(, $arg)*) }
    };
}

macro_rules! msgs {
    ($f:path $(, $arg:expr)*) => {
        // SAFETY: as `mm`.
        unsafe { $f(sys::SteamAPI_SteamNetworkingMessages_SteamAPI_v002() $(, $arg)*) }
    };
}

fn utils() -> *mut sys::ISteamNetworkingUtils {
    // SAFETY: as `mm`.
    unsafe { sys::SteamAPI_SteamNetworkingUtils_SteamAPI_v004() }
}

/// A Steam-id networking identity.
fn identity(user: SteamId) -> sys::SteamNetworkingIdentity {
    // SAFETY: a zeroed identity is Steam's "invalid" one, which SetSteamID64 overwrites.
    let mut id: sys::SteamNetworkingIdentity = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    unsafe { sys::SteamAPI_SteamNetworkingIdentity_SetSteamID64(&mut id, user.0) };
    id
}

pub(crate) struct SteamLobbies;

impl SteamLobbies {
    pub fn is_member(&self, lobby: SteamId, user: SteamId) -> bool {
        (0..self.member_count(lobby)).any(|i| self.member(lobby, i) == user)
    }

    /// A chat message's sender and bytes (`GetLobbyChatEntry`; Steam caps messages at 4 KB).
    pub fn chat_entry(&self, lobby: SteamId, chat: i32) -> (SteamId, Vec<u8>) {
        let mut data = vec![0u8; 4096];
        let (mut user, mut kind) = (0u64, 0i32);
        // CSteamID is a packed u64 and EChatEntryType an int; Steam writes both.
        let n = mm!(
            SteamAPI_ISteamMatchmaking_GetLobbyChatEntry,
            lobby.0,
            chat,
            (&mut user as *mut u64).cast(),
            data.as_mut_ptr().cast(),
            data.len() as i32,
            (&mut kind as *mut i32).cast()
        );
        data.truncate(n.max(0) as usize);
        (SteamId(user), data)
    }
}

impl Lobbies for SteamLobbies {
    fn leave(&self, lobby: SteamId) {
        mm!(SteamAPI_ISteamMatchmaking_LeaveLobby, lobby.0)
    }
    fn member_count(&self, lobby: SteamId) -> i32 {
        mm!(SteamAPI_ISteamMatchmaking_GetNumLobbyMembers, lobby.0)
    }
    fn member(&self, lobby: SteamId, index: i32) -> SteamId {
        SteamId(mm!(SteamAPI_ISteamMatchmaking_GetLobbyMemberByIndex, lobby.0, index))
    }
    fn data(&self, lobby: SteamId, key: &str) -> String {
        owned(mm!(SteamAPI_ISteamMatchmaking_GetLobbyData, lobby.0, c(key).as_ptr()))
    }
    fn set_data(&self, lobby: SteamId, key: &str, value: &str) -> bool {
        mm!(SteamAPI_ISteamMatchmaking_SetLobbyData, lobby.0, c(key).as_ptr(), c(value).as_ptr())
    }
    fn data_count(&self, lobby: SteamId) -> i32 {
        mm!(SteamAPI_ISteamMatchmaking_GetLobbyDataCount, lobby.0)
    }
    fn delete_data(&self, lobby: SteamId, key: &str) -> bool {
        mm!(SteamAPI_ISteamMatchmaking_DeleteLobbyData, lobby.0, c(key).as_ptr())
    }
    fn data_by_index(&self, lobby: SteamId, index: i32) -> Option<(String, String)> {
        // k_nMaxLobbyKeyLength and k_cubChatMetadataMax.
        let mut value = vec![0; 8192];
        let key = written(255, |k, kn| {
            mm!(SteamAPI_ISteamMatchmaking_GetLobbyDataByIndex, lobby.0, index, k, kn as i32, value.as_mut_ptr(), value.len() as i32)
        })?;
        *value.last_mut().unwrap() = 0;
        Some((key, owned(value.as_ptr())))
    }
}

impl PeerMessages for SteamLobbies {
    fn session_state(&self, user: SteamId) -> SessionState {
        let mut buf = ffi::zeroed_bytes::<sys::SteamNetConnectionInfo_t>();
        let (info, range) = ffi::aligned::<sys::SteamNetConnectionInfo_t>(&mut buf);
        let state = msgs!(ffi::peer_session_info, &identity(user), info, std::ptr::null_mut());
        let b = &buf[range];
        let debug = super::pump::c_str_at(b, offset_of!(sys::SteamNetConnectionInfo_t, m_szEndDebug), 128);
        SessionState { state, end_reason: field!(b, sys::SteamNetConnectionInfo_t, m_eEndReason: i32), debug }
    }

    fn send(&self, user: SteamId, data: &[u8], flags: i32, channel: i32) -> SteamResult {
        msgs!(ffi::peer_send, &identity(user), data.as_ptr(), data.len() as u32, flags, channel)
    }

    fn receive(&self, channel: i32, out: &mut [*mut c_void]) -> i32 {
        msgs!(sys::SteamAPI_ISteamNetworkingMessages_ReceiveMessagesOnChannel, channel, out.as_mut_ptr().cast(), out.len() as i32)
    }

    fn accept(&self, user: SteamId) -> bool {
        msgs!(sys::SteamAPI_ISteamNetworkingMessages_AcceptSessionWithUser, &identity(user))
    }

    fn close(&self, user: SteamId, channel: i32) -> bool {
        msgs!(sys::SteamAPI_ISteamNetworkingMessages_CloseChannelWithUser, &identity(user), channel)
    }

    unsafe fn release(&self, message: *mut c_void) {
        // SAFETY: the caller's contract: a message from `receive`, released once.
        unsafe { sys::SteamAPI_SteamNetworkingMessage_t_Release(message.cast()) }
    }

    fn status(&self, user: SteamId) -> RealTimeStatus {
        type S = sys::SteamNetConnectionRealTimeStatus_t;
        let mut buf = ffi::zeroed_bytes::<S>();
        let (status, range) = ffi::aligned::<S>(&mut buf);
        msgs!(ffi::peer_session_info, &identity(user), std::ptr::null_mut(), status);
        let b = &buf[range];
        RealTimeStatus {
            state: field!(b, S, m_eState: i32),
            ping: field!(b, S, m_nPing: i32),
            quality_local: field!(b, S, m_flConnectionQualityLocal: f32),
            quality_remote: field!(b, S, m_flConnectionQualityRemote: f32),
            out_packets_per_sec: field!(b, S, m_flOutPacketsPerSec: f32),
            out_bytes_per_sec: field!(b, S, m_flOutBytesPerSec: f32),
            in_packets_per_sec: field!(b, S, m_flInPacketsPerSec: f32),
            in_bytes_per_sec: field!(b, S, m_flInBytesPerSec: f32),
            send_rate_bytes_per_sec: field!(b, S, m_nSendRateBytesPerSecond: i32),
            pending_unreliable: field!(b, S, m_cbPendingUnreliable: i32),
            pending_reliable: field!(b, S, m_cbPendingReliable: i32),
            sent_unacked_reliable: field!(b, S, m_cbSentUnackedReliable: i32),
            queue_time_usec: field!(b, S, m_usecQueueTime: i64),
            max_jitter_usec: field!(b, S, m_usecMaxJitter: i32),
        }
    }
}

// SAFETY (every unsafe block below): the networking utils outlive the client that offers them, and each pointer is valid for its call.
impl NetUtils for SteamLobbies {
    fn set_config_int(&self, key: i32, value: i32) -> bool {
        unsafe { ffi::net_set_int(utils(), key, value) }
    }

    fn set_config_str(&self, key: i32, value: &str) -> bool {
        unsafe { ffi::net_set_str(utils(), key, c(value).as_ptr()) }
    }

    fn init_relay_network(&self) {
        unsafe { sys::SteamAPI_ISteamNetworkingUtils_InitRelayNetworkAccess(utils()) }
    }

    fn local_ping_location(&self) -> String {
        let mut location: sys::SteamNetworkPingLocation_t = unsafe { std::mem::zeroed() };
        // Negative: no measurement yet.
        if unsafe { sys::SteamAPI_ISteamNetworkingUtils_GetLocalPingLocation(utils(), &mut location) } < 0.0 {
            return String::new();
        }
        // k_cchMaxSteamNetworkingPingLocationString.
        super::strings::buffered(1024, |p, n| unsafe { sys::SteamAPI_ISteamNetworkingUtils_ConvertPingLocationToString(utils(), &location, p, n as i32) })
    }

    fn estimate_ping(&self, location: &str) -> i32 {
        let mut parsed: sys::SteamNetworkPingLocation_t = unsafe { std::mem::zeroed() };
        if !unsafe { sys::SteamAPI_ISteamNetworkingUtils_ParsePingLocationString(utils(), c(location).as_ptr(), &mut parsed) } {
            return -1;
        }
        unsafe { sys::SteamAPI_ISteamNetworkingUtils_EstimatePingTimeFromLocalHost(utils(), &parsed) }.max(-1)
    }
}
