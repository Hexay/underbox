//! Works around naga sizing non-square `RowMajor` struct members (HLSL's default `column_major
//! float4x3`) as column-major: the member gets the transposed matrix type, which has the right
//! layout, and naga's own row-major handling transposes it back on load. Square matrices are
//! unaffected.

use crate::spirv::{HEADER_WORDS, instructions, op};
use std::collections::HashMap;

const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_MATRIX: u32 = 24;
const OP_TYPE_STRUCT: u32 = 30;
const OP_MEMBER_DECORATE: u32 = 72;
const DECORATION_ROW_MAJOR: u32 = 4;

pub fn transpose_members(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let insts: Vec<&[u32]> = instructions(words).collect();
    let row_major: Vec<(u32, u32)> = insts
        .iter()
        .filter(|i| op(i) == OP_MEMBER_DECORATE && i.len() >= 4 && i[3] == DECORATION_ROW_MAJOR)
        .map(|i| (i[1], i[2]))
        .collect();
    let types: HashMap<u32, &[u32]> = insts.iter().filter(|i| matches!(op(i), OP_TYPE_VECTOR | OP_TYPE_MATRIX)).map(|i| (i[1], *i)).collect();
    // matrix id -> (component type, rows, columns)
    let shape = |id: u32| {
        let m = types.get(&id).filter(|m| op(m) == OP_TYPE_MATRIX)?;
        let col = types.get(&m[2])?;
        Some((col[2], col[3], m[3]))
    };

    let mut bound = words[3];
    let mut fresh = || {
        bound += 1;
        bound - 1
    };
    let mut vectors: HashMap<(u32, u32), u32> = types.values().filter(|t| op(t) == OP_TYPE_VECTOR).map(|t| ((t[2], t[3]), t[1])).collect();
    let mut matrices: HashMap<(u32, u32), u32> = HashMap::new();
    let mut out = words[..HEADER_WORDS].to_vec();
    for inst in insts {
        if op(inst) != OP_TYPE_STRUCT || !row_major.iter().any(|(s, _)| *s == inst[1]) {
            out.extend(inst);
            continue;
        }
        let mut st = inst.to_vec();
        for (n, member) in st[2..].iter_mut().enumerate() {
            let Some((comp, rows, cols)) = shape(*member).filter(|(_, r, c)| r != c) else { continue };
            if !row_major.contains(&(inst[1], n as u32)) {
                continue;
            }
            let vec = *vectors.entry((comp, cols)).or_insert_with(|| {
                let id = fresh();
                out.extend([(4 << 16) | OP_TYPE_VECTOR, id, comp, cols]);
                id
            });
            *member = *matrices.entry((vec, rows)).or_insert_with(|| {
                let id = fresh();
                out.extend([(4 << 16) | OP_TYPE_MATRIX, id, vec, rows]);
                id
            });
        }
        out.extend(st);
    }
    out[3] = bound;
    out
}
