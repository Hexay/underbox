//! Preprocessor-style integer expressions, as `#if` and vfx `ActiveIf( ... )` write them:
//! `( S_X == 1 ) && !defined( Y ) || F_Z >= 2`. C precedence; division by zero yields 0.

/// What names mean while evaluating.
pub(super) trait Scope {
    fn defined(&self, name: &str) -> bool;
    /// An identifier's value; [`super::active`] documents unknown names.
    fn value(&self, name: &str, depth: u32) -> i64;
}

#[derive(Clone, Debug, PartialEq)]
enum Tok<'a> {
    Num(i64),
    Ident(&'a str),
    Op(&'static str),
}

const OPS: [&str; 24] =
    ["&&", "||", "==", "!=", "<=", ">=", "<<", ">>", "!", "~", "-", "+", "*", "/", "%", "<", ">", "&", "^", "|", "(", ")", "?", ":"];

fn tokens(s: &str) -> Vec<Tok<'_>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let end = (i..b.len()).find(|&j| !b[j].is_ascii_alphanumeric()).unwrap_or(b.len());
            let lit = s[i..end].trim_end_matches(['u', 'U', 'l', 'L']);
            let v = match lit.strip_prefix("0x").or_else(|| lit.strip_prefix("0X")) {
                Some(hex) => i64::from_str_radix(hex, 16),
                None if lit.len() > 1 && lit.starts_with('0') => i64::from_str_radix(&lit[1..], 8),
                None => lit.parse(),
            };
            out.push(Tok::Num(v.unwrap_or(0)));
            i = end;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let end = (i..b.len()).find(|&j| !(b[j].is_ascii_alphanumeric() || b[j] == b'_')).unwrap_or(b.len());
            out.push(Tok::Ident(&s[i..end]));
            i = end;
        } else if let Some(op) = OPS.iter().find(|op| s[i..].starts_with(**op)) {
            out.push(Tok::Op(op));
            i += op.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Binary operators by precedence, loosest first (`?:` is handled above them).
const LEVELS: [&[&str]; 10] =
    [&["||"], &["&&"], &["|"], &["^"], &["&"], &["==", "!="], &["<", ">", "<=", ">="], &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];

struct Parser<'a, 's> {
    toks: Vec<Tok<'a>>,
    at: usize,
    scope: &'s dyn Scope,
    depth: u32,
}

impl<'a> Parser<'a, '_> {
    fn peek(&self) -> Option<&Tok<'a>> {
        self.toks.get(self.at)
    }

    fn eat(&mut self, op: &str) -> bool {
        let hit = matches!(self.peek(), Some(Tok::Op(o)) if *o == op);
        self.at += hit as usize;
        hit
    }

    fn ternary(&mut self) -> i64 {
        let c = self.binary(0);
        if !self.eat("?") {
            return c;
        }
        let a = self.ternary();
        self.eat(":");
        let b = self.ternary();
        if c != 0 { a } else { b }
    }

    fn binary(&mut self, level: usize) -> i64 {
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1);
        while let Some(&Tok::Op(op)) = self.peek() {
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.at += 1;
            let rhs = self.binary(level + 1);
            lhs = apply(op, lhs, rhs);
        }
        lhs
    }

    fn unary(&mut self) -> i64 {
        match self.peek().cloned() {
            Some(Tok::Op("!")) => self.skip_then(|p| (p.unary() == 0) as i64),
            Some(Tok::Op("~")) => self.skip_then(|p| !p.unary()),
            Some(Tok::Op("-")) => self.skip_then(|p| p.unary().wrapping_neg()),
            Some(Tok::Op("+")) => self.skip_then(Parser::unary),
            Some(Tok::Op("(")) => self.skip_then(|p| {
                let v = p.ternary();
                p.eat(")");
                v
            }),
            Some(Tok::Num(n)) => self.skip_then(|_| n),
            Some(Tok::Ident("defined")) => self.skip_then(|p| {
                let paren = p.eat("(");
                let name = match p.peek() {
                    Some(&Tok::Ident(n)) => n,
                    _ => "",
                };
                p.at += 1;
                if paren {
                    p.eat(")");
                }
                p.scope.defined(name) as i64
            }),
            Some(Tok::Ident(name)) => self.skip_then(|p| {
                if p.eat("(") {
                    p.skip_group();
                    return 0;
                }
                p.scope.value(name, p.depth)
            }),
            _ => {
                self.at += 1;
                0
            }
        }
    }

    fn skip_then(&mut self, f: impl FnOnce(&mut Self) -> i64) -> i64 {
        self.at += 1;
        f(self)
    }

    /// Past the `)` closing an already-eaten `(` (a function-like macro call: evaluates to 0).
    fn skip_group(&mut self) {
        let mut open = 1;
        while open > 0 && self.at < self.toks.len() {
            match self.toks[self.at] {
                Tok::Op("(") => open += 1,
                Tok::Op(")") => open -= 1,
                _ => {}
            }
            self.at += 1;
        }
    }
}

fn apply(op: &str, a: i64, b: i64) -> i64 {
    match op {
        "||" => (a != 0 || b != 0) as i64,
        "&&" => (a != 0 && b != 0) as i64,
        "|" => a | b,
        "^" => a ^ b,
        "&" => a & b,
        "==" => (a == b) as i64,
        "!=" => (a != b) as i64,
        "<" => (a < b) as i64,
        ">" => (a > b) as i64,
        "<=" => (a <= b) as i64,
        ">=" => (a >= b) as i64,
        "<<" => a.wrapping_shl(b as u32),
        ">>" => a.wrapping_shr(b as u32),
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" => a.checked_div(b).unwrap_or(0),
        _ => a.checked_rem(b).unwrap_or(0),
    }
}

/// `expr`'s value; `depth` is the macro-expansion depth it is evaluated at.
pub(super) fn eval(expr: &str, scope: &dyn Scope, depth: u32) -> i64 {
    Parser { toks: tokens(expr), at: 0, scope, depth }.ternary()
}
