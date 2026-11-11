//! A real Steam game server through steamworks-rs. Steam starts in `server_init` (it needs the ports);
//! before that, after `server_shutdown`, or if it fails (no steamclient, no app id), every call goes to
//! an [`Offline`] fallback. Steam needs the app id from `steam_appid.txt` in the working directory or
//! `SteamAppId` in the env, and `libsteam_api.so` on the loader path. Callbacks are pumped by hand
//! (never steamworks-rs's `run_callbacks`, which would swallow our call results).

pub(crate) mod decode;
mod ffi;
mod inventory;
mod lobby;
mod peers;
pub(crate) mod pump;
mod servers;
pub(crate) mod strings;
mod ugc;
mod ugc_query;
pub(crate) mod user;

pub(crate) use inventory::SteamInventory;
pub(crate) use lobby::SteamLobbies;
pub(crate) use peers::SteamPeers;
pub(crate) use pump::Calls;
pub(crate) use servers::SteamServers;
pub(crate) use ugc::SteamUgc;

use crate::steam_client::SteamClient;
use crate::{BeginAuth, Client, Event, Inventory, Offline, PeerMessages, Platform, ServerInfo, SteamId, Ticket, Workshop};
use std::ffi::c_void;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, OnceLock};
use steamworks::{AuthSessionError, Server, ServerMode, sys};

pub struct Steam {
    started: OnceLock<()>,
    /// Dropping it shuts Steam down (steamworks' Manager calls SteamGameServer_Shutdown).
    live: Mutex<Option<Live>>,
    fallback: Offline,
    info: Mutex<ServerInfo>,
    calls: Arc<Calls>,
    workshop: SteamUgc,
    inventory: SteamInventory,
    peers: SteamPeers,
    client: OnceLock<Option<SteamClient>>,
}

impl Default for Steam {
    fn default() -> Self {
        let calls = Arc::new(Calls::default());
        Self {
            started: OnceLock::new(),
            live: Mutex::default(),
            fallback: Offline::default(),
            info: Mutex::default(),
            workshop: SteamUgc::server(calls.clone()),
            inventory: SteamInventory::server(calls.clone()),
            peers: SteamPeers::server(),
            calls,
            client: OnceLock::new(),
        }
    }
}

struct Live {
    server: Server,
}

impl Live {
    fn start(port: u16, query_port: u16, version: &str) -> Option<Self> {
        // TODO: AuthenticationAndSecure once VAC is wanted; game_server.rs reports bSecure false to match.
        match Server::init(Ipv4Addr::UNSPECIFIED, port, query_port, ServerMode::Authentication, version) {
            Ok((server, _client)) => Some(Self { server }),
            Err(e) => {
                eprintln!("platform: Steam game server init failed ({e}); falling back to offline");
                None
            }
        }
    }

    fn raw(&self) -> *mut sys::ISteamGameServer {
        // SAFETY: Steam is initialised for as long as a Live exists.
        unsafe { sys::SteamAPI_SteamGameServer_v015() }
    }
}

/// An auth ticket from `get(buffer, capacity, &mut len, identity)`, for `target` (0: anyone).
pub(crate) fn auth_ticket(target: SteamId, get: impl FnOnce(*mut c_void, i32, *mut u32, *const sys::SteamNetworkingIdentity) -> u32) -> Ticket {
    // SAFETY: a zeroed identity is Steam's "invalid" one, which SetSteamID64 overwrites.
    let mut identity: sys::SteamNetworkingIdentity = unsafe { std::mem::zeroed() };
    let identity = if target.0 == 0 {
        std::ptr::null()
    } else {
        // SAFETY: as above.
        unsafe { sys::SteamAPI_SteamNetworkingIdentity_SetSteamID64(&mut identity, target.0) };
        &identity as *const _
    };
    let mut bytes = vec![0u8; 1024];
    let mut len = 0u32;
    let handle = get(bytes.as_mut_ptr().cast(), 1024, &mut len, identity);
    bytes.truncate(len as usize);
    Ticket { handle, bytes }
}

fn begin_result(r: Result<(), AuthSessionError>) -> BeginAuth {
    match r {
        Ok(()) => BeginAuth::Ok,
        Err(AuthSessionError::InvalidTicket) => BeginAuth::InvalidTicket,
        Err(AuthSessionError::DuplicateRequest) => BeginAuth::DuplicateRequest,
        Err(AuthSessionError::InvalidVersion) => BeginAuth::InvalidVersion,
        Err(AuthSessionError::GameMismatch) => BeginAuth::GameMismatch,
        Err(AuthSessionError::ExpiredTicket) => BeginAuth::ExpiredTicket,
    }
}

/// Steam reports an all-zero address until the server is logged on.
fn ip_string(addr: sys::SteamIPAddress_t) -> Option<String> {
    let raw = addr.__bindgen_anon_1;
    // SAFETY: m_eType says which union member is live, and both are plain bytes.
    let ip = match addr.m_eType {
        sys::ESteamIPType::k_ESteamIPTypeIPv4 => Ipv4Addr::from(unsafe { raw.m_unIPv4 }).to_string(),
        sys::ESteamIPType::k_ESteamIPTypeIPv6 => Ipv6Addr::from(unsafe { raw.m_rgubIPv6 }).to_string(),
        _ => return None,
    };
    (ip != "0.0.0.0" && ip != "::").then_some(ip)
}

