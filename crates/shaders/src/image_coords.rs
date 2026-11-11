//! naga's SPIR-V frontend splits an arrayed image's coordinates by looking up a vector type of
//! the coordinates' scalar kind, which it only registers for floats; a storage image array
//! addressed by `uint3` (`RWTexture2DArray`, envmap_filtering_cs, ddgi_depth_copy_cs) panics
//! when the module declares no `uint2`. This pass declares the 2- and 3-component vectors of
//! every 32-bit integer type the module lacks.

use crate::spirv::{HEADER_WORDS, instructions, op};

const OP_TYPE_INT: u32 = 21;
const OP_TYPE_VECTOR: u32 = 23;

fn inst(opcode: u32, operands: &[u32]) -> Vec<u32> {
    let mut v = vec![((operands.len() as u32 + 1) << 16) | opcode];
    v.extend(operands);
    v
}

pub fn declare_integer_vectors(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let vectors: Vec<(u32, u32)> = instructions(words).filter(|i| op(i) == OP_TYPE_VECTOR && i.len() >= 4).map(|i| (i[2], i[3])).collect();
    let mut bound = words[3];
    let mut out = words[..HEADER_WORDS].to_vec();
    for i in instructions(words) {
        out.extend_from_slice(i);
        if op(i) == OP_TYPE_INT && i.len() >= 4 && i[2] == 32 {
            for size in [2, 3] {
                if !vectors.contains(&(i[1], size)) {
                    out.extend(inst(OP_TYPE_VECTOR, &[bound, i[1], size]));
                    bound += 1;
                }
            }
        }
    }
    out[3] = bound;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_only_the_missing_vectors() {
        let mut m = vec![0x0723_0203, 0x10000, 0, 10, 0];
        m.extend(inst(OP_TYPE_INT, &[1, 32, 0]));
        m.extend(inst(OP_TYPE_VECTOR, &[2, 1, 3]));
        m.extend(inst(OP_TYPE_INT, &[3, 16, 1]));
        let out = declare_integer_vectors(&m);
        let vectors: Vec<_> = instructions(&out).filter(|i| op(i) == OP_TYPE_VECTOR).map(<[u32]>::to_vec).collect();
        assert_eq!(vectors, [inst(OP_TYPE_VECTOR, &[10, 1, 2]), inst(OP_TYPE_VECTOR, &[2, 1, 3])]);
        assert_eq!(out[3], 11);
    }
}
