//! Server lists over `ISteamMatchmakingServers`. Steam reports through a C++ response object it
//! calls into while callbacks are pumped; ours queues what it hears for [`ServerBrowser::take`].

use super::pump::read_at;
use super::strings::c;
use crate::{ServerBrowser, ServerListEvent, ServerQuery, SteamId};
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::offset_of;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use steamworks::sys;

type Heard = Arc<Mutex<HashMap<u64, Vec<ServerListEvent>>>>;

/// `ISteamMatchmakingServerListResponse`: a vtable pointer first, then our state.
#[repr(C)]
struct Response {
    vtable: *const VTable,
    id: u64,
    heard: Heard,
}

/// The interface's virtuals in declaration order. Gotcha: `this` comes first, which is what the
/// Itanium ABI and MSVC x64 both do; a 32-bit MSVC build would need thiscall.
#[repr(C)]
struct VTable {
    responded: extern "C" fn(*mut Response, *mut c_void, i32),
    failed: extern "C" fn(*mut Response, *mut c_void, i32),
    complete: extern "C" fn(*mut Response, *mut c_void, i32),
}

// SAFETY: the vtable pointer is to a static; the rest is Send + Sync.
unsafe impl Send for Response {}
// SAFETY: as above.
unsafe impl Sync for Response {}

static VTABLE: VTable = VTable { responded, failed, complete };

fn servers() -> *mut sys::ISteamMatchmakingServers {
    // SAFETY: only reached while the client that offers this service is up.
    unsafe { sys::SteamAPI_SteamMatchmakingServers_v002() }
}

/// A server's `gameserveritem_t` bytes and the id in them.
fn item(details: &[u8]) -> ServerListEvent {
    let server = SteamId(read_at::<u64>(details, offset_of!(sys::gameserveritem_t, m_steamID)));
    ServerListEvent::Responded { item: details.to_vec(), server }
}

extern "C" fn responded(this: *mut Response, request: *mut c_void, index: i32) {
    // SAFETY: Steam passes back the response we gave it, alive until its request is released.
    let this = unsafe { &*this };
    // SAFETY: a live request and one of its servers; Steam owns the struct until the request is released.
    let details = unsafe { sys::SteamAPI_ISteamMatchmakingServers_GetServerDetails(servers(), request.cast(), index) };
    if details.is_null() {
        return;
    }
    // SAFETY: as above; the struct is plain bytes.
    let bytes = unsafe { std::slice::from_raw_parts(details.cast::<u8>(), size_of::<sys::gameserveritem_t>()) };
    this.heard.lock().unwrap().entry(this.id).or_default().push(item(bytes));
}

extern "C" fn failed(_: *mut Response, _: *mut c_void, _: i32) {}

extern "C" fn complete(this: *mut Response, _: *mut c_void, _: i32) {
    // SAFETY: as in `responded`.
    let this = unsafe { &*this };
    this.heard.lock().unwrap().entry(this.id).or_default().push(ServerListEvent::Finished);
}

struct Query {
    request: usize,
    _response: Box<Response>,
}

#[derive(Default)]
pub(crate) struct SteamServers {
    next: AtomicU64,
    queries: Mutex<HashMap<u64, Query>>,
    heard: Heard,
}

impl ServerBrowser for SteamServers {
    fn request(&self, kind: ServerQuery, filters: &[(String, String)]) -> Option<u64> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let mut response = Box::new(Response { vtable: &VTABLE, id, heard: self.heard.clone() });
        let mut pairs: Vec<sys::MatchMakingKeyValuePair_t> = filters.iter().map(|(k, v)| pair(k, v)).collect();
        let mut list = pairs.as_mut_ptr();
        let r = (&mut *response as *mut Response).cast();
        // SAFETY: a live interface; Steam copies the filters during the call and calls `response` until it's released.
        let request = unsafe {
            let app = sys::SteamAPI_ISteamUtils_GetAppID(sys::SteamAPI_SteamUtils_v010());
            match kind {
                ServerQuery::Internet => sys::SteamAPI_ISteamMatchmakingServers_RequestInternetServerList(servers(), app, &mut list, pairs.len() as u32, r),
                ServerQuery::Lan => sys::SteamAPI_ISteamMatchmakingServers_RequestLANServerList(servers(), app, r),
                ServerQuery::Favorites => sys::SteamAPI_ISteamMatchmakingServers_RequestFavoritesServerList(servers(), app, &mut list, pairs.len() as u32, r),
            }
        };
        if request.is_null() {
            return None;
        }
        self.queries.lock().unwrap().insert(id, Query { request: request as usize, _response: response });
        Some(id)
    }

    fn take(&self, id: u64) -> Vec<ServerListEvent> {
        self.heard.lock().unwrap().remove(&id).unwrap_or_default()
    }

    fn cancel(&self, id: u64) {
        if let Some(q) = self.queries.lock().unwrap().remove(&id) {
            // SAFETY: a request we made and haven't released; releasing cancels it, so `response` is unused after.
            unsafe { sys::SteamAPI_ISteamMatchmakingServers_ReleaseRequest(servers(), q.request as *mut c_void) };
        }
        self.heard.lock().unwrap().remove(&id);
    }
}

/// A filter as Steam takes it; each side is cut to its 255 characters.
fn pair(key: &str, value: &str) -> sys::MatchMakingKeyValuePair_t {
    let mut p = sys::MatchMakingKeyValuePair_t { m_szKey: [0; 256], m_szValue: [0; 256] };
    for (dst, src) in [(&mut p.m_szKey, c(key)), (&mut p.m_szValue, c(value))] {
        for (d, s) in dst.iter_mut().zip(src.as_bytes().iter().take(255)) {
            *d = *s as _;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_items() {
        let p = pair("gametagsand", "protocol:1");
        assert_eq!(p.m_szKey[..11].iter().map(|&b| b as u8).collect::<Vec<_>>(), b"gametagsand");
        assert_eq!(p.m_szValue[10], 0);
        let mut details = vec![0u8; size_of::<sys::gameserveritem_t>()];
        let at = offset_of!(sys::gameserveritem_t, m_steamID);
        details[at..at + 8].copy_from_slice(&90071992547409920u64.to_le_bytes());
        let ServerListEvent::Responded { item: bytes, server } = item(&details) else { panic!() };
        assert_eq!((bytes.len(), server), (details.len(), SteamId(90071992547409920)));
    }

    #[test]
    fn responses_queue_by_query() {
        let s = SteamServers::default();
        let mut r = Response { vtable: &VTABLE, id: 4, heard: s.heard.clone() };
        complete(&mut r, std::ptr::null_mut(), 0);
        assert_eq!(s.take(4), [ServerListEvent::Finished]);
        assert!(s.take(4).is_empty());
    }
}
