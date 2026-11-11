//! net's pluggable transports: GameNetworkingSockets for IP, and the P2P transport for Steam ids
//! (`p2p`). Each connection and listen socket handle belongs to one, told apart by its id
//! (`p2p::P2p::owns`, `is_listen_socket`); poll groups are GNS's and may hold both kinds.

use super::p2p::{P2p, Stats, conn::Received, message};
use abi::structs::SteamNetConnectionRealTimeStatus_t as RealTime;
use gns::Status;
use platform::{PeerMessages, SteamId};
use std::ffi::{CStr, c_void};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

pub trait Transport: Sync {
    fn accept(&self, conn: u32) -> i32;
    fn close(&self, conn: u32, reason: i32, debug: Option<&CStr>, linger: bool) -> bool;
    fn state(&self, conn: u32) -> i32;
    fn description(&self, conn: u32) -> Option<String>;
    fn steam_id(&self, conn: u32) -> u64;
    fn set_poll_group(&self, conn: u32, pg: u32) -> bool;
    /// Fill `out` with messages, each released through `ReleaseMessage`; how many.
    fn receive(&self, conn: u32, out: &mut [*mut c_void]) -> usize;
    fn allocate_message(&self, conn: u32, size: i32, flags: i32) -> *mut c_void;
    fn send(&self, conn: u32, data: &[u8], flags: i32) -> i32;
    fn realtime_status(&self, conn: u32, out: &mut RealTime) -> bool;
}

pub struct Gns;

impl Transport for Gns {
    fn accept(&self, conn: u32) -> i32 {
        gns::accept(conn)
    }
    fn close(&self, conn: u32, reason: i32, debug: Option<&CStr>, linger: bool) -> bool {
        gns::close(conn, reason, debug, linger)
    }
    fn state(&self, conn: u32) -> i32 {
        gns::connection_state(conn)
    }
    fn description(&self, conn: u32) -> Option<String> {
        gns::connection_description(conn)
    }
    fn steam_id(&self, conn: u32) -> u64 {
        gns::connection_steam_id(conn)
    }
    fn set_poll_group(&self, conn: u32, pg: u32) -> bool {
        gns::set_poll_group(conn, pg)
    }
    fn receive(&self, conn: u32, out: &mut [*mut c_void]) -> usize {
        gns::receive_connection(conn, out)
    }
    fn allocate_message(&self, conn: u32, size: i32, flags: i32) -> *mut c_void {
        gns::allocate_message(conn, size, flags)
    }
    fn send(&self, conn: u32, data: &[u8], flags: i32) -> i32 {
        gns::send(conn, data, flags)
    }
    fn realtime_status(&self, conn: u32, out: &mut RealTime) -> bool {
        gns::realtime_status(conn, out)
    }
}

static STATE: Mutex<P2p> = Mutex::new(P2p::new());

/// The messages interface Steam-id traffic rides on: the game server's on a dedicated server,
/// else the local user's. `None` offline.
pub fn wire() -> Option<&'static dyn PeerMessages> {
    let p = platform::get();
    if crate::app::dict::current_config().dedicated_server { p.server_peers() } else { p.client().peers() }
}

pub struct SteamP2p;

