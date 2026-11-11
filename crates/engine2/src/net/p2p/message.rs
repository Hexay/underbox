//! P2P messages as `SteamNetworkingMessage_t`s, the struct managed reads in place (its
//! `SteamNetworkMessage`). `m_pfnRelease` is ours, so GNS's own release path frees them too.

use std::ffi::c_void;

/// `SteamNetworkingMessage_t` (steamnetworkingtypes.h, 8-byte packing).
#[repr(C)]
pub struct Message {
    pub data: *mut c_void,
    pub size: i32,
    pub conn: u32,
    /// `SteamNetworkingIdentity`: type, size, then the Steam id in its union.
    pub identity_type: i32,
    pub identity_size: i32,
    pub identity_steam_id: u64,
    identity_rest: [u8; 120],
    pub conn_user_data: i64,
    pub time_received: i64,
    pub number: i64,
    free_data: Option<unsafe extern "C" fn(*mut Message)>,
    release: Option<unsafe extern "C" fn(*mut Message)>,
    pub channel: i32,
    pub flags: i32,
    pub user_data: i64,
    pub lane: u16,
    _pad: u16,
}

/// `k_ESteamNetworkingIdentityType_SteamID` and its payload size.
const IDENTITY_STEAM_ID: i32 = 16;

unsafe extern "C" fn release(m: *mut Message) {
    // SAFETY: `m` came from `Box::into_raw` in `allocate`, and Steam's contract releases each once.
    let m = unsafe { Box::from_raw(m) };
    // SAFETY: `data` and `size` are the boxed slice `allocate` leaked; nothing changes them.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(m.data.cast::<u8>(), m.size as usize)) });
}

/// A message with a `size`-byte buffer for `conn`, owned by whoever releases it.
pub fn allocate(conn: u32, size: usize, flags: i32) -> *mut Message {
    let data = Box::into_raw(vec![0u8; size].into_boxed_slice()).cast();
    Box::into_raw(Box::new(Message {
        data,
        size: size as i32,
        conn,
        identity_type: 0,
        identity_size: 0,
        identity_steam_id: 0,
        identity_rest: [0; 120],
        conn_user_data: 0,
        time_received: 0,
        number: 0,
        free_data: None,
        release: Some(release),
        channel: 0,
        flags,
        user_data: 0,
        lane: 0,
        _pad: 0,
    }))
}

/// A received message from `peer`, as managed reads it.
pub fn received(conn: u32, peer: u64, data: &[u8], flags: i32, number: i64) -> *mut Message {
    let m = allocate(conn, data.len(), flags);
    // SAFETY: just allocated with room for `data`.
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), (*m).data.cast(), data.len());
        (*m).identity_type = IDENTITY_STEAM_ID;
        (*m).identity_size = 8;
        (*m).identity_steam_id = peer;
        (*m).number = number;
        (*m).time_received = now_usec();
    }
    m
}

fn now_usec() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_micros() as i64
}

/// # Safety
/// `m` is a live `SteamNetworkingMessage_t`, ours or GNS's (same layout).
pub unsafe fn connection(m: *const c_void) -> u32 {
    unsafe { (*m.cast::<Message>()).conn }
}

/// # Safety
/// `m` is a live message, ours or GNS's.
pub unsafe fn data(m: *mut c_void) -> *mut c_void {
    unsafe { (*m.cast::<Message>()).data }
}

/// # Safety
/// `m` is a live message; the slice lives as long as it does.
pub unsafe fn bytes<'a>(m: *const c_void) -> &'a [u8] {
    let m = unsafe { &*m.cast::<Message>() };
    if m.data.is_null() || m.size <= 0 { &[] } else { unsafe { std::slice::from_raw_parts(m.data.cast(), m.size as usize) } }
}

/// # Safety
/// `m` is a live message; it is freed.
pub unsafe fn release_any(m: *mut c_void) {
    let m = m.cast::<Message>();
    if let Some(f) = unsafe { (*m).release } {
        unsafe { f(m) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn matches_steams_layout() {
        assert_eq!(size_of::<Message>(), gns::message_struct_size());
        // Managed's SteamNetworkMessage: Data, Size, Connection, IdentityType @16, IdentitySteamId @24, ...
        assert_eq!((offset_of!(Message, conn), offset_of!(Message, identity_steam_id)), (12, 24));
        assert_eq!((offset_of!(Message, number), offset_of!(Message, release), offset_of!(Message, flags)), (168, 184, 196));
    }

    #[test]
    fn received_messages_read_back_and_free_through_gns() {
        let m = received(7, 0x0110_0001_0000_0042, b"hello", 8, 3);
        // SAFETY: a live message, released once below by GNS's own release path.
        unsafe {
            assert_eq!(bytes(m.cast()), b"hello");
            assert_eq!(connection(m.cast()), 7);
            assert_eq!(((*m).identity_type, (*m).identity_steam_id, (*m).number, (*m).flags), (16, 0x0110_0001_0000_0042, 3, 8));
            gns::release(m.cast());
        }
    }

    #[test]
    fn allocated_buffers_are_writable() {
        let m = allocate(9, 4, 0);
        // SAFETY: a live 4-byte message, released once.
        unsafe {
            (*m).data.cast::<u8>().copy_from_nonoverlapping([1u8, 2, 3, 4].as_ptr(), 4);
            assert_eq!(bytes(m.cast()), [1, 2, 3, 4]);
            release_any(m.cast());
        }
    }
}
