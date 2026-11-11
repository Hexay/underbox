//! The Steam client (the logged-in user) through steam_api's flat C API. Callbacks are pumped by
//! manual dispatch and handed over raw, as native engine2 does: managed's own Steamworks layer
//! decodes them and reads its call results itself, so each one is delivered before it is freed.
//! The ones the platform reports itself (lobbies, inventory, web tickets) are decoded on the way.

use crate::steam::user::steam;
use crate::steam::{Calls, SteamInventory, SteamLobbies, SteamServers, SteamUgc, auth_ticket, decode, pump, strings};
use crate::{
    Apps, BeginAuth, Client, CloudStorage, Event, Friends, Inventory, InventoryEvent, Lobbies, LobbyEvent, NetUtils, PeerMessages, ServerBrowser,
    SteamId, TextFilter, Ticket, Workshop,
};
use std::collections::HashSet;
use std::ffi::CStr;
use std::sync::{Arc, Mutex};
use steamworks::sys;

pub struct SteamClient {
    pipe: sys::HSteamPipe,
    calls: Arc<Calls>,
    workshop: SteamUgc,
    lobbies: SteamLobbies,
    /// Lobbies we've entered; Steam only answers membership for those we're still in.
    joined: Mutex<HashSet<u64>>,
    inventory: SteamInventory,
    servers: SteamServers,
    /// What call results reported, delivered after the pump.
    events: Arc<Mutex<Vec<Event>>>,
    web: Mutex<WebTicket>,
}

/// The web API ticket asked for, and once it has come, its hex.
#[derive(Default)]
struct WebTicket {
    handle: Option<u32>,
    ticket: Option<String>,
}

impl SteamClient {
    /// Start the Steam client API; `None` (with the reason logged) without a running Steam or app id.
    pub fn start() -> Option<Self> {
        let mut err: sys::SteamErrMsg = [0; 1024];
        // SAFETY: InitFlat writes a NUL-terminated message into the buffer it's given.
        let ok = unsafe { sys::SteamAPI_InitFlat(&mut err) } == sys::ESteamAPIInitResult::k_ESteamAPIInitResult_OK;
        if !ok {
            // SAFETY: as above.
            let why = unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy();
            eprintln!("platform: Steam client init failed ({why}); using an offline identity");
            return None;
        }
        // SAFETY: Steam is initialised.
        unsafe { sys::SteamAPI_ManualDispatch_Init() };
        let calls = Arc::new(Calls::default());
        let events = Arc::new(Mutex::default());
        Some(Self {
            // SAFETY: as above.
            pipe: unsafe { sys::SteamAPI_GetHSteamPipe() },
            workshop: SteamUgc::client(calls.clone()),
            inventory: SteamInventory::client(calls.clone(), events.clone()),
            calls,
            lobbies: SteamLobbies,
            joined: Mutex::default(),
            servers: SteamServers::default(),
            events,
            web: Mutex::default(),
        })
    }

    /// Report what the platform decodes itself; true if it's ours alone (managed never sees it).
    fn observe(&self, kind: i32, data: &[u8], event: &mut dyn FnMut(Event)) -> bool {
        if let Some(e) = decode::lobby_event(kind, data) {
            if let LobbyEvent::Entered { lobby } | LobbyEvent::Created { lobby } = e {
                self.joined.lock().unwrap().insert(lobby.0);
            }
            event(Event::Lobby(e));
        }
        if let Some((lobby, chat)) = decode::chat_message(kind, data) {
            let (member, data) = self.lobbies.chat_entry(lobby, chat);
            event(Event::Lobby(LobbyEvent::Chat { lobby, member, data }));
        }
        // Native accepts peer sessions from people we share a lobby with (managed never calls Accept).
        if let Some(user) = decode::session_request(kind, data) {
            let joined: Vec<u64> = self.joined.lock().unwrap().iter().copied().collect();
            if joined.into_iter().any(|l| self.lobbies.is_member(SteamId(l), user)) {
                self.lobbies.accept(user);
            }
        }
        if kind == decode::ids::INVENTORY_DEFINITIONS {
            event(Event::Inventory(InventoryEvent::DefinitionsUpdated));
        }
        if let Some((order, ok)) = decode::micro_txn(kind, data) {
            self.inventory.authorized(order, ok);
        }
        if let Some((handle, result, bytes)) = decode::web_ticket(kind, data) {
            let mut web = self.web.lock().unwrap();
            if web.handle == Some(handle) {
                web.ticket = Some(if result == 1 { strings::hex(&bytes) } else { String::new() }).filter(|t| !t.is_empty());
            }
        }
        false
    }
}