impl SteamP2p {
    /// The state and the wire, when there is one.
    fn with<R>(&self, none: R, f: impl FnOnce(&mut P2p, &'static dyn PeerMessages) -> R) -> R {
        match wire() {
            Some(w) => f(&mut lock(), w),
            None => none,
        }
    }

    pub fn owns(conn: u32) -> bool {
        lock().owns(conn)
    }

    pub fn listen(&self, vport: i32) -> u32 {
        lock().listen(vport)
    }

    pub fn close_listen(&self, sock: u32) -> bool {
        self.with(false, |p, w| p.close_listen(w, sock, Instant::now()))
    }

    pub fn connect(&self, peer: u64, vport: i32) -> u32 {
        self.with(0, |p, w| p.connect(w, SteamId(peer), vport, Instant::now()))
    }

    /// Read the wire and run timers; the status changes since the last call.
    pub fn pump(&self) -> Vec<Status> {
        self.with(vec![], |p, w| {
            p.pump(w, Instant::now());
            p.take_changes()
        })
    }

    /// Fill `out` from poll group `pg`; how many.
    pub fn receive_poll_group(&self, pg: u32, out: &mut [*mut c_void]) -> usize {
        let got = self.with(vec![], |p, w| {
            p.pump(w, Instant::now());
            p.receive_poll_group(pg, out.len())
        });
        fill(out, got.into_iter())
    }
}

fn lock() -> MutexGuard<'static, P2p> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Write received messages into managed's array as `SteamNetworkingMessage_t`s.
fn fill(out: &mut [*mut c_void], got: impl Iterator<Item = (u32, u64, Received)>) -> usize {
    let mut n = 0;
    for ((conn, peer, m), slot) in got.zip(out.iter_mut()) {
        *slot = message::received(conn, peer, &m.data, m.flags, m.number).cast();
        n += 1;
    }
    n
}

impl Transport for SteamP2p {
    fn accept(&self, conn: u32) -> i32 {
        self.with(super::p2p::conn::result::NO_CONNECTION, |p, w| p.accept(w, conn))
    }
    fn close(&self, conn: u32, reason: i32, debug: Option<&CStr>, linger: bool) -> bool {
        let debug = debug.map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        self.with(false, |p, w| p.close(w, conn, reason, &debug, linger, Instant::now()))
    }
    fn state(&self, conn: u32) -> i32 {
        lock().state(conn)
    }
    fn description(&self, conn: u32) -> Option<String> {
        lock().description(conn)
    }
    fn steam_id(&self, conn: u32) -> u64 {
        lock().peer(conn)
    }
    fn set_poll_group(&self, conn: u32, pg: u32) -> bool {
        lock().set_poll_group(conn, pg)
    }
    fn receive(&self, conn: u32, out: &mut [*mut c_void]) -> usize {
        let (peer, got) = self.with((0, vec![]), |p, w| {
            p.pump(w, Instant::now());
            (p.peer(conn), p.receive_connection(conn, out.len()))
        });
        fill(out, got.into_iter().map(|m| (conn, peer, m)))
    }
    fn allocate_message(&self, conn: u32, size: i32, flags: i32) -> *mut c_void {
        message::allocate(conn, size.max(0) as usize, flags).cast()
    }
    fn send(&self, conn: u32, data: &[u8], flags: i32) -> i32 {
        self.with(super::p2p::conn::result::NO_CONNECTION, |p, w| p.send(w, conn, data, flags, Instant::now()))
    }
    fn realtime_status(&self, conn: u32, out: &mut RealTime) -> bool {
        let Some(Stats { state, ping_ms, pending_reliable, unacked_reliable }) = lock().stats(conn) else { return false };
        (out.m_e_state, out.m_n_ping, out.m_cb_pending_reliable, out.m_cb_sent_unacked_reliable) = (state, ping_ms, pending_reliable, unacked_reliable);
        (out.m_fl_connection_quality_local, out.m_fl_connection_quality_remote) = (1.0, 1.0);
        true
    }
}

/// The transport owning connection `conn`.
pub fn of(conn: u32) -> &'static dyn Transport {
    if SteamP2p::owns(conn) { &SteamP2p } else { &Gns }
}

/// Send a batch of allocated messages, each through its connection's transport; all are consumed.
///
/// # Safety
/// Every pointer is an allocated, unsent message (ours or GNS's).
pub unsafe fn send_messages(msgs: &mut [*mut c_void]) {
    let mut native = Vec::with_capacity(msgs.len());
    for &m in msgs.iter() {
        // SAFETY: the caller's contract.
        let conn = unsafe { message::connection(m) };
        if SteamP2p::owns(conn) {
            // SAFETY: as above; the message is ours to free once sent.
            unsafe {
                SteamP2p.send(conn, message::bytes(m), (*m.cast::<message::Message>()).flags);
                message::release_any(m);
            }
        } else {
            native.push(m);
        }
    }
    // SAFETY: the rest are GNS's own allocated messages.
    let results = unsafe { gns::send_messages(&mut native) };
    let failed: Vec<_> = results.iter().filter(|&&r| r < 0).collect();
    static LOGGED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if !failed.is_empty() && LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 20 {
        super::log_error(&format!("networking: {} of {} sends refused (EResult {:?})", failed.len(), results.len(), failed));
    }
}
