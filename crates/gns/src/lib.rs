//! Steam Networking Sockets from Valve's GameNetworkingSockets: the same API and wire protocol as
//! Steam's own, without Steam. Messages stay native `SteamNetworkingMessage_t*`s (managed reads
//! them in place), so they cross this API as opaque pointers.

pub mod ffi;
#[cfg(test)]
mod tests;

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::{Mutex, OnceLock};

/// `ESteamNetworkingConnectionState`.
pub mod state {
    pub const NONE: i32 = 0;
    pub const CONNECTING: i32 = 1;
    pub const FINDING_ROUTE: i32 = 2;
    pub const CONNECTED: i32 = 3;
    pub const CLOSED_BY_PEER: i32 = 4;
    pub const PROBLEM_DETECTED_LOCALLY: i32 = 5;
}

/// `ESteamNetworkingSendType` bits.
pub mod send {
    pub const UNRELIABLE: i32 = 0;
    pub const RELIABLE: i32 = 8;
}

/// `ESteamNetworkingConfigValue` keys we set.
pub mod config {
    pub const SEND_BUFFER_SIZE: i32 = 9;
    pub const SEND_RATE_MIN: i32 = 10;
    pub const SEND_RATE_MAX: i32 = 11;
    pub const IP_ALLOW_WITHOUT_AUTH: i32 = 23;
    pub const TIMEOUT_CONNECTED: i32 = 25;
    pub const RECV_BUFFER_SIZE: i32 = 47;
    pub const RECV_BUFFER_MESSAGES: i32 = 48;
    pub const RECV_MAX_MESSAGE_SIZE: i32 = 49;
}

/// A connection's state change (`SteamNetConnectionStatusChangedCallback_t`, flattened).
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub conn: u32,
    /// The listen socket that accepted it; 0 for connections we initiated.
    pub listen_socket: u32,
    pub state: i32,
    pub old_state: i32,
    pub end_reason: i32,
    pub remote_steam_id: u64,
    pub end_debug: String,
    pub description: String,
}

