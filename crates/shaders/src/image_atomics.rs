//! naga's SPIR-V frontend rejects `OpImageTexelPointer` ("unsupported storage class": the Image
//! pointer class), which DXC emits for `Interlocked*` on a `RWTexture2D<uint>` (CMAA2's
//! `InterlockedExchange( Cmaa2BlendHeads[quad], .. )`), and naga's own image atomics return no
//! value. This pass lowers each such atomic to an image read, the operation, and an image write.
//!
//! Not atomic: two invocations racing on one texel can lose one's update. CMAA2 tolerates that:
//! the heads form linked lists of blend items, a lost exchange drops one item from a quad's list
//! (that pixel blends from fewer samples) and two "first" writers queue the quad twice (resolved
//! twice to the same colour); the list itself stays acyclic and every link valid.
//! A module with an atomic this pass doesn't know is returned unchanged.

use crate::spirv::{HEADER_WORDS, inst, instructions, op};
use std::collections::HashMap;

const OP_TYPE_BOOL: u32 = 20;
const OP_TYPE_INT: u32 = 21;
const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_POINTER: u32 = 32;
const OP_VARIABLE: u32 = 59;
const OP_IMAGE_TEXEL_POINTER: u32 = 60;
const OP_LOAD: u32 = 61;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_COMPOSITE_EXTRACT: u32 = 81;
const OP_IMAGE_READ: u32 = 98;
const OP_IMAGE_WRITE: u32 = 99;
const OP_IADD: u32 = 128;
const OP_ISUB: u32 = 130;
const OP_SELECT: u32 = 169;
const OP_IEQUAL: u32 = 170;
const OP_UGREATER_THAN: u32 = 172;
const OP_SGREATER_THAN: u32 = 173;
const OP_ULESS_THAN: u32 = 176;
const OP_SLESS_THAN: u32 = 177;
const OP_BITWISE_OR: u32 = 197;
const OP_BITWISE_XOR: u32 = 198;
const OP_BITWISE_AND: u32 = 199;
const OP_ATOMIC_LOAD: u32 = 227;
const OP_ATOMIC_STORE: u32 = 228;
const OP_ATOMIC_EXCHANGE: u32 = 229;
const OP_ATOMIC_COMPARE_EXCHANGE: u32 = 230;
const OP_ATOMIC_IADD: u32 = 234;
const OP_ATOMIC_ISUB: u32 = 235;
const OP_ATOMIC_SMIN: u32 = 236;
const OP_ATOMIC_UMIN: u32 = 237;
const OP_ATOMIC_SMAX: u32 = 238;
const OP_ATOMIC_UMAX: u32 = 239;
const OP_ATOMIC_AND: u32 = 240;
const OP_ATOMIC_OR: u32 = 241;
const OP_ATOMIC_XOR: u32 = 242;
const STORAGE_IMAGE: u32 = 11;

/// How an atomic's new value comes from the old one and its operand.
enum Update {
    Keep,
    Replace,
    Binary(u32),
    /// Keep the old value where `compare(old, value)` holds (min with `<`, max with `>`).
    Pick(u32),
    CompareExchange,
}

fn update(opcode: u32) -> Option<Update> {
    Some(match opcode {
        OP_ATOMIC_LOAD => Update::Keep,
        OP_ATOMIC_STORE | OP_ATOMIC_EXCHANGE => Update::Replace,
        OP_ATOMIC_IADD => Update::Binary(OP_IADD),
        OP_ATOMIC_ISUB => Update::Binary(OP_ISUB),
        OP_ATOMIC_AND => Update::Binary(OP_BITWISE_AND),
        OP_ATOMIC_OR => Update::Binary(OP_BITWISE_OR),
        OP_ATOMIC_XOR => Update::Binary(OP_BITWISE_XOR),
        OP_ATOMIC_UMIN => Update::Pick(OP_ULESS_THAN),
        OP_ATOMIC_SMIN => Update::Pick(OP_SLESS_THAN),
        OP_ATOMIC_UMAX => Update::Pick(OP_UGREATER_THAN),
        OP_ATOMIC_SMAX => Update::Pick(OP_SGREATER_THAN),
        OP_ATOMIC_COMPARE_EXCHANGE => Update::CompareExchange,
        _ => return None,
    })
}

struct Texel {
    image_pointer: u32,
    coord: u32,
    scalar: u32,
}

