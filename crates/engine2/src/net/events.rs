//! Connection state changes → managed's SteamNetwork callbacks. Native's dispatch rules are closed;
//! these follow from what managed expects: listen sockets ask ShouldAcceptConnection before
//! accepting, report OnSocketConnection once connected and OnSocketDisconnection when a connected
//! peer drops; outgoing connections report only their loss (OnDisconnection), and managed polls
//! GetConnectionState for the rest.

use abi::callbacks::sandbox_network_steam_network as managed;
use abi::structs::{HSteamListenSocket, HSteamNetConnection};
use gns::{Status, state};
use std::ffi::CString;

fn handles(s: &Status) -> (HSteamListenSocket, HSteamNetConnection) {
    (HSteamListenSocket { id: s.listen_socket }, HSteamNetConnection { id: s.conn })
}

/// Steam's `k_ESteamNetConnectionEnd_App_Generic`: closed by the application.
const END_APP: i32 = 1000;

/// Handle one change; calls into managed, so no engine lock may be held.
pub fn dispatch(s: &Status) {
    let (socket, conn) = handles(s);
    let incoming = s.listen_socket != 0;
    match s.state {
        state::CONNECTING if incoming => {
            if managed::should_accept_connection(socket, conn) {
                gns::accept(s.conn);
            } else {
                gns::close(s.conn, END_APP, Some(c"rejected"), false);
            }
        }
        state::CONNECTED if incoming => managed::on_socket_connection(socket, conn),
        state::CLOSED_BY_PEER | state::PROBLEM_DETECTED_LOCALLY => {
            if !incoming {
                let reason = CString::new(s.end_debug.as_str()).unwrap_or_default();
                managed::on_disconnection(conn, s.end_reason, Some(&reason));
            } else if s.old_state == state::CONNECTED {
                managed::on_socket_disconnection(socket, conn);
            }
            // The handle stays allocated until closed locally, whoever ended it.
            gns::close(s.conn, 0, None, false);
        }
        _ => {}
    }
}
