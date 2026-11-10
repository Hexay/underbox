//! `IPhysicsWorld.SetCollisionRulesFromJson`: managed's serialized `CollisionRules`
//! (`{"Defaults": {tag: result}, "Pairs": [{"a", "b", "r"}]}`) as tag-token rules.

use abi::StringToken;
use physics::{CollisionRules, Rule};
use serde_json::Value;

/// A `Result` as JSON: its name (JsonStringEnumConverter) or its number.
fn rule(v: &Value) -> Rule {
    let by_name = |s: &str| match s.to_ascii_lowercase().as_str() {
        "collide" => Rule::Collide,
        "trigger" => Rule::Trigger,
        "ignore" => Rule::Ignore,
        _ => Rule::Unset,
    };
    match v {
        Value::String(s) => by_name(s),
        Value::Number(n) => [Rule::Unset, Rule::Collide, Rule::Trigger, Rule::Ignore].get(n.as_u64().unwrap_or(0) as usize).copied().unwrap_or_default(),
        _ => Rule::Unset,
    }
}

fn token(tag: &str) -> u32 {
    StringToken::of(tag).0
}

/// `None` for JSON that isn't a rules object.
pub fn parse(json: &str) -> Option<CollisionRules> {
    let root: Value = serde_json::from_str(json).ok()?;
    let defaults = root.get("Defaults").and_then(Value::as_object).into_iter().flatten().map(|(tag, r)| (token(tag), rule(r)));
    let pairs = root.get("Pairs").and_then(Value::as_array).into_iter().flatten().filter_map(|p| {
        Some((token(p.get("a")?.as_str()?), token(p.get("b")?.as_str()?), rule(p.get("r")?)))
    });
    Some(CollisionRules::new(defaults, pairs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_managed_rules() {
        let json = r#"{"__version":2,"Defaults":{"solid":"Collide","trigger":"Trigger","ladder":3},
            "Pairs":[{"a":"solid","b":"solid","r":"Collide"},{"a":"trigger","b":"playerclip","r":"Ignore"}]}"#;
        let rules = parse(json).unwrap();
        let [solid, trigger, ladder, clip] = ["solid", "trigger", "ladder", "PlayerClip"].map(token);
        assert_eq!(rules.between(&[trigger], &[clip]), Rule::Ignore, "tags are case-blind tokens");
        assert_eq!(rules.between(&[solid], &[ladder]), Rule::Ignore);
        assert_eq!(rules.between(&[solid], &[trigger]), Rule::Trigger);
        assert!(parse("not json").is_none());
    }
}
