//! The shim's C functions (src/shim.cpp). Handles are Steam's u32 ids; 0 is invalid.

use std::ffi::{c_char, c_int, c_void};

#[repr(C)]
pub struct RawStatus {
    pub conn: u32,
    pub listen_socket: u32,
    pub state: i32,
    pub old_state: i32,
    pub end_reason: i32,
    pub remote_steam_id: u64,
    pub end_debug: [c_char; 128],
    pub description: [c_char; 128],
}

pub type StatusFn = unsafe extern "C" fn(*const RawStatus);
pub type DebugFn = unsafe extern "C" fn(c_int, *const c_char);

unsafe extern "C" {
    pub fn gns_init(err: *mut c_char, len: usize) -> bool;
    pub fn gns_kill();
    pub fn gns_run_callbacks(sink: StatusFn);
    pub fn gns_set_debug_output(level: c_int, f: Option<DebugFn>);
    pub fn gns_set_config_int(key: c_int, value: i32) -> bool;
    pub fn gns_set_config_string(key: c_int, value: *const c_char) -> bool;
    pub fn gns_listen_ip(port: u16) -> u32;
    pub fn gns_listen_p2p(virtual_port: c_int) -> u32;
    pub fn gns_listen_address(sock: u32, buf: *mut c_char, len: usize) -> bool;
    pub fn gns_close_listen(sock: u32) -> bool;
    pub fn gns_identity(buf: *mut c_char, len: usize) -> bool;
    pub fn gns_connect_ip(address: *const c_char) -> u32;
    pub fn gns_accept(conn: u32) -> c_int;
    pub fn gns_close(conn: u32, reason: c_int, debug: *const c_char, linger: bool) -> bool;
    pub fn gns_create_poll_group() -> u32;
    pub fn gns_destroy_poll_group(pg: u32) -> bool;
    pub fn gns_set_poll_group(conn: u32, pg: u32) -> bool;
    pub fn gns_receive_poll_group(pg: u32, out: *mut *mut c_void, max: c_int) -> c_int;
    pub fn gns_receive_connection(conn: u32, out: *mut *mut c_void, max: c_int) -> c_int;
    pub fn gns_allocate_message(conn: u32, size: c_int, flags: c_int) -> *mut c_void;
    pub fn gns_message_data(msg: *mut c_void) -> *mut c_void;
    pub fn gns_message_size(msg: *mut c_void) -> c_int;
    pub fn gns_message_connection(msg: *mut c_void) -> u32;
    pub fn gns_release_message(msg: *mut c_void);
    pub fn gns_send_messages(msgs: *mut *mut c_void, n: c_int, out: *mut i64);
    pub fn gns_send(conn: u32, data: *const c_void, size: u32, flags: c_int) -> c_int;
    pub fn gns_flush(conn: u32) -> c_int;
    pub fn gns_connection_state(conn: u32) -> c_int;
    pub fn gns_connection_description(conn: u32, buf: *mut c_char, len: usize) -> bool;
    pub fn gns_connection_steam_id(conn: u32) -> u64;
    pub fn gns_realtime_status_size() -> usize;
    pub fn gns_message_struct_size() -> usize;
    pub fn gns_realtime_status(conn: u32, out: *mut c_void, size: usize) -> bool;
}
