//! Two transports over the in-memory messages interface, as a client and a dedicated server.

use super::conn::SEND_RELIABLE;
use super::loopback::{Endpoint, Net};
use super::*;
use std::sync::Arc;
use std::time::Duration;

const SERVER: u64 = 90071992549575139;
const CLIENT: u64 = 76561197960287930;

struct Side {
    p2p: P2p,
    wire: Endpoint,
    changes: Vec<Status>,
}

struct World {
    net: Arc<Net>,
    t: Instant,
    server: Side,
    client: Side,
}

impl World {
    fn new() -> World {
        let net = Net::new();
        let side = |id| Side { p2p: P2p::default(), wire: net.endpoint(id), changes: vec![] };
        World { server: side(SERVER), client: side(CLIENT), net, t: Instant::now() }
    }

    /// Pump both sides in 5 ms steps for `ms`, answering incoming connections as `accept` says.
    fn run(&mut self, ms: u64, accept: bool) {
        for _ in 0..ms / 5 {
            self.t += Duration::from_millis(5);
            for s in [&mut self.server, &mut self.client] {
                s.p2p.pump(&s.wire, self.t);
                for c in s.p2p.take_changes() {
                    if c.listen_socket != 0 && c.state == state::CONNECTING {
                        if accept {
                            assert_eq!(s.p2p.accept(&s.wire, c.conn), result::OK);
                        } else {
                            s.p2p.close(&s.wire, c.conn, end::APP_GENERIC, "rejected", false, self.t);
                        }
                    }
                    s.changes.push(c);
                }
            }
        }
    }

    /// Listen on vport 77, connect the client, run until connected; (listen socket, client conn, server conn).
    fn connected(&mut self) -> (u32, u32, u32) {
        let sock = self.server.p2p.listen(77);
        let conn = self.client.p2p.connect(&self.client.wire, SteamId(SERVER), 77, self.t);
        self.run(200, true);
        assert_eq!(self.client.p2p.state(conn), state::CONNECTED);
        let incoming = self.server.changes.iter().find(|c| c.state == state::CONNECTED).expect("server saw it connect").conn;
        (sock, conn, incoming)
    }
}

fn kinds(changes: &[Status]) -> Vec<(i32, i32)> {
    changes.iter().map(|c| (c.old_state, c.state)).collect()
}

#[test]
fn connects_by_steam_id_through_a_session_request() {
    let mut w = World::new();
    let (sock, conn, incoming) = w.connected();
    assert_eq!(kinds(&w.server.changes), [(state::NONE, state::CONNECTING), (state::CONNECTING, state::CONNECTED)]);
    let s = &w.server.changes[1];
    assert_eq!((s.listen_socket, s.remote_steam_id), (sock, CLIENT));
    assert!(P2p::is_listen_socket(sock) && !P2p::is_listen_socket(conn));
    assert_eq!(w.client.p2p.peer(conn), SERVER);
    assert_eq!(w.server.p2p.peer(incoming), CLIENT);
    assert_eq!(w.client.p2p.description(conn).unwrap(), format!("#{conn} P2P steamid:{SERVER} vport 77"));
    assert_eq!(kinds(&w.client.changes), [(state::CONNECTING, state::CONNECTED)]);
}

#[test]
fn delivers_reliable_and_unreliable_messages_through_poll_groups() {
    let mut w = World::new();
    let (_, conn, incoming) = w.connected();
    assert!(w.server.p2p.set_poll_group(incoming, 0x8001_0001));
    let big: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();
    assert_eq!(w.client.p2p.send(&w.client.wire, conn, b"hello", SEND_RELIABLE, w.t), result::OK);
    assert_eq!(w.client.p2p.send(&w.client.wire, conn, &big, SEND_RELIABLE, w.t), result::OK);
    assert_eq!(w.client.p2p.send(&w.client.wire, conn, b"fast", 0, w.t), result::OK);
    w.run(500, true);
    let got = w.server.p2p.receive_poll_group(0x8001_0001, 16);
    let shape: Vec<(usize, i32, i64)> = got.iter().map(|(_, _, m)| (m.data.len(), m.flags, m.number)).collect();
    assert_eq!(shape, [(5, SEND_RELIABLE, 1), (big.len(), SEND_RELIABLE, 2), (4, 0, 3)], "in send order on a lossless link");
    assert!(got[0].2.data == b"hello" && got[1].2.data == big && got[2].2.data == b"fast");
    assert!(got.iter().all(|&(c, peer, _)| c == incoming && peer == CLIENT));
    assert_eq!(w.server.p2p.send(&w.server.wire, incoming, b"welcome", SEND_RELIABLE, w.t), result::OK);
    w.run(100, true);
    assert_eq!(w.client.p2p.receive_connection(conn, 4)[0].data, b"welcome");
    assert!(w.client.p2p.stats(conn).unwrap().ping_ms >= 5, "one 5 ms step each way at least");
}

