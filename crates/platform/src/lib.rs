//! Platform services behind one trait (docs/PLAN.md): Steam, offline/dev, or custom backends.
//! The engine core talks only to [`Platform`]; native's Steam slots are a view over it.

mod offline;
mod steam_id;

pub use offline::Offline;
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
}

/// Something the platform reports back asynchronously, drained by [`Platform::poll`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ServerConnected,
    /// The verdict on a [`Platform::begin_auth`] ticket.
    AuthResult { user: SteamId, owner: SteamId, ok: bool },
}

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;

    fn server_init(&self, port: u16, query_port: u16);
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

/// The process's platform; [`Offline`] unless a backend was installed first.
pub fn get() -> &'static Arc<dyn Platform> {
    INSTALLED.get_or_init(|| Arc::new(Offline::default()))
}

/// Choose the backend; false if one is already in use.
pub fn install(p: Arc<dyn Platform>) -> bool {
    INSTALLED.set(p).is_ok()
}
