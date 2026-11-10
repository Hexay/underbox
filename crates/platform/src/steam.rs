//! A real Steam game server through steamworks-rs. Steam starts in `server_init` (it needs the ports);
//! before that, after `server_shutdown`, or if it fails (no steamclient, no app id), every call goes to
//! an [`Offline`] fallback. Steam needs the app id from `steam_appid.txt` in the working directory or
//! `SteamAppId` in the env, and `libsteam_api.so` on the loader path.

use crate::steam_client::SteamClient;
use crate::{AuthResponse, BeginAuth, Client, Event, Offline, Platform, ServerInfo, SteamId, SteamResult};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, OnceLock};
use steamworks::{
    AuthSessionError, AuthSessionValidateError as V, CallbackHandle, Server, ServerMode, SteamError,
    SteamServerConnectFailure, SteamServersConnected, SteamServersDisconnected, ValidateAuthTicketResponse, sys,
};

type Events = Arc<Mutex<Vec<Event>>>;

#[derive(Default)]
pub struct Steam {
    started: OnceLock<()>,
    /// Dropping it shuts Steam down (steamworks' Manager calls SteamGameServer_Shutdown).
    live: Mutex<Option<Live>>,
    fallback: Offline,
    info: Mutex<ServerInfo>,
    events: Events,
    client: OnceLock<Option<SteamClient>>,
}

struct Live {
    server: Server,
    _callbacks: [CallbackHandle; 4],
}

impl Live {
    fn start(port: u16, query_port: u16, version: &str, events: &Events) -> Option<Self> {
        // TODO: AuthenticationAndSecure once VAC is wanted; game_server.rs reports bSecure false to match.
        match Server::init(Ipv4Addr::UNSPECIFIED, port, query_port, ServerMode::Authentication, version) {
            Ok((server, _client)) => {
                let _callbacks = listen(&server, events);
                Some(Self { server, _callbacks })
            }
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

/// Queue the callbacks the trait reports; they fire inside `run_callbacks` (from `poll`).
fn listen(server: &Server, events: &Events) -> [CallbackHandle; 4] {
    let sink = || {
        let events = events.clone();
        move |e: Event| events.lock().unwrap().push(e)
    };
    let (connected, failed, lost, auth) = (sink(), sink(), sink(), sink());
    [
        server.register_callback(move |_: SteamServersConnected| connected(Event::ServerConnected)),
        server.register_callback(move |f: SteamServerConnectFailure| {
            failed(Event::ServerConnectFailure { result: result_code(f.reason), retrying: f.still_retrying })
        }),
        server.register_callback(move |d: SteamServersDisconnected| lost(Event::ServersDisconnected { result: result_code(d.reason) })),
        server.register_callback(move |r: ValidateAuthTicketResponse| auth(auth_event(&r))),
    ]
}

/// steamworks only converts EResult → SteamError, so invert it by search; unknown codes become Fail (2).
fn result_code(e: SteamError) -> SteamResult {
    (1..=130).find(|&n| SteamError::try_from(n as i64).ok() == Some(e)).unwrap_or(2)
}

fn auth_event(r: &ValidateAuthTicketResponse) -> Event {
    let response = match r.response {
        Ok(()) => AuthResponse::Ok,
        Err(V::UserNotConnectedToSteam) => AuthResponse::UserNotConnectedToSteam,
        Err(V::NoLicenseOrExpired) => AuthResponse::NoLicenseOrExpired,
        Err(V::VACBanned) => AuthResponse::VacBanned,
        Err(V::LoggedInElseWhere) => AuthResponse::LoggedInElsewhere,
        Err(V::VACCheckTimedOut) => AuthResponse::VacCheckTimedOut,
        Err(V::AuthTicketCancelled) => AuthResponse::TicketCanceled,
        Err(V::AuthTicketInvalidAlreadyUsed) => AuthResponse::TicketAlreadyUsed,
        Err(V::PublisherIssuedBan) => AuthResponse::PublisherIssuedBan,
        Err(V::AuthTicketNetworkIdentityFailure) => AuthResponse::NetworkIdentityFailure,
        Err(_) => AuthResponse::TicketInvalid,
    };
    Event::AuthResult { user: SteamId(r.steam_id.raw()), owner: SteamId(r.owner_steam_id.raw()), response }
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
            let live = Live::start(port, query_port, version, &self.events);
            if let Some(l) = &live {
                push_info(&l.server, None, &self.info.lock().unwrap());
            }
            *self.live.lock().unwrap() = live;
        });
    }

    fn server_shutdown(&self) {
        self.live.lock().unwrap().take();
        self.events.lock().unwrap().clear();
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

    fn poll(&self) -> Vec<Event> {
        // Callbacks push into `events` (its own lock), so running them under the live lock is fine.
        match self.with(|l| l.server.run_callbacks()) {
            Some(()) => std::mem::take(&mut self.events.lock().unwrap()),
            None => self.fallback.poll(),
        }
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
