//! `ISteamNetworkingMessages` for the client (the user's) or the game server (its own Steam id).

use super::ffi;
use super::pump::field;
use crate::{PeerMessages, RealTimeStatus, SessionState, SteamId, SteamResult};
use std::ffi::c_void;
use std::mem::offset_of;
use std::sync::Mutex;
use steamworks::sys;

pub(crate) struct SteamPeers {
    server: bool,
    /// Session requests nothing else accepted, for [`PeerMessages::session_requests`].
    requests: Mutex<Vec<SteamId>>,
}

/// A Steam-id networking identity.
pub(crate) fn identity(user: SteamId) -> sys::SteamNetworkingIdentity {
    // SAFETY: a zeroed identity is Steam's "invalid" one, which SetSteamID64 overwrites.
    let mut id: sys::SteamNetworkingIdentity = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    unsafe { sys::SteamAPI_SteamNetworkingIdentity_SetSteamID64(&mut id, user.0) };
    id
}

impl SteamPeers {
    pub fn client() -> Self {
        Self { server: false, requests: Mutex::default() }
    }

    pub fn server() -> Self {
        Self { server: true, requests: Mutex::default() }
    }

    fn raw(&self) -> *mut sys::ISteamNetworkingMessages {
        // SAFETY: the client or game server is initialised while it hands this service out.
        unsafe { if self.server { sys::SteamAPI_SteamGameServerNetworkingMessages_SteamAPI_v002() } else { sys::SteamAPI_SteamNetworkingMessages_SteamAPI_v002() } }
    }

    pub fn requested(&self, user: SteamId) {
        self.requests.lock().unwrap().push(user);
    }
}

// SAFETY (every unsafe block below): `raw()` is live while this service is offered, and each pointer is valid for its call.
impl PeerMessages for SteamPeers {
    fn session_state(&self, user: SteamId) -> SessionState {
        let mut buf = ffi::zeroed_bytes::<sys::SteamNetConnectionInfo_t>();
        let (info, range) = ffi::aligned::<sys::SteamNetConnectionInfo_t>(&mut buf);
        let state = unsafe { ffi::peer_session_info(self.raw(), &identity(user), info, std::ptr::null_mut()) };
        let b = &buf[range];
        let debug = super::pump::c_str_at(b, offset_of!(sys::SteamNetConnectionInfo_t, m_szEndDebug), 128);
        SessionState { state, end_reason: field!(b, sys::SteamNetConnectionInfo_t, m_eEndReason: i32), debug }
    }

    fn send(&self, user: SteamId, data: &[u8], flags: i32, channel: i32) -> SteamResult {
        unsafe { ffi::peer_send(self.raw(), &identity(user), data.as_ptr(), data.len() as u32, flags, channel) }
    }

    fn receive(&self, channel: i32, out: &mut [*mut c_void]) -> i32 {
        unsafe { sys::SteamAPI_ISteamNetworkingMessages_ReceiveMessagesOnChannel(self.raw(), channel, out.as_mut_ptr().cast(), out.len() as i32) }
    }

    fn receive_copied(&self, channel: i32, max: usize) -> Vec<(SteamId, Vec<u8>)> {
        let mut out = vec![std::ptr::null_mut::<c_void>(); max];
        let n = self.receive(channel, &mut out).max(0) as usize;
        out[..n]
            .iter()
            .map(|&m| {
                let m = m.cast::<sys::SteamNetworkingMessage_t>();
                // SAFETY: Steam handed us `n` live messages; each is read, then released once.
                unsafe {
                    let (data, size) = ((*m).m_pData, (*m).m_cbSize.max(0) as usize);
                    let bytes = if data.is_null() { Vec::new() } else { std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec() };
                    let from = sys::SteamAPI_SteamNetworkingIdentity_GetSteamID64(&mut (*m).m_identityPeer);
                    sys::SteamAPI_SteamNetworkingMessage_t_Release(m);
                    (SteamId(from), bytes)
                }
            })
            .collect()
    }

    fn accept(&self, user: SteamId) -> bool {
        unsafe { sys::SteamAPI_ISteamNetworkingMessages_AcceptSessionWithUser(self.raw(), &identity(user)) }
    }

    fn close(&self, user: SteamId, channel: i32) -> bool {
        unsafe { sys::SteamAPI_ISteamNetworkingMessages_CloseChannelWithUser(self.raw(), &identity(user), channel) }
    }

    unsafe fn release(&self, message: *mut c_void) {
        // SAFETY: the caller's contract: a message from `receive`, released once.
        unsafe { sys::SteamAPI_SteamNetworkingMessage_t_Release(message.cast()) }
    }

    fn status(&self, user: SteamId) -> RealTimeStatus {
        type S = sys::SteamNetConnectionRealTimeStatus_t;
        let mut buf = ffi::zeroed_bytes::<S>();
        let (status, range) = ffi::aligned::<S>(&mut buf);
        unsafe { ffi::peer_session_info(self.raw(), &identity(user), std::ptr::null_mut(), status) };
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

    fn session_requests(&self) -> Vec<SteamId> {
        std::mem::take(&mut *self.requests.lock().unwrap())
    }
}
