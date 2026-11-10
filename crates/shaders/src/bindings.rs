//! WGSL forbids two resources at one (group, binding), which Vulkan HLSL uses for the bindless
//! arrays (`g_bindless_Texture2D`, `..._TextureCube`, ... all at set 1 binding 16). Every alias
//! after the first moves past the highest binding of its set, in declaration order; reflection
//! reports where each name ended up.

use crate::spirv::{HEADER_WORDS, instructions, op};
use std::collections::{HashMap, HashSet};

const OP_DECORATE: u32 = 71;
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;

pub fn unalias(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let decorated = |kind: u32| -> HashMap<u32, u32> {
        instructions(words).filter(|i| op(i) == OP_DECORATE && i.len() >= 4 && i[2] == kind).map(|i| (i[1], i[3])).collect()
    };
    let sets = decorated(DECORATION_DESCRIPTOR_SET);
    let mut next: HashMap<u32, u32> = HashMap::new();
    for (id, b) in decorated(DECORATION_BINDING) {
        let n = next.entry(sets.get(&id).copied().unwrap_or(0)).or_default();
        *n = (*n).max(b + 1);
    }
    let mut seen = HashSet::new();
    let mut out = words[..HEADER_WORDS].to_vec();
    for i in instructions(words) {
        let mut i = i.to_vec();
        if op(&i) == OP_DECORATE && i.len() >= 4 && i[2] == DECORATION_BINDING {
            let set = sets.get(&i[1]).copied().unwrap_or(0);
            if !seen.insert((set, i[3])) {
                let n = next.entry(set).or_default();
                i[3] = *n;
                *n += 1;
            }
        }
        out.extend(i);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deco(id: u32, kind: u32, value: u32) -> [u32; 4] {
        [(4 << 16) | OP_DECORATE, id, kind, value]
    }

    #[test]
    fn later_aliases_move_past_the_set() {
        let mut w = vec![0x0723_0203, 0, 0, 10, 0];
        for (id, set, binding) in [(1, 1, 16), (2, 1, 16), (3, 1, 15), (4, 0, 16)] {
            w.extend(deco(id, DECORATION_DESCRIPTOR_SET, set));
            w.extend(deco(id, DECORATION_BINDING, binding));
        }
        let out = unalias(&w);
        let bindings: Vec<(u32, u32)> =
            instructions(&out).filter(|i| op(i) == OP_DECORATE && i[2] == DECORATION_BINDING).map(|i| (i[1], i[3])).collect();
        assert_eq!(bindings, [(1, 16), (2, 17), (3, 15), (4, 16)]);
    }
}
