//! Running flex rules: each rule's stack program over controller values, in order, so later
//! rules can fetch earlier descriptors. Op semantics follow VRF's `FlexOps` (Source's studio
//! flex ops); an index popped off the stack is the float's bit pattern.

use super::{Controller, Op, Rule};

/// Each of `descriptors` flex weights for `values` (one per controller). Without rules a
/// descriptor follows the controller of the same name.
pub fn evaluate(rules: &[Rule], controllers: &[Controller], descriptors: &[String], values: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0f32; descriptors.len()];
    if rules.is_empty() {
        for (d, name) in descriptors.iter().enumerate() {
            out[d] = controllers.iter().position(|c| c.name.eq_ignore_ascii_case(name)).and_then(|c| values.get(c)).copied().unwrap_or(0.0);
        }
        return out;
    }
    for r in rules {
        let value = Ctx { controllers, values, flex: &out, stack: Vec::new() }.run(&r.ops);
        if let Some(slot) = out.get_mut(r.flex) {
            *slot = value;
        }
    }
    out
}

/// `value` from `[a, b]` to `[c, d]`, clamped to the output range.
pub fn remap_clamped(value: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    if a == b {
        return if value >= b { d } else { c };
    }
    c + (d - c) * ((value - a) / (b - a)).clamp(0.0, 1.0)
}

struct Ctx<'a> {
    controllers: &'a [Controller],
    values: &'a [f32],
    flex: &'a [f32],
    stack: Vec<f32>,
}

