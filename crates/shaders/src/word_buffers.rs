//! `RWStructuredBuffer<S>` whose struct fields are `Interlocked*` targets → a
//! `RWStructuredBuffer<uint>` of S's words. naga's SPIR-V front end upgrades a plain `uint`
//! element to atomic but not a struct member (occlusion_cull_cs's `OcclusionArgs`). S must be all
//! 4-byte scalars, so the bytes, the binding and managed's buffer stay as they were. Accesses:
//! - `B[i].f` → `B[(i) * N + k]`, wrapped in `asint`/`asfloat` when read from a non-`uint` field,
//!   its value in `asuint` when assigned;
//! - `B[i] = v;` → `B_UbStore( i, v );` and any other `B[i]` → `B_UbLoad( i )`, helpers
//!   declared on the declaration's line so line numbers hold.
//!
//! A buffer used any other way (compound assignment to a non-`uint` field, passed whole) is left
//! alone.

use crate::scan::{blank_comments, ident_at, is_ident, match_close, skip_trivia};
use std::collections::HashMap;

/// A struct's fields as (name, `uint` | `int` | `float`).
type Fields = Vec<(String, &'static str)>;

pub fn rewrite(src: &str) -> String {
    let mut out = src.to_string();
    let clean = blank_comments(src);
    let structs = structs(&clean);
    for (buffer, ty) in candidates(&clean, &structs) {
        if let Some(r) = rewrite_buffer(&out, &buffer, &ty, &structs[&ty]) {
            out = r;
        }
    }
    out
}

/// Every ident occurrence of `word` (whole word) in `s`.
fn occurrences<'a>(s: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    s.match_indices(word).map(|(i, _)| i).filter(move |&i| (i == 0 || !is_ident(s.as_bytes()[i - 1])) && ident_at(s, i) == Some(word))
}

fn scalar(ty: &str) -> Option<&'static str> {
    match ty {
        "uint" | "dword" => Some("uint"),
        "int" => Some("int"),
        "float" => Some("float"),
        _ => None,
    }
}

fn structs(s: &str) -> HashMap<String, Fields> {
    let mut out = HashMap::new();
    for i in occurrences(s, "struct") {
        let at = skip_trivia(s, i + 6);
        let Some(name) = ident_at(s, at) else { continue };
        let open = skip_trivia(s, at + name.len());
        if s.as_bytes().get(open) != Some(&b'{') {
            continue;
        }
        let Some(close) = match_close(s, open, false) else { continue };
        let fields: Option<Fields> = s[open + 1..close]
            .split(';')
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(|f| match f.split_whitespace().collect::<Vec<_>>()[..] {
                [ty, field] if field.bytes().all(is_ident) => scalar(ty).map(|t| (field.to_string(), t)),
                _ => None,
            })
            .collect();
        if let Some(fields) = fields.filter(|f| !f.is_empty()) {
            out.insert(name.to_string(), fields);
        }
    }
    out
}

/// `(buffer, struct)` for each `RWStructuredBuffer<S> B` with S in `structs` and an
/// `Interlocked*( B[...]` somewhere.
fn candidates(s: &str, structs: &HashMap<String, Fields>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for i in occurrences(s, "RWStructuredBuffer") {
        let Some((ty, name, _)) = declaration(s, i) else { continue };
        if !structs.contains_key(ty) {
            continue;
        }
        let atomic = s.match_indices("Interlocked").any(|(j, _)| {
            let word = ident_at(s, j).unwrap_or_default();
            let open = skip_trivia(s, j + word.len());
            s.as_bytes().get(open) == Some(&b'(') && ident_at(s, skip_trivia(s, open + 1)) == Some(name)
        });
        if atomic {
            out.push((name.to_string(), ty.to_string()));
        }
    }
    out
}

/// `RWStructuredBuffer < S > B` at `i`: S, B and the span of `S`.
fn declaration(s: &str, i: usize) -> Option<(&str, &str, (usize, usize))> {
    let lt = skip_trivia(s, i + "RWStructuredBuffer".len());
    (s.as_bytes().get(lt) == Some(&b'<')).then_some(())?;
    let at = skip_trivia(s, lt + 1);
    let ty = ident_at(s, at)?;
    let gt = skip_trivia(s, at + ty.len());
    (s.as_bytes().get(gt) == Some(&b'>')).then_some(())?;
    let name = ident_at(s, skip_trivia(s, gt + 1))?;
    Some((ty, name, (at, at + ty.len())))
}

