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
    /// Controller n's negative half as 0..1.
    TwoWay0(usize),
    /// Controller n's positive half as 0..1.
    TwoWay1(usize),
    /// Controller n's value, weighted by a 4-point ramp over a popped controller.
    NWay(usize),
    /// DME eyelids; n is the lid split controller.
    LowerEyelid(usize),
    UpperEyelid(usize),
    /// Pops value, then the in and out ranges; pushes the clamped remap.
    Remap,
    /// An op code we don't know; its rule yields 0.
    Unsupported(i64),
}

mod eval;
pub use eval::{evaluate, remap_clamped};

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
        15 => Op::TwoWay0(n),
        16 => Op::TwoWay1(n),
        17 => Op::NWay(n),
        18 => Op::Combo(n),
        19 => Op::Dominate(n),
        20 => Op::LowerEyelid(n),
        21 => Op::UpperEyelid(n),
        22 => Op::Sqrt,
        23 => Op::Remap,
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
        Op::TwoWay0(n) => ("2WAY_0", n as i64),
        Op::TwoWay1(n) => ("2WAY_1", n as i64),
        Op::NWay(n) => ("NWAY", n as i64),
        Op::LowerEyelid(n) => ("DME_LOWER_EYELID", n as i64),
        Op::UpperEyelid(n) => ("DME_UPPER_EYELID", n as i64),
        Op::Remap => ("REMAPVALCLAMPED", 0),
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
        let unsupported = [Rule { flex: 0, ops: vec![Op::Const(1.0), Op::Unsupported(99)] }];
        assert_eq!(evaluate(&unsupported, &[], &["x".to_owned()], &[]), [0.0]);
    }

    #[test]
    fn every_op_code_parses_and_round_trips() {
        let ops = vec![Op::TwoWay0(1), Op::TwoWay1(2), Op::NWay(3), Op::LowerEyelid(4), Op::UpperEyelid(5), Op::Remap];
        let rules = vec![Rule { flex: 0, ops }];
        assert_eq!(super::rules(&Object(vec![("m_FlexRules".into(), write_rules(&rules))])), rules);
        assert_eq!(op(17, 3), Op::NWay(3));
    }
}
