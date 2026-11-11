//! vfx expressions: `RenderState` values (`D_X == 0 ? GREATER_EQUAL : LESS_EQUAL`) and the `#if`
//! guards around declarations ([`Guards`]), evaluated against a variant's defines with C
//! preprocessor rules: a name that isn't defined is a [`Value::Sym`], which counts as 0.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    /// An undefined name: an enum token such as `SRC_ALPHA`.
    Sym(String),
}

impl Value {
    pub fn int(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            Value::Sym(_) => 0,
        }
    }

    pub fn truthy(&self) -> bool {
        self.int() != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Name(String),
    Op(&'static str),
}

const OPS: [&str; 22] = ["&&", "||", "==", "!=", "<=", ">=", "<<", ">>", "?", ":", "(", ")", "!", "~", "<", ">", "+", "-", "*", "/", "%", ","];

fn lex(s: &str) -> Option<Vec<Tok>> {
    let b = s.as_bytes();
    let (mut i, mut out) = (0, Vec::new());
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let end = (i..b.len()).find(|&j| !b[j].is_ascii_alphanumeric()).unwrap_or(b.len());
            let t = s[i..end].trim_end_matches(['u', 'U', 'l', 'L']);
            let v = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).map_or_else(|| t.parse().ok(), |h| i64::from_str_radix(h, 16).ok())?;
            out.push(Tok::Num(v));
            i = end;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let end = (i..b.len()).find(|&j| !(b[j].is_ascii_alphanumeric() || b[j] == b'_')).unwrap_or(b.len());
            out.push(match &s[i..end] {
                n if n.eq_ignore_ascii_case("true") => Tok::Num(1),
                n if n.eq_ignore_ascii_case("false") => Tok::Num(0),
                n => Tok::Name(n.into()),
            });
            i = end;
        } else {
            let op = OPS.iter().find(|op| s[i..].starts_with(**op))?;
            out.push(Tok::Op(op));
            i += op.len();
        }
    }
    Some(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    at: usize,
    value: &'a dyn Fn(&str) -> Option<i64>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn eat(&mut self, op: &str) -> bool {
        let hit = matches!(self.peek(), Some(Tok::Op(o)) if *o == op);
        self.at += usize::from(hit);
        hit
    }

    fn ternary(&mut self) -> Option<Value> {
        let cond = self.binary(0)?;
        if !self.eat("?") {
            return Some(cond);
        }
        let a = self.ternary()?;
        self.eat(":").then_some(())?;
        let b = self.ternary()?;
        Some(if cond.truthy() { a } else { b })
    }

    /// Binary operators by C precedence, lowest first.
    fn binary(&mut self, level: usize) -> Option<Value> {
        const LEVELS: [&[&str]; 7] = [&["||"], &["&&"], &["==", "!="], &["<", ">", "<=", ">="], &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];
        let Some(ops) = LEVELS.get(level) else { return self.unary() };
        let mut left = self.binary(level + 1)?;
        while let Some(op) = ops.iter().find(|op| matches!(self.peek(), Some(Tok::Op(o)) if o == *op)) {
            self.at += 1;
            let right = self.binary(level + 1)?;
            left = apply(op, &left, &right)?;
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<Value> {
        for op in ["!", "-", "~", "+"] {
            if self.eat(op) {
                let v = self.unary()?.int();
                return Some(Value::Int(match op {
                    "!" => i64::from(v == 0),
                    "-" => -v,
                    "~" => !v,
                    _ => v,
                }));
            }
        }
        match self.toks.get(self.at).cloned()? {
            Tok::Num(v) => {
                self.at += 1;
                Some(Value::Int(v))
            }
            Tok::Name(n) if n == "defined" => {
                self.at += 1;
                let paren = self.eat("(");
                let Some(Tok::Name(name)) = self.toks.get(self.at).cloned() else { return None };
                self.at += 1;
                (!paren || self.eat(")")).then_some(())?;
                Some(Value::Int(i64::from((self.value)(&name).is_some())))
            }
            Tok::Name(n) => {
                self.at += 1;
                Some((self.value)(&n).map_or(Value::Sym(n), Value::Int))
            }
            Tok::Op("(") => {
                self.at += 1;
                let v = self.ternary()?;
                self.eat(")").then_some(v)
            }
            Tok::Op(_) => None,
        }
    }
}

fn apply(op: &str, l: &Value, r: &Value) -> Option<Value> {
    if let (Value::Sym(a), Value::Sym(b)) = (l, r)
        && matches!(op, "==" | "!=")
    {
        return Some(Value::Int(i64::from((a == b) == (op == "=="))));
    }
    let (a, b) = (l.int(), r.int());
    Some(Value::Int(match op {
        "||" => i64::from(a != 0 || b != 0),
        "&&" => i64::from(a != 0 && b != 0),
        "==" => i64::from(a == b),
        "!=" => i64::from(a != b),
        "<" => i64::from(a < b),
        ">" => i64::from(a > b),
        "<=" => i64::from(a <= b),
        ">=" => i64::from(a >= b),
        "<<" => a.checked_shl(u32::try_from(b).ok()?)?,
        ">>" => a.checked_shr(u32::try_from(b).ok()?)?,
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" => a.checked_div(b)?,
        _ => a.checked_rem(b)?,
    }))
}

/// `expr` with names `value` defines; `None` when it doesn't parse (or divides by zero).
pub fn eval(expr: &str, value: &dyn Fn(&str) -> Option<i64>) -> Option<Value> {
    let mut p = Parser { toks: lex(expr)?, at: 0, value };
    let v = p.ternary()?;
    (p.at == p.toks.len()).then_some(v)
}

/// The `#if`/`#ifdef`/`#elif`/`#else` conditions in force at each point of a text, as one
/// expression for [`eval`] (empty: unconditional). Integer `#define`s seen anywhere in the text
/// are substituted, so `D_X == OUTLINE_INSIDE` doesn't depend on them later.
pub struct Guards {
    /// (offset of the first line it covers, guard), in offset order.
    spans: Vec<(usize, String)>,
}

struct Frame {
    taken: Vec<String>,
    current: String,
}

impl Guards {
    pub fn new(text: &str) -> Guards {
        let macros = macros(text);
        let sub = |e: &str| substitute(e, &macros);
        let (mut stack, mut spans, mut offset) = (Vec::<Frame>::new(), vec![(0, String::new())], 0);
        for line in text.split_inclusive('\n') {
            let next = offset + line.len();
            offset = next;
            let Some(d) = line.trim_start().strip_prefix('#') else { continue };
            let d = d.trim_start();
            let (word, rest) = d.split_at(d.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(d.len()));
            let rest = rest.trim();
            let either = |taken: &[String]| format!("!(({}))", taken.join(") || ("));
            match word {
                "if" => stack.push(Frame { taken: Vec::new(), current: sub(rest) }),
                "ifdef" => stack.push(Frame { taken: Vec::new(), current: format!("defined( {rest} )") }),
                "ifndef" => stack.push(Frame { taken: Vec::new(), current: format!("!defined( {rest} )") }),
                "elif" | "else" => {
                    let Some(f) = stack.last_mut() else { continue };
                    f.taken.push(std::mem::take(&mut f.current));
                    f.current = if word == "elif" { format!("{} && ({})", either(&f.taken), sub(rest)) } else { either(&f.taken) };
                }
                "endif" => {
                    stack.pop();
                }
                _ => continue,
            }
            let guard = stack.iter().map(|f| format!("({})", f.current)).collect::<Vec<_>>().join(" && ");
            spans.push((next, guard));
        }
        Guards { spans }
    }

    pub fn at(&self, offset: usize) -> &str {
        let i = self.spans.partition_point(|(start, _)| *start <= offset);
        &self.spans[i.saturating_sub(1)].1
    }
}

fn macros(text: &str) -> HashMap<String, i64> {
    text.lines()
        .filter_map(|l| l.trim_start().strip_prefix('#')?.trim_start().strip_prefix("define"))
        .filter_map(|rest| {
            let mut parts = rest.split_whitespace();
            let (name, value) = (parts.next()?, parts.next()?);
            let value = eval(value, &|_| None).filter(|_| parts.next().is_none())?;
            matches!(value, Value::Int(_)).then(|| (name.to_owned(), value.int()))
        })
        .collect()
}

fn substitute(expr: &str, macros: &HashMap<String, i64>) -> String {
    let Some(toks) = lex(expr) else { return expr.into() };
    let mut out = Vec::with_capacity(toks.len());
    for (i, t) in toks.iter().enumerate() {
        let after_defined = i > 0 && matches!(&toks[i - 1], Tok::Name(n) if n == "defined") || i > 1 && matches!(&toks[i - 2], Tok::Name(n) if n == "defined");
        out.push(match t {
            Tok::Name(n) if !after_defined && macros.contains_key(n) => macros[n].to_string(),
            Tok::Name(n) => n.clone(),
            Tok::Num(v) => v.to_string(),
            Tok::Op(o) => (*o).to_string(),
        });
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<i64> {
        [("D_DOF_TYPE", 0), ("S_MODE_DEPTH", 1), ("F_RENDER_BACKFACES", 1)].iter().find(|(n, _)| *n == name).map(|v| v.1)
    }

    #[test]
    fn values_and_operators() {
        let e = |s: &str| eval(s, &env);
        assert_eq!(e("D_DOF_TYPE == 0 ? GREATER_EQUAL : LESS_EQUAL"), Some(Value::Sym("GREATER_EQUAL".into())));
        assert_eq!(e("F_RENDER_BACKFACES ? NONE : DEFAULT"), Some(Value::Sym("NONE".into())));
        assert_eq!(e("!S_TRANSLUCENT && !S_SOURCE_IGNORE_Z"), Some(Value::Int(1)), "undefined names are 0");
        assert_eq!(e("D_OPAQUE || S_MODE_DEPTH == 1"), Some(Value::Int(1)));
        assert_eq!(e("S_MODE_DEPTH != 0"), Some(Value::Int(1)));
        assert_eq!(e("D_DOF_TYPE == 0 && D_DEBUG_COC == 0 ? true : false"), Some(Value::Int(1)));
        assert_eq!(e("(1 + 2) * 3 - 8 / 2 % 3"), Some(Value::Int(8)));
        assert_eq!(e("defined( S_MODE_DEPTH ) && !defined X"), Some(Value::Int(1)));
        assert_eq!(e("254"), Some(Value::Int(254)));
        assert_eq!(e("1 +"), None);
        assert_eq!(e("1 / 0"), None);
    }

    #[test]
    fn guards_follow_branches_and_macros() {
        let src = "#define OUTLINE_INSIDE 2\n@a\n#if D_BLEND\n@b\n#elif D_X == OUTLINE_INSIDE\n@c\n#else\n@d\n#endif\n#ifndef H\n@e\n#endif\n@f\n";
        let g = Guards::new(src);
        let at = |w: &str| g.at(src.find(w).unwrap()).to_owned();
        let holds = |w: &str, vals: &[(&str, i64)]| eval(&at(w), &|n| vals.iter().find(|v| v.0 == n).map(|v| v.1)).unwrap().truthy();
        assert_eq!(at("@a"), "");
        assert_eq!(at("@f"), "");
        assert!(holds("@b", &[("D_BLEND", 1)]) && !holds("@b", &[]));
        assert!(holds("@c", &[("D_X", 2)]) && !holds("@c", &[("D_X", 2), ("D_BLEND", 1)]));
        assert!(holds("@d", &[]) && !holds("@d", &[("D_X", 2)]));
        assert!(holds("@e", &[]) && !holds("@e", &[("H", 1)]));
    }
}
