//! Connect to a Steam-sockets server, print each state change, hold for a while, report how it ended.
//! usage: cargo run -p gns --example probe -- <host:port> [seconds=5]

use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let addr = args.next().expect("usage: probe <host:port> [seconds]");
    let hold = Duration::from_secs(args.next().and_then(|s| s.parse().ok()).unwrap_or(5));
    gns::init().expect("init");
    gns::set_config_int(gns::config::IP_ALLOW_WITHOUT_AUTH, 1);
    let conn = gns::connect_ip(&addr);
    assert_ne!(conn, 0, "bad address {addr}");
    let start = Instant::now();
    let mut connected_at = None;
    loop {
        for s in gns::run_callbacks() {
            println!("{:>6}ms state {} -> {} reason {} {:?}", start.elapsed().as_millis(), s.old_state, s.state, s.end_reason, s.end_debug);
            match s.state {
                gns::state::CONNECTED => connected_at = Some(Instant::now()),
                gns::state::CLOSED_BY_PEER | gns::state::PROBLEM_DETECTED_LOCALLY => {
                    println!("closed after {}ms", start.elapsed().as_millis());
                    return;
                }
                _ => {}
            }
        }
        if connected_at.is_some_and(|t| t.elapsed() > hold) {
            println!("still connected after {}s; closing", hold.as_secs());
            gns::close(conn, 0, Some(c"probe done"), false);
            return;
        }
        assert!(start.elapsed() < hold + Duration::from_secs(10), "timed out connecting");
        std::thread::sleep(Duration::from_millis(5));
    }
}
