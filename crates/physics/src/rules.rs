//! Collision rules: what two shapes do when they meet, from their tags (managed's `CollisionRules`).

use std::collections::{HashMap, HashSet};

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
    /// Shapes with any of these tags run the pair filter; see `needs_filter`.
    filtering: HashSet<u32>,
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl CollisionRules {
    /// Duplicate pairs keep their least colliding rule, as managed does.
    pub fn new(defaults: impl IntoIterator<Item = (u32, Rule)>, pairs: impl IntoIterator<Item = (u32, u32, Rule)>) -> Self {
        let mut rules = Self { defaults: defaults.into_iter().collect(), ..Self::default() };
        for (a, b, rule) in pairs.into_iter().filter(|p| p.2 != Rule::Unset) {
            let slot = rules.pairs.entry(key(a, b)).or_default();
            *slot = (*slot).max(rule);
        }
        rules.filtering = rules.defaults.iter().filter(|(_, r)| **r > Rule::Collide).map(|(t, _)| *t).collect();
        let mut open: Vec<_> = rules.pairs.iter().filter(|(_, r)| **r > Rule::Collide).map(|(k, _)| *k).collect();
        // Greedy cover: one tag of every uncolliding pair, the tag in the most open pairs first.
        loop {
            open.retain(|(a, b)| !rules.filtering.contains(a) && !rules.filtering.contains(b));
            let mut degree: HashMap<u32, usize> = HashMap::new();
            for t in open.iter().flat_map(|(a, b)| [*a, *b]) {
                *degree.entry(t).or_default() += 1;
            }
            let Some((tag, _)) = degree.into_iter().max_by_key(|&(t, n)| (n, std::cmp::Reverse(t))) else { break };
            rules.filtering.insert(tag);
        }
        rules
    }

    /// Whether a shape tagged `tags` must run the pair filter. Any pair of shapes that both don't
    /// collides: every uncolliding pair rule and default involves a filtering tag.
    pub fn needs_filter(&self, tags: &[u32]) -> bool {
        tags.iter().any(|t| self.filtering.contains(t))
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
    fn one_tag_of_each_uncolliding_rule_filters() {
        let rules = stock();
        for tags in [&[SOLID, DEBRIS][..], &[SOLID, PLAYERCLIP], &[]] {
            assert!(!rules.needs_filter(tags), "{tags:?}");
        }
        assert!(rules.needs_filter(&[SOLID, TRIGGER]), "covers trigger-solid and trigger-playerclip");
        assert!(rules.needs_filter(&[LADDER]), "an ignoring default");
    }

    #[test]
    fn duplicate_pairs_keep_the_least_colliding() {
        let rules = CollisionRules::new([], [(1, 2, Rule::Collide), (2, 1, Rule::Ignore), (1, 2, Rule::Trigger)]);
        assert_eq!(rules.between(&[1], &[2]), Rule::Ignore);
    }
}
