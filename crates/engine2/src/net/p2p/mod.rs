//! Steam-id connections and listen sockets over `ISteamNetworkingMessages`: our own framing
//! (`frame`), connection state machine (`conn`) and reliability (`reliable`) on one messages
//! channel, since open-source GNS has no Steam relay. Only underbox peers speak it; a stock Steam
//! client's `ConnectP2P` uses Steam's own SDR protocol instead (docs/steam.md).

pub mod conn;
pub mod frame;
#[cfg(test)]
pub mod loopback;
pub mod message;
mod pump;
mod reliable;
#[cfg(test)]
mod tests;

use conn::{Conn, Received, end, result};
use frame::Frame;
use gns::{Status, state};
use platform::{PeerMessages, SteamId};
use pump::{describe, status};
use std::collections::BTreeMap;
use std::time::Instant;

/// The messages channel all P2P frames use ("UBXP"); lobby sockets use their lobby id's low bits.
pub const CHANNEL: i32 = 0x5542_5850;
/// `k_nSteamNetworkingSend_AutoRestartBrokenSession`: a dropped Steam session reopens, and our
/// reliability layer resends what it lost.
const WIRE_FLAGS: i32 = 32;
/// GNS listen sockets never set the top bit; connection handles always have a nonzero high half,
/// so ours (1..=0xFFFF) can't collide with a GNS one either.
const LISTEN_BIT: u32 = 0x8000_0000;
const MAX_HANDLE: u32 = 0xFFFF;
/// Wire messages read per pump.
const RECEIVE_BATCH: usize = 256;

pub struct P2p {
    next: u32,
    next_listen: u32,
    /// Listen socket → virtual port.
    listeners: BTreeMap<u32, i32>,
    conns: BTreeMap<u32, Conn>,
    changes: Vec<Status>,
}

/// Steam's `SteamNetConnectionRealTimeStatus_t`, the part we can fill.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub state: i32,
    pub ping_ms: i32,
    pub pending_reliable: i32,
    pub unacked_reliable: i32,
}

type Outbox = Vec<(SteamId, Frame)>;

impl Default for P2p {
    fn default() -> Self {
        Self::new()
    }
}

impl P2p {
    pub const fn new() -> Self {
        P2p { next: 0, next_listen: 0, listeners: BTreeMap::new(), conns: BTreeMap::new(), changes: Vec::new() }
    }

    fn handle(&mut self) -> u32 {
        loop {
            self.next = self.next % MAX_HANDLE + 1;
            if !self.conns.contains_key(&self.next) {
                return self.next;
            }
        }
    }

    pub fn is_listen_socket(sock: u32) -> bool {
        sock & LISTEN_BIT != 0
    }

    /// Ours and still visible to the application (not closed, not lingering).
    pub fn owns(&self, conn: u32) -> bool {
        self.live(conn).is_some()
    }

    fn live(&self, conn: u32) -> Option<&Conn> {
        self.conns.get(&conn).filter(|c| c.linger_until.is_none() && !c.is_finished())
    }

    fn live_mut(&mut self, conn: u32) -> Option<&mut Conn> {
        self.conns.get_mut(&conn).filter(|c| c.linger_until.is_none() && !c.is_finished())
    }

    pub fn listen(&mut self, vport: i32) -> u32 {
        self.next_listen += 1;
        let sock = LISTEN_BIT | self.next_listen;
        self.listeners.insert(sock, vport);
        sock
    }

    /// Close a listen socket and every connection it accepted, as Steam does.
    pub fn close_listen(&mut self, wire: &dyn PeerMessages, sock: u32, now: Instant) -> bool {
        if self.listeners.remove(&sock).is_none() {
            return false;
        }
        let mut out = vec![];
        for c in self.conns.values_mut().filter(|c| c.listen_socket == sock) {
            let mut frames = vec![];
            c.close(end::APP_GENERIC, "Listen socket closed", false, now, &mut frames);
            out.extend(frames.into_iter().map(|f| (c.peer, f)));
        }
        self.conns.retain(|_, c| !c.is_finished());
        self.flush(wire, out);
        true
    }

