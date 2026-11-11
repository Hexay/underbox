//! naga's SPIR-V frontend rejects `OpImageTexelPointer` ("unsupported storage class": the Image
//! pointer class), which DXC emits for `Interlocked*` on a `RWTexture2D<uint>` (CMAA2's
//! `InterlockedExchange( Cmaa2BlendHeads[quad], .. )`), and naga's own image atomics return no
//! value. This pass moves such an image's texels into a storage buffer of words named
//! `<image>_texel_words` (row-major, [`row_words`] per row) and makes every texel access in
//! the module a real atomic on its word: atomics keep their op, image reads and writes become
//! atomic loads and stores. The image stays bound for its size. Other modules reach the same
//! texels through the image itself (CMAA2 clears its heads in one pass and walks them in
//! another), so the renderer copies the image into the words before each dispatch and back
//! after (engine2 `shading/texel_words.rs`); the row pitch is what a buffer-texture copy needs.
//! A module whose image isn't a 2D `uint` one or has no name to bind by is returned unchanged.

use crate::spirv::{HEADER_WORDS, inst, instructions, op, string};
use std::collections::HashMap;

const OP_NAME: u32 = 5;
const OP_TYPE_INT: u32 = 21;
const OP_TYPE_VECTOR: u32 = 23;
const OP_TYPE_IMAGE: u32 = 25;
const OP_TYPE_RUNTIME_ARRAY: u32 = 29;
const OP_TYPE_STRUCT: u32 = 30;
const OP_TYPE_POINTER: u32 = 32;
const OP_CONSTANT: u32 = 43;
const OP_FUNCTION: u32 = 54;
const OP_VARIABLE: u32 = 59;
const OP_IMAGE_TEXEL_POINTER: u32 = 60;
const OP_LOAD: u32 = 61;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_DECORATE: u32 = 71;
const OP_MEMBER_DECORATE: u32 = 72;
const OP_COMPOSITE_CONSTRUCT: u32 = 80;
const OP_COMPOSITE_EXTRACT: u32 = 81;
const OP_IMAGE_READ: u32 = 98;
const OP_IMAGE_WRITE: u32 = 99;
const OP_IMAGE_QUERY_SIZE: u32 = 104;
const OP_BITCAST: u32 = 124;
const OP_IADD: u32 = 128;
const OP_IMUL: u32 = 132;
const OP_BITWISE_AND: u32 = 199;
const OP_ATOMIC_LOAD: u32 = 227;
const OP_ATOMIC_STORE: u32 = 228;
const DECORATION_BLOCK: u32 = 2;
const DECORATION_ARRAY_STRIDE: u32 = 6;
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;
const DECORATION_OFFSET: u32 = 35;
const STORAGE_UNIFORM_CONSTANT: u32 = 0;
const STORAGE_IMAGE: u32 = 11;
const STORAGE_BUFFER: u32 = 12;
const DIM_2D: u32 = 1;
const SCOPE_DEVICE: u32 = 1;

/// The suffix of the word buffer standing in for an image's texels.
pub const TEXEL_WORDS: &str = "_texel_words";

/// Words per row of an image `width` texels wide: rows padded to 256 bytes, wgpu's
/// buffer-texture copy alignment.
pub const fn row_words(width: u32) -> u32 {
    (width + ROW_ALIGN) & !ROW_ALIGN
}

const ROW_ALIGN: u32 = 63;

fn encode(s: &str) -> Vec<u32> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.resize(bytes.len() / 4 * 4 + 4, 0);
    bytes.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

#[derive(Default)]
struct Scan {
    names: HashMap<u32, String>,
    pointee: HashMap<u32, (u32, u32)>,
    var_type: HashMap<u32, u32>,
    images: HashMap<u32, (u32, u32)>,
    vectors: HashMap<u32, (u32, u32)>,
    uint: Option<u32>,
    sets: HashMap<u32, u32>,
    bindings: HashMap<u32, u32>,
    /// Image loads: the variable each loaded.
    loads: HashMap<u32, u32>,
    /// Texel pointers: their image variable and coordinate.
    texels: HashMap<u32, (u32, u32)>,
    /// Result types inside functions, by first appearance (definitions precede uses).
    types: HashMap<u32, u32>,
}

