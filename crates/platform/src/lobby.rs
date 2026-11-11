//! Lobbies and the peer-to-peer traffic between their members (party voice, the lobby game
//! transport), plus the relay-network utilities those ride on. Creating, joining and listing lobbies
//! is managed's own (it calls steam_api directly); membership and chat arrive as
//! [`crate::LobbyEvent`]s.

use crate::{SteamId, SteamResult};
use std::ffi::c_void;

pub trait Lobbies: Send + Sync {
    fn leave(&self, lobby: SteamId);
    fn member_count(&self, lobby: SteamId) -> i32;
    fn member(&self, lobby: SteamId, index: i32) -> SteamId;
    /// "" for a missing key.
    fn data(&self, lobby: SteamId, key: &str) -> String;
    fn set_data(&self, lobby: SteamId, key: &str, value: &str) -> bool;
    fn data_count(&self, lobby: SteamId) -> i32;
    fn delete_data(&self, lobby: SteamId, key: &str) -> bool;
    fn data_by_index(&self, lobby: SteamId, index: i32) -> Option<(String, String)>;
}

/// A peer session's state (`ESteamNetworkingConnectionState`) and, once it's over, why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionState {
    pub state: i32,
    pub end_reason: i32,
    pub debug: String,
}

/// Steam's `SteamNetConnectionRealTimeStatus_t`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RealTimeStatus {
    pub state: i32,
    pub ping: i32,
    pub quality_local: f32,
    pub quality_remote: f32,
    pub out_packets_per_sec: f32,
    pub out_bytes_per_sec: f32,
    pub in_packets_per_sec: f32,
    pub in_bytes_per_sec: f32,
    pub send_rate_bytes_per_sec: i32,
    pub pending_unreliable: i32,
    pub pending_reliable: i32,
    pub sent_unacked_reliable: i32,
    pub queue_time_usec: i64,
    pub max_jitter_usec: i32,
}

/// `ISteamNetworkingMessages`: datagrams to a user by Steam id.
pub trait PeerMessages: Send + Sync {
    fn session_state(&self, user: SteamId) -> SessionState;
    fn send(&self, user: SteamId, data: &[u8], flags: i32, channel: i32) -> SteamResult;
    /// Fill `out` with messages, each a `SteamNetworkingMessage_t*` that managed reads directly and hands
    /// back to [`PeerMessages::release`]. Returns how many.
    fn receive(&self, channel: i32, out: &mut [*mut c_void]) -> i32;
    /// Up to `max` messages on `channel` as (sender, bytes); each is released once copied.
    fn receive_copied(&self, channel: i32, max: usize) -> Vec<(SteamId, Vec<u8>)>;
    /// Users who asked to open a session since the last call and that nothing else accepted.
    fn session_requests(&self) -> Vec<SteamId> {
        Vec::new()
    }
    fn accept(&self, user: SteamId) -> bool;
    fn close(&self, user: SteamId, channel: i32) -> bool;
    /// # Safety
    /// `message` came from [`PeerMessages::receive`] and is released once.
    unsafe fn release(&self, message: *mut c_void);
    fn status(&self, user: SteamId) -> RealTimeStatus;
}

/// `ISteamNetworkingUtils`: global transport settings and relay ping locations.
pub trait NetUtils: Send + Sync {
    /// `key` is an `ESteamNetworkingConfigValue`.
    fn set_config_int(&self, key: i32, value: i32) -> bool;
    fn set_config_str(&self, key: i32, value: &str) -> bool;
    fn init_relay_network(&self);
    /// "" until the relay network has measured where we are.
    fn local_ping_location(&self) -> String;
    /// Round trip to a peer's location in ms, or -1 when there's no estimate.
    fn estimate_ping(&self, location: &str) -> i32;
}
