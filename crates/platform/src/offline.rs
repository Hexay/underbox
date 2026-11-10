//! No platform services: the game server is "logged on" at once as an anonymous server, and every
//! auth ticket is accepted (dev and LAN; anyone can claim any identity).

use crate::{AuthResponse, BeginAuth, Event, Platform, ServerInfo, SteamId};
use std::sync::Mutex;

#[derive(Default)]
pub struct Offline {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    logged_on: bool,
    info: ServerInfo,
    sessions: Vec<SteamId>,
    events: Vec<Event>,
}

impl Offline {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }
}

impl Platform for Offline {
    fn name(&self) -> &'static str {
        "offline"
    }

    fn server_init(&self, _port: u16, _query_port: u16, _version: &str) {}

    fn server_shutdown(&self) {
        *self.state() = State::default();
    }

    fn server_log_on(&self, _token: Option<&str>) {
        let mut s = self.state();
        if !s.logged_on {
            s.logged_on = true;
            s.events.push(Event::ServerConnected);
        }
    }

    fn server_log_off(&self) {
        self.state().logged_on = false;
    }

    fn server_logged_on(&self) -> bool {
        self.state().logged_on
    }

    fn server_id(&self) -> SteamId {
        SteamId::anonymous_game_server(std::process::id())
    }

    fn server_public_ip(&self) -> Option<String> {
        None
    }

    fn server_info(&self, edit: &mut dyn FnMut(&mut ServerInfo)) {
        edit(&mut self.state().info);
    }

    fn begin_auth(&self, user: SteamId, _ticket: &[u8]) -> BeginAuth {
        let mut s = self.state();
        if s.sessions.contains(&user) {
            return BeginAuth::DuplicateRequest;
        }
        s.sessions.push(user);
        s.events.push(Event::AuthResult { user, owner: user, response: AuthResponse::Ok });
        BeginAuth::Ok
    }

    fn end_auth(&self, user: SteamId) {
        self.state().sessions.retain(|&u| u != user);
    }

    fn poll(&self) -> Vec<Event> {
        std::mem::take(&mut self.state().events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_on_and_accepts_tickets() {
        let p = Offline::default();
        assert!(!p.server_logged_on());
        p.server_log_on(None);
        assert!(p.server_logged_on());
        let user = SteamId(76561197960265729);
        assert_eq!(p.begin_auth(user, b"x"), BeginAuth::Ok);
        assert_eq!(p.begin_auth(user, b"x"), BeginAuth::DuplicateRequest);
        assert_eq!(p.poll(), [Event::ServerConnected, Event::AuthResult { user, owner: user, response: AuthResponse::Ok }]);
        assert!(p.poll().is_empty());
        p.end_auth(user);
        assert_eq!(p.begin_auth(user, b"x"), BeginAuth::Ok);
        p.server_shutdown();
        assert!(!p.server_logged_on());
        assert!(p.poll().is_empty(), "shutdown drops pending events");
    }
}