fn scan(words: &[u32]) -> Scan {
    let mut s = Scan::default();
    let mut in_function = false;
    for i in instructions(words) {
        in_function |= op(i) == OP_FUNCTION;
        if in_function && i.len() >= 3 {
            s.types.entry(i[2]).or_insert(i[1]);
        }
        match op(i) {
            OP_NAME if i.len() >= 3 => drop(s.names.insert(i[1], string(&i[2..]))),
            OP_TYPE_INT if i.len() >= 4 && i[2] == 32 && i[3] == 0 => s.uint = s.uint.or(Some(i[1])),
            OP_TYPE_VECTOR if i.len() >= 4 => drop(s.vectors.insert(i[1], (i[2], i[3]))),
            OP_TYPE_IMAGE if i.len() >= 4 => drop(s.images.insert(i[1], (i[2], i[3]))),
            OP_TYPE_POINTER if i.len() >= 4 => drop(s.pointee.insert(i[1], (i[2], i[3]))),
            OP_VARIABLE if i.len() >= 4 => drop(s.var_type.insert(i[2], i[1])),
            OP_DECORATE if i.len() >= 4 && i[2] == DECORATION_DESCRIPTOR_SET => drop(s.sets.insert(i[1], i[3])),
            OP_DECORATE if i.len() >= 4 && i[2] == DECORATION_BINDING => drop(s.bindings.insert(i[1], i[3])),
            OP_LOAD if i.len() >= 4 => drop(s.loads.insert(i[2], i[3])),
            OP_IMAGE_TEXEL_POINTER if i.len() >= 6 => drop(s.texels.insert(i[2], (i[3], i[4]))),
            _ => {}
        }
    }
    s
}

/// An image whose texels move to words: its image type and its buffer variable.
struct Moved {
    image_type: u32,
    buffer: u32,
}

/// Ids the lowering declares once.
struct Ids {
    uint: u32,
    uvec2: u32,
    zero: u32,
    scope: u32,
    align: u32,
    align_mask: u32,
    array: u32,
    block: u32,
    block_pointer: u32,
    word_pointer: u32,
}

struct Lowering {
    bound: u32,
    out: Vec<u32>,
}

impl Lowering {
    fn fresh(&mut self) -> u32 {
        self.bound += 1;
        self.bound - 1
    }

    fn emit(&mut self, opcode: u32, operands: &[u32]) {
        self.out.extend(inst(opcode, operands));
    }

    /// A pointer to the word of `coord`'s texel: y × [`row_words`] + x, of `image`'s (loaded) width.
    fn word(&mut self, ids: &Ids, m: &Moved, image: u32, coord: u32, result: Option<u32>) -> u32 {
        let (size, texels, padded, width) = (self.fresh(), self.fresh(), self.fresh(), self.fresh());
        let (c, x, y, row, at) = (self.fresh(), self.fresh(), self.fresh(), self.fresh(), self.fresh());
        self.emit(OP_IMAGE_QUERY_SIZE, &[ids.uvec2, size, image]);
        self.emit(OP_COMPOSITE_EXTRACT, &[ids.uint, texels, size, 0]);
        self.emit(OP_IADD, &[ids.uint, padded, texels, ids.align]);
        self.emit(OP_BITWISE_AND, &[ids.uint, width, padded, ids.align_mask]);
        // Coordinates may be signed (`int2`); the bits are the same.
        self.emit(OP_BITCAST, &[ids.uvec2, c, coord]);
        self.emit(OP_COMPOSITE_EXTRACT, &[ids.uint, x, c, 0]);
        self.emit(OP_COMPOSITE_EXTRACT, &[ids.uint, y, c, 1]);
        self.emit(OP_IMUL, &[ids.uint, row, y, width]);
        self.emit(OP_IADD, &[ids.uint, at, row, x]);
        let pointer = result.unwrap_or_else(|| self.fresh());
        self.emit(OP_ACCESS_CHAIN, &[ids.word_pointer, pointer, m.buffer, ids.zero, at]);
        pointer
    }

    fn declare(&mut self, ids: &Ids, declare_uvec2: bool, moved: &[(u32, Moved)]) {
        if declare_uvec2 {
            self.emit(OP_TYPE_VECTOR, &[ids.uvec2, ids.uint, 2]);
        }
        self.emit(OP_CONSTANT, &[ids.uint, ids.zero, 0]);
        self.emit(OP_CONSTANT, &[ids.uint, ids.scope, SCOPE_DEVICE]);
        self.emit(OP_CONSTANT, &[ids.uint, ids.align, ROW_ALIGN]);
        self.emit(OP_CONSTANT, &[ids.uint, ids.align_mask, !ROW_ALIGN]);
        self.emit(OP_TYPE_RUNTIME_ARRAY, &[ids.array, ids.uint]);
        self.emit(OP_TYPE_STRUCT, &[ids.block, ids.array]);
        self.emit(OP_TYPE_POINTER, &[ids.block_pointer, STORAGE_BUFFER, ids.block]);
        self.emit(OP_TYPE_POINTER, &[ids.word_pointer, STORAGE_BUFFER, ids.uint]);
        for (_, m) in moved {
            self.emit(OP_VARIABLE, &[ids.block_pointer, m.buffer, STORAGE_BUFFER]);
        }
    }
}

