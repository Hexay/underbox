//! Rust identifiers from engine names.

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "yield",
];

/// `IPhysicsBody` → `i_physics_body`, `g_pRenderDevice` → `g_p_render_device`, `m_nID` → `m_n_id`.
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let mut out = String::with_capacity(name.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_lower) {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    let collapsed = out.split('_').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("_");
    if collapsed.is_empty() { "_".into() } else { collapsed }
}

pub fn screaming(name: &str) -> String {
    snake(name).to_ascii_uppercase()
}

/// A field or argument name, safe as a Rust identifier.
pub fn ident(name: &str) -> String {
    let s = snake(strip_backing_field(name));
    let s = if s.starts_with(|c: char| c.is_ascii_digit()) { format!("_{s}") } else { s };
    match s.as_str() {
        "self" | "super" | "crate" => format!("{s}_"),
        k if KEYWORDS.contains(&k) => format!("r#{s}"),
        _ => s,
    }
}

/// A module name (keywords can't be raw in paths we print with `::`, so suffix instead).
pub fn module(name: &str) -> String {
    let s = snake(name);
    if KEYWORDS.contains(&s.as_str()) || s == "_" { format!("{s}_") } else { s }
}

/// A type name kept as the engine spells it (`CSceneObject`, `Foo::Bar` → `Foo_Bar`).
pub fn type_name(name: &str) -> String {
    name.replace("::", "_").replace(['.', '<', '>', ' ', '*', ','], "_")
}

/// `<Position>k__BackingField` → `Position`.
pub fn strip_backing_field(name: &str) -> &str {
    name.strip_prefix('<').and_then(|n| n.split_once(">k__BackingField")).map_or(name, |(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_cases() {
        assert_eq!(snake("IPhysicsBody"), "i_physics_body");
        assert_eq!(snake("g_pRenderDevice"), "g_p_render_device");
        assert_eq!(snake("CSceneObject"), "c_scene_object");
        assert_eq!(snake("globalBox2d"), "global_box2d");
        assert_eq!(snake("m_nID"), "m_n_id");
        assert_eq!(snake("SetHTMLKey"), "set_html_key");
        assert_eq!(ident("<MinZ>k__BackingField"), "min_z");
        assert_eq!(ident("type"), "r#type");
    }
}