pub fn lower_image_atomics(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let mut pointee = HashMap::new();
    let mut image_pointers = Vec::new();
    let mut pointer_type = HashMap::new();
    let mut texels = HashMap::new();
    let mut bool_type = None;
    let mut vec4 = HashMap::new();
    for i in instructions(words) {
        match op(i) {
            OP_TYPE_POINTER if i.len() >= 4 => {
                pointee.insert(i[1], i[3]);
                if i[2] == STORAGE_IMAGE {
                    image_pointers.push(i[1]);
                }
            }
            OP_TYPE_BOOL => bool_type = Some(i[1]),
            OP_TYPE_VECTOR if i.len() >= 4 && i[3] == 4 => {
                vec4.insert(i[2], i[1]);
            }
            OP_VARIABLE | OP_ACCESS_CHAIN if i.len() >= 3 => {
                pointer_type.insert(i[2], i[1]);
            }
            OP_IMAGE_TEXEL_POINTER if i.len() >= 6 => {
                let scalar = pointee.get(&i[1]).copied().unwrap_or(0);
                texels.insert(i[2], Texel { image_pointer: i[3], coord: i[4], scalar });
            }
            _ => {}
        }
    }
    if texels.is_empty() {
        return words.to_vec();
    }
    let atomic_on_texel = |i: &[u32]| {
        if !(OP_ATOMIC_LOAD..=OP_ATOMIC_XOR).contains(&op(i)) {
            return None;
        }
        let pointer_at = if op(i) == OP_ATOMIC_STORE { 1 } else { 3 };
        i.get(pointer_at).filter(|p| texels.contains_key(p)).map(|_| pointer_at)
    };
    let unsupported = instructions(words).any(|i| atomic_on_texel(i).is_some() && update(op(i)).is_none());
    if unsupported {
        return words.to_vec();
    }

    let mut bound = words[3];
    let mut fresh = || {
        bound += 1;
        bound - 1
    };
    let mut declare_bool = bool_type.is_none();
    let bool_type = bool_type.unwrap_or_else(&mut fresh);
    let mut scalars: Vec<u32> = texels.values().map(|t| t.scalar).collect();
    scalars.sort_unstable();
    scalars.dedup();
    scalars.retain(|s| !vec4.contains_key(s));
    let mut new_vec4 = Vec::new();
    for s in scalars {
        let id = fresh();
        vec4.insert(s, id);
        new_vec4.push((s, id));
    }

    let mut out = words[..HEADER_WORDS].to_vec();
    for i in instructions(words) {
        match op(i) {
            OP_IMAGE_TEXEL_POINTER if texels.contains_key(&i[2]) => continue,
            OP_TYPE_POINTER if image_pointers.contains(&i[1]) => continue,
            _ => {}
        }
        let Some(at) = atomic_on_texel(i) else {
            out.extend_from_slice(i);
            if op(i) == OP_TYPE_INT && i.len() >= 4 {
                if std::mem::take(&mut declare_bool) {
                    out.extend(inst(OP_TYPE_BOOL, &[bool_type]));
                }
                for (_, id) in new_vec4.iter().filter(|(s, _)| *s == i[1]) {
                    out.extend(inst(OP_TYPE_VECTOR, &[*id, i[1], 4]));
                }
            }
            continue;
        };
        let t = &texels[&i[at]];
        let image_type = pointer_type.get(&t.image_pointer).and_then(|p| pointee.get(p)).copied().unwrap_or(0);
        let image = fresh();
        out.extend(inst(OP_LOAD, &[image_type, image, t.image_pointer]));
        let value = *i.last().unwrap();
        if op(i) == OP_ATOMIC_STORE {
            out.extend(inst(OP_IMAGE_WRITE, &[image, t.coord, value]));
            continue;
        }
        let texel = fresh();
        out.extend(inst(OP_IMAGE_READ, &[vec4[&t.scalar], texel, image, t.coord]));
        let old = i[2];
        out.extend(inst(OP_COMPOSITE_EXTRACT, &[t.scalar, old, texel, 0]));
        let new = match update(op(i)).unwrap() {
            Update::Keep => continue,
            Update::Replace => value,
            Update::Binary(o) => {
                let n = fresh();
                out.extend(inst(o, &[t.scalar, n, old, value]));
                n
            }
            Update::Pick(compare) => {
                let (keep, n) = (fresh(), fresh());
                out.extend(inst(compare, &[bool_type, keep, old, value]));
                out.extend(inst(OP_SELECT, &[t.scalar, n, keep, old, value]));
                n
            }
            Update::CompareExchange => {
                let (swap, n) = (fresh(), fresh());
                out.extend(inst(OP_IEQUAL, &[bool_type, swap, old, i[8]]));
                out.extend(inst(OP_SELECT, &[t.scalar, n, swap, i[7], old]));
                n
            }
        };
        out.extend(inst(OP_IMAGE_WRITE, &[image, t.coord, new]));
    }
    out[3] = bound;
    out
}

#[cfg(test)]
#[path = "image_atomics_tests.rs"]
mod tests;
