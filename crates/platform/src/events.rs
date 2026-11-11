//! What a platform reports back asynchronously, drained by `run_callbacks`.

use crate::{AuthResponse, SteamId, SteamResult};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ServerConnected,
    ServerConnectFailure { result: SteamResult, retrying: bool },
    ServersDisconnected { result: SteamResult },
    /// The verdict on a `begin_auth` ticket.
    AuthResult { user: SteamId, owner: SteamId, response: AuthResponse },
    Lobby(LobbyEvent),
    Inventory(InventoryEvent),
}

/// Lobby membership and traffic, as managed's `LobbyManager` hears it.
#[derive(Debug, Clone, PartialEq)]
pub enum LobbyEvent {
    Invite { lobby: SteamId, from: SteamId },
    Created { lobby: SteamId },
    Entered { lobby: SteamId },
    MemberEntered { lobby: SteamId, member: SteamId },
    MemberLeft { lobby: SteamId, member: SteamId },
    /// `target` is the lobby itself for lobby data, else the member whose data changed.
    DataChanged { lobby: SteamId, target: SteamId },
    Chat { lobby: SteamId, member: SteamId, data: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum InventoryEvent {
    DefinitionsUpdated,
    Prices { success: bool, currency: String },
    Purchase { success: bool, order: u64, transaction: u64 },
}