/// Past the `;` ending the statement that continues at `i`, and the `;`'s index.
fn statement_end(s: &str, mut i: usize) -> Option<usize> {
    let b = s.as_bytes();
    while i < b.len() {
        match b[i] {
            b';' => return Some(i),
            b'(' | b'[' | b'{' => i = match_close(s, i, false)?,
            b')' | b']' | b'}' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_assign(s: &str, i: usize) -> bool {
    let b = s.as_bytes();
    b.get(i) == Some(&b'=') && b.get(i + 1) != Some(&b'=')
}

fn is_compound_assign(s: &str, i: usize) -> bool {
    let b = s.as_bytes();
    let op = |n: usize| b.get(i + n).copied();
    matches!((op(0), op(1), op(2)), (Some(b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'), Some(b'='), _) | (Some(b'<'), Some(b'<'), Some(b'=')) | (Some(b'>'), Some(b'>'), Some(b'=')))
        || matches!((op(0), op(1)), (Some(b'+'), Some(b'+')) | (Some(b'-'), Some(b'-')))
}

fn helpers(buffer: &str, ty: &str, fields: &Fields) -> String {
    let n = fields.len();
    let load: String = fields.iter().enumerate().map(|(k, (f, t))| format!(" s.{f} = {}( {buffer}[i * {n} + {k}] );", if *t == "uint" { "" } else { cast(t) })).collect();
    let store: String = fields.iter().enumerate().map(|(k, (f, t))| format!(" {buffer}[i * {n} + {k}] = {}( s.{f} );", if *t == "uint" { "" } else { "asuint" })).collect();
    format!(" {ty} {buffer}_UbLoad( uint i ) {{ {ty} s;{load} return s; }} void {buffer}_UbStore( uint i, {ty} s ) {{{store} }}")
}

fn cast(t: &str) -> &'static str {
    if t == "int" { "asint" } else { "asfloat" }
}

fn rewrite_buffer(src: &str, buffer: &str, ty: &str, fields: &Fields) -> Option<String> {
    let s = blank_comments(src);
    let n = fields.len();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for i in occurrences(&s, buffer) {
        let open = skip_trivia(&s, i + buffer.len());
        if s.as_bytes().get(open) != Some(&b'[') {
            let (ty_start, ty_end) = occurrences(&s, "RWStructuredBuffer").find_map(|d| declaration(&s, d).filter(|(_, b, _)| *b == buffer).map(|(_, _, span)| span))?;
            // Only the declaration itself may name the buffer without indexing it.
            (skip_trivia(&s, ty_end + 1) == i).then_some(())?;
            let semi = statement_end(&s, open)?;
            edits.push((ty_start, ty_end, "uint".to_string()));
            edits.push((semi + 1, semi + 1, helpers(buffer, ty, fields)));
            continue;
        }
        let close = match_close(&s, open, false)?;
        let index = src[open + 1..close].trim();
        occurrences(index, buffer).next().is_none().then_some(())?;
        let after = skip_trivia(&s, close + 1);
        if s.as_bytes().get(after) == Some(&b'.') {
            let at = skip_trivia(&s, after + 1);
            let field = ident_at(&s, at)?;
            let (k, (_, t)) = fields.iter().enumerate().find(|(_, (f, _))| f == field)?;
            let end = at + field.len();
            let access = format!("{buffer}[({index}) * {n} + {k}]");
            let next = skip_trivia(&s, end);
            if *t == "uint" {
                edits.push((i, end, access));
            } else if is_assign(&s, next) {
                let semi = statement_end(&s, next + 1)?;
                edits.push((i, semi, format!("{access} = asuint( {} )", src[next + 1..semi].trim())));
            } else if is_compound_assign(&s, next) {
                return None;
            } else {
                edits.push((i, end, format!("{}( {access} )", cast(t))));
            }
            continue;
        }
        if is_assign(&s, after) {
            let semi = statement_end(&s, after + 1)?;
            edits.push((i, semi, format!("{buffer}_UbStore( {index}, {} )", src[after + 1..semi].trim())));
        } else if is_compound_assign(&s, after) {
            return None;
        } else {
            edits.push((i, close + 1, format!("{buffer}_UbLoad( {index} )")));
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = src.to_string();
    for (start, end, text) in edits {
        out.replace_range(start..end, &text);
    }
    Some(out)
}

#[cfg(test)]
#[path = "word_buffers_tests.rs"]
mod tests;