impl Ctx<'_> {
    fn pop(&mut self) -> f32 {
        self.stack.pop().unwrap_or(0.0)
    }

    fn pop_index(&mut self) -> i64 {
        i64::from(self.pop().to_bits() as i32)
    }

    fn value(&self, i: i64) -> f32 {
        usize::try_from(i).ok().and_then(|i| self.values.get(i)).copied().unwrap_or(0.0)
    }

    /// Controller `i`'s value from its own range into `[min, max]`; a negative index reads 0.
    fn remapped(&self, i: i64, min: f32, max: f32) -> f32 {
        let Some(c) = usize::try_from(i).ok().and_then(|i| self.controllers.get(i)) else { return if i < 0 { 0.0 } else { self.value(i) } };
        remap_clamped(self.value(i), c.min, c.max, min, max)
    }

    /// (closed, lid-V weight, gaze) of a DME eyelid op: pops lid, blink, then gaze controllers.
    fn eyelid(&mut self, lid_v: usize) -> (f32, f32, f32) {
        let lid_v = self.remapped(lid_v as i64, 0.0, 1.0);
        let lid = self.pop_index();
        let lid = self.remapped(lid, 0.0, 1.0);
        let (blink, gaze) = (self.pop_index(), self.pop_index());
        let (blink, gaze) = (self.remapped(blink, 0.0, 1.0), self.remapped(gaze, -1.0, 1.0));
        (blink.max(lid), lid_v, gaze)
    }

    fn run(mut self, ops: &[Op]) -> f32 {
        for &o in ops {
            let v = match o {
                Op::Const(c) => c,
                Op::Fetch1(i) => self.value(i as i64),
                Op::Fetch2(i) => self.flex.get(i).copied().unwrap_or(0.0),
                Op::Neg | Op::Sqrt | Op::Abs | Op::Sin | Op::Cos => {
                    let a = self.pop();
                    match o {
                        Op::Neg => -a,
                        Op::Sqrt => a.max(0.0).sqrt(),
                        Op::Abs => a.abs(),
                        Op::Sin => a.sin(),
                        _ => a.cos(),
                    }
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Max | Op::Min => {
                    let (b, a) = (self.pop(), self.pop());
                    match o {
                        Op::Add => a + b,
                        Op::Sub => a - b,
                        Op::Mul => a * b,
                        Op::Div if b.abs() <= 1e-4 => 0.0,
                        Op::Div => a / b,
                        Op::Max => a.max(b),
                        _ => a.min(b),
                    }
                }
                Op::Combo(n) => (0..n).map(|_| self.pop()).product(),
                Op::Dominate(n) => {
                    let d: f32 = (0..n).map(|_| self.pop()).product();
                    self.pop() * (1.0 - d)
                }
                Op::TwoWay0(c) => remap_clamped(self.value(c as i64), -1.0, 0.0, 1.0, 0.0),
                Op::TwoWay1(c) => remap_clamped(self.value(c as i64), 0.0, 1.0, 0.0, 1.0),
                Op::NWay(value) => {
                    let t = self.pop_index();
                    let t = self.value(t);
                    let (t4, t3, t2, t1) = (self.pop(), self.pop(), self.pop(), self.pop());
                    let v = self.value(value as i64);
                    if t < t1 || t >= t4 {
                        0.0
                    } else if t < t2 {
                        remap_clamped(t, t1, t2, 0.0, v)
                    } else if t < t3 {
                        v
                    } else {
                        remap_clamped(t, t3, t4, v, 0.0)
                    }
                }
                Op::LowerEyelid(c) => {
                    let (closed, lid_v, gaze) = self.eyelid(c);
                    closed * (1.0 - lid_v) * if gaze > 0.0 { 1.0 - gaze } else { 1.0 }
                }
                Op::UpperEyelid(c) => {
                    let (closed, lid_v, gaze) = self.eyelid(c);
                    closed * lid_v * if gaze < 0.0 { 1.0 + gaze } else { 1.0 }
                }
                Op::Remap => {
                    let (d, c, b, a, value) = (self.pop(), self.pop(), self.pop(), self.pop(), self.pop());
                    remap_clamped(value, a, b, c, d)
                }
                Op::Nop => continue,
                Op::Unsupported(_) => return 0.0,
            };
            self.stack.push(v);
        }
        self.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, min: f32, max: f32) -> Controller {
        Controller { name: name.into(), kind: "default".into(), min, max }
    }

    fn one(ops: Vec<Op>, controllers: &[Controller], values: &[f32]) -> f32 {
        evaluate(&[Rule { flex: 0, ops }], controllers, &["x".to_owned()], values)[0]
    }

    fn index(i: i32) -> Op {
        Op::Const(f32::from_bits(i as u32))
    }

    #[test]
    fn two_way_splits_a_signed_controller() {
        let cs = [c("lr", -1.0, 1.0)];
        assert_eq!(one(vec![Op::TwoWay0(0)], &cs, &[-0.25]), 0.25);
        assert_eq!(one(vec![Op::TwoWay1(0)], &cs, &[-0.25]), 0.0);
        assert_eq!(one(vec![Op::TwoWay1(0)], &cs, &[0.75]), 0.75);
        assert_eq!(one(vec![Op::TwoWay0(0)], &cs, &[0.75]), 0.0);
    }

    #[test]
    fn n_way_ramps_up_holds_and_ramps_down() {
        let cs = [c("multi", 0.0, 1.0), c("value", 0.0, 1.0)];
        let ops = |t: f32| (vec![Op::Const(0.0), Op::Const(0.25), Op::Const(0.5), Op::Const(0.75), index(0), Op::NWay(1)], [t, 0.8]);
        for (t, want) in [(0.0, 0.0), (0.125, 0.4), (0.3, 0.8), (0.625, 0.4), (0.9, 0.0)] {
            let (o, v) = ops(t);
            assert!((one(o, &cs, &v) - want).abs() < 1e-6, "t {t}");
        }
    }

    #[test]
    fn eyelids_close_with_blink_and_follow_gaze() {
        // Controllers: 0 lid-V split, 1 close lid, 2 blink, 3 eyes up/down.
        let cs = [c("v", 0.0, 1.0), c("lid", 0.0, 1.0), c("blink", 0.0, 1.0), c("ud", -1.0, 1.0)];
        let prog = |op: Op| vec![index(3), index(2), index(1), op];
        let vals = [0.25, 0.2, 1.0, 0.0];
        assert_eq!(one(prog(Op::UpperEyelid(0)), &cs, &vals), 0.25);
        assert_eq!(one(prog(Op::LowerEyelid(0)), &cs, &vals), 0.75);
        // Looking down (-0.5) halves the upper lid; looking up (0.5) halves the lower.
        assert_eq!(one(prog(Op::UpperEyelid(0)), &cs, &[0.25, 0.2, 1.0, -0.5]), 0.125);
        assert_eq!(one(prog(Op::LowerEyelid(0)), &cs, &[0.25, 0.2, 1.0, 0.5]), 0.375);
        // An unconnected blink (-1) leaves the lid controller.
        assert_eq!(one(vec![index(-1), index(-1), index(1), Op::UpperEyelid(0)], &cs, &[1.0, 0.5, 1.0, 0.0]), 0.5);
    }

    #[test]
    fn remap_clamps_into_the_output_range() {
        let ops = |v: f32| vec![Op::Const(v), Op::Const(0.0), Op::Const(2.0), Op::Const(10.0), Op::Const(20.0), Op::Remap];
        assert_eq!(one(ops(1.0), &[], &[]), 15.0);
        assert_eq!(one(ops(5.0), &[], &[]), 20.0);
        assert_eq!(one(ops(-1.0), &[], &[]), 10.0);
    }
}