    pub fn connect(&mut self, wire: &dyn PeerMessages, peer: SteamId, vport: i32, now: Instant) -> u32 {
        let h = self.handle();
        self.conns.insert(h, Conn::outgoing(h, peer, vport, now));
        self.pump_timers(wire, now);
        h
    }

    /// The application took an incoming connection; Steam's `EResult`.
    pub fn accept(&mut self, wire: &dyn PeerMessages, conn: u32) -> i32 {
        let Some(c) = self.live_mut(conn) else { return result::INVALID_PARAM };
        let mut frames = vec![];
        let Some(old) = c.accept(&mut frames) else { return result::INVALID_STATE };
        let (peer, s) = (c.peer, status(c, old));
        self.changes.push(s);
        self.flush(wire, frames.into_iter().map(|f| (peer, f)).collect());
        result::OK
    }

    pub fn close(&mut self, wire: &dyn PeerMessages, conn: u32, reason: i32, debug: &str, linger: bool, now: Instant) -> bool {
        let Some(c) = self.live_mut(conn) else { return false };
        let mut frames = vec![];
        c.close(reason, debug, linger, now, &mut frames);
        let peer = c.peer;
        self.conns.retain(|_, c| !c.is_finished());
        self.flush(wire, frames.into_iter().map(|f| (peer, f)).collect());
        true
    }

    pub fn state(&self, conn: u32) -> i32 {
        self.live(conn).map_or(state::NONE, |c| c.state)
    }

    pub fn peer(&self, conn: u32) -> u64 {
        self.live(conn).map_or(0, |c| c.peer.0)
    }

    /// Like Steam's `m_szConnectionDescription`.
    pub fn description(&self, conn: u32) -> Option<String> {
        self.live(conn).map(describe)
    }

    pub fn set_poll_group(&mut self, conn: u32, pg: u32) -> bool {
        self.live_mut(conn).map(|c| c.poll_group = pg).is_some()
    }

    pub fn stats(&self, conn: u32) -> Option<Stats> {
        self.live(conn).map(|c| Stats {
            state: c.state,
            ping_ms: c.rtt().map_or(-1, |r| r.as_millis() as i32),
            pending_reliable: c.pending_reliable() as i32,
            unacked_reliable: c.unacked_reliable() as i32,
        })
    }

    pub fn send(&mut self, wire: &dyn PeerMessages, conn: u32, data: &[u8], flags: i32, now: Instant) -> i32 {
        let Some(c) = self.live_mut(conn) else { return result::INVALID_PARAM };
        let mut frames = vec![];
        let r = c.send(data, flags, &mut frames);
        let peer = c.peer;
        self.flush(wire, frames.into_iter().map(|f| (peer, f)).collect());
        if r == result::OK && flags & conn::SEND_RELIABLE != 0 {
            self.pump_timers(wire, now);
        }
        r
    }

    pub fn receive_connection(&mut self, conn: u32, max: usize) -> Vec<Received> {
        self.live_mut(conn).map_or(vec![], |c| c.inbox.drain(..max.min(c.inbox.len())).collect())
    }

    /// Up to `max` messages from the connections in poll group `pg`, oldest connection first.
    /// Each with its connection and that connection's peer.
    pub fn receive_poll_group(&mut self, pg: u32, max: usize) -> Vec<(u32, u64, Received)> {
        let mut out = vec![];
        for c in self.conns.values_mut().filter(|c| pg != 0 && c.poll_group == pg && c.linger_until.is_none()) {
            let n = (max - out.len()).min(c.inbox.len());
            out.extend(c.inbox.drain(..n).map(|m| (c.handle, c.peer.0, m)));
        }
        out
    }

    /// Status changes since the last call, for managed's callbacks.
    pub fn take_changes(&mut self) -> Vec<Status> {
        std::mem::take(&mut self.changes)
    }
}
