//! One P2P connection's state machine, as Steam's sockets show one to the application:
//! Connecting → Connected → ClosedByPeer / ProblemDetectedLocally. Frames it wants sent go to an
//! outbox; the transport puts them on the wire.

use super::frame::Frame;
use super::reliable::{Receiver, Segment, Sender};
use gns::state;
use platform::SteamId;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// `ESteamNetConnectionEnd` values we report.
pub mod end {
    pub const APP_GENERIC: i32 = 1000;
    pub const MISC_GENERIC: i32 = 5001;
    pub const MISC_TIMEOUT: i32 = 5003;
    pub const PEER_SENT_NO_CONNECTION: i32 = 5010;
}

/// `EResult`s the send path returns.
pub mod result {
    pub const OK: i32 = 1;
    pub const NO_CONNECTION: i32 = 3;
    pub const INVALID_PARAM: i32 = 8;
    pub const INVALID_STATE: i32 = 11;
    pub const IGNORED: i32 = 41;
}

/// `k_nSteamNetworkingSend_Reliable`.
pub const SEND_RELIABLE: i32 = 8;
/// `k_nSteamNetworkingSend_NoDelay`.
pub const SEND_NO_DELAY: i32 = 4;
/// `k_cbMaxSteamNetworkingSocketsMessageSizeSend`.
pub const MAX_MESSAGE: usize = 512 * 1024;
pub const CONNECT_RETRY: Duration = Duration::from_millis(250);
/// Steam's default `TimeoutInitial` and `TimeoutConnected`.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
pub const KEEPALIVE: Duration = Duration::from_secs(1);
/// How long a closed connection keeps sending its unacked reliable data (`linger`).
pub const LINGER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq)]
pub struct Received {
    pub data: Vec<u8>,
    pub flags: i32,
    pub number: i64,
}

pub struct Conn {
    pub handle: u32,
    pub peer: SteamId,
    /// The peer's handle for this connection; 0 until an outgoing connection is accepted.
    pub remote: u32,
    pub vport: i32,
    /// The listen socket that took it; 0 for connections we opened.
    pub listen_socket: u32,
    pub state: i32,
    pub end_reason: i32,
    pub end_debug: String,
    pub poll_group: u32,
    pub inbox: VecDeque<Received>,
    /// Closed locally but still flushing reliable data, until this instant.
    pub linger_until: Option<Instant>,
    tx: Sender,
    rx: Receiver,
    /// Unreliable messages sent before the connection came up; reliable ones wait in `tx`.
    early: Vec<Vec<u8>>,
    received: i64,
    opened: Instant,
    heard: Instant,
    sent: Instant,
    connect_sent: Option<Instant>,
}

impl Conn {
    fn new(handle: u32, peer: SteamId, vport: i32, now: Instant) -> Self {
        Conn {
            handle,
            peer,
            remote: 0,
            vport,
            listen_socket: 0,
            state: state::CONNECTING,
            end_reason: 0,
            end_debug: String::new(),
            poll_group: 0,
            inbox: VecDeque::new(),
            linger_until: None,
            tx: Sender::default(),
            rx: Receiver::default(),
            early: Vec::new(),
            received: 0,
            opened: now,
            heard: now,
            sent: now,
            connect_sent: None,
        }
    }

    pub fn outgoing(handle: u32, peer: SteamId, vport: i32, now: Instant) -> Self {
        Self::new(handle, peer, vport, now)
    }

