//! Flex controllers and rules (a morph set's `m_FlexControllers`, `m_FlexRules`): what managed
//! and animations drive by name, and the stack programs that turn controller values into each
//! flex descriptor's (morph target's) weight.

use super::kv::{self, float, int, int_value, list, obj, objects, string, text};
use ::kv::{Object, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Controller {
    pub name: String,
    pub kind: String,
    pub min: f32,
    pub max: f32,
}

/// `FLEX_OP_*` codes (VRF `FlexOpCode`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Const(f32),
    /// Push controller n's value.
    Fetch1(usize),
    /// Push flex descriptor n's value from an earlier rule.
    Fetch2(usize),
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Max,
    Min,
    /// Product of the top n.
    Combo(usize),
    /// The value under the top n, times 1 - their product.
    Dominate(usize),
    Sqrt,
    Abs,
    Sin,
    Cos,
    Nop,
    /// An op we don't evaluate yet (2-way, n-way, eyelids, remap); its rule yields 0.
    Unsupported(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// The flex descriptor it sets.
    pub flex: usize,
    pub ops: Vec<Op>,
}

const NAMES: [&str; 26] = [
    "CONST", "FETCH1", "FETCH2", "ADD", "SUB", "MUL", "DIV", "NEG", "EXP", "OPEN", "CLOSE", "COMMA", "MAX", "MIN", "2WAY_0", "2WAY_1", "NWAY",
    "COMBO", "DOMINATE", "DME_LOWER_EYELID", "DME_UPPER_EYELID", "SQRT", "REMAPVALCLAMPED", "SIN", "COS", "ABS",
];

/// `data` is float bits for the arithmetic ops' constants, a plain integer for indices and counts.
fn op(code: i64, data: i64) -> Op {
    let n = data.max(0) as usize;
    match code {
        1 => Op::Const(f32::from_bits(data as u32)),
        2 => Op::Fetch1(n),
        3 => Op::Fetch2(n),
        4 => Op::Add,
        5 => Op::Sub,
        6 => Op::Mul,
        7 => Op::Div,
        8 => Op::Neg,
        9..=12 => Op::Nop,
        13 => Op::Max,
        14 => Op::Min,
        18 => Op::Combo(n),
        19 => Op::Dominate(n),
        22 => Op::Sqrt,
        24 => Op::Sin,
        25 => Op::Cos,
        26 => Op::Abs,
        c => Op::Unsupported(c),
    }
}

fn code(o: &Object) -> i64 {
    match o.get("m_OpCode") {
        Some(Value::String(s)) => s.strip_prefix("FLEX_OP_").and_then(|s| NAMES.iter().position(|n| *n == s)).map_or(0, |i| i as i64 + 1),
        Some(v) => kv::int_of(v).unwrap_or(0),
        None => 0,
    }
}

pub fn controllers(o: &Object) -> Vec<Controller> {
    objects(o, "m_FlexControllers").map(|c| Controller { name: string(c, "m_szName"), kind: string(c, "m_szType"), min: float(c, "min"), max: float(c, "max") }).collect()
}

pub fn rules(o: &Object) -> Vec<Rule> {
    objects(o, "m_FlexRules")
        .map(|r| Rule { flex: int(r, "m_nFlex").max(0) as usize, ops: objects(r, "m_FlexOps").map(|x| op(code(x), int(x, "m_Data"))).collect() })
        .collect()
}

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
        let value = run(&r.ops, values, &out);
        if let Some(slot) = out.get_mut(r.flex) {
            *slot = value;
        }
    }
    out
}