pub fn lower_image_atomics(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let s = scan(words);
    let Some(uint) = s.uint.filter(|_| !s.texels.is_empty()) else { return words.to_vec() };
    let mut l = Lowering { bound: words[3], out: words[..HEADER_WORDS].to_vec() };
    let mut variables: Vec<u32> = s.texels.values().map(|(v, _)| *v).collect();
    variables.sort_unstable();
    variables.dedup();
    let mut moved: Vec<(u32, Moved)> = Vec::new();
    for v in variables {
        let image_type = s.var_type.get(&v).and_then(|p| s.pointee.get(p)).filter(|(class, _)| *class == STORAGE_UNIFORM_CONSTANT).map(|(_, t)| *t);
        let Some(image_type) = image_type.filter(|t| s.images.get(t) == Some(&(uint, DIM_2D))) else { return words.to_vec() };
        if !s.names.contains_key(&v) || !s.sets.contains_key(&v) {
            return words.to_vec();
        }
        moved.push((v, Moved { image_type, buffer: l.fresh() }));
    }
    let existing = s.vectors.iter().find(|(_, c)| **c == (uint, 2)).map(|(id, _)| *id);
    let uvec2 = existing.unwrap_or_else(|| l.fresh());
    let ids = Ids { uint, uvec2, zero: l.fresh(), scope: l.fresh(), align: l.fresh(), align_mask: l.fresh(), array: l.fresh(), block: l.fresh(), block_pointer: l.fresh(), word_pointer: l.fresh() };
    let find = |v: u32| moved.iter().position(|(id, _)| *id == v);
    let mut declared = false;
    for i in instructions(words) {
        let image_of = |at: usize| i.get(at).and_then(|load| s.loads.get(load)).and_then(|v| find(*v));
        match op(i) {
            OP_TYPE_POINTER if s.pointee.get(&i[1]).is_some_and(|(class, _)| *class == STORAGE_IMAGE) => continue,
            OP_FUNCTION if !std::mem::replace(&mut declared, true) => l.declare(&ids, existing.is_none(), &moved),
            OP_NAME if find(i[1]).is_some() => {
                let m = &moved[find(i[1]).unwrap()].1;
                l.out.extend_from_slice(i);
                l.emit(OP_NAME, &[[m.buffer].as_slice(), &encode(&format!("{}{TEXEL_WORDS}", s.names[&i[1]]))].concat());
                continue;
            }
            OP_DECORATE if i.len() >= 4 && i[2] == DECORATION_DESCRIPTOR_SET && find(i[1]).is_some() => {
                let order = find(i[1]).unwrap();
                let set = i[3];
                let last = s.bindings.iter().filter(|(id, _)| s.sets.get(id) == Some(&set)).map(|(_, b)| *b).max().unwrap_or(0);
                l.out.extend_from_slice(i);
                l.emit(OP_DECORATE, &[moved[order].1.buffer, DECORATION_DESCRIPTOR_SET, set]);
                l.emit(OP_DECORATE, &[moved[order].1.buffer, DECORATION_BINDING, last + 1 + order as u32]);
                if order == 0 {
                    l.emit(OP_DECORATE, &[ids.block, DECORATION_BLOCK]);
                    l.emit(OP_MEMBER_DECORATE, &[ids.block, 0, DECORATION_OFFSET, 0]);
                    l.emit(OP_DECORATE, &[ids.array, DECORATION_ARRAY_STRIDE, 4]);
                }
                continue;
            }
            OP_IMAGE_TEXEL_POINTER if s.texels.contains_key(&i[2]) => {
                let (v, coord) = s.texels[&i[2]];
                let m = &moved[find(v).unwrap()].1;
                let image = l.fresh();
                l.emit(OP_LOAD, &[m.image_type, image, v]);
                l.word(&ids, m, image, coord, Some(i[2]));
                continue;
            }
            OP_IMAGE_READ if i.len() >= 5 && image_of(3).is_some() => {
                let m = &moved[image_of(3).unwrap()].1;
                let pointer = l.word(&ids, m, i[3], i[4], None);
                let n = s.vectors.get(&i[1]).map(|(_, n)| *n as usize);
                let word = if n.is_some() { l.fresh() } else { i[2] };
                l.emit(OP_ATOMIC_LOAD, &[uint, word, pointer, ids.scope, ids.zero]);
                if let Some(n) = n {
                    l.emit(OP_COMPOSITE_CONSTRUCT, &[[i[1], i[2]].as_slice(), &vec![word; n]].concat());
                }
                continue;
            }
            OP_IMAGE_WRITE if i.len() >= 4 && image_of(1).is_some() => {
                let m = &moved[image_of(1).unwrap()].1;
                let pointer = l.word(&ids, m, i[1], i[2], None);
                let value = if s.types.get(&i[3]).is_some_and(|t| s.vectors.contains_key(t)) {
                    let first = l.fresh();
                    l.emit(OP_COMPOSITE_EXTRACT, &[uint, first, i[3], 0]);
                    first
                } else {
                    i[3]
                };
                l.emit(OP_ATOMIC_STORE, &[pointer, ids.scope, ids.zero, value]);
                continue;
            }
            _ => {}
        }
        l.out.extend_from_slice(i);
    }
    l.out[3] = l.bound;
    l.out
}

#[cfg(test)]
#[path = "image_atomics_tests.rs"]
mod tests;
