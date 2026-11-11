//! The game-server browser (`ISteamMatchmakingServers`): server lists, filtered by key/value pairs.

use crate::SteamId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerQuery {
    Internet,
    Lan,
    Favorites,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerListEvent {
    /// One server's `gameserveritem_t`, byte for byte as Steam lays it out (managed reads it so).
    Responded { item: Vec<u8>, server: SteamId },
    Finished,
}

pub trait ServerBrowser: Send + Sync {
    /// Start a query; its id, or `None` if it couldn't start.
    fn request(&self, kind: ServerQuery, filters: &[(String, String)]) -> Option<u64>;
    /// What query `id` heard since the last call. Updated while callbacks run.
    fn take(&self, id: u64) -> Vec<ServerListEvent>;
    fn cancel(&self, id: u64);
}