impl Drop for SteamClient {
    fn drop(&mut self) {
        // SAFETY: started in `start`.
        unsafe { sys::SteamAPI_Shutdown() }
    }
}

impl Client for SteamClient {
    fn user(&self) -> SteamId {
        SteamId(steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_GetSteamID))
    }

    fn logged_on(&self) -> bool {
        steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_BLoggedOn)
    }

    fn ticket(&self, target: SteamId) -> Ticket {
        auth_ticket(target, |p, cap, len, id| steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_GetAuthSessionTicket, p, cap, len, id))
    }

    fn cancel_ticket(&self, handle: u32) {
        steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_CancelAuthTicket, handle)
    }

    fn begin_auth(&self, user: SteamId, ticket: &[u8]) -> BeginAuth {
        use sys::EBeginAuthSessionResult as R;
        match steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_BeginAuthSession, ticket.as_ptr().cast(), ticket.len() as i32, user.0) {
            R::k_EBeginAuthSessionResultOK => BeginAuth::Ok,
            R::k_EBeginAuthSessionResultDuplicateRequest => BeginAuth::DuplicateRequest,
            R::k_EBeginAuthSessionResultInvalidVersion => BeginAuth::InvalidVersion,
            R::k_EBeginAuthSessionResultGameMismatch => BeginAuth::GameMismatch,
            R::k_EBeginAuthSessionResultExpiredTicket => BeginAuth::ExpiredTicket,
            _ => BeginAuth::InvalidTicket,
        }
    }

    fn end_auth(&self, user: SteamId) {
        steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_EndAuthSession, user.0)
    }

    fn friends(&self) -> Option<&dyn Friends> {
        Some(self)
    }

    fn apps(&self) -> Option<&dyn Apps> {
        Some(self)
    }

    fn storage(&self) -> Option<&dyn CloudStorage> {
        Some(self)
    }

    fn text_filter(&self) -> Option<&dyn TextFilter> {
        Some(self)
    }

    fn workshop(&self) -> Option<&dyn Workshop> {
        Some(&self.workshop)
    }

    fn lobbies(&self) -> Option<&dyn Lobbies> {
        Some(&self.lobbies)
    }

    fn peers(&self) -> Option<&dyn PeerMessages> {
        Some(&self.lobbies)
    }

    fn net_utils(&self) -> Option<&dyn NetUtils> {
        Some(&self.lobbies)
    }

    /// `UNDERBOX_WEB_API_IDENTITY` names the service the ticket is for; unset, it's for any.
    fn request_web_ticket(&self) {
        let mut web = self.web.lock().unwrap();
        if web.handle.is_some() {
            return;
        }
        let identity = std::env::var("UNDERBOX_WEB_API_IDENTITY").ok().map(|s| strings::c(&s));
        let identity = identity.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
        let handle = steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_GetAuthTicketForWebApi, identity);
        // k_HAuthTicketInvalid: no ticket is coming.
        web.handle = (handle != 0).then_some(handle);
    }

    fn web_ticket(&self) -> Option<String> {
        self.web.lock().unwrap().ticket.clone()
    }

    fn cancel_web_ticket(&self) {
        if let Some(h) = std::mem::take(&mut *self.web.lock().unwrap()).handle {
            self.cancel_ticket(h);
        }
    }

    fn run_callbacks(&self, event: &mut dyn FnMut(Event), raw: &mut dyn FnMut(i32, &[u8])) {
        pump::pump(self.pipe, &self.calls, &mut |kind, data| {
            if !self.observe(kind, data, event) {
                raw(kind, data);
            }
        });
        let queued = std::mem::take(&mut *self.events.lock().unwrap());
        queued.into_iter().for_each(event);
    }

    fn inventory(&self) -> Option<&dyn Inventory> {
        Some(&self.inventory)
    }

    fn servers(&self) -> Option<&dyn ServerBrowser> {
        Some(&self.servers)
    }

    fn write_screenshot(&self, rgb: &[u8], width: i32, height: i32) -> bool {
        // Steam only reads the pixels; its signature just isn't const. k_INVALID_SCREENSHOT_HANDLE is 0.
        let pixels = rgb.as_ptr().cast_mut().cast();
        steam!(SteamAPI_SteamScreenshots_v003, SteamAPI_ISteamScreenshots_WriteScreenshot, pixels, rgb.len() as u32, width, height) != 0
    }
}
