//! Top-level structure of a `.shader` file: Valve blocks (`NAME { ... }` with an upper-case name,
//! such as HEADER, MODES, FEATURES, COMMON, VS, PS, CS) and loose HLSL between them (the shared
//! `struct VertexInput { ... };` definitions).

use crate::scan::{blank_comments, ident_at, line_of, match_close, skip_trivia};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// `name { body }`; `line` is the line the body starts on.
    Block { name: String, body: String, line: u32 },
    /// HLSL outside any block, starting at `line`.
    Code { text: String, line: u32 },
}

fn is_block_name(name: &str) -> bool {
    name.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        && name.as_bytes()[0].is_ascii_uppercase()
}

/// Splits a `.shader` source into blocks and loose code, in file order. Comments are blanked
/// first, so offsets (and lines) match the original.
pub fn split(src: &str) -> Result<Vec<Item>, String> {
    let s = blank_comments(src);
    let b = s.as_bytes();
    let mut items = Vec::new();
    let mut code_start: Option<usize> = None;
    let mut i = skip_trivia(&s, 0);
    while i < b.len() {
        if let Some(name) = ident_at(&s, i).filter(|n| is_block_name(n)) {
            let brace = skip_trivia(&s, i + name.len());
            if b.get(brace) == Some(&b'{') {
                flush(&s, &mut code_start, i, &mut items);
                let close = match_close(&s, brace, false)
                    .ok_or_else(|| format!("line {}: block {name} is never closed", line_of(&s, i)))?;
                items.push(Item::Block {
                    name: name.into(),
                    body: s[brace + 1..close].into(),
                    line: line_of(&s, brace + 1),
                });
                i = skip_trivia(&s, close + 1);
                continue;
            }
        }
        code_start.get_or_insert(i);
        i = skip_trivia(&s, statement_end(&s, i)?);
    }
    flush(&s, &mut code_start, b.len(), &mut items);
    Ok(items)
}

fn flush(s: &str, start: &mut Option<usize>, end: usize, items: &mut Vec<Item>) {
    if let Some(st) = start.take() {
        let text = s[st..end].trim_end();
        if !text.is_empty() {
            items.push(Item::Code { text: text.into(), line: line_of(s, st) });
        }
    }
}

/// End of the loose statement starting at `i`: a preprocessor line, a `;`-terminated statement,
/// or a braced definition (function, or `struct X { };`).
fn statement_end(s: &str, i: usize) -> Result<usize, String> {
    let b = s.as_bytes();
    if b[i] == b'#' {
        let mut j = i;
        loop {
            let nl = s[j..].find('\n').map_or(b.len(), |n| j + n);
            if nl > 0 && b[nl - 1] == b'\\' && nl < b.len() {
                j = nl + 1;
            } else {
                return Ok(nl);
            }
        }
    }
    let mut j = i;
    while j < b.len() {
        match b[j] {
            b';' => return Ok(j + 1),
            b'{' => {
                let close =
                    match_close(s, j, false).ok_or_else(|| format!("line {}: unclosed brace", line_of(s, j)))?;
                let next = skip_trivia(s, close + 1);
                return Ok(if b.get(next) == Some(&b';') { next + 1 } else { close + 1 });
            }
            b'(' | b'[' => {
                j = match_close(s, j, false).ok_or_else(|| format!("line {}: unclosed bracket", line_of(s, j)))? + 1;
                continue;
            }
            _ => {}
        }
        j += 1;
    }
    Ok(b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_and_loose_code() {
        let src = "// c\nHEADER { A = 1; }\nstruct V { float4 p : POSITION; };\n#define X 1\nPS { float4 MainPs() : SV_Target0 { return 0; } }\n";
        let items = split(src).unwrap();
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], Item::Block { name, line: 2, .. } if name == "HEADER"));
        match &items[1] {
            Item::Code { text, line } => {
                assert_eq!(*line, 3);
                assert!(text.starts_with("struct V") && text.ends_with("#define X 1"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&items[2], Item::Block { name, body, .. } if name == "PS" && body.contains("MainPs")));
    }
}
