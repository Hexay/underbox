//! A real Steam game server through steamworks-rs. Steam starts in `server_init` (it needs the ports);
//! before that, or if it fails (no steamclient, no app id), every call goes to an [`Offline`] fallback.
//! Steam needs the app id from `steam_appid.txt` in the working directory or `SteamAppId` in the env.

use crate::{BeginAuth, Event, Offline, Platform, ServerInfo, SteamId};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, OnceLock};
use steamworks::{
    AuthSessionError, CallbackHandle, Server, ServerMode, SteamServersConnected, ValidateAuthTicketResponse, sys,
};

// TODO: carry managed's SteamGameServer_Init version through server_init; Steam's browser matches clients on it.
const VERSION: &str = "1.0.0.0";

type Events = Arc<Mutex<Vec<Event>>>;

#[derive(Default)]
pub struct Steam {
    /// Set once by `server_init`; `None` inside means Steam failed to start.
    live: OnceLock<Option<Live>>,
    fallback: Offline,
    info: Mutex<ServerInfo>,
    events: Events,
}

struct Live {
    server: Server,
    _callbacks: [CallbackHandle; 2],
}

impl Live {
    fn start(port: u16, query_port: u16, events: &Events) -> Option<Self> {
        // TODO: AuthenticationAndSecure once VAC is wanted; game_server.rs reports bSecure false to match.
        match Server::init(Ipv4Addr::UNSPECIFIED, port, query_port, ServerMode::Authentication, VERSION) {
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
fn listen(server: &Server, events: &Events) -> [CallbackHandle; 2] {
    let sink = || {
        let events = events.clone();
        move |e: Event| events.lock().unwrap().push(e)
    };
    let (connected, auth) = (sink(), sink());
    // TODO: SteamServerConnectFailure/SteamServersDisconnected have no Event yet; managed never sees them.
    [
        server.register_callback(move |_: SteamServersConnected| connected(Event::ServerConnected)),
        server.register_callback(move |r: ValidateAuthTicketResponse| auth(auth_event(&r))),
    ]
}

fn auth_event(r: &ValidateAuthTicketResponse) -> Event {
    Event::AuthResult { user: SteamId(r.steam_id.raw()), owner: SteamId(r.owner_steam_id.raw()), ok: r.response.is_ok() }
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
    fn live(&self) -> Option<&Live> {
        self.live.get().and_then(Option::as_ref)
    }
}

impl Platform for Steam {
    fn name(&self) -> &'static str {
        "steam"
    }

    fn server_init(&self, port: u16, query_port: u16) {
        let live = self.live.get_or_init(|| Live::start(port, query_port, &self.events));
        if let Some(live) = live {
            push_info(&live.server, None, &self.info.lock().unwrap());
        }
    }

    fn server_log_on(&self, token: Option<&str>) {
        match (self.live(), token) {
            (Some(l), Some(token)) => l.server.log_on(token),
            (Some(l), None) => l.server.log_on_anonymous(),
            (None, _) => self.fallback.server_log_on(token),
        }
    }

    fn server_log_off(&self) {
        match self.live() {
            // SAFETY: a live server pointer.
            Some(l) => unsafe { sys::SteamAPI_ISteamGameServer_LogOff(l.raw()) },
            None => self.fallback.server_log_off(),
        }
    }

    fn server_logged_on(&self) -> bool {
        match self.live() {
            // SAFETY: a live server pointer.
            Some(l) => unsafe { sys::SteamAPI_ISteamGameServer_BLoggedOn(l.raw()) },
            None => self.fallback.server_logged_on(),
        }
    }

    fn server_id(&self) -> SteamId {
        match self.live() {
            Some(l) => SteamId(l.server.steam_id().raw()),
            None => self.fallback.server_id(),
        }
    }

    fn server_public_ip(&self) -> Option<String> {
        // SAFETY: a live server pointer.
        self.live().and_then(|l| ip_string(unsafe { sys::SteamAPI_ISteamGameServer_GetPublicIP(l.raw()) }))
    }

    fn server_info(&self, edit: &mut dyn FnMut(&mut ServerInfo)) {
        let mut info = self.info.lock().unwrap();
        let old = info.clone();
        edit(&mut info);
        if let Some(l) = self.live() {
            push_info(&l.server, Some(&old), &info);
        }
    }

    fn begin_auth(&self, user: SteamId, ticket: &[u8]) -> BeginAuth {
        match self.live() {
            Some(l) => begin_result(l.server.begin_authentication_session(steamworks::SteamId::from_raw(user.0), ticket)),
            None => self.fallback.begin_auth(user, ticket),
        }
    }

    fn end_auth(&self, user: SteamId) {
        match self.live() {
            Some(l) => l.server.end_authentication_session(steamworks::SteamId::from_raw(user.0)),
            None => self.fallback.end_auth(user),
        }
    }

    fn poll(&self) -> Vec<Event> {
        match self.live() {
            Some(l) => {
                l.server.run_callbacks();
                std::mem::take(&mut self.events.lock().unwrap())
            }
            None => self.fallback.poll(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steamworks::AuthSessionValidateError;

    #[test]
    fn maps_begin_auth_results() {
        assert_eq!(begin_result(Ok(())), BeginAuth::Ok);
        assert_eq!(begin_result(Err(AuthSessionError::DuplicateRequest)), BeginAuth::DuplicateRequest);
        assert_eq!(begin_result(Err(AuthSessionError::ExpiredTicket)) as i64, 5);
    }

    #[test]
    fn maps_auth_ticket_response() {
        let r = |response| ValidateAuthTicketResponse {
            steam_id: steamworks::SteamId::from_raw(1),
            owner_steam_id: steamworks::SteamId::from_raw(2),
            response,
        };
        let (user, owner) = (SteamId(1), SteamId(2));
        assert_eq!(auth_event(&r(Ok(()))), Event::AuthResult { user, owner, ok: true });
        let banned = auth_event(&r(Err(AuthSessionValidateError::VACBanned)));
        assert_eq!(banned, Event::AuthResult { user, owner, ok: false });
    }

    #[test]
    fn formats_public_ip() {
        let v4 = |ip: u32| sys::SteamIPAddress_t {
            __bindgen_anon_1: sys::SteamIPAddress_t__bindgen_ty_1 { m_unIPv4: ip },
            m_eType: sys::ESteamIPType::k_ESteamIPTypeIPv4,
        };
        assert_eq!(ip_string(v4(0x7f00_0001)).as_deref(), Some("127.0.0.1"));
        assert_eq!(ip_string(v4(0)), None);
    }

    #[test]
    fn acts_offline_before_init() {
        let p = Steam::default();
        p.server_info(&mut |i| i.max_players = 8);
        p.server_log_on(None);
        assert!(p.server_logged_on());
        assert_eq!(p.poll(), [Event::ServerConnected]);
        assert_eq!(p.server_public_ip(), None);
    }

    #[test]
    #[ignore = "calls the real SteamGameServer_Init; passes only where no Steam client is installed"]
    fn falls_back_when_steam_init_fails() {
        let p = Steam::default();
        p.server_init(27015, 27016);
        assert!(p.live().is_none());
        p.server_log_on(None);
        assert_eq!(p.poll(), [Event::ServerConnected]);
    }
}
