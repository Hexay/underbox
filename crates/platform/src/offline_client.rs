//! A local user with no platform behind it: an identity (`UNDERBOX_STEAMID`, else one derived from the
//! OS user name) whose tickets the offline server accepts, and no social, app or cloud services.

use crate::{AuthResponse, BeginAuth, Client, Event, SteamId, Ticket};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct OfflineClient {
    user: SteamId,
    next_ticket: AtomicU32,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    sessions: Vec<SteamId>,
    events: Vec<Event>,
}

impl OfflineClient {
    pub fn new(user: SteamId) -> Self {
        Self { user, next_ticket: AtomicU32::new(1), state: Mutex::default() }
    }

    /// The identity this machine's user gets.
    pub fn local_user() -> SteamId {
        if let Some(id) = std::env::var("UNDERBOX_STEAMID").ok().and_then(|s| s.trim().parse().ok()) {
            return SteamId(id);
        }
        let name = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
        SteamId::individual(fnv1a(name.as_bytes()) & 0x7FFF_FFFF | 1)
    }
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811C_9DC5u32, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

impl Client for OfflineClient {
    fn user(&self) -> SteamId {
        self.user
    }

    fn logged_on(&self) -> bool {
        false
    }

    /// The ticket is just our id: an offline server takes anyone at their word.
    fn ticket(&self, _target: SteamId) -> Ticket {
        Ticket { handle: self.next_ticket.fetch_add(1, Ordering::Relaxed), bytes: self.user.0.to_le_bytes().to_vec() }
    }

    fn cancel_ticket(&self, _handle: u32) {}

    fn begin_auth(&self, user: SteamId, _ticket: &[u8]) -> BeginAuth {
        let mut s = self.state.lock().unwrap();
        if s.sessions.contains(&user) {
            return BeginAuth::DuplicateRequest;
        }
        s.sessions.push(user);
        s.events.push(Event::AuthResult { user, owner: user, response: AuthResponse::Ok });
        BeginAuth::Ok
    }

    fn end_auth(&self, user: SteamId) {
        self.state.lock().unwrap().sessions.retain(|&u| u != user);
    }

    fn run_callbacks(&self, event: &mut dyn FnMut(Event), _raw: &mut dyn FnMut(i32, &[u8])) {
        let events = std::mem::take(&mut self.state.lock().unwrap().events);
        events.into_iter().for_each(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_tickets_and_auth() {
        let c = OfflineClient::new(SteamId::individual(42));
        assert_eq!(c.user().0, 76561197960265728 + 42);
        let (a, b) = (c.ticket(SteamId(0)), c.ticket(SteamId(0)));
        assert_ne!(a.handle, b.handle);
        assert_eq!(a.bytes, c.user().0.to_le_bytes());
        let peer = SteamId::individual(7);
        assert_eq!(c.begin_auth(peer, b"t"), BeginAuth::Ok);
        assert_eq!(c.begin_auth(peer, b"t"), BeginAuth::DuplicateRequest);
        let mut got = vec![];
        c.run_callbacks(&mut |e| got.push(e), &mut |_, _| unreachable!());
        assert_eq!(got, [Event::AuthResult { user: peer, owner: peer, response: AuthResponse::Ok }]);
        assert!(c.friends().is_none() && c.apps().is_none() && c.storage().is_none());
    }

    #[test]
    fn local_user_is_an_individual_account() {
        assert_eq!(OfflineClient::local_user().account_type(), 1);
    }
}
