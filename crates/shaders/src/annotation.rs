//! vfx annotations: `Type name < Attribute( "X" ); Default( 1 ); >;` on globals, struct members
//! (`Semantic( Color )`) and samplers (`Filter( BILINEAR ); AddressU( CLAMP );`). DXC doesn't
//! accept them, so `strip` records and removes them.
//!
//! An annotation is told apart from templates and comparisons by shape: `<` follows a name (or
//! semantic / array size), the content is a run of `Name( ... );` items, and `>` is followed by `;`.

use crate::scan::{blank_bytes, is_ident, match_close, skip_trivia};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotated {
    /// The declared name: the variable, member or sampler.
    pub name: String,
    /// The declaration before the annotation, whitespace-collapsed (`float2 TextureSize`).
    pub decl: String,
    /// `(key, args)` per item: `("Attribute", "\"TextureSize\"")`.
    pub items: Vec<(String, String)>,
}

impl Annotated {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// Removes annotations from comment-free `text`, keeping line structure.
pub fn strip(text: &str, out: &mut Vec<Annotated>) -> String {
    let b = text.as_bytes();
    let mut res = b.to_vec();
    let mut i = 0;
    while let Some(n) = text[i..].find('<') {
        let lt = i + n;
        i = lt + 1;
        let Some((gt, items)) = parse_items(text, lt + 1) else { continue };
        if b.get(skip_trivia(text, gt + 1)) != Some(&b';') {
            continue;
        }
        let Some((name, decl)) = declared(text, lt) else { continue };
        out.push(Annotated { name, decl, items });
        blank_bytes(&mut res[lt..=gt]);
        i = gt + 1;
    }
    String::from_utf8(res).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// The `Name( args );` items from `start` up to the closing `>`, and that `>`'s index.
fn parse_items(s: &str, start: usize) -> Option<(usize, Vec<(String, String)>)> {
    let b = s.as_bytes();
    let mut items = Vec::new();
    let mut i = skip_trivia(s, start);
    loop {
        match b.get(i)? {
            b'>' => return (!items.is_empty()).then_some((i, items)),
            c if c.is_ascii_alphabetic() => {
                let end = (i..b.len()).find(|&j| !is_ident(b[j]))?;
                let key = s[i..end].to_string();
                let open = skip_trivia(s, end);
                let (args, after) = if b.get(open) == Some(&b'(') {
                    let close = match_close(s, open, false)?;
                    (s[open + 1..close].trim().to_string(), close + 1)
                } else {
                    (String::new(), open)
                };
                let semi = skip_trivia(s, after);
                if b.get(semi) != Some(&b';') || args.contains('{') {
                    return None;
                }
                items.push((key, args));
                i = skip_trivia(s, semi + 1);
            }
            _ => return None,
        }
    }
}

/// The name declared just before the `<` at `lt`, skipping `: SEMANTIC` / `: register(...)` and
/// `[N]`, plus the declaration text from the previous statement boundary.
fn declared(s: &str, lt: usize) -> Option<(String, String)> {
    let b = s.as_bytes();
    let start = s[..lt].rfind([';', '{', '}', '\n']).map_or(0, |p| p + 1);
    let head = s[start..lt].trim();
    let head = if head.starts_with('#') { return None } else { head };
    let decl_part = head.split(':').next()?.trim();
    let decl_part = decl_part.split('[').next()?.trim();
    let name_start = decl_part.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |p| p + 1);
    let name = &decl_part[name_start..];
    if name.is_empty() || !b[start..lt].iter().any(|c| c.is_ascii_alphabetic()) || name.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    let decl = head.split_whitespace().collect::<Vec<_>>().join(" ");
    Some((name.into(), decl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_annotations_only() {
        let src = "Texture2D<float4> t < Attribute( \"T\" ); >;\n\
            float4 c : COLOR0 < Semantic( Color ); >;\n\
            SamplerState s < Filter( BILINEAR ); BorderColor( float4(0,0,0,0); ); >;\n\
            bool b < Attribute( \"B\" ); Default( 0 ); >;\n\
            for ( int i = 0; i < n; i++ ) { x = a > b; }\n\
            y = p < q; z = r > w;\n\
            RWTexture2D<float4> u;\n";
        let mut found = Vec::new();
        let out = strip(src, &mut found);
        let names: Vec<_> = found.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["t", "c", "s", "b"]);
        assert_eq!(found[0].decl, "Texture2D<float4> t");
        assert_eq!(found[0].get("Attribute"), Some("\"T\""));
        assert_eq!(found[2].get("BorderColor"), Some("float4(0,0,0,0);"));
        let kept: Vec<_> = out.lines().map(str::trim).collect();
        assert_eq!(kept[0].split_whitespace().collect::<Vec<_>>(), ["Texture2D<float4>", "t", ";"]);
        assert!(kept[4].contains("i < n") && kept[5].contains("p < q") && kept[6] == "RWTexture2D<float4> u;");
    }
}
