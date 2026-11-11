//! Stateful value nodes (ease, spring): advanced once per update in node order, then read by
//! `Values` like any other node.

use super::value::Values;
use super::{Easing, ValueNode};
use std::f32::consts::{FRAC_PI_2, PI};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Smooth {
    started: bool,
    pub value: f32,
    velocity: f32,
    from: f32,
    to: f32,
    elapsed: f32,
}

pub fn ease(e: Easing, t: f32) -> f32 {
    use Easing::*;
    let t = t.clamp(0.0, 1.0);
    let in_out = |f: fn(f32) -> f32| if t < 0.5 { f(2.0 * t) / 2.0 } else { 1.0 - f(2.0 - 2.0 * t) / 2.0 };
    let out = |f: fn(f32) -> f32| 1.0 - f(1.0 - t);
    let expo = |t: f32| if t <= 0.0 { 0.0 } else { 2f32.powf(10.0 * t - 10.0) };
    let circ = |t: f32| 1.0 - (1.0 - t * t).max(0.0).sqrt();
    let sine = |t: f32| 1.0 - (t * FRAC_PI_2).cos();
    match e {
        Linear => t,
        None => 1.0,
        InQuad => t * t,
        OutQuad => out(|t| t * t),
        InOutQuad => in_out(|t| t * t),
        InCubic => t.powi(3),
        OutCubic => out(|t| t.powi(3)),
        InOutCubic => in_out(|t| t.powi(3)),
        InQuart => t.powi(4),
        OutQuart => out(|t| t.powi(4)),
        InOutQuart => in_out(|t| t.powi(4)),
        InQuint => t.powi(5),
        OutQuint => out(|t| t.powi(5)),
        InOutQuint => in_out(|t| t.powi(5)),
        InSine => sine(t),
        OutSine => (t * FRAC_PI_2).sin(),
        InOutSine => (1.0 - (PI * t).cos()) / 2.0,
        InExpo => expo(t),
        OutExpo => out(expo),
        InOutExpo => in_out(expo),
        InCirc => circ(t),
        OutCirc => out(circ),
        InOutCirc => in_out(circ),
    }
}

/// Implicit-Euler damped spring step (stable at any `dt`).
fn spring(s: &mut Smooth, target: f32, hertz: f32, damping: f32, dt: f32) {
    let omega = 2.0 * PI * hertz.max(0.0);
    let f = 1.0 + 2.0 * dt * damping.max(0.0) * omega;
    let hoo = dt * omega * omega;
    let inv = 1.0 / (f + dt * hoo);
    let x = (f * s.value + dt * s.velocity + dt * hoo * target) * inv;
    s.velocity = (s.velocity + hoo * (target - s.value)) * inv;
    s.value = x;
}

/// Advance every ease/spring in `states` (one per value node) by `dt`.
pub(super) fn advance(v: &Values, states: &mut [Smooth], dt: f32) {
    for (id, node) in v.def.values.iter().enumerate() {
        let (input, start) = match node {
            ValueNode::Ease { input, start, .. } | ValueNode::Spring { input, start, .. } => (*input, *start),
            _ => continue,
        };
        let target = Values { smooth: states, ..*v }.float(input);
        let s = &mut states[id];
        if !s.started {
            let first = start.unwrap_or(target);
            *s = Smooth { started: true, value: first, from: first, to: first, ..Smooth::default() };
        }
        match *node {
            ValueNode::Ease { time, easing, .. } => {
                if target != s.to {
                    (s.from, s.to, s.elapsed) = (s.value, target, 0.0);
                }
                s.elapsed += dt;
                let t = if time > 0.0 { s.elapsed / time } else { 1.0 };
                s.value = s.from + (s.to - s.from) * ease(easing, t);
            }
            ValueNode::Spring { hertz, damping, .. } => spring(s, target, hertz, damping, dt),
            _ => {}
        }
    }
}
