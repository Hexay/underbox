//! Playback time: a position that advances with the wall clock while running.

use std::time::Instant;

#[derive(Default)]
pub struct Clock {
    base: f64,
    since: Option<Instant>,
}

impl Clock {
    pub fn now(&self) -> f64 {
        self.base + self.since.map_or(0.0, |s| s.elapsed().as_secs_f64())
    }

    pub fn pause(&mut self) {
        self.base = self.now();
        self.since = None;
    }

    pub fn resume(&mut self) {
        if self.since.is_none() {
            self.since = Some(Instant::now());
        }
    }

    /// Jump to `t`, running or not as before.
    pub fn set(&mut self, t: f64) {
        self.base = t;
        if self.since.is_some() {
            self.since = Some(Instant::now());
        }
    }
}
