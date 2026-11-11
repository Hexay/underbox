use super::*;
use std::time::{Duration, Instant};

/// Pump until `done` holds over everything seen so far, accepting incoming connections.
fn pump(seen: &mut Vec<Status>, done: impl Fn(&[Status]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(seen) {
        assert!(Instant::now() < deadline, "timed out; saw {seen:#?}");
        for s in run_callbacks() {
            if s.listen_socket != 0 && s.state == state::CONNECTING {
                assert_eq!(accept(s.conn), 1);
            }
            seen.push(s);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

unsafe extern "C" fn print(level: i32, msg: *const c_char) {
    eprintln!("gns[{level}] {}", unsafe { CStr::from_ptr(msg) }.to_string_lossy());
}

// run_callbacks drains one process-wide queue: concurrent tests would steal each other's statuses.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// A listen socket and a connected client: (listen socket, client end, server end, statuses seen).
fn connected_pair() -> (u32, u32, u32, Vec<Status>) {
    init().unwrap();
    set_config_int(config::IP_ALLOW_WITHOUT_AUTH, 1);
    // GNS refuses port 0; borrow a free one from the OS.
    let free = std::net::UdpSocket::bind("[::]:0").unwrap().local_addr().unwrap().port();
    let sock = listen_ip(free);
    assert_ne!(sock, 0);
    let addr = listen_address(sock).unwrap();
    let port = addr.rsplit(':').next().unwrap();
    let client = connect_ip(&format!("127.0.0.1:{port}"));
    assert_ne!(client, 0);
    let mut seen = Vec::new();
    pump(&mut seen, |s| s.iter().any(|s| s.conn == client && s.state == state::CONNECTED)
        && s.iter().any(|s| s.listen_socket == sock && s.state == state::CONNECTED));
    let server_side = seen.iter().find(|s| s.listen_socket == sock && s.state == state::CONNECTED).unwrap().conn;
    (sock, client, server_side, seen)
}

/// Queue `count` reliable messages of `size` bytes on `conn` at once; the send results.
fn burst(conn: u32, count: usize, size: i32) -> Vec<i64> {
    let mut msgs: Vec<_> = (0..count).map(|_| allocate_message(conn, size, send::RELIABLE)).collect();
    unsafe { send_messages(&mut msgs) }
}

#[test]
fn loopback_connect_send_receive() {
    let _serial = serial();
    set_debug_output(4, Some(print));
    // Managed's SteamNetworkMessage reads the first 208 bytes in place (through m_nUserData).
    assert!(message_struct_size() >= 208);
    let (sock, client, server_side, mut seen) = connected_pair();
    let pg = create_poll_group();
    assert!(set_poll_group(server_side, pg));

    let msg = allocate_message(client, 5, send::RELIABLE);
    unsafe {
        std::ptr::copy_nonoverlapping(b"hello".as_ptr(), message_data(msg).cast(), 5);
        send_messages(&mut [msg]);
    }
    let mut got = [std::ptr::null_mut(); 4];
    let deadline = Instant::now() + Duration::from_secs(5);
    let n = loop {
        run_callbacks();
        let n = receive_poll_group(pg, &mut got);
        if n > 0 || Instant::now() > deadline {
            break n;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(n, 1);
    unsafe {
        assert_eq!(message_bytes(got[0]), b"hello");
        release(got[0]);
    }

    assert!(close(client, 0, Some(c"bye"), false));
    pump(&mut seen, |s| s.iter().any(|s| s.conn == server_side && s.state == state::CLOSED_BY_PEER));
    assert!(close(server_side, 0, None, false));
    assert!(destroy_poll_group(pg));
    assert!(close_listen(sock));
}

// gns_set_app_id writes through a mirror of an internal class: a wrong layout would read back wrong
// or break the cert exchange, which names the app on both ends.
#[test]
fn app_id_reaches_certs_and_peers_still_connect() {
    let _serial = serial();
    init().unwrap();
    let before = app_id();
    set_app_id(590830);
    assert_eq!(app_id(), 590830);
    let (sock, client, _, _) = connected_pair();
    assert_eq!(connection_state(client), state::CONNECTED);
    assert!(close_listen(sock));
    set_app_id(before);
}

// A joining client's handshake queues every string table at once (a game's code archive alone is
// hundreds of KB); with GNS's 512 KB default send buffer the overflow is refused, not queued.
#[test]
fn reliable_burst_needs_a_bigger_send_buffer() {
    let _serial = serial();
    let (sock, client, _, _) = connected_pair();
    let refused = burst(client, 6, 200 * 1024);
    assert!(refused.contains(&-25), "expected LimitExceeded past the default buffer: {refused:?}");
    assert!(close_listen(sock));

    set_config_int(config::SEND_BUFFER_SIZE, 64 * 1024 * 1024);
    let (sock, client, _, _) = connected_pair();
    let sent = burst(client, 6, 200 * 1024);
    assert!(sent.iter().all(|&r| r > 0), "{sent:?}");
    assert!(close_listen(sock));
}
