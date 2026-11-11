//! Clones access chains into the blocks that use them. naga's SPIR-V front end spills a value used
//! outside the block (body) that defines it into a local, and a pointer can't be stored in one
//! ("Local variable has a type that can't be stored"): DXC leaves such chains into cbuffers after
//! inlining (Fog.hlsl's sky settings in atmosphere_sky, terrain's bindless lookups). A clone
//! right before each such use keeps every pointer block-local; its index operands are values,
//! which naga spills fine.

use crate::spirv::{HEADER_WORDS, instructions, op};
use std::collections::HashMap;

const OP_FUNCTION_END: u32 = 56;
const OP_LOAD: u32 = 61;
const OP_STORE: u32 = 62;
const OP_COPY_MEMORY: u32 = 63;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_IN_BOUNDS_ACCESS_CHAIN: u32 = 66;
const OP_ATOMIC_LOAD: u32 = 227;
const OP_ATOMIC_STORE: u32 = 228;
const OP_ATOMIC_LAST: u32 = 242;
const OP_LABEL: u32 = 248;

/// Word positions in `inst` that hold a pointer operand.
fn pointer_operands(inst: &[u32]) -> &'static [usize] {
    match op(inst) {
        OP_LOAD | OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN => &[3],
        OP_STORE | OP_ATOMIC_STORE => &[1],
        OP_COPY_MEMORY => &[1, 2],
        o if (OP_ATOMIC_LOAD..=OP_ATOMIC_LAST).contains(&o) => &[3],
        _ => &[],
    }
}

fn is_chain(inst: &[u32]) -> bool {
    matches!(op(inst), OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN)
}

/// Each instruction with the block it sits in (`None` outside functions).
fn with_blocks(words: &[u32]) -> Vec<(Option<u32>, &[u32])> {
    let mut block = None;
    instructions(words)
        .map(|inst| {
            match op(inst) {
                OP_LABEL => block = Some(inst[1]),
                OP_FUNCTION_END => block = None,
                _ => {}
            }
            (block, inst)
        })
        .collect()
}

struct Clones<'a> {
    chains: HashMap<u32, (Option<u32>, &'a [u32])>,
    made: HashMap<(u32, u32), u32>,
    bound: u32,
}

impl Clones<'_> {
    /// `id` as used in `block`: itself when defined there (or not a chain), else a clone emitted
    /// into `out`.
    fn local(&mut self, id: u32, block: u32, out: &mut Vec<u32>) -> u32 {
        let Some(&(def, chain)) = self.chains.get(&id) else { return id };
        if def == Some(block) {
            return id;
        }
        if let Some(&made) = self.made.get(&(block, id)) {
            return made;
        }
        let mut clone = chain.to_vec();
        clone[3] = self.local(chain[3], block, out);
        clone[2] = self.bound;
        self.bound += 1;
        self.made.insert((block, id), clone[2]);
        out.extend(&clone);
        clone[2]
    }
}

pub fn access_chains(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let insts = with_blocks(words);
    let chains = insts.iter().filter(|(_, i)| is_chain(i)).map(|(b, i)| (i[2], (*b, *i))).collect();
    let mut clones = Clones { chains, made: HashMap::new(), bound: words[3] };
    let mut out = words[..HEADER_WORDS].to_vec();
    for (block, inst) in insts {
        let mut inst = inst.to_vec();
        if let Some(block) = block {
            for &p in pointer_operands(&inst) {
                if let Some(&id) = inst.get(p) {
                    inst[p] = clones.local(id, block, &mut out);
                }
            }
        }
        out.extend(inst);
    }
    out[3] = clones.bound;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(opcode: u32, operands: &[u32]) -> Vec<u32> {
        let mut v = vec![((operands.len() as u32 + 1) << 16) | opcode];
        v.extend_from_slice(operands);
        v
    }

    #[test]
    fn chains_used_in_another_block_are_cloned_there() {
        // %10 = chain(%5, %6) in block %1; loaded in block %2 and twice in %3.
        let mut words = vec![0x0723_0203, 0x0001_0000, 0, 100, 0];
        for i in [inst(54, &[7, 8, 0, 9]), inst(OP_LABEL, &[1]), inst(OP_ACCESS_CHAIN, &[11, 10, 5, 6]), inst(OP_ACCESS_CHAIN, &[11, 12, 10, 6]), inst(OP_LABEL, &[2]), inst(OP_LOAD, &[4, 20, 12]), inst(OP_LABEL, &[3]), inst(OP_LOAD, &[4, 21, 10]), inst(OP_LOAD, &[4, 22, 10]), inst(OP_FUNCTION_END, &[])] {
            words.extend(i);
        }
        let out = access_chains(&words);
        assert_eq!(out[3], 103, "three clones");
        let body: Vec<_> = instructions(&out).map(|i| i.to_vec()).collect();
        assert_eq!(body[5], inst(OP_ACCESS_CHAIN, &[11, 100, 5, 6]), "the base chain, cloned into %2");
        assert_eq!(body[6], inst(OP_ACCESS_CHAIN, &[11, 101, 100, 6]));
        assert_eq!(body[7], inst(OP_LOAD, &[4, 20, 101]));
        assert_eq!(body[9], inst(OP_ACCESS_CHAIN, &[11, 102, 5, 6]));
        assert_eq!(body[10], inst(OP_LOAD, &[4, 21, 102]));
        assert_eq!(body[11], inst(OP_LOAD, &[4, 22, 102]), "one clone per block");
    }
}
