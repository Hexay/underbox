//! Per-step lookups keyed by an arena handle's index, stored densely: no hashing, and `clear`
//! keeps the allocation (docs/perf-physics.md, round 5).

/// A handle as Rapier's arenas store it: `(index, generation)` from `into_raw_parts`.
pub(crate) type Key = (u32, u32);

pub(crate) struct Dense<V> {
    slots: Vec<Option<(u32, V)>>,
}

impl<V> Default for Dense<V> {
    fn default() -> Self {
        Self { slots: Vec::new() }
    }
}

impl<V: Copy> Dense<V> {
    pub fn clear(&mut self) {
        self.slots.fill(None);
    }

    pub fn insert(&mut self, (index, generation): Key, value: V) {
        let i = index as usize;
        if i >= self.slots.len() {
            self.slots.resize(i + 1, None);
        }
        self.slots[i] = Some((generation, value));
    }

    pub fn get(&self, (index, generation): Key) -> Option<V> {
        match self.slots.get(index as usize) {
            Some(&Some((g, v))) if g == generation => Some(v),
            _ => None,
        }
    }

    pub fn contains(&self, key: Key) -> bool {
        self.get(key).is_some()
    }

    pub fn remove(&mut self, (index, generation): Key) {
        if let Some(slot) = self.slots.get_mut(index as usize)
            && slot.is_some_and(|(g, _)| g == generation)
        {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Dense;

    #[test]
    fn generations_dont_alias() {
        let mut d = Dense::default();
        d.insert((3, 1), 'a');
        assert_eq!(d.get((3, 1)), Some('a'));
        assert_eq!(d.get((3, 2)), None);
        assert_eq!(d.get((9, 1)), None);
        d.remove((3, 2));
        assert!(d.contains((3, 1)));
        d.remove((3, 1));
        assert!(!d.contains((3, 1)));
        d.insert((0, 0), 'b');
        d.clear();
        assert!(!d.contains((0, 0)));
    }
}