fn text(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn read_string(f: impl FnOnce(*mut c_char, usize) -> bool) -> Option<String> {
    let mut buf = [0 as c_char; 256];
    f(buf.as_mut_ptr(), buf.len()).then(|| text(&buf))
}

static INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// Start the library once per process; later calls return the first result.
pub fn init() -> Result<(), String> {
    INIT.get_or_init(|| {
        let mut err = [0 as c_char; 1024];
        // SAFETY: err is writable for its length.
        if unsafe { ffi::gns_init(err.as_mut_ptr(), err.len()) } { Ok(()) } else { Err(text(&err)) }
    })
    .clone()
}

static PENDING: Mutex<Vec<Status>> = Mutex::new(Vec::new());

unsafe extern "C" fn collect(raw: *const ffi::RawStatus) {
    // SAFETY: the shim passes a valid status for the duration of the call.
    let r = unsafe { &*raw };
    PENDING.lock().unwrap().push(Status {
        conn: r.conn,
        listen_socket: r.listen_socket,
        state: r.state,
        old_state: r.old_state,
        end_reason: r.end_reason,
        remote_steam_id: r.remote_steam_id,
        end_debug: text(&r.end_debug),
        description: text(&r.description),
    });
}

/// Pump the library and return the connection state changes since the last call.
pub fn run_callbacks() -> Vec<Status> {
    // SAFETY: collect matches StatusFn.
    unsafe { ffi::gns_run_callbacks(collect) };
    std::mem::take(&mut *PENDING.lock().unwrap())
}

pub fn set_config_int(key: i32, value: i32) -> bool {
    unsafe { ffi::gns_set_config_int(key, value) }
}

pub fn set_config_string(key: i32, value: &str) -> bool {
    let value = CString::new(value).unwrap_or_default();
    unsafe { ffi::gns_set_config_string(key, value.as_ptr()) }
}

/// Route the library's log to `f` (`FSteamNetworkingSocketsDebugOutput`) at `level` and above.
pub fn set_debug_output(level: i32, f: Option<ffi::DebugFn>) {
    unsafe { ffi::gns_set_debug_output(level, f) }
}

pub fn listen_ip(port: u16) -> u32 {
    unsafe { ffi::gns_listen_ip(port) }
}

/// A SteamID + virtual-port listener; 0 without Steam's signalling (P2P is Steam-only here).
pub fn listen_p2p(virtual_port: i32) -> u32 {
    unsafe { ffi::gns_listen_p2p(virtual_port) }
}

pub fn listen_address(sock: u32) -> Option<String> {
    read_string(|b, n| unsafe { ffi::gns_listen_address(sock, b, n) })
}

pub fn close_listen(sock: u32) -> bool {
    unsafe { ffi::gns_close_listen(sock) }
}

pub fn identity() -> Option<String> {
    read_string(|b, n| unsafe { ffi::gns_identity(b, n) })
}

/// Connect to `host:port`; 0 if the address doesn't parse.
pub fn connect_ip(address: &str) -> u32 {
    let Ok(address) = CString::new(address) else { return 0 };
    unsafe { ffi::gns_connect_ip(address.as_ptr()) }
}

/// Steam's `EResult`; 1 is OK.
pub fn accept(conn: u32) -> i32 {
    unsafe { ffi::gns_accept(conn) }
}

pub fn close(conn: u32, reason: i32, debug: Option<&CStr>, linger: bool) -> bool {
    unsafe { ffi::gns_close(conn, reason, debug.map_or(std::ptr::null(), CStr::as_ptr), linger) }
}

pub fn create_poll_group() -> u32 {
    unsafe { ffi::gns_create_poll_group() }
}

pub fn destroy_poll_group(pg: u32) -> bool {
    unsafe { ffi::gns_destroy_poll_group(pg) }
}

pub fn set_poll_group(conn: u32, pg: u32) -> bool {
    unsafe { ffi::gns_set_poll_group(conn, pg) }
}

/// Fill `out` with received messages; each must be [`release`]d.
pub fn receive_poll_group(pg: u32, out: &mut [*mut c_void]) -> usize {
    unsafe { ffi::gns_receive_poll_group(pg, out.as_mut_ptr(), out.len() as i32).max(0) as usize }
}

pub fn receive_connection(conn: u32, out: &mut [*mut c_void]) -> usize {
    unsafe { ffi::gns_receive_connection(conn, out.as_mut_ptr(), out.len() as i32).max(0) as usize }
}

/// A message to `conn` with a `size`-byte buffer ([`message_data`]) to fill, then [`send_messages`].
pub fn allocate_message(conn: u32, size: i32, flags: i32) -> *mut c_void {
    unsafe { ffi::gns_allocate_message(conn, size.max(0), flags) }
}

/// # Safety
/// `msg` must be a live message from this library.
pub unsafe fn message_data(msg: *mut c_void) -> *mut c_void {
    unsafe { ffi::gns_message_data(msg) }
}

/// # Safety
/// `msg` must be a live message from this library.
pub unsafe fn message_bytes<'a>(msg: *mut c_void) -> &'a [u8] {
    unsafe {
        let (data, size) = (ffi::gns_message_data(msg), ffi::gns_message_size(msg));
        if data.is_null() || size <= 0 { &[] } else { std::slice::from_raw_parts(data.cast(), size as usize) }
    }
}

/// # Safety
/// `msg` must be a live message from this library; it is freed.
pub unsafe fn release(msg: *mut c_void) {
    unsafe { ffi::gns_release_message(msg) }
}

/// Each message's number, or the negated `EResult` it failed with (e.g. -25, `LimitExceeded`, when
/// reliable data outgrows the send buffer).
///
/// # Safety
/// Every pointer must be an allocated, unsent message; all are consumed, sent or not.
pub unsafe fn send_messages(msgs: &mut [*mut c_void]) -> Vec<i64> {
    let mut results = vec![0i64; msgs.len()];
    unsafe { ffi::gns_send_messages(msgs.as_mut_ptr(), msgs.len() as i32, results.as_mut_ptr()) };
    results
}

/// Send one copy of `data`; Steam's `EResult` (1 is OK).
pub fn send(conn: u32, data: &[u8], flags: i32) -> i32 {
    unsafe { ffi::gns_send(conn, data.as_ptr().cast(), data.len() as u32, flags) }
}

pub fn flush(conn: u32) -> i32 {
    unsafe { ffi::gns_flush(conn) }
}

pub fn connection_state(conn: u32) -> i32 {
    unsafe { ffi::gns_connection_state(conn) }
}

pub fn connection_description(conn: u32) -> Option<String> {
    read_string(|b, n| unsafe { ffi::gns_connection_description(conn, b, n) })
}

pub fn connection_steam_id(conn: u32) -> u64 {
    unsafe { ffi::gns_connection_steam_id(conn) }
}

/// `sizeof(SteamNetConnectionRealTimeStatus_t)`, to check a mirror of it against.
pub fn realtime_status_size() -> usize {
    unsafe { ffi::gns_realtime_status_size() }
}

/// `sizeof(SteamNetworkingMessage_t)`.
pub fn message_struct_size() -> usize {
    unsafe { ffi::gns_message_struct_size() }
}

/// Fill a `SteamNetConnectionRealTimeStatus_t` mirror `T` (its common prefix); false (zeroed) if
/// the connection is gone.
pub fn realtime_status<T>(conn: u32, out: &mut T) -> bool {
    unsafe { ffi::gns_realtime_status(conn, (out as *mut T).cast(), size_of::<T>()) }
}
