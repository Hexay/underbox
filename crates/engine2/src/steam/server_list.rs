//! The server browser (`CServerList`) over [`platform::ServerBrowser`]. What Steam hears while the
//! client's callbacks run is handed to the managed `ServerList` right after. Without a browser
//! (offline) a query starts and finishes empty at once.

use abi::callbacks::sandbox_services_server_list as managed;
use abi::slots::c_server_list as sl;
use abi::{ManagedObject, NativeRef, RawPtr, Registry, bind, class};
use platform::{ServerBrowser, ServerListEvent, ServerQuery};
use std::ffi::CStr;
use std::sync::Mutex;

type List = NativeRef<class::CServerList>;

/// A server-list query: the managed `ServerList` it reports to, its filters, and the query in flight.
struct ServerList {
    owner: ManagedObject,
    filters: Mutex<Vec<(String, String)>>,
    running: Mutex<Option<u64>>,
}

/// Lists with a query in flight, polled after each callback pump.
static RUNNING: Mutex<Vec<List>> = Mutex::new(Vec::new());

fn browser() -> Option<&'static dyn ServerBrowser> {
    super::client::local()?.servers()
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn start(l: List, kind: ServerQuery) {
    let Some(list) = objects::get::<_, ServerList>(l) else { return };
    stop(l, &list);
    managed::on_started(list.owner);
    let filters = list.filters.lock().unwrap().clone();
    match browser().and_then(|b| b.request(kind, &filters)) {
        Some(id) => {
            *list.running.lock().unwrap() = Some(id);
            RUNNING.lock().unwrap().push(l);
        }
        None => managed::on_finished(list.owner),
    }
}

fn stop(l: List, list: &ServerList) {
    if let Some(id) = list.running.lock().unwrap().take() {
        browser().inspect(|b| b.cancel(id));
    }
    RUNNING.lock().unwrap().retain(|&r| r != l);
}

/// Hand each running list what its query heard. Runs managed code, so no lock is held across it.
pub(super) fn poll() {
    let Some(b) = browser() else { return };
    let lists = RUNNING.lock().unwrap().clone();
    for l in lists {
        let Some(list) = objects::get::<_, ServerList>(l) else { continue };
        let Some(id) = *list.running.lock().unwrap() else { continue };
        for e in b.take(id) {
            match e {
                // Managed reads the gameserveritem_t during the call; it's ours until then.
                ServerListEvent::Responded { mut item, server } => managed::on_server_responded(list.owner, RawPtr::from_raw(item.as_mut_ptr().cast()), server.0),
                ServerListEvent::Finished => {
                    stop(l, &list);
                    managed::on_finished(list.owner);
                }
            }
        }
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, sl::CREATE, |owner| objects::create(ServerList { owner, filters: Mutex::default(), running: Mutex::default() }));
    bind!(r, sl::DESTROY, |l| {
        if let Some(list) = objects::get::<_, ServerList>(l) {
            stop(l, &list);
        }
        objects::destroy(l);
    });
    bind!(r, sl::ADD_FILTER, |l, k, v| {
        if let Some(list) = objects::get::<_, ServerList>(l) {
            list.filters.lock().unwrap().push((text(k), text(v)));
        }
    });
    bind!(r, sl::START_QUERY, |l| start(l, ServerQuery::Internet));
    bind!(r, sl::START_LAN_QUERY, |l| start(l, ServerQuery::Lan));
}