fn run(ops: &[Op], values: &[f32], flex: &[f32]) -> f32 {
    let mut stack: Vec<f32> = Vec::new();
    let pop = |s: &mut Vec<f32>| s.pop().unwrap_or(0.0);
    for &o in ops {
        let v = match o {
            Op::Const(c) => c,
            Op::Fetch1(i) => values.get(i).copied().unwrap_or(0.0),
            Op::Fetch2(i) => flex.get(i).copied().unwrap_or(0.0),
            Op::Neg | Op::Sqrt | Op::Abs | Op::Sin | Op::Cos => {
                let a = pop(&mut stack);
                match o {
                    Op::Neg => -a,
                    Op::Sqrt => a.max(0.0).sqrt(),
                    Op::Abs => a.abs(),
                    Op::Sin => a.sin(),
                    _ => a.cos(),
                }
            }
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Max | Op::Min => {
                let (b, a) = (pop(&mut stack), pop(&mut stack));
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
            Op::Combo(n) => (0..n).map(|_| pop(&mut stack)).product(),
            Op::Dominate(n) => {
                let d: f32 = (0..n).map(|_| pop(&mut stack)).product();
                pop(&mut stack) * (1.0 - d)
            }
            Op::Nop => continue,
            Op::Unsupported(_) => return 0.0,
        };
        stack.push(v);
    }
    stack.pop().unwrap_or(0.0)
}

pub fn write_controllers(c: &[Controller]) -> Value {
    list(c.iter().map(|c| obj(vec![("m_szName", text(&c.name)), ("m_szType", text(&c.kind)), ("min", Value::Double(c.min.into())), ("max", Value::Double(c.max.into()))])))
}

fn write_op(o: Op) -> Value {
    let (code, data): (&str, i64) = match o {
        Op::Const(c) => ("CONST", i64::from(c.to_bits())),
        Op::Fetch1(i) => ("FETCH1", i as i64),
        Op::Fetch2(i) => ("FETCH2", i as i64),
        Op::Add => ("ADD", 0),
        Op::Sub => ("SUB", 0),
        Op::Mul => ("MUL", 0),
        Op::Div => ("DIV", 0),
        Op::Neg => ("NEG", 0),
        Op::Max => ("MAX", 0),
        Op::Min => ("MIN", 0),
        Op::Combo(n) => ("COMBO", n as i64),
        Op::Dominate(n) => ("DOMINATE", n as i64),
        Op::Sqrt => ("SQRT", 0),
        Op::Abs => ("ABS", 0),
        Op::Sin => ("SIN", 0),
        Op::Cos => ("COS", 0),
        Op::Nop => ("COMMA", 0),
        Op::Unsupported(c) => return obj(vec![("m_OpCode", int_value(c)), ("m_Data", int_value(0))]),
    };
    obj(vec![("m_OpCode", text(&format!("FLEX_OP_{code}"))), ("m_Data", int_value(data))])
}

pub fn write_rules(r: &[Rule]) -> Value {
    list(r.iter().map(|r| obj(vec![("m_nFlex", int_value(r.flex as i64)), ("m_FlexOps", list(r.ops.iter().map(|&o| write_op(o))))])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str) -> Controller {
        Controller { name: name.into(), kind: "default".into(), min: 0.0, max: 1.0 }
    }

    #[test]
    fn rules_run_as_stack_programs_and_round_trip() {
        let controllers = [c("smile"), c("open")];
        let descs = ["smile".to_owned(), "smile_open".to_owned(), "neutral".to_owned()];
        let rules = vec![
            Rule { flex: 0, ops: vec![Op::Fetch1(0)] },
            Rule { flex: 1, ops: vec![Op::Fetch1(0), Op::Fetch1(1), Op::Combo(2)] },
            Rule { flex: 2, ops: vec![Op::Const(1.0), Op::Fetch2(0), Op::Sub, Op::Const(0.0), Op::Max] },
        ];
        assert_eq!(evaluate(&rules, &controllers, &descs, &[0.5, 0.5]), [0.5, 0.25, 0.5]);
        let back = super::rules(&Object(vec![("m_FlexRules".into(), write_rules(&rules))]));
        assert_eq!(back, rules);
        assert_eq!(super::controllers(&Object(vec![("m_FlexControllers".into(), write_controllers(&controllers))])), controllers);
    }

    #[test]
    fn without_rules_descriptors_follow_controllers_by_name() {
        assert_eq!(evaluate(&[], &[c("blink")], &["BLINK".to_owned(), "other".to_owned()], &[0.75]), [0.75, 0.0]);
        let unsupported = [Rule { flex: 0, ops: vec![Op::Const(1.0), Op::Unsupported(17)] }];
        assert_eq!(evaluate(&unsupported, &[], &["x".to_owned()], &[]), [0.0]);
    }
}
