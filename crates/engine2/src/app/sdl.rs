//! The SDL surface a windowless server uses: frame pacing and an always-empty event queue.

use abi::slots::global_sdl as sdl;
use abi::{Registry, bind};
use std::time::{Duration, Instant};

/// OS sleeps overshoot by up to about this much, so the tail is spun.
const SPIN: Duration = Duration::from_millis(1);

/// SDL_DelayPrecise: wait `ns` nanoseconds, sleeping the bulk and spinning the tail.
fn delay_precise(ns: u64) {
    let deadline = Instant::now() + Duration::from_nanos(ns);
    if let Some(sleep) = deadline.checked_duration_since(Instant::now()).and_then(|d| d.checked_sub(SPIN)) {
        std::thread::sleep(sleep);
    }
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, sdl::DELAY_PRECISE, delay_precise);
    // No window, so no input or window events.
    bind!(r, sdl::POLL_EVENT, |_| false);
}

#[cfg(test)]
mod tests {
    #[test]
    fn delay_precise_waits_at_least_the_duration() {
        let start = std::time::Instant::now();
        super::delay_precise(3_000_000);
        let took = start.elapsed();
        assert!(took >= std::time::Duration::from_millis(3) && took < std::time::Duration::from_millis(20), "{took:?}");
    }
}
