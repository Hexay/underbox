use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn message(i: usize, len: usize) -> Vec<u8> {
    (0..len).map(|b| (i * 31 + b) as u8).collect()
}

/// Run a sender and receiver against each other with a one-way delay, dropping the datagrams `lose`
/// picks (by send index) in each direction, until everything is acked; returns messages in arrival order.
fn simulate(messages: &[Vec<u8>], lose: impl Fn(usize) -> bool) -> (Vec<Vec<u8>>, Sender, Duration) {
    let t0 = Instant::now();
    let (mut tx, mut rx) = (Sender::default(), Receiver::default());
    messages.iter().for_each(|m| tx.push(m));
    let (mut wire, mut acks, mut got) = (VecDeque::<(Instant, Segment)>::new(), VecDeque::<(Instant, (u32, u64))>::new(), vec![]);
    let (mut sent, mut acked) = (0, 0);
    let mut now = t0;
    while !tx.is_idle() {
        assert!(now - t0 < Duration::from_secs(120), "never finished");
        for s in tx.poll(now) {
            sent += 1;
            if !lose(sent) {
                wire.push_back((now + ms(30), s));
            }
        }
        while wire.front().is_some_and(|(at, _)| *at <= now) {
            got.extend(rx.on_segment(wire.pop_front().unwrap().1));
        }
        if let Some(a) = rx.take_ack() {
            acked += 1;
            if !lose(acked) {
                acks.push_back((now + ms(30), a));
            }
        }
        while acks.front().is_some_and(|(at, _)| *at <= now) {
            let (next, mask) = acks.pop_front().unwrap().1;
            tx.on_ack(next, mask, now);
        }
        now += ms(5);
    }
    (got, tx, now - t0)
}

#[test]
fn splits_messages_into_segments() {
    let mut tx = Sender::default();
    tx.push(&message(0, SEGMENT * 2 + 1));
    tx.push(&[]);
    let segs = tx.poll(Instant::now());
    let shape: Vec<(u32, bool, usize)> = segs.iter().map(|s| (s.seq, s.last, s.payload.len())).collect();
    assert_eq!(shape, [(0, false, SEGMENT), (1, false, SEGMENT), (2, true, 1), (3, true, 0)]);
    assert_eq!(tx.unacked_bytes(), SEGMENT * 2 + 1);
}

#[test]
fn delivers_in_order_without_loss() {
    let msgs: Vec<_> = (0..20).map(|i| message(i, i * 300)).collect();
    let (got, tx, _) = simulate(&msgs, |_| false);
    assert_eq!(got, msgs);
    let rtt = tx.rtt().unwrap();
    assert!(rtt >= ms(55) && rtt <= ms(75), "rtt ~60ms, got {rtt:?}");
}

#[test]
fn recovers_every_message_once_under_heavy_loss() {
    let msgs: Vec<_> = (0..40).map(|i| message(i, (i % 7) * 900 + 1)).collect();
    // A third of data and acks lost, in bursts.
    let (got, _, took) = simulate(&msgs, |i| i % 3 == 0 || (i / 10) % 5 == 4);
    assert_eq!(got, msgs, "every message, in order, exactly once");
    assert!(took < Duration::from_secs(30), "took {took:?}");
}

#[test]
fn large_messages_beyond_the_window_get_through() {
    let msgs = vec![message(1, SEGMENT * (WINDOW as usize + 40)), message(2, 10)];
    let (got, _, _) = simulate(&msgs, |i| i % 11 == 0);
    assert_eq!(got.len(), 2);
    assert!(got == msgs);
}

#[test]
fn window_limits_segments_in_flight() {
    let mut tx = Sender::default();
    tx.push(&message(0, SEGMENT * (WINDOW as usize + 10)));
    let now = Instant::now();
    assert_eq!(tx.poll(now).len(), WINDOW as usize);
    assert_eq!(tx.queued_bytes(), SEGMENT * 10);
    tx.on_ack(5, 0, now + ms(10));
    assert_eq!(tx.poll(now + ms(10)).iter().map(|s| s.seq).collect::<Vec<_>>(), (WINDOW..WINDOW + 5).collect::<Vec<_>>());
}

#[test]
fn retransmit_timer_backs_off() {
    let mut tx = Sender::default();
    tx.push(b"x");
    let t = Instant::now();
    assert_eq!(tx.poll(t).len(), 1);
    assert!(tx.poll(t + ms(199)).is_empty());
    assert_eq!(tx.poll(t + ms(200)).len(), 1, "first RTO 200ms");
    assert!(tx.poll(t + ms(599)).is_empty());
    assert_eq!(tx.poll(t + ms(600)).len(), 1, "then 400ms");
    tx.on_ack(1, 0, t + ms(650));
    assert!(tx.is_idle() && tx.rtt().is_none(), "acks of resent segments give no RTT sample");
}

#[test]
fn acks_report_gaps() {
    let mut rx = Receiver::default();
    let seg = |seq, last| Segment { seq, last, payload: vec![seq as u8] };
    assert_eq!(rx.take_ack(), None, "nothing to ack yet");
    assert!(rx.on_segment(seg(1, true)).is_empty());
    assert!(rx.on_segment(seg(3, true)).is_empty());
    assert_eq!(rx.take_ack(), Some((0, 0b101)), "1 and 3 held past the missing 0");
    assert_eq!(rx.on_segment(seg(0, false)), vec![vec![0, 1]]);
    assert_eq!(rx.take_ack(), Some((2, 0b1)));
    assert!(rx.on_segment(seg(0, false)).is_empty(), "duplicates are dropped...");
    assert_eq!(rx.take_ack(), Some((2, 0b1)), "...but re-acked");
    assert!(rx.on_segment(seg(2 + WINDOW, true)).is_empty());
    assert_eq!(rx.take_ack(), Some((2, 0b1)), "beyond the window: not kept");
}
