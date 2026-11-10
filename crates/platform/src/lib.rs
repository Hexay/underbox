//! Platform services behind one trait (docs/PLAN.md): Steam, offline/dev, or custom backends.
//! The engine core talks only to [`Platform`]; native's Steam slots are a view over it.

mod offline;
#[cfg(feature = "steam")]
mod steam;
mod steam_id;

pub use offline::Offline;
#[cfg(feature = "steam")]
pub use steam::Steam;
pub use steam_id::SteamId;

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

/// Something the platform reports back asynchronously, drained by [`Platform::poll`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ServerConnected,
    ServerConnectFailure { result: SteamResult, retrying: bool },
    ServersDisconnected { result: SteamResult },
    /// The verdict on a [`Platform::begin_auth`] ticket.
    AuthResult { user: SteamId, owner: SteamId, response: AuthResponse },
}

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

    fn poll(&self) -> Vec<Event>;
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
