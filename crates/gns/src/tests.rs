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

#[test]
fn loopback_connect_send_receive() {
    init().unwrap();
    set_debug_output(4, Some(print));
    set_config_int(config::IP_ALLOW_WITHOUT_AUTH, 1);
    // Managed's SteamNetworkMessage reads the first 208 bytes in place (through m_nUserData).
    assert!(message_struct_size() >= 208);
    // GNS refuses port 0; borrow a free one from the OS.
    let free = std::net::UdpSocket::bind("[::]:0").unwrap().local_addr().unwrap().port();
    let sock = listen_ip(free);
    assert_ne!(sock, 0);
    let addr = listen_address(sock).unwrap();
    let port = addr.rsplit(':').next().unwrap();
    let client = connect_ip(&format!("127.0.0.1:{port}"));
    assert_ne!(client, 0);

    let mut seen = Vec::new();
    pump(&mut seen, |s| s.iter().filter(|s| s.state == state::CONNECTED).count() == 2);
    let server_side = seen.iter().find(|s| s.listen_socket == sock && s.state == state::CONNECTED).unwrap().conn;
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
