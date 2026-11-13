//! An in-memory `ISteamNetworkingMessages` for tests: endpoints on one [`Net`], with Steam's session
//! rule (a stranger's messages wait for `AcceptSessionWithUser`, announced as a session request),
//! and loss on demand.

use platform::{PeerMessages, RealTimeStatus, SessionState, SteamId, SteamResult};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Inbox {
    ready: VecDeque<(i32, SteamId, Vec<u8>)>,
    /// From peers we haven't accepted or written to.
    held: Vec<(i32, SteamId, Vec<u8>)>,
    known: HashSet<SteamId>,
    requests: Vec<SteamId>,
}

type LoseRule = Box<dyn Fn(usize) -> bool + Send>;

#[derive(Default)]
pub struct Net {
    inboxes: Mutex<HashMap<SteamId, Inbox>>,
    /// Datagram counter and the rule that drops by it.
    sent: Mutex<usize>,
    lose: Mutex<Option<LoseRule>>,
    cut: Mutex<bool>,
}

impl Net {
    pub fn new() -> Arc<Net> {
        Arc::default()
    }

    pub fn endpoint(self: &Arc<Self>, me: u64) -> Endpoint {
        self.inboxes.lock().unwrap().entry(SteamId(me)).or_default();
        Endpoint { net: self.clone(), me: SteamId(me) }
    }

    /// Drop the datagrams `f` picks by their send index (1-based).
    pub fn lose(&self, f: impl Fn(usize) -> bool + Send + 'static) {
        *self.lose.lock().unwrap() = Some(Box::new(f));
    }

    /// Drop everything (a dead link) until set back.
    pub fn cut(&self, cut: bool) {
        *self.cut.lock().unwrap() = cut;
    }

    pub fn datagrams(&self) -> usize {
        *self.sent.lock().unwrap()
    }
}

pub struct Endpoint {
    net: Arc<Net>,
    me: SteamId,
}

impl PeerMessages for Endpoint {
    fn session_state(&self, _: SteamId) -> SessionState {
        SessionState::default()
    }

    fn send(&self, user: SteamId, data: &[u8], _flags: i32, channel: i32) -> SteamResult {
        let n = {
            let mut sent = self.net.sent.lock().unwrap();
            *sent += 1;
            *sent
        };
        let mut inboxes = self.net.inboxes.lock().unwrap();
        inboxes.get_mut(&self.me).unwrap().known.insert(user);
        let lost = *self.net.cut.lock().unwrap() || self.net.lose.lock().unwrap().as_ref().is_some_and(|f| f(n));
        let Some(to) = inboxes.get_mut(&user).filter(|_| !lost) else { return 1 };
        if to.known.contains(&self.me) {
            to.ready.push_back((channel, self.me, data.to_vec()));
        } else {
            if !to.held.iter().any(|(_, from, _)| *from == self.me) {
                to.requests.push(self.me);
            }
            to.held.push((channel, self.me, data.to_vec()));
        }
        1
    }

    fn receive(&self, _: i32, _: &mut [*mut c_void]) -> i32 {
        unreachable!("the transport reads copies")
    }

    fn receive_copied(&self, channel: i32, max: usize) -> Vec<(SteamId, Vec<u8>)> {
        let mut inboxes = self.net.inboxes.lock().unwrap();
        let inbox = inboxes.get_mut(&self.me).unwrap();
        let mut out = vec![];
        let mut rest = VecDeque::new();
        while let Some((c, from, data)) = inbox.ready.pop_front() {
            if c == channel && out.len() < max { out.push((from, data)) } else { rest.push_back((c, from, data)) }
        }
        inbox.ready = rest;
        out
    }

    fn session_requests(&self) -> Vec<SteamId> {
        std::mem::take(&mut self.net.inboxes.lock().unwrap().get_mut(&self.me).unwrap().requests)
    }

    fn accept(&self, user: SteamId) -> bool {
        let mut inboxes = self.net.inboxes.lock().unwrap();
        let inbox = inboxes.get_mut(&self.me).unwrap();
        inbox.known.insert(user);
        let (from_user, others): (Vec<_>, Vec<_>) = std::mem::take(&mut inbox.held).into_iter().partition(|(_, f, _)| *f == user);
        inbox.held = others;
        inbox.ready.extend(from_user);
        true
    }

    fn close(&self, _: SteamId, _: i32) -> bool {
        true
    }

    unsafe fn release(&self, _: *mut c_void) {}

    fn status(&self, _: SteamId) -> RealTimeStatus {
        RealTimeStatus::default()
    }
}
