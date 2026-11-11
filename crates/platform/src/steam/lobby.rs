//! Lobbies and networking utilities over the Steam client (peer messages: `peers.rs`).

use super::ffi;
use super::strings::{c, owned, written};
use crate::{Lobbies, NetUtils, SteamId};
use steamworks::sys;

macro_rules! mm {
    ($f:ident $(, $arg:expr)*) => {
        // SAFETY: the client is initialised while it hands this service out, and each argument is valid for the call.
        unsafe { sys::$f(sys::SteamAPI_SteamMatchmaking_v009() $(, $arg)*) }
    };
}

fn utils() -> *mut sys::ISteamNetworkingUtils {
    // SAFETY: as `mm`.
    unsafe { sys::SteamAPI_SteamNetworkingUtils_SteamAPI_v004() }
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
