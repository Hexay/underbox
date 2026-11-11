//! Platform services behind one trait (docs/PLAN.md): Steam, offline/dev, or custom backends.
//! The engine core talks only to [`Platform`]; native's Steam slots are a view over it.

mod client;
mod events;
mod inventory;
mod lobby;
mod offline;
mod offline_client;
mod servers;
#[cfg(feature = "steam")]
mod steam;
#[cfg(feature = "steam")]
mod steam_client;
mod steam_id;
pub mod workshop;
mod workshop_query;

pub use client::{Apps, Client, CloudStorage, Friends, TextFilter, Ticket};
pub use events::{Event, InventoryEvent, LobbyEvent};
pub use inventory::{Inventory, ItemInstance, PENDING, ResultHandle};
pub use lobby::{Lobbies, NetUtils, PeerMessages, RealTimeStatus, SessionState};
pub use offline::Offline;
pub use offline_client::OfflineClient;
pub use servers::{ServerBrowser, ServerListEvent, ServerQuery};
#[cfg(feature = "steam")]
pub use steam::Steam;
pub use steam_id::SteamId;
pub use workshop::Workshop;

use std::sync::{Arc, OnceLock};

/// What a game server advertises (server browser, master server).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerInfo {
    pub product: String,
    pub mod_dir: String,
    pub description: String,
    pub name: String,
    pub map: String,
    pub tags: String,
    pub max_players: i32,
    pub dedicated: bool,
    pub password_protected: bool,
    pub advertise: bool,
}

/// Steam's `EBeginAuthSessionResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum BeginAuth {
    Ok = 0,
    InvalidTicket = 1,
    DuplicateRequest = 2,
    InvalidVersion = 3,
    GameMismatch = 4,
    ExpiredTicket = 5,
}

/// Steam's `EAuthSessionResponse`: the verdict on an auth ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum AuthResponse {
    Ok = 0,
    UserNotConnectedToSteam = 1,
    NoLicenseOrExpired = 2,
    VacBanned = 3,
    LoggedInElsewhere = 4,
    VacCheckTimedOut = 5,
    TicketCanceled = 6,
    TicketAlreadyUsed = 7,
    TicketInvalid = 8,
    PublisherIssuedBan = 9,
    NetworkIdentityFailure = 10,
}

/// Steam's `EResult` as a raw code (1 is OK).
pub type SteamResult = i32;

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;

    /// `version` is the game's, which Steam's server browser matches clients against.
    fn server_init(&self, port: u16, query_port: u16, version: &str);
    /// End the game-server session entirely; later calls behave as if never initialised.
    fn server_shutdown(&self);
    /// Log the game server on: with a game-server token, or anonymously when `None`.
    fn server_log_on(&self, token: Option<&str>);
    fn server_log_off(&self);
    fn server_logged_on(&self) -> bool;
    fn server_id(&self) -> SteamId;
    fn server_public_ip(&self) -> Option<String>;
    fn server_info(&self, edit: &mut dyn FnMut(&mut ServerInfo));

    /// Start validating a connecting user's auth ticket; the verdict arrives as an [`Event`].
    fn begin_auth(&self, user: SteamId, ticket: &[u8]) -> BeginAuth;
    fn end_auth(&self, user: SteamId);
    /// The game server's own ticket, for server-to-server auth; empty when it has no identity.
    fn server_ticket(&self, _target: SteamId) -> Ticket {
        Ticket::default()
    }
    fn cancel_server_ticket(&self, _handle: u32) {}

    /// The game server's events, then any backend-native callbacks verbatim, as [`Client::run_callbacks`].
    fn run_callbacks(&self, event: &mut dyn FnMut(Event), raw: &mut dyn FnMut(i32, &[u8]));

    /// The game server's workshop (its items download through it) and inventory checks.
    fn server_workshop(&self) -> Option<&dyn Workshop> {
        None
    }
    fn server_inventory(&self) -> Option<&dyn Inventory> {
        None
    }

    /// Start the local user's services now, for `app_id` (0: the backend's own default). A game client
    /// calls this at engine pre-init, before managed's own steam_api calls need the client up.
    fn start_client(&self, _app_id: u32) {
        self.client();
    }

    /// The local user's services, started on first call. Only a game client asks; a dedicated
    /// server never does, so it never claims a user identity.
    fn client(&self) -> &dyn Client;
}

static INSTALLED: OnceLock<Arc<dyn Platform>> = OnceLock::new();

/// The process's platform: the one installed first, else [`select`]ed by `UNDERBOX_PLATFORM`.
pub fn get() -> &'static Arc<dyn Platform> {
    INSTALLED.get_or_init(|| select(std::env::var("UNDERBOX_PLATFORM").ok().as_deref()))
}

/// Choose the backend; false if one is already in use.
pub fn install(p: Arc<dyn Platform>) -> bool {
    INSTALLED.set(p).is_ok()
}

/// The backend called `name` (`steam` or `offline`); [`Offline`] for none or one this build lacks.
pub fn select(name: Option<&str>) -> Arc<dyn Platform> {
    match name {
        #[cfg(feature = "steam")]
        Some("steam") => Arc::new(Steam::default()),
        None | Some("" | "offline") => Arc::new(Offline::default()),
        Some(other) => {
            eprintln!("platform: no '{other}' backend in this build; using offline");
            Arc::new(Offline::default())
        }
    }
}

/// Drain `run_callbacks` into a list (tests, and callers with nothing raw to handle).
pub fn events(run: impl FnOnce(&mut dyn FnMut(Event), &mut dyn FnMut(i32, &[u8]))) -> Vec<Event> {
    let mut out = vec![];
    run(&mut |e| out.push(e), &mut |_, _| {});
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_by_name_with_offline_fallback() {
        assert_eq!(select(None).name(), "offline");
        assert_eq!(select(Some("custom")).name(), "offline");
        assert_eq!(select(Some("steam")).name(), if cfg!(feature = "steam") { "steam" } else { "offline" });
    }
}
