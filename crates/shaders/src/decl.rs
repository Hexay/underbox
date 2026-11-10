//! vfx statements inside blocks: combos, features, rules, render state, attributes, material
//! inputs and external descriptor sets. `extract` records them and blanks them out of the text,
//! leaving HLSL (plus annotations; see `annotation.rs`).

use crate::scan::{blank_bytes, blank_comments, ident_at, line_of, match_close, skip_trivia, split_args, unquote};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComboRange {
    Values { min: u32, max: u32 },
    /// `StaticCombo( S_X, F_X, ... )`: the range and value come from material feature `F_X`.
    Feature(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboDecl {
    pub name: String,
    pub range: ComboRange,
    /// `Sys( ALL )`, `Sys( PC )`: the platforms the combo exists on.
    pub sys: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDecl {
    pub name: String,
    pub min: u32,
    pub max: u32,
    pub labels: Vec<(u32, String)>,
    pub group: Option<String>,
}

/// `Allow1( A, B )`, `Requires1( A, B )` and friends, from `FeatureRule`/`StaticComboRule`/`DynamicComboRule`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDecl {
    pub kind: String,
    pub names: Vec<String>,
    pub message: Option<String>,
}

/// `BoolAttribute( name, expr )`, `TextureAttribute( name, g_tX )`, …; `ty` is the prefix ("Bool").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeDecl {
    pub ty: String,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decls {
    pub static_combos: Vec<ComboDecl>,
    pub dynamic_combos: Vec<ComboDecl>,
    pub features: Vec<FeatureDecl>,
    pub feature_rules: Vec<RuleDecl>,
    pub static_rules: Vec<RuleDecl>,
    pub dynamic_rules: Vec<RuleDecl>,
    /// `RenderState( Name, expr )` in declaration order; later ones override earlier ones.
    pub render_states: Vec<(String, String)>,
    pub attributes: Vec<AttributeDecl>,
    /// `CreateInputTexture2D( ... )` and friends: macro name, then its arguments.
    pub inputs: Vec<Vec<String>>,
    /// `ExternalDescriptorSet name Slot n;`
    pub descriptor_sets: Vec<(String, u32)>,
}

/// Records the vfx statements in `text` and returns the text with them (and comments) blanked.
/// `ExternalDescriptorSet` becomes a `#define` of its slot, which register macros paste.
pub fn extract(text: &str, decls: &mut Decls) -> Result<String, String> {
    let s = blank_comments(text);
    let mut out = s.clone().into_bytes();
    let mut line_start = 0;
    while line_start < s.len() {
        let line_end = s[line_start..].find('\n').map_or(s.len(), |n| line_start + n);
        let i = line_start + (s[line_start..line_end].len() - s[line_start..line_end].trim_start().len());
        line_start = line_end + 1;
        let Some(word) = ident_at(&s, i) else { continue };
        let err = |m: String| format!("line {}: {word}: {m}", line_of(&s, i));
        if word == "ExternalDescriptorSet" {
            let end = s[i..].find(';').map(|n| i + n).ok_or_else(|| err("missing ;".into()))?;
            let parts: Vec<&str> = s[i + word.len()..end].split_whitespace().collect();
            let [name, "Slot", slot] = parts[..] else { return Err(err(format!("bad form {parts:?}"))) };
            let slot = slot.parse().map_err(|_| err(format!("bad slot {slot}")))?;
            decls.descriptor_sets.push((name.into(), slot));
            blank_bytes(&mut out[i..=end]);
            let def = format!("#define {name} {slot}");
            if def.len() <= end + 1 - i && !s[i..end].contains('\n') {
                out[i..i + def.len()].copy_from_slice(def.as_bytes());
            } else {
                return Err(err("must fit on one line".into()));
            }
            line_start = line_start.max(end + 1);
            continue;
        }
        if !is_statement(word) {
            continue;
        }
        let open = skip_trivia(&s, i + word.len());
        if s.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let close = match_close(&s, open, false).ok_or_else(|| err("unclosed (".into()))?;
        let semi = skip_trivia(&s, close + 1);
        let end = if s.as_bytes().get(semi) == Some(&b';') { semi } else { close };
        record(word, &s[open + 1..close], decls).map_err(err)?;
        blank_bytes(&mut out[i..=end]);
        line_start = line_start.max(end + 1);
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}

fn is_statement(word: &str) -> bool {
    matches!(
        word,
        "StaticCombo"
            | "DynamicCombo"
            | "Feature"
            | "FeatureRule"
            | "StaticComboRule"
            | "DynamicComboRule"
            | "RenderState"
    ) || (word.ends_with("Attribute") && word.len() > "Attribute".len())
        || word.starts_with("CreateInputTexture")
}

fn record(word: &str, inner: &str, d: &mut Decls) -> Result<(), String> {
    let args = split_args(inner);
    let arg = |n: usize| args.get(n).copied().ok_or_else(|| format!("missing argument {}", n + 1));
    match word {
        "StaticCombo" | "DynamicCombo" => {
            let combo = ComboDecl { name: arg(0)?.into(), range: combo_range(arg(1)?)?, sys: sys(args.get(2)) };
            let list = if word == "StaticCombo" { &mut d.static_combos } else { &mut d.dynamic_combos };
            if !list.iter().any(|c| c.name == combo.name) {
                list.push(combo);
            }
        }
        "Feature" => {
            let (min, max, labels) = values_with_labels(arg(1)?)?;
            let feature = FeatureDecl { name: arg(0)?.into(), min, max, labels, group: args.get(2).map(|g| unquote(g).into()) };
            if !d.features.iter().any(|f| f.name == feature.name) {
                d.features.push(feature);
            }
        }
        "FeatureRule" | "StaticComboRule" | "DynamicComboRule" => {
            let rule = rule(arg(0)?, args.get(1).map(|m| unquote(m).to_string()))?;
            match word {
                "FeatureRule" => d.feature_rules.push(rule),
                "StaticComboRule" => d.static_rules.push(rule),
                _ => d.dynamic_rules.push(rule),
            }
        }
        "RenderState" => d.render_states.push((arg(0)?.into(), args[1..].join(", "))),
        w if w.starts_with("CreateInputTexture") => {
            d.inputs.push(std::iter::once(w.to_string()).chain(args.iter().map(|a| a.to_string())).collect())
        }
        w => d.attributes.push(AttributeDecl {
            ty: w.trim_end_matches("Attribute").into(),
            name: unquote(arg(0)?).into(),
            value: args[1..].join(", "),
        }),
    }
    Ok(())
}

fn sys(arg: Option<&&str>) -> String {
    arg.and_then(|a| a.trim().strip_prefix("Sys"))
        .and_then(|a| a.trim().strip_prefix('('))
        .and_then(|a| a.strip_suffix(')'))
        .map_or_else(|| "ALL".into(), |a| a.trim().to_ascii_uppercase())
}

fn combo_range(arg: &str) -> Result<ComboRange, String> {
    if arg.starts_with(|c: char| c.is_ascii_digit()) {
        let (min, max, _) = values_with_labels(arg)?;
        Ok(ComboRange::Values { min, max })
    } else {
        Ok(ComboRange::Feature(arg.into()))
    }
}

/// `0..2` or `0..2 ( 0="Normal", 1="Multiply", 2="Lighten" )`.
fn values_with_labels(arg: &str) -> Result<(u32, u32, Vec<(u32, String)>), String> {
    let (range, labels) = match arg.find('(') {
        Some(p) => (&arg[..p], arg[p + 1..].trim().strip_suffix(')').ok_or("unclosed label list")?),
        None => (arg, ""),
    };
    let (a, b) = range.trim().split_once("..").ok_or_else(|| format!("bad range {range}"))?;
    let num = |x: &str| x.trim().parse::<u32>().map_err(|_| format!("bad range {range}"));
    let (min, max) = (num(a)?, num(b)?);
    if min > max {
        return Err(format!("empty range {range}"));
    }
    let labels = split_args(labels)
        .into_iter()
        .filter_map(|l| l.split_once('='))
        .filter_map(|(v, name)| Some((v.trim().parse().ok()?, unquote(name).to_string())))
        .collect();
    Ok((min, max, labels))
}

fn rule(expr: &str, message: Option<String>) -> Result<RuleDecl, String> {
    let open = expr.find('(').ok_or_else(|| format!("bad rule {expr}"))?;
    let inner = expr[open + 1..].trim_end().strip_suffix(')').ok_or_else(|| format!("bad rule {expr}"))?;
    Ok(RuleDecl {
        kind: expr[..open].trim().into(),
        names: split_args(inner).into_iter().map(String::from).collect(),
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_blanks() {
        let src = "Feature( F_BLEND, 0..2 ( 0=\"A\", 2=\"C\" ), \"Blend\" );\n\
            StaticCombo( S_BLEND, F_BLEND, Sys( ALL ) );\n\
            DynamicCombo( D_X, 1..6, Sys( PC ) ); // c\n\
            FeatureRule( Allow1( F_A, F_B ), \"msg\" );\n\
            RenderState( CullMode, F_B ? NONE : DEFAULT );\n\
            BoolAttribute( translucent, true );\n\
            ExternalDescriptorSet g_set Slot 1;\n\
            float4 x;\n";
        let mut d = Decls::default();
        let out = extract(src, &mut d).unwrap();
        assert_eq!(out.lines().count(), src.lines().count());
        assert_eq!(out.split_whitespace().collect::<Vec<_>>(), ["#define", "g_set", "1", "float4", "x;"]);
        assert_eq!(d.features[0].labels, [(0, "A".into()), (2, "C".into())]);
        assert_eq!(d.static_combos[0].range, ComboRange::Feature("F_BLEND".into()));
        assert_eq!(d.dynamic_combos[0], ComboDecl { name: "D_X".into(), range: ComboRange::Values { min: 1, max: 6 }, sys: "PC".into() });
        assert_eq!(d.feature_rules[0].names, ["F_A", "F_B"]);
        assert_eq!(d.render_states[0], ("CullMode".into(), "F_B ? NONE : DEFAULT".into()));
        assert_eq!(d.attributes[0].ty, "Bool");
        assert_eq!(d.descriptor_sets, [("g_set".into(), 1)]);
    }
}
