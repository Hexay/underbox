use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A client connection to vport 77 and the listener's side of it, accepted.
fn pair(t: Instant) -> (Conn, Conn) {
    let mut client = Conn::outgoing(1, SteamId(200), 77, t);
    let mut out = vec![];
    client.poll(t, &mut out);
    assert_eq!(out, [Frame::Connect { src: 1, vport: 77 }]);
    let mut server = Conn::incoming(5, SteamId(100), 1, 77, 0x8000_0001, t);
    let mut out = vec![];
    assert_eq!(server.accept(&mut out), Some(state::CONNECTING));
    assert_eq!(out, [Frame::Accept { dst: 1, src: 5 }]);
    assert_eq!(client.on_frame(out.remove(0), t, &mut vec![]), Some(state::CONNECTING));
    assert_eq!((client.state, client.remote), (state::CONNECTED, 5));
    (client, server)
}

#[test]
fn connect_retries_then_times_out() {
    let t = Instant::now();
    let mut c = Conn::outgoing(1, SteamId(9), 77, t);
    let mut out = vec![];
    assert_eq!(c.poll(t, &mut out), None);
    assert_eq!(c.poll(t + ms(100), &mut out), None);
    assert_eq!(out.len(), 1, "no resend before the retry interval");
    c.poll(t + CONNECT_RETRY, &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(c.poll(t + CONNECT_TIMEOUT, &mut out), Some(state::CONNECTING));
    assert_eq!((c.state, c.end_reason), (state::PROBLEM_DETECTED_LOCALLY, end::MISC_TIMEOUT));
}

#[test]
fn only_waiting_incoming_connections_accept() {
    let t = Instant::now();
    let (mut client, mut server) = pair(t);
    assert_eq!(server.accept(&mut vec![]), None, "already accepted");
    assert_eq!(client.accept(&mut vec![]), None, "outgoing");
    let mut out = vec![];
    server.on_frame(Frame::Connect { src: 1, vport: 77 }, t, &mut out);
    assert_eq!(out, [Frame::Accept { dst: 1, src: 5 }], "a repeated connect gets the accept again");
}

#[test]
fn sends_are_checked_against_state_and_size() {
    let t = Instant::now();
    let mut waiting = Conn::outgoing(1, SteamId(9), 77, t);
    assert_eq!(waiting.send(b"x", SEND_NO_DELAY, &mut vec![]), result::IGNORED);
    let (mut client, _) = pair(t);
    assert_eq!(client.send(&vec![0; MAX_MESSAGE + 1], SEND_RELIABLE, &mut vec![]), result::INVALID_PARAM);
    let mut out = vec![];
    assert_eq!(client.send(b"u", 0, &mut out), result::OK);
    assert_eq!(out, [Frame::Unreliable { dst: 5, payload: b"u".to_vec() }], "unreliable goes out at once");
    assert_eq!(client.send(b"r", SEND_RELIABLE, &mut out), result::OK);
    assert_eq!(out.len(), 1, "reliable waits for the next poll");
    assert_eq!(client.pending_reliable(), 1);
}

#[test]
fn messages_flow_both_ways_with_flags_and_numbers() {
    let t = Instant::now();
    let (mut client, mut server) = pair(t);
    let mut out = vec![];
    client.send(b"one", SEND_RELIABLE, &mut out);
    client.send(b"two", 0, &mut out);
    client.poll(t + ms(1), &mut out);
    for f in out {
        server.on_frame(f, t + ms(2), &mut vec![]);
    }
    let got: Vec<_> = server.inbox.drain(..).collect();
    assert_eq!(got, [Received { data: b"two".to_vec(), flags: 0, number: 1 }, Received { data: b"one".to_vec(), flags: SEND_RELIABLE, number: 2 }]);
    let mut acks = vec![];
    server.poll(t + ms(3), &mut acks);
    assert_eq!(acks, [Frame::Ack { dst: 1, next: 1, mask: 0 }]);
    client.on_frame(acks.remove(0), t + ms(4), &mut vec![]);
    assert_eq!(client.unacked_reliable(), 0);
    assert_eq!(client.rtt(), Some(ms(3)));
}

#[test]
fn messages_sent_while_connecting_go_out_once_connected() {
    let t = Instant::now();
    let mut c = Conn::outgoing(1, SteamId(9), 77, t);
    let mut out = vec![];
    assert_eq!(c.send(b"early-r", SEND_RELIABLE, &mut out), result::OK);
    assert_eq!(c.send(b"early-u", 0, &mut out), result::OK);
    c.poll(t, &mut out);
    assert_eq!(out, [Frame::Connect { src: 1, vport: 77 }], "nothing but the connect until accepted");
    c.on_frame(Frame::Accept { dst: 1, src: 5 }, t, &mut vec![]);
    let mut out = vec![];
    c.poll(t, &mut out);
    assert_eq!(out, [
        Frame::Unreliable { dst: 5, payload: b"early-u".to_vec() },
        Frame::Reliable { dst: 5, seq: 0, last: true, payload: b"early-r".to_vec() },
    ]);
}

#[test]
fn an_unterminated_reliable_message_is_cut_off_at_the_limit() {
    let t = Instant::now();
    let (_, mut server) = pair(t);
    let mut out = vec![];
    let mut changed = None;
    for seq in 0..=(MAX_MESSAGE / 1000) as u32 {
        changed = changed.or(server.on_frame(Frame::Reliable { dst: 5, seq, last: false, payload: vec![0; 1000] }, t, &mut out));
    }
    assert_eq!(changed, Some(state::CONNECTED));
    assert_eq!((server.state, server.end_reason), (state::PROBLEM_DETECTED_LOCALLY, end::MISC_GENERIC));
    assert!(matches!(out.as_slice(), [Frame::Close { dst: 1, reason: end::MISC_GENERIC, .. }]));
}

#[test]
fn keepalives_and_idle_timeout() {
    let t = Instant::now();
    let (mut client, _) = pair(t);
    let mut out = vec![];
    client.poll(t + KEEPALIVE, &mut out);
    assert_eq!(out, [Frame::Ping { dst: 5 }]);
    assert_eq!(client.poll(t + IDLE_TIMEOUT, &mut out), Some(state::CONNECTED));
    assert_eq!((client.state, client.end_reason), (state::PROBLEM_DETECTED_LOCALLY, end::MISC_TIMEOUT));
}

#[test]
fn peer_close_carries_its_reason() {
    let t = Instant::now();
    let (mut client, mut server) = pair(t);
    let mut out = vec![];
    server.close(1000, "Server Full", false, t, &mut out);
    assert!(server.is_finished());
    assert_eq!(client.on_frame(out.remove(0), t, &mut vec![]), Some(state::CONNECTED));
    assert_eq!((client.state, client.end_reason, client.end_debug.as_str()), (state::CLOSED_BY_PEER, 1000, "Server Full"));
}

#[test]
fn linger_flushes_reliable_data_before_closing() {
    let t = Instant::now();
    let (mut client, mut server) = pair(t);
    let mut out = vec![];
    client.send(b"last words", SEND_RELIABLE, &mut out);
    client.close(1000, "bye", true, t, &mut out);
    assert!(out.is_empty() && !client.is_finished(), "still flushing");
    client.poll(t + ms(1), &mut out);
    assert!(matches!(out.as_slice(), [Frame::Reliable { .. }]));
    let mut acks = vec![];
    server.on_frame(out.remove(0), t + ms(2), &mut vec![]);
    server.poll(t + ms(2), &mut acks);
    client.on_frame(acks.remove(0), t + ms(3), &mut vec![]);
    client.poll(t + ms(4), &mut out);
    assert_eq!(out, [Frame::Close { dst: 5, reason: 1000, debug: "bye".into() }]);
    assert!(client.is_finished());
    assert_eq!(server.inbox.pop_front().unwrap().data, b"last words");
}
