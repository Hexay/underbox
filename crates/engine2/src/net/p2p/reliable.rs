//! Ordered, reliable messages over lossy datagrams: messages split into numbered segments, a
//! selective-repeat window, cumulative acks with a 64-segment bitmap, and per-segment retransmit
//! timers that back off. Unacked data survives a Steam session restarting under it.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Payload bytes per segment: under Steam's ~1200-byte packet once our header and Steam's are added.
pub const SEGMENT: usize = 1100;
/// Segments in flight past the oldest unacked one; the receiver keeps out-of-order ones this far ahead.
pub const WINDOW: u32 = 512;
const FIRST_RTO: Duration = Duration::from_millis(200);
const MIN_RTO: Duration = Duration::from_millis(50);
const MAX_RTO: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub seq: u32,
    pub last: bool,
    pub payload: Vec<u8>,
}

struct InFlight {
    seg: Segment,
    sent: Instant,
    rto: Duration,
    resent: bool,
}

#[derive(Default)]
pub struct Sender {
    next_seq: u32,
    queued: VecDeque<Segment>,
    in_flight: BTreeMap<u32, InFlight>,
    srtt: Option<Duration>,
}

impl Sender {
    pub fn push(&mut self, message: &[u8]) {
        let mut chunks: Vec<&[u8]> = message.chunks(SEGMENT).collect();
        if chunks.is_empty() {
            chunks.push(&[]);
        }
        let n = chunks.len();
        for (i, c) in chunks.into_iter().enumerate() {
            self.queued.push_back(Segment { seq: self.next_seq, last: i + 1 == n, payload: c.to_vec() });
            self.next_seq += 1;
        }
    }

    fn rto(&self) -> Duration {
        self.srtt.map_or(FIRST_RTO, |s| (s * 2 + Duration::from_millis(20)).clamp(MIN_RTO, MAX_RTO))
    }

    /// Segments to put on the wire now: new ones the window allows, then any whose timer ran out.
    pub fn poll(&mut self, now: Instant) -> Vec<Segment> {
        let mut out = vec![];
        for f in self.in_flight.values_mut().filter(|f| now >= f.sent + f.rto) {
            f.sent = now;
            f.rto = (f.rto * 2).min(MAX_BACKOFF);
            f.resent = true;
            out.push(f.seg.clone());
        }
        let base = self.in_flight.keys().next().or(self.queued.front().map(|s| &s.seq)).copied().unwrap_or(self.next_seq);
        let rto = self.rto();
        while self.queued.front().is_some_and(|s| s.seq < base + WINDOW) {
            let seg = self.queued.pop_front().unwrap();
            out.push(seg.clone());
            self.in_flight.insert(seg.seq, InFlight { seg, sent: now, rto, resent: false });
        }
        out
    }

    pub fn on_ack(&mut self, next: u32, mask: u64, now: Instant) {
        let mut acked: Vec<u32> = self.in_flight.range(..next).map(|(&s, _)| s).collect();
        acked.extend((0..64).filter(|i| mask & (1 << i) != 0).map(|i| next.wrapping_add(1 + i)));
        for seq in acked {
            // Karn: a resent segment's ack can't say which send it answers.
            if let Some(f) = self.in_flight.remove(&seq).filter(|f| !f.resent) {
                let sample = now.saturating_duration_since(f.sent);
                self.srtt = Some(self.srtt.map_or(sample, |s| (s * 7 + sample) / 8));
            }
        }
    }

    /// Smoothed round trip, once one has been measured.
    pub fn rtt(&self) -> Option<Duration> {
        self.srtt
    }

    pub fn queued_bytes(&self) -> usize {
        self.queued.iter().map(|s| s.payload.len()).sum()
    }

    pub fn unacked_bytes(&self) -> usize {
        self.in_flight.values().map(|f| f.seg.payload.len()).sum()
    }

    pub fn is_idle(&self) -> bool {
        self.queued.is_empty() && self.in_flight.is_empty()
    }
}

#[derive(Default)]
pub struct Receiver {
    next: u32,
    ahead: BTreeMap<u32, Segment>,
    partial: Vec<u8>,
    ack_due: bool,
}

impl Receiver {
    /// Take a segment; returns the messages it completed, in order.
    pub fn on_segment(&mut self, seg: Segment) -> Vec<Vec<u8>> {
        self.ack_due = true;
        if seg.seq < self.next || seg.seq >= self.next + WINDOW {
            return vec![];
        }
        self.ahead.entry(seg.seq).or_insert(seg);
        let mut done = vec![];
        while let Some(s) = self.ahead.remove(&self.next) {
            self.partial.extend_from_slice(&s.payload);
            self.next += 1;
            if s.last {
                done.push(std::mem::take(&mut self.partial));
            }
        }
        done
    }

    /// Bytes of the message being reassembled.
    pub fn partial_len(&self) -> usize {
        self.partial.len()
    }

    /// `(next, mask)` for an ack frame, when a segment arrived since the last one.
    pub fn take_ack(&mut self) -> Option<(u32, u64)> {
        if !std::mem::take(&mut self.ack_due) {
            return None;
        }
        let mask = self.ahead.range(self.next + 1..self.next + 65).fold(0u64, |m, (&s, _)| m | 1 << (s - self.next - 1));
        Some((self.next, mask))
    }
}

#[cfg(test)]
#[path = "reliable_tests.rs"]
mod tests;
