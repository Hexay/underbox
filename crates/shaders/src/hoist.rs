//! vfx gathers a program's annotated globals into the constant buffer it generates ahead of the
//! code, so code may read one above its declaration: `ui/scissor.hlsl`, included at the top of
//! `ui/pixel.hlsl`, reads pixel's `BoxSize` under `D_WORLDPANEL`. DXC rejects a use before the
//! declaration, so [`globals`] moves those declarations to the program's start.

use crate::annotation::Annotated;
use crate::scan::{blank_bytes, is_ident, occurrences};

/// `text` (annotations stripped) with every file-scope value global that is read above its
/// declaration moved onto a line before the first. The expander's `#line` markers keep the lines
/// below numbered as before.
pub fn globals(text: &str, annotations: &[Annotated]) -> String {
    let line_starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let mut res = text.as_bytes().to_vec();
    let mut hoisted = Vec::new();
    for a in annotations {
        if !is_value(&a.decl) || annotations.iter().filter(|b| b.name == a.name).count() != 1 {
            continue;
        }
        let Some(&line) = (a.line as usize).checked_sub(1).and_then(|l| line_starts.get(l)) else { continue };
        let Some(span) = statement(text, line, a) else { continue };
        let used_above = occurrences(&text[..span.0], &a.name).any(|i| i == 0 || text.as_bytes()[i - 1] != b'.');
        if used_above && brace_depth(&text[..span.0]) == 0 {
            hoisted.push(text[span.0..=span.1].split_whitespace().collect::<Vec<_>>().join(" "));
            blank_bytes(&mut res[span.0..=span.1]);
        }
    }
    if hoisted.is_empty() {
        return text.to_owned();
    }
    let rest = String::from_utf8(res).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    format!("{}\n{rest}", hoisted.join(" "))
}

/// A plain scalar, vector or matrix global: no `static`, array, semantic or user type.
fn is_value(decl: &str) -> bool {
    let words: Vec<_> = decl.split_whitespace().filter(|w| !matches!(*w, "uniform" | "row_major" | "column_major")).collect();
    let [ty, name] = words[..] else { return false };
    let base = ty.trim_end_matches(|c: char| c.is_ascii_digit() || c == 'x');
    matches!(base, "bool" | "int" | "uint" | "dword" | "half" | "float" | "double" | "min16float" | "min16int" | "min16uint") && name.bytes().all(is_ident)
}

/// The `[start, semicolon]` span of `a`'s declaration on the line starting at `line`.
fn statement(text: &str, line: usize, a: &Annotated) -> Option<(usize, usize)> {
    let end = text[line..].find('\n').map_or(text.len(), |n| line + n);
    let at = line + occurrences(&text[line..end], &a.name).next()?;
    let semi = at + text[at..end].find(';')?;
    let start = text[..at].rfind([';', '{', '}', '\n']).map_or(0, |p| p + 1);
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    (squash(&text[start..semi]) == squash(&a.decl)).then_some((start, semi))
}

fn brace_depth(s: &str) -> i32 {
    s.bytes().map(|c| i32::from(c == b'{') - i32::from(c == b'}')).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> String {
        let mut found = Vec::new();
        let stripped = crate::annotation::strip(src, &mut found);
        globals(&stripped, &found)
    }

    #[test]
    fn a_global_read_above_its_declaration_moves_to_the_top() {
        let src = "#line 1 \"a\"\nfloat2 F() { return BoxSize; }\nfloat2 BoxSize < Attribute( \"BoxSize\" ); >;\nfloat2 Later < Attribute( \"L\" ); >;\n";
        let out = run(src);
        let lines: Vec<_> = out.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        assert_eq!(lines, ["float2 BoxSize ;", "#line 1 \"a\"", "float2 F() { return BoxSize; }", "", "float2 Later ;"]);
    }

    #[test]
    fn members_resources_and_locals_stay() {
        let src = "float2 G( S s ) { return s.BoxSize; }\nfloat2 BoxSize < Attribute( \"B\" ); >;\n\
            float4 H() { return t.Sample( s, 0 ); }\nTexture2D t < Attribute( \"T\" ); >;\n";
        let mut found = Vec::new();
        let stripped = crate::annotation::strip(src, &mut found);
        assert_eq!(globals(&stripped, &found), stripped);
    }
}