/// Push the fields of `new` that differ from `old` (all of them when `old` is `None`).
fn push_info(server: &Server, old: Option<&ServerInfo>, new: &ServerInfo) {
    macro_rules! sync {
        ($($field:ident => $set:ident($($arg:tt)*)),* $(,)?) => {$(
            if old.is_none_or(|o| o.$field != new.$field) {
                server.$set($($arg)* new.$field);
            }
        )*};
    }
    sync! {
        product => set_product(&),
        mod_dir => set_mod_dir(&),
        description => set_game_description(&),
        name => set_server_name(&),
        map => set_map_name(&),
        tags => set_game_tags(&),
        max_players => set_max_players(),
        dedicated => set_dedicated_server(),
        password_protected => set_password_protected(),
        advertise => set_advertise_server_active(),
    }
}

impl Steam {
    /// `f` on the live server, or `None` when Steam isn't running.
    fn with<R>(&self, f: impl FnOnce(&Live) -> R) -> Option<R> {
        self.live.lock().unwrap().as_ref().map(f)
    }
}

impl Platform for Steam {
    fn name(&self) -> &'static str {
        "steam"
    }

    fn server_init(&self, port: u16, query_port: u16, version: &str) {
        self.started.get_or_init(|| {
            let live = Live::start(port, query_port, version);
            if let Some(l) = &live {
                push_info(&l.server, None, &self.info.lock().unwrap());
            }
            *self.live.lock().unwrap() = live;
        });
    }

    fn server_shutdown(&self) {
        self.live.lock().unwrap().take();
        self.fallback.server_shutdown();
    }

    fn server_log_on(&self, token: Option<&str>) {
        let live = self.with(|l| match token {
            Some(token) => l.server.log_on(token),
            None => l.server.log_on_anonymous(),
        });
        live.unwrap_or_else(|| self.fallback.server_log_on(token))
    }

    fn server_log_off(&self) {
        // SAFETY: a live server pointer.
        self.with(|l| unsafe { sys::SteamAPI_ISteamGameServer_LogOff(l.raw()) }).unwrap_or_else(|| self.fallback.server_log_off())
    }

    fn server_logged_on(&self) -> bool {
        // SAFETY: a live server pointer.
        self.with(|l| unsafe { sys::SteamAPI_ISteamGameServer_BLoggedOn(l.raw()) }).unwrap_or_else(|| self.fallback.server_logged_on())
    }

    fn server_id(&self) -> SteamId {
        self.with(|l| SteamId(l.server.steam_id().raw())).unwrap_or_else(|| self.fallback.server_id())
    }

    fn server_public_ip(&self) -> Option<String> {
        // SAFETY: a live server pointer.
        self.with(|l| ip_string(unsafe { sys::SteamAPI_ISteamGameServer_GetPublicIP(l.raw()) })).flatten()
    }

    fn server_info(&self, edit: &mut dyn FnMut(&mut ServerInfo)) {
        let mut info = self.info.lock().unwrap();
        let old = info.clone();
        edit(&mut info);
        self.with(|l| push_info(&l.server, Some(&old), &info));
    }

    fn begin_auth(&self, user: SteamId, ticket: &[u8]) -> BeginAuth {
        let id = steamworks::SteamId::from_raw(user.0);
        self.with(|l| begin_result(l.server.begin_authentication_session(id, ticket)))
            .unwrap_or_else(|| self.fallback.begin_auth(user, ticket))
    }

    fn end_auth(&self, user: SteamId) {
        let id = steamworks::SteamId::from_raw(user.0);
        self.with(|l| l.server.end_authentication_session(id)).unwrap_or_else(|| self.fallback.end_auth(user))
    }

    fn server_ticket(&self, target: SteamId) -> Ticket {
        // SAFETY: a live server pointer and the buffers auth_ticket passes.
        let get = |l: &Live| auth_ticket(target, |p, cap, len, id| unsafe { sys::SteamAPI_ISteamGameServer_GetAuthSessionTicket(l.raw(), p, cap, len, id) });
        self.with(get).unwrap_or_default()
    }

    fn cancel_server_ticket(&self, handle: u32) {
        // SAFETY: a live server pointer.
        self.with(|l| unsafe { sys::SteamAPI_ISteamGameServer_CancelAuthTicket(l.raw(), handle) });
    }

    fn run_callbacks(&self, event: &mut dyn FnMut(Event), raw: &mut dyn FnMut(i32, &[u8])) {
        // Not under the live lock: the handlers run managed code, which calls back into this platform.
        if self.with(|_| ()).is_none() {
            return self.fallback.run_callbacks(event, raw);
        }
        // SAFETY: the game server is initialised.
        let pipe = unsafe { sys::SteamGameServer_GetHSteamPipe() };
        pump::pump(pipe, &self.calls, &mut |kind, b| match decode::server_event(kind, b) {
            Some(e) => event(e),
            None => {
                if let Some(user) = decode::session_request(kind, b) {
                    self.peers.requested(user);
                }
                raw(kind, b)
            }
        });
    }

    fn server_workshop(&self) -> Option<&dyn Workshop> {
        self.with(|_| &self.workshop as &dyn Workshop)
    }

    fn server_inventory(&self) -> Option<&dyn Inventory> {
        self.with(|_| &self.inventory as &dyn Inventory)
    }

    fn server_peers(&self) -> Option<&dyn PeerMessages> {
        self.with(|_| &self.peers as &dyn PeerMessages)
    }

    fn start_client(&self, app_id: u32) {
        if app_id != 0 && std::env::var_os("SteamAppId").is_none() {
            // SAFETY: called once at engine pre-init on managed's main thread, before the engine's own
            // threads exist; steam_api reads it during SteamAPI_Init just below.
            unsafe { std::env::set_var("SteamAppId", app_id.to_string()) };
        }
        self.client();
    }

    fn client(&self) -> &dyn Client {
        match self.client.get_or_init(SteamClient::start) {
            Some(c) => c,
            None => self.fallback.client(),
        }
    }
}

#[cfg(test)]
#[path = "steam_tests.rs"]
mod tests;
