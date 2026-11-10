//! Combo spaces and indices. A program's static (or dynamic) combos form a mixed-radix number in
//! declaration order: `value_i = (id / stride_i) % count_i + min_i`, `stride_i = Π count_j (j < i)`.
//! This is the decode native `CVfx::DescribeCombo` documents (hshader.def), so ids we hand managed
//! through `CVfxComboIterator` mean the same thing there.

use crate::decl::RuleDecl;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combo {
    pub name: String,
    pub min: u32,
    pub max: u32,
    /// The material feature this static combo follows (`StaticCombo( S_X, F_X, ... )`).
    pub feature: Option<String>,
    pub sys: String,
}

impl Combo {
    pub fn count(&self) -> u64 {
        u64::from(self.max - self.min) + 1
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboSet {
    pub combos: Vec<Combo>,
    pub rules: Vec<RuleDecl>,
}

/// Returned by `CVfxComboIterator::InvalidIndex`.
pub const INVALID_INDEX: u64 = u64::MAX;

impl ComboSet {
    /// Number of ids, rules ignored (`Π count`).
    pub fn count(&self) -> u64 {
        self.combos.iter().fold(1u64, |n, c| n.saturating_mul(c.count()))
    }

    pub fn find(&self, name: &str) -> Option<&Combo> {
        self.combos.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Values per combo, in declaration order.
    pub fn decode(&self, id: u64) -> Vec<u32> {
        let mut stride = 1u64;
        self.combos
            .iter()
            .map(|c| {
                let v = ((id / stride) % c.count()) as u32 + c.min;
                stride = stride.saturating_mul(c.count());
                v
            })
            .collect()
    }

    /// The id for the values `value` gives (by combo name); missing combos take their minimum and
    /// out-of-range values clamp, as native does for `SetComboValue`.
    pub fn encode(&self, value: impl Fn(&str) -> Option<i64>) -> u64 {
        let mut stride = 1u64;
        let mut id = 0u64;
        for c in &self.combos {
            let v = value(&c.name).map_or(c.min, |v| v.clamp(i64::from(c.min), i64::from(c.max)) as u32);
            id += u64::from(v - c.min) * stride;
            stride = stride.saturating_mul(c.count());
        }
        id
    }

    /// `(name, value)` pairs for id, ready to become `#define`s.
    pub fn defines(&self, id: u64) -> Vec<(String, u32)> {
        self.combos.iter().map(|c| c.name.clone()).zip(self.decode(id)).collect()
    }

    /// Whether id passes every rule.
    pub fn is_valid(&self, id: u64) -> bool {
        if id >= self.count() {
            return false;
        }
        let values = self.decode(id);
        let value = |name: &str| {
            let idx = self.combos.iter().position(|c| c.name == name || c.feature.as_deref() == Some(name));
            idx.map_or(0, |i| values[i])
        };
        self.rules.iter().all(|r| rule_holds(r, &value))
    }

    /// First valid id at or after `from`, or [`INVALID_INDEX`]: `FirstStaticCombo` is `next_valid(0)`,
    /// `NextStaticCombo` is `next_valid(current + 1)`.
    pub fn next_valid(&self, from: u64) -> u64 {
        (from..self.count()).find(|&id| self.is_valid(id)).unwrap_or(INVALID_INDEX)
    }

    pub fn valid_ids(&self) -> impl Iterator<Item = u64> + '_ {
        let mut next = self.next_valid(0);
        std::iter::from_fn(move || {
            let id = next;
            (id != INVALID_INDEX).then(|| {
                next = self.next_valid(id + 1);
                id
            })
        })
    }
}

/// `AllowN( a, b, ... )`: at most N of them are non-zero. `RequiresN( a, b, ... )`: if `a` is
/// non-zero, at least N of the rest are. Unknown kinds hold, so they never hide combos.
fn rule_holds(rule: &RuleDecl, value: &impl Fn(&str) -> u32) -> bool {
    let n = |prefix: &str| rule.kind.strip_prefix(prefix).and_then(|n| n.parse::<usize>().ok());
    let set = |names: &[String]| names.iter().filter(|x| value(x) != 0).count();
    if let Some(max) = n("Allow") {
        set(&rule.names) <= max
    } else if let Some(min) = n("Requires") {
        rule.names.split_first().is_none_or(|(first, rest)| value(first) == 0 || set(rest) >= min)
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(name: &str, min: u32, max: u32) -> Combo {
        Combo { name: name.into(), min, max, feature: None, sys: "ALL".into() }
    }

    #[test]
    fn mixed_radix_round_trip() {
        let set = ComboSet { combos: vec![combo("A", 0, 1), combo("B", 1, 6), combo("C", 0, 2)], rules: vec![] };
        assert_eq!(set.count(), 2 * 6 * 3);
        assert_eq!(set.decode(0), [0, 1, 0]);
        assert_eq!(set.decode(1 + 2 * 3 + 12 * 2), [1, 4, 2]);
        assert_eq!(set.encode(|n| [("A", 1), ("B", 4), ("C", 2)].iter().find(|x| x.0 == n).map(|x| x.1)), 31);
        assert_eq!(set.encode(|n| (n == "B").then_some(99)), 5 * 2);
        assert_eq!(set.defines(31)[1], ("B".into(), 4));
    }

    #[test]
    fn rules_filter_iteration() {
        let rules = vec![RuleDecl { kind: "Allow1".into(), names: vec!["A".into(), "B".into()], message: None }];
        let set = ComboSet { combos: vec![combo("A", 0, 1), combo("B", 0, 1)], rules };
        assert_eq!(set.valid_ids().collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(set.next_valid(3), INVALID_INDEX);
        let req = RuleDecl { kind: "Requires1".into(), names: vec!["A".into(), "B".into()], message: None };
        let set = ComboSet { rules: vec![req], ..set };
        assert_eq!(set.valid_ids().collect::<Vec<_>>(), [0, 2, 3]);
    }
}
