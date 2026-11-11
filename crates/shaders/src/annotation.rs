//! vfx annotations: `Type name < Attribute( "X" ); Default( 1 ); >;` on globals, struct members
//! (`Semantic( Color )`) and samplers (`Filter( BILINEAR ); AddressU( CLAMP );`). DXC doesn't
//! accept them, so `strip` records and removes them.
//!
//! An annotation is told apart from templates and comparisons by shape: `<` follows a name (or
//! semantic / array size), the content is a run of `Name( ... );` items, and `>` is followed by `;`.

use crate::scan::{blank_bytes, is_ident, line_of, match_close, skip_trivia};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotated {
    /// The declared name: the variable, member or sampler.
    pub name: String,
    /// The declaration before the annotation, whitespace-collapsed (`float2 TextureSize`).
    pub decl: String,
    /// `(key, args)` per item: `("Attribute", "\"TextureSize\"")`.
    pub items: Vec<(String, String)>,
    /// For `SpecializedConstant()` globals: the index of the `UB_SPEC_<n>` define holding its value.
    pub spec_id: Option<u32>,
    /// 1-based line in the program's expanded text, matching `Program::directives`.
    #[serde(default)]
    pub line: u32,
}

impl Annotated {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// A specialized constant's default value (its `Default`, else 0).
    pub fn default_value(&self) -> &str {
        self.get("Default").filter(|d| !d.is_empty()).unwrap_or("0")
    }
}

/// Removes annotations from comment-free `text`, keeping line structure. A global marked
/// `SpecializedConstant()` becomes `static const T name = UB_SPEC_<n>;`, numbered in order: each
/// variant compiles with its value defined, so no `OpSpecConstant` (and no `OpSpecConstantOp`,
/// which naga rejects) reaches SPIR-V.
pub fn strip(text: &str, out: &mut Vec<Annotated>) -> String {
    strip_keeping_macros(text, out, &mut Vec::new())
}

/// [`strip`], also recording into `kept` the annotations whose last item is a bare macro name
/// (`SrgbRead( false ); BENT_NORMAL_ACTIVE_IF >`); those stay in the text for DXC to expand.
pub fn strip_keeping_macros(text: &str, out: &mut Vec<Annotated>, kept: &mut Vec<Annotated>) -> String {
    let b = text.as_bytes();
    let mut res = b.to_vec();
    let mut i = 0;
    let mut next_spec = 0;
    while let Some(n) = text[i..].find('<') {
        let lt = i + n;
        i = lt + 1;
        let (strict, (gt, items)) = match parse_items(text, lt + 1, false) {
            Some(found) => (true, found),
            None => match parse_items(text, lt + 1, true) {
                Some(found) => (false, found),
                None => continue,
            },
        };
        let semi = skip_trivia(text, gt + 1);
        if b.get(semi) != Some(&b';') {
            continue;
        }
        let Some((name, decl, start)) = declared(text, lt) else { continue };
        let mut a = Annotated { name, decl, items, spec_id: None, line: line_of(text, lt) };
        if !strict {
            kept.push(a);
            i = gt + 1;
            continue;
        }
        blank_bytes(&mut res[lt..=gt]);
        if a.get("SpecializedConstant").is_some() {
            let new = format!("static const {} = UB_SPEC_{next_spec};", a.decl);
            if new.len() <= semi + 1 - start && !text[start..semi].contains('\n') {
                blank_bytes(&mut res[start..=semi]);
                res[start..start + new.len()].copy_from_slice(new.as_bytes());
                a.spec_id = Some(next_spec);
                next_spec += 1;
            }
        }
        out.push(a);
        i = gt + 1;
    }
    String::from_utf8(res).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// The `Name( args );` items from `start` up to the closing `>`, and that `>`'s index. `bare`
/// also accepts an argument-less name right before `>` without its `;`.
fn parse_items(s: &str, start: usize, bare: bool) -> Option<(usize, Vec<(String, String)>)> {
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
                if bare && args.is_empty() && after == open && b.get(semi) == Some(&b'>') {
                    items.push((key, args));
                    i = semi;
                    continue;
                }
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
/// `[N]`, plus the declaration text from the previous statement boundary and where it starts.
fn declared(s: &str, lt: usize) -> Option<(String, String, usize)> {
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
    Some((name.into(), decl, start))
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

    #[test]
    fn annotations_ending_in_a_macro_are_kept_in_the_text() {
        let src = "float a < Default( 1 ); >;\nTexture2D g_tBentNormal < SrgbRead( false ); BENT_NORMAL_ACTIVE_IF >;\n";
        let (mut found, mut kept) = (Vec::new(), Vec::new());
        let out = strip_keeping_macros(src, &mut found, &mut kept);
        assert_eq!((found.len(), found[0].line), (1, 1));
        assert_eq!((kept[0].name.as_str(), kept[0].line), ("g_tBentNormal", 2));
        assert_eq!(kept[0].get("BENT_NORMAL_ACTIVE_IF"), Some(""));
        assert_eq!(out.lines().nth(1), src.lines().nth(1), "left for DXC");
    }

    #[test]
    fn specialized_constants_get_ids() {
        let src = "int A < Attribute( \"A\" ); Default( 2 ); SpecializedConstant(); >;\nint B < Attribute(\"B\"); SpecializedConstant(); >;\n";
        let mut found = Vec::new();
        let out = strip(src, &mut found);
        let lines: Vec<_> = out.lines().map(str::trim).collect();
        assert_eq!(lines, ["static const int A = UB_SPEC_0;", "static const int B = UB_SPEC_1;"]);
        assert_eq!(found.iter().map(|a| (a.spec_id, a.default_value())).collect::<Vec<_>>(), [(Some(0), "2"), (Some(1), "0")]);
    }
}
