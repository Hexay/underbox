//! Collision rules: what two shapes do when they meet, from their tags (managed's `CollisionRules`).

use std::collections::HashMap;

/// managed's `CollisionRules.Result`, ordered from most to least colliding (after `Unset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum Rule {
    #[default]
    Unset,
    Collide,
    /// No contact response, but trigger callbacks.
    Trigger,
    Ignore,
}

/// Rules keyed by tag token (managed's `StringToken`, see `CollisionRules.BuildRuntimeData`).
#[derive(Debug, Clone, Default)]
pub struct CollisionRules {
    defaults: HashMap<u32, Rule>,
    pairs: HashMap<(u32, u32), Rule>,
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl CollisionRules {
    /// Duplicate pairs keep their least colliding rule, as managed does.
    pub fn new(defaults: impl IntoIterator<Item = (u32, Rule)>, pairs: impl IntoIterator<Item = (u32, u32, Rule)>) -> Self {
        let mut rules = Self { defaults: defaults.into_iter().collect(), pairs: HashMap::new() };
        for (a, b, rule) in pairs.into_iter().filter(|p| p.2 != Rule::Unset) {
            let slot = rules.pairs.entry(key(a, b)).or_default();
            *slot = (*slot).max(rule);
        }
        rules
    }

    /// The rule between two tag sets: the least colliding explicit pair rule, else the least
    /// colliding per-tag default, never less than `Collide` (managed's `GetCollisionRule(set, set)`).
    pub fn between(&self, left: &[u32], right: &[u32]) -> Rule {
        let mut rule = Rule::Unset;
        for &a in left {
            for &b in right {
                if let Some(&r) = self.pairs.get(&key(a, b)) {
                    rule = rule.max(r);
                }
            }
        }
        if rule == Rule::Unset {
            let default = |t: &u32| self.defaults.get(t).copied().unwrap_or_default();
            rule = left.iter().chain(right).map(default).max().unwrap_or_default();
        }
        rule.max(Rule::Collide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOLID: u32 = 1;
    const TRIGGER: u32 = 2;
    const PLAYERCLIP: u32 = 3;
    const LADDER: u32 = 4;
    const DEBRIS: u32 = 5;

    /// managed's defaults (CollisionRules.OnValidate), with tags as small tokens.
    fn stock() -> CollisionRules {
        let defaults = [(SOLID, Rule::Collide), (TRIGGER, Rule::Trigger), (LADDER, Rule::Ignore)];
        let pairs = [(SOLID, SOLID, Rule::Collide), (TRIGGER, PLAYERCLIP, Rule::Ignore), (TRIGGER, SOLID, Rule::Trigger), (PLAYERCLIP, SOLID, Rule::Collide)];
        CollisionRules::new(defaults, pairs)
    }

    #[test]
    fn explicit_pairs_win_over_defaults() {
        let rules = stock();
        assert_eq!(rules.between(&[SOLID], &[SOLID]), Rule::Collide);
        assert_eq!(rules.between(&[SOLID], &[TRIGGER]), Rule::Trigger);
        assert_eq!(rules.between(&[PLAYERCLIP], &[TRIGGER]), Rule::Ignore);
        assert_eq!(rules.between(&[SOLID, PLAYERCLIP], &[SOLID]), Rule::Collide);
    }

    #[test]
    fn defaults_apply_only_without_a_pair_rule() {
        let rules = stock();
        assert_eq!(rules.between(&[LADDER], &[DEBRIS]), Rule::Ignore);
        assert_eq!(rules.between(&[DEBRIS], &[DEBRIS]), Rule::Collide, "unset collides");
        assert_eq!(rules.between(&[], &[]), Rule::Collide);
        // solid+ladder vs solid has an explicit Collide pair, so ladder's Ignore default is skipped.
        assert_eq!(rules.between(&[SOLID, LADDER], &[SOLID]), Rule::Collide);
    }

    #[test]
    fn duplicate_pairs_keep_the_least_colliding() {
        let rules = CollisionRules::new([], [(1, 2, Rule::Collide), (2, 1, Rule::Ignore), (1, 2, Rule::Trigger)]);
        assert_eq!(rules.between(&[1], &[2]), Rule::Ignore);
    }
}
