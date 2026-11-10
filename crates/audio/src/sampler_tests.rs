use super::*;
use crate::blocks;

fn pcm(rate: u32, samples: Vec<f32>) -> Source {
    Source::Pcm(Arc::new(Pcm::new(rate, vec![samples])))
}

fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32).collect()
}

fn read(s: &mut Sampler, pitch: f32) -> Block {
    let mut out = blocks(1);
    s.read(pitch, &mut out);
    out[0]
}

#[test]
fn same_rate_copies_samples() {
    let mut s = Sampler::new(pcm(RATE, ramp(1000)));
    let b = read(&mut s, 1.0);
    assert_eq!(b, std::array::from_fn(|i| i as f32));
    assert_eq!(s.position(), 512);
}

#[test]
fn half_rate_source_interpolates_linearly() {
    let mut s = Sampler::new(pcm(RATE / 2, ramp(1000)));
    let b = read(&mut s, 1.0);
    assert_eq!(&b[..5], &[0.0, 0.5, 1.0, 1.5, 2.0]);
    assert_eq!(s.position(), 256);
}

#[test]
fn pitch_scales_the_step() {
    let mut s = Sampler::new(pcm(RATE, ramp(2000)));
    let b = read(&mut s, 2.0);
    assert_eq!(&b[..3], &[0.0, 2.0, 4.0]);
    assert_eq!(s.position(), 1024);
    let mut s = Sampler::new(pcm(RATE, ramp(2000)));
    assert!(s.set_time_scale(0.5));
    assert!(!s.set_time_scale(0.0));
    assert_eq!(&read(&mut s, 1.0)[..3], &[0.0, 0.5, 1.0]);
}

#[test]
fn a_one_shot_ends_with_silence_and_finishes() {
    let mut s = Sampler::new(pcm(RATE, vec![1.0; 100]));
    let b = read(&mut s, 1.0);
    assert!(b[..100].iter().all(|&v| v == 1.0));
    assert!(b[100..].iter().all(|&v| v == 0.0));
    assert!(!s.should_continue());
}

#[test]
fn loop_points_wrap_and_never_finish() {
    let mut p = Pcm::new(RATE, vec![ramp(10)]);
    p.loop_start = Some(4);
    p.loop_end = Some(8);
    let mut s = Sampler::new(Source::Pcm(Arc::new(p)));
    let b = read(&mut s, 1.0);
    assert_eq!(&b[..14], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 4.0, 5.0, 6.0, 7.0, 4.0, 5.0]);
    assert!(s.should_continue());

    let mut s = Sampler::new(pcm(RATE, ramp(3)));
    s.set_looping(true);
    assert_eq!(&read(&mut s, 1.0)[..7], &[0.0, 1.0, 2.0, 0.0, 1.0, 2.0, 0.0]);
}

#[test]
fn delay_seek_and_sample_end() {
    let mut s = Sampler::new(pcm(RATE, vec![1.0; 2000]));
    s.delay_or_skip(10);
    let b = read(&mut s, 1.0);
    assert!(b[..10].iter().all(|&v| v == 0.0) && b[10..].iter().all(|&v| v == 1.0));
    assert_eq!(s.position(), 502);

    let mut s = Sampler::new(pcm(RATE, ramp(2000)));
    s.set_position(100, 0.0);
    s.set_sample_end(110);
    let b = read(&mut s, 1.0);
    assert_eq!(&b[..10], &ramp(110)[100..]);
    assert!(b[10..].iter().all(|&v| v == 0.0));
    assert!(!s.should_continue());
}

#[test]
fn channels_past_the_source_are_silent() {
    let mut s = Sampler::new(Source::Pcm(Arc::new(Pcm::new(RATE, vec![vec![0.5; 600], vec![-0.5; 600]]))));
    let mut out = blocks(3);
    out[2] = [9.0; BLOCK];
    s.read(1.0, &mut out);
    assert_eq!((out[0][0], out[1][0], out[2][0]), (0.5, -0.5, 0.0));
}

#[test]
fn streams_play_what_arrives_and_hold_on_underrun() {
    let st = Arc::new(Stream::new(1, RATE));
    st.write(&[16384; 300], 300, 1);
    let mut s = Sampler::new(Source::Stream(st.clone()));
    let b = read(&mut s, 1.0);
    assert!(b[..299].iter().all(|&v| v == 0.5), "plays the 300 frames it has, keeping one for interpolation");
    assert!(b[299..].iter().all(|&v| v == 0.0));
    assert!(s.should_continue(), "an open stream waits for more");

    st.write(&[-16384; 1000], 1000, 1);
    let b = read(&mut s, 1.0);
    assert_eq!((b[0], b[1]), (0.5, -0.5), "resumes exactly where it held");
    assert_eq!(s.position(), 299 + 512);

    st.close();
    read(&mut s, 1.0);
    assert!(!s.should_continue(), "closed and drained");
}