#[test]
fn reliable_stream_survives_heavy_loss() {
    let mut w = World::new();
    let (_, conn, incoming) = w.connected();
    w.net.lose(|i| i % 3 == 0 || i % 7 == 0);
    let msgs: Vec<Vec<u8>> = (0..30).map(|i| vec![i as u8; 1 + i * 997]).collect();
    for m in &msgs {
        w.client.p2p.send(&w.client.wire, conn, m, SEND_RELIABLE, w.t);
    }
    w.run(5_000, true);
    let got: Vec<Vec<u8>> = w.server.p2p.receive_connection(incoming, 64).into_iter().map(|m| m.data).collect();
    assert_eq!(got, msgs, "all of them, in order, once");
    assert_eq!(w.client.p2p.state(conn), state::CONNECTED, "loss alone doesn't drop the connection");
}

#[test]
fn nothing_listening_on_the_port_closes_the_client() {
    let mut w = World::new();
    w.server.p2p.listen(1);
    let conn = w.client.p2p.connect(&w.client.wire, SteamId(SERVER), 77, w.t);
    w.run(100, true);
    let c = &w.client.changes[0];
    assert_eq!((c.conn, c.old_state, c.state, c.end_reason), (conn, state::CONNECTING, state::CLOSED_BY_PEER, end::PEER_SENT_NO_CONNECTION));
    assert_eq!(c.end_debug, "Nothing listening on virtual port 77");
}

#[test]
fn rejected_and_kicked_clients_hear_why() {
    let mut w = World::new();
    w.server.p2p.listen(77);
    let conn = w.client.p2p.connect(&w.client.wire, SteamId(SERVER), 77, w.t);
    w.run(100, false);
    assert_eq!(w.client.changes.last().map(|c| (c.state, c.end_debug.as_str())), Some((state::CLOSED_BY_PEER, "rejected")));
    w.client.p2p.close(&w.client.wire, conn, 0, "", false, w.t);

    let mut w = World::new();
    let (sock, conn, _) = w.connected();
    assert!(w.server.p2p.close_listen(&w.server.wire, sock, w.t));
    w.run(50, true);
    let last = w.client.changes.last().unwrap();
    assert_eq!((last.conn, last.state, last.end_reason), (conn, state::CLOSED_BY_PEER, end::APP_GENERIC));
}

#[test]
fn a_dead_link_times_out_both_ends() {
    let mut w = World::new();
    let (_, conn, incoming) = w.connected();
    w.net.cut(true);
    w.run(conn::IDLE_TIMEOUT.as_millis() as u64 + 50, true);
    for (side, c) in [(&w.client, conn), (&w.server, incoming)] {
        let last = side.changes.last().unwrap();
        assert_eq!((last.conn, last.state, last.end_reason), (c, state::PROBLEM_DETECTED_LOCALLY, end::MISC_TIMEOUT));
    }
}

#[test]
fn without_a_listener_sessions_stay_unaccepted_and_connects_time_out() {
    let mut w = World::new();
    let conn = w.client.p2p.connect(&w.client.wire, SteamId(SERVER), 77, w.t);
    w.run(conn::CONNECT_TIMEOUT.as_millis() as u64 + 50, true);
    assert!(w.server.changes.is_empty(), "the server never read the held connect");
    assert_eq!(kinds(&w.client.changes), [(state::CONNECTING, state::PROBLEM_DETECTED_LOCALLY)]);
    assert!(w.net.datagrams() >= 40, "connect resent every 250 ms");
    w.client.p2p.close(&w.client.wire, conn, 0, "", false, w.t);
    assert_eq!(w.client.p2p.state(conn), state::NONE);
}

#[test]
fn handles_are_reused_only_once_free() {
    let mut p = P2p::default();
    let wire = Net::new().endpoint(1);
    let a = p.connect(&wire, SteamId(2), 77, Instant::now());
    let b = p.connect(&wire, SteamId(2), 77, Instant::now());
    assert_ne!(a, b);
    assert!(a <= 0xFFFF && b <= 0xFFFF, "GNS handles always have a nonzero high half");
    p.next = 0xFFFF;
    assert_eq!(p.connect(&wire, SteamId(2), 77, Instant::now()), b + 1, "wraps past the live ones");
}