    pub fn incoming(handle: u32, peer: SteamId, remote: u32, vport: i32, listen_socket: u32, now: Instant) -> Self {
        Conn { remote, listen_socket, ..Self::new(handle, peer, vport, now) }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.state, state::CONNECTING | state::CONNECTED)
    }

    /// Move to `new`, returning the old state for the status change.
    fn enter(&mut self, new: i32, reason: i32, debug: &str) -> Option<i32> {
        let old = std::mem::replace(&mut self.state, new);
        if new != state::CONNECTED {
            (self.end_reason, self.end_debug) = (reason, debug.to_string());
        }
        (old != new).then_some(old)
    }

    /// The application took an incoming connection; `None` unless it was waiting to be.
    pub fn accept(&mut self, out: &mut Vec<Frame>) -> Option<i32> {
        if self.listen_socket == 0 || self.state != state::CONNECTING {
            return None;
        }
        out.push(Frame::Accept { dst: self.remote, src: self.handle });
        self.enter(state::CONNECTED, 0, "")
    }

    /// A frame for this connection from its peer; returns the old state if it changed.
    pub fn on_frame(&mut self, f: Frame, now: Instant, out: &mut Vec<Frame>) -> Option<i32> {
        self.heard = now;
        match f {
            Frame::Connect { .. } if self.listen_socket != 0 && self.state == state::CONNECTED => {
                // Our accept was lost: say it again.
                out.push(Frame::Accept { dst: self.remote, src: self.handle });
            }
            Frame::Accept { src, .. } if self.listen_socket == 0 && self.state == state::CONNECTING => {
                self.remote = src;
                return self.enter(state::CONNECTED, 0, "");
            }
            Frame::Close { reason, debug, .. } if self.is_open() => return self.enter(state::CLOSED_BY_PEER, reason, &debug),
            Frame::Unreliable { payload, .. } if self.state == state::CONNECTED => self.deliver(payload, 0),
            Frame::Reliable { seq, last, payload, .. } if self.state == state::CONNECTED => {
                for m in self.rx.on_segment(Segment { seq, last, payload }) {
                    self.deliver(m, SEND_RELIABLE);
                }
                // A hostile peer could otherwise grow one unterminated message without bound.
                if self.rx.partial_len() > MAX_MESSAGE {
                    let debug = "Reliable message over the size limit";
                    out.push(Frame::Close { dst: self.remote, reason: end::MISC_GENERIC, debug: debug.into() });
                    return self.enter(state::PROBLEM_DETECTED_LOCALLY, end::MISC_GENERIC, debug);
                }
            }
            Frame::Ack { next, mask, .. } => self.tx.on_ack(next, mask, now),
            _ => {}
        }
        None
    }

    fn deliver(&mut self, data: Vec<u8>, flags: i32) {
        if self.linger_until.is_none() {
            self.received += 1;
            self.inbox.push_back(Received { data, flags, number: self.received });
        }
    }

    /// Queue one message; Steam's `EResult`. Unreliable ones go straight to `out`.
    pub fn send(&mut self, data: &[u8], flags: i32, out: &mut Vec<Frame>) -> i32 {
        // Same answers, in the same order, as GNS's APISendMessageToConnection.
        match self.state {
            state::CONNECTED => {}
            state::CONNECTING if flags & SEND_NO_DELAY != 0 => return result::IGNORED,
            state::CONNECTING => {}
            _ => return result::NO_CONNECTION,
        }
        if data.len() > MAX_MESSAGE {
            return result::INVALID_PARAM;
        }
        if flags & SEND_RELIABLE != 0 {
            self.tx.push(data);
        } else if self.state == state::CONNECTED {
            out.push(Frame::Unreliable { dst: self.remote, payload: data.to_vec() });
        } else {
            self.early.push(data.to_vec());
        }
        result::OK
    }

    /// Close from our side: tell the peer now, or once reliable data is flushed when lingering.
    pub fn close(&mut self, reason: i32, debug: &str, linger: bool, now: Instant, out: &mut Vec<Frame>) {
        if self.state == state::CONNECTED && linger && !self.tx.is_idle() {
            self.linger_until = Some(now + LINGER);
            (self.end_reason, self.end_debug) = (reason, debug.to_string());
            return;
        }
        if self.is_open() && (self.remote != 0 || self.listen_socket != 0) {
            out.push(Frame::Close { dst: self.remote, reason, debug: debug.to_string() });
        }
        self.state = state::NONE;
    }

    /// Timers: connect retries, retransmits, acks, keepalives, timeouts. Returns the old state if it changed.
    pub fn poll(&mut self, now: Instant, out: &mut Vec<Frame>) -> Option<i32> {
        let start = out.len();
        let changed = self.timers(now, out);
        if out.len() > start {
            self.sent = now;
        }
        changed
    }

    fn timers(&mut self, now: Instant, out: &mut Vec<Frame>) -> Option<i32> {
        match self.state {
            state::CONNECTING if self.listen_socket == 0 => {
                if now - self.opened >= CONNECT_TIMEOUT {
                    return self.enter(state::PROBLEM_DETECTED_LOCALLY, end::MISC_TIMEOUT, "Timed out attempting to connect");
                }
                if self.connect_sent.is_none_or(|t| now - t >= CONNECT_RETRY) {
                    self.connect_sent = Some(now);
                    out.push(Frame::Connect { src: self.handle, vport: self.vport });
                }
            }
            state::CONNECTED => {
                if now - self.heard >= IDLE_TIMEOUT {
                    return self.enter(state::PROBLEM_DETECTED_LOCALLY, end::MISC_TIMEOUT, "Connection dropped");
                }
                let dst = self.remote;
                out.extend(self.early.drain(..).map(|payload| Frame::Unreliable { dst, payload }));
                out.extend(self.tx.poll(now).into_iter().map(|s| Frame::Reliable { dst, seq: s.seq, last: s.last, payload: s.payload }));
                if let Some((next, mask)) = self.rx.take_ack() {
                    out.push(Frame::Ack { dst, next, mask });
                }
                if self.linger_until.is_some_and(|t| self.tx.is_idle() || now >= t) {
                    out.push(Frame::Close { dst, reason: self.end_reason, debug: self.end_debug.clone() });
                    self.state = state::NONE;
                } else if now - self.sent >= KEEPALIVE {
                    out.push(Frame::Ping { dst });
                }
            }
            _ => {}
        }
        None
    }

    /// Done with: closed locally and flushed. The transport frees the handle.
    pub fn is_finished(&self) -> bool {
        self.state == state::NONE
    }

    pub fn rtt(&self) -> Option<Duration> {
        self.tx.rtt()
    }

    pub fn pending_reliable(&self) -> usize {
        self.tx.queued_bytes()
    }

    pub fn unacked_reliable(&self) -> usize {
        self.tx.unacked_bytes()
    }
}

#[cfg(test)]
#[path = "conn_tests.rs"]
mod tests;
