//! Source rewrites for syntax the core shaders share with Slang but DXC rejects. Each is a
//! token-level rewrite on comment-free text, so behaviour is unchanged:
//! - `T x = {};` → `T x = (T)0;` (empty brace initialiser).
//! - `point` used as an identifier (water/waves.hlsl) → `point_`; as the geometry-shader input
//!   modifier (`point VS_OUTPUT i[1]`) it stays.

use crate::scan::{ident_at, is_ident, is_ident_start, skip_trivia};

pub fn rewrite(src: &str) -> String {
    src.lines()
        .map(|l| if l.trim_start().starts_with('#') { l.to_string() } else { line(l) })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn line(l: &str) -> String {
    let b = l.as_bytes();
    let mut out = String::with_capacity(l.len());
    let mut i = 0;
    let mut prev_words: [&str; 2] = ["", ""];
    while i < b.len() {
        if !is_ident_start(b[i]) || (i > 0 && is_ident(b[i - 1])) {
            let assign = b[i] == b'=' && b.get(i + 1) != Some(&b'=') && (i == 0 || !b"=!<>+-*/%&|^".contains(&b[i - 1]));
            if let Some(end) = assign.then(|| empty_braces(l, i + 1)).flatten() {
                out.push_str(&format!("= ({})0;", prev_words[0]));
                i = end;
                continue;
            }
            if !b[i].is_ascii_whitespace() {
                prev_words = ["", ""];
            }
            let ch = l[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let word = ident_at(l, i).unwrap_or_default();
        let next = ident_at(l, skip_trivia(l, i + word.len()));
        if word == "point" && next.is_none() {
            out.push_str("point_");
        } else {
            out.push_str(word);
        }
        prev_words = [prev_words[1], word];
        i += word.len();
    }
    out
}

/// End (past `;`) of `{ } ;` starting after `=` at `i`.
fn empty_braces(l: &str, i: usize) -> Option<usize> {
    let b = l.as_bytes();
    let open = skip_trivia(l, i);
    (b.get(open) == Some(&b'{')).then_some(())?;
    let close = skip_trivia(l, open + 1);
    (b.get(close) == Some(&b'}')).then_some(())?;
    let semi = skip_trivia(l, close + 1);
    (b.get(semi) == Some(&b';')).then_some(semi + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites() {
        assert_eq!(line("        Light light = {};"), "        Light light = (Light)0;");
        assert_eq!(line("float2 W( float2 point, out float3 o ) { return point.xy * 2; }"), "float2 W( float2 point_, out float3 o ) { return point_.xy * 2; }");
        assert_eq!(line("void MainGs( point VS_OUTPUT i[1] )"), "void MainGs( point VS_OUTPUT i[1] )");
        assert_eq!(line("int a[2] = { 1, 2 }; x == y;"), "int a[2] = { 1, 2 }; x == y;");
    }
}
