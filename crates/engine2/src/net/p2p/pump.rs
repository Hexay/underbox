//! The transport's receive and timer side: frames off the wire to their connections, connection
//! timers, and the status changes they cause.

use super::conn::{Conn, end};
use super::frame::Frame;
use super::{CHANNEL, Outbox, P2p, RECEIVE_BATCH, WIRE_FLAGS};
use gns::{Status, state};
use platform::{PeerMessages, SteamId};
use std::time::Instant;

impl P2p {
    /// Read the wire, accept sessions while listening, and run every connection's timers.
    pub fn pump(&mut self, wire: &dyn PeerMessages, now: Instant) {
        for user in wire.session_requests() {
            if !self.listeners.is_empty() {
                wire.accept(user);
            }
        }
        let mut out = vec![];
        loop {
            let batch = wire.receive_copied(CHANNEL, RECEIVE_BATCH);
            let n = batch.len();
            for (from, bytes) in batch {
                if let Some(f) = Frame::decode(&bytes) {
                    self.on_frame(from, f, now, &mut out);
                }
            }
            if n < RECEIVE_BATCH {
                break;
            }
        }
        self.flush(wire, out);
        self.pump_timers(wire, now);
    }

    fn on_frame(&mut self, from: SteamId, f: Frame, now: Instant, out: &mut Outbox) {
        let dst = match f {
            Frame::Connect { src, vport } => match self.find_incoming(from, src) {
                Some(h) => h,
                None => return self.on_connect(from, src, vport, now, out),
            },
            Frame::Accept { dst, .. }
            | Frame::Close { dst, .. }
            | Frame::Unreliable { dst, .. }
            | Frame::Reliable { dst, .. }
            | Frame::Ack { dst, .. }
            | Frame::Ping { dst } => dst,
        };
        let Some(c) = self.conns.get_mut(&dst).filter(|c| c.peer == from) else {
            if let Frame::Accept { src, .. } = f {
                // We gave up on it; let the listener free its side.
                out.push((from, Frame::Close { dst: src, reason: end::PEER_SENT_NO_CONNECTION, debug: "No such connection".into() }));
            }
            return;
        };
        let mut frames = vec![];
        let changed = c.on_frame(f, now, &mut frames);
        out.extend(frames.into_iter().map(|fr| (from, fr)));
        if let Some(old) = changed {
            report(&mut self.changes, c, old);
        }
        self.conns.retain(|_, c| !c.is_finished());
    }

    fn find_incoming(&self, peer: SteamId, remote: u32) -> Option<u32> {
        self.conns.values().find(|c| c.listen_socket != 0 && c.peer == peer && c.remote == remote).map(|c| c.handle)
    }

    fn on_connect(&mut self, from: SteamId, src: u32, vport: i32, now: Instant, out: &mut Outbox) {
        let Some((&sock, _)) = self.listeners.iter().find(|(_, v)| **v == vport) else {
            let debug = format!("Nothing listening on virtual port {vport}");
            out.push((from, Frame::Close { dst: src, reason: end::PEER_SENT_NO_CONNECTION, debug }));
            return;
        };
        let h = self.handle();
        let c = Conn::incoming(h, from, src, vport, sock, now);
        self.changes.push(status(&c, state::NONE));
        self.conns.insert(h, c);
    }

    pub(super) fn pump_timers(&mut self, wire: &dyn PeerMessages, now: Instant) {
        let mut out = vec![];
        for c in self.conns.values_mut() {
            let mut frames = vec![];
            if let Some(old) = c.poll(now, &mut frames) {
                report(&mut self.changes, c, old);
            }
            out.extend(frames.into_iter().map(|f| (c.peer, f)));
        }
        self.conns.retain(|_, c| !c.is_finished());
        self.flush(wire, out);
    }

    pub(super) fn flush(&self, wire: &dyn PeerMessages, out: Outbox) {
        for (peer, f) in out {
            // A failed send is a lost datagram: retransmits and timeouts cover it.
            wire.send(peer, &f.encode(), WIRE_FLAGS, CHANNEL);
        }
    }
}

pub(super) fn describe(c: &Conn) -> String {
    format!("#{} P2P steamid:{} vport {}", c.handle, c.peer.0, c.vport)
}

pub(super) fn status(c: &Conn, old: i32) -> Status {
    Status {
        conn: c.handle,
        listen_socket: c.listen_socket,
        state: c.state,
        old_state: old,
        end_reason: c.end_reason,
        remote_steam_id: c.peer.0,
        end_debug: c.end_debug.clone(),
        description: describe(c),
    }
}

/// Queue `c`'s state change for managed, unless the application already closed it (lingering):
/// then it is simply done.
fn report(changes: &mut Vec<Status>, c: &mut Conn, old: i32) {
    if c.linger_until.is_some() {
        c.state = state::NONE;
    } else {
        changes.push(status(c, old));
    }
}
