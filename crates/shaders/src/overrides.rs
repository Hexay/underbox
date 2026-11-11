//! A shader's own function wins over our built-in one with the same signature. The built-ins
//! guess at Valve's closed headers (`builtin`), so a game shader that natively compiled while
//! defining, say, `float3 SrgbGammaToLinear( float3 )` proves native's header has no such
//! overload; DXC would call ours a redefinition. The built-in definition becomes a prototype,
//! so built-in and core code calling it before the shader's definition still resolve.

use crate::scan::{blank_bytes, blank_comments, ident_at, is_ident, match_close, skip_trivia, split_args};

/// A top-level function definition: its signature, where it starts and where its body ends.
struct Def {
    signature: String,
    start: usize,
    brace: usize,
    body_end: usize,
}

pub fn rewrite(src: &str) -> String {
    let code = code_only(src);
    let regions = builtin_regions(src);
    let in_builtin = |i: usize| regions.iter().rev().find(|(at, _)| *at <= i).is_some_and(|(_, b)| *b);
    let defs = definitions(&code);
    let own: Vec<&str> = defs.iter().filter(|d| !in_builtin(d.start)).map(|d| d.signature.as_str()).collect();
    let shadowed: Vec<&Def> = defs.iter().filter(|d| in_builtin(d.start) && own.contains(&d.signature.as_str())).collect();
    if shadowed.is_empty() {
        return src.into();
    }
    let mut out = src.as_bytes().to_vec();
    for d in shadowed {
        out[d.brace] = b';';
        blank_bytes(&mut out[d.brace + 1..=d.body_end]);
    }
    String::from_utf8(out).expect("only ASCII bytes were replaced")
}

/// `src` with comments and preprocessor lines blanked, offsets kept.
fn code_only(src: &str) -> String {
    let mut b = blank_comments(src).into_bytes();
    let mut start = 0;
    while start < b.len() {
        let end = b[start..].iter().position(|&c| c == b'\n').map_or(b.len(), |n| start + n);
        if b[start..end].iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'#') {
            blank_bytes(&mut b[start..end]);
        }
        start = end + 1;
    }
    String::from_utf8(b).expect("blanking keeps ASCII")
}

/// `(offset, inside a built-in)` at each `#line` marker the include expander wrote.
fn builtin_regions(src: &str) -> Vec<(usize, bool)> {
    let mut at = 0;
    let mut out = Vec::new();
    for line in src.split_inclusive('\n') {
        if let Some(rest) = line.trim_start().strip_prefix("#line") {
            out.push((at, rest.contains(&format!("\"{}/", crate::builtin::PREFIX))));
        }
        at += line.len();
    }
    out
}

fn definitions(code: &str) -> Vec<Def> {
    let b = code.as_bytes();
    let (mut out, mut i, mut depth) = (Vec::new(), 0, 0usize);
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'(' if depth == 0 => {
                if let Some(d) = definition(code, i) {
                    i = d.body_end + 1;
                    out.push(d);
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// `type name( params ) {` with `(` at `open`.
fn definition(code: &str, open: usize) -> Option<Def> {
    let (name, name_start) = ident_before(code, open)?;
    let (_, start) = ident_before(code, name_start)?;
    let params_end = match_close(code, open, false)?;
    let brace = skip_trivia(code, params_end + 1);
    (code.as_bytes().get(brace) == Some(&b'{')).then_some(())?;
    let body_end = match_close(code, brace, false)?;
    let types: Vec<String> = split_args(&code[open + 1..params_end]).iter().map(|p| param_type(p)).collect();
    Some(Def { signature: format!("{name}({})", types.join(",")), start, brace, body_end })
}

/// The identifier ending just before `i` (whitespace allowed) and its start.
fn ident_before(code: &str, i: usize) -> Option<(&str, usize)> {
    let b = code.as_bytes();
    let end = (0..i).rev().find(|&j| !b[j].is_ascii_whitespace())? + 1;
    let start = (0..end).rev().find(|&j| !is_ident(b[j])).map_or(0, |j| j + 1);
    ident_at(code, start).filter(|id| start + id.len() == end).map(|id| (id, start))
}

/// A parameter's type without qualifiers, name, semantic or default.
fn param_type(p: &str) -> String {
    // `out`/`inout` stay: they take part in overloading.
    const QUALIFIERS: [&str; 4] = ["in", "const", "uniform", "precise"];
    let p = p.split([':', '=']).next().unwrap_or_default();
    let words: Vec<&str> = p.split_whitespace().filter(|w| !QUALIFIERS.contains(w)).collect();
    words[..words.len().saturating_sub(1)].join(" ")
}

#[cfg(test)]
#[path = "overrides_tests.rs"]
mod tests;
