//! Splits textures and samplers that HLSL uses both with and without depth comparison (shadow maps
//! read through the bindless `Texture2D` array, say) into a second binding for the comparison uses.
//! WGSL types them differently and naga rejects the mix ("inconsistent comparison sampling").
//! The clone binds at one past the highest binding of its set and is named `<name>_cmp`; image
//! clones are typed depth up front because naga can't retype a binding array.

use crate::spirv::{HEADER_WORDS, instructions, op, string};
use std::collections::HashMap;

const OP_NAME: u32 = 5;
const OP_TYPE_IMAGE: u32 = 25;
const OP_TYPE_SAMPLED_IMAGE: u32 = 27;
const OP_TYPE_ARRAY: u32 = 28;
const OP_TYPE_RUNTIME_ARRAY: u32 = 29;
const OP_TYPE_POINTER: u32 = 32;
const OP_VARIABLE: u32 = 59;
const OP_LOAD: u32 = 61;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_IN_BOUNDS_ACCESS_CHAIN: u32 = 66;
const OP_DECORATE: u32 = 71;
const OP_COPY_OBJECT: u32 = 83;
const OP_SAMPLED_IMAGE: u32 = 86;
const REGULAR: [u32; 5] = [87, 88, 91, 92, 96];
const COMPARE: [u32; 5] = [89, 90, 93, 94, 97];
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;

fn inst(opcode: u32, operands: &[u32]) -> Vec<u32> {
    let mut v = vec![((operands.len() as u32 + 1) << 16) | opcode];
    v.extend_from_slice(operands);
    v
}

fn encode(s: &str) -> Vec<u32> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.resize(bytes.len() / 4 * 4 + 4, 0);
    bytes.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

struct Module {
    insts: Vec<Vec<u32>>,
    def: HashMap<u32, usize>,
    bound: u32,
}

impl Module {
    fn get(&self, id: u32) -> Option<&[u32]> {
        self.def.get(&id).map(|&i| self.insts[i].as_slice())
    }

    fn fresh(&mut self) -> u32 {
        self.bound += 1;
        self.bound - 1
    }

    /// The global variable a loaded image/sampler (or its pointer) comes from.
    fn root(&self, id: u32) -> Option<u32> {
        let d = self.get(id)?;
        match op(d) {
            OP_VARIABLE => Some(id),
            OP_LOAD | OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN | OP_COPY_OBJECT => self.root(d[3]),
            _ => None,
        }
    }

    /// The (image, sampler) variables an `OpSampledImage` combines.
    fn sources(&self, sampled: u32) -> Option<(Option<u32>, Option<u32>)> {
        let d = self.get(sampled).filter(|d| op(d) == OP_SAMPLED_IMAGE)?;
        Some((self.root(d[3]), self.root(d[4])))
    }

    /// Re-emits the load chain of `id` from `map`ped variables with `types` retyped, before `at`.
    fn clone_chain(&mut self, id: u32, map: &HashMap<u32, u32>, types: &HashMap<u32, u32>, out: &mut Vec<Vec<u32>>) -> u32 {
        if !self.root(id).is_some_and(|v| map.contains_key(&v)) {
            return id;
        }
        let d = self.get(id).unwrap().to_vec();
        match op(&d) {
            OP_VARIABLE => map[&id],
            OP_COPY_OBJECT => self.clone_chain(d[3], map, types, out),
            _ => {
                let base = self.clone_chain(d[3], map, types, out);
                let new = self.fresh();
                let mut v = d.clone();
                v[1] = *types.get(&d[1]).unwrap_or(&d[1]);
                v[2] = new;
                v[3] = base;
                out.push(v);
                new
            }
        }
    }
}

pub fn split(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let insts: Vec<Vec<u32>> = instructions(words).map(<[u32]>::to_vec).collect();
    let def = insts.iter().enumerate().filter_map(|(n, i)| Some((result(i)?, n))).collect();
    let mut m = Module { insts, def, bound: words[3] };

    let mut kinds: HashMap<u32, (bool, bool)> = HashMap::new();
    for i in &m.insts {
        let cmp = COMPARE.contains(&op(i));
        if !cmp && !REGULAR.contains(&op(i)) {
            continue;
        }
        let Some((img, smp)) = m.sources(i[3]) else { continue };
        for var in [img, smp].into_iter().flatten() {
            let k = kinds.entry(var).or_default();
            if cmp { k.1 = true } else { k.0 = true }
        }
    }
    let mixed: Vec<u32> = kinds.iter().filter(|(_, k)| k.0 && k.1).map(|(v, _)| *v).collect();
    if mixed.is_empty() {
        return words.to_vec();
    }

    let mut map = HashMap::new();
    let mut types = HashMap::new();
    let mut globals: Vec<(usize, Vec<Vec<u32>>)> = Vec::new();
    let mut decorations = Vec::new();
    let mut next_binding: HashMap<u32, u32> = HashMap::new();
    for i in &m.insts {
        if op(i) == OP_DECORATE && i[2] == DECORATION_DESCRIPTOR_SET {
            let binding = m.insts.iter().find(|b| op(b) == OP_DECORATE && b[1] == i[1] && b[2] == DECORATION_BINDING);
            let n = next_binding.entry(i[3]).or_default();
            *n = (*n).max(binding.map_or(0, |b| b[3] + 1));
        }
    }
    for &var in &mixed {
        let at = m.def[&var];
        let ptr_ty = m.insts[at][1];
        let mut new_types = Vec::new();
        let var_ty = retype(&mut m, ptr_ty, &mut types, &mut new_types);
        let clone = m.fresh();
        new_types.push(inst(OP_VARIABLE, &[var_ty, clone, m.insts[at][3]]));
        globals.push((at, new_types));
        map.insert(var, clone);
        for i in &m.insts {
            if op(i) == OP_DECORATE && i[1] == var && i[2] == DECORATION_DESCRIPTOR_SET {
                let n = next_binding.entry(i[3]).or_default();
                decorations.push(inst(OP_DECORATE, &[clone, DECORATION_DESCRIPTOR_SET, i[3]]));
                decorations.push(inst(OP_DECORATE, &[clone, DECORATION_BINDING, *n]));
                *n += 1;
            }
            if op(i) == OP_NAME && i[1] == var {
                decorations.insert(0, inst(OP_NAME, &[[clone].as_slice(), &encode(&format!("{}_cmp", string(&i[2..])))].concat()));
            }
        }
    }

    let mut out = words[..HEADER_WORDS].to_vec();
    let first_decorate = m.insts.iter().position(|i| op(i) == OP_DECORATE).unwrap_or(0);
    for n in 0..m.insts.len() {
        let i = m.insts[n].clone();
        if n == first_decorate {
            out.extend(decorations.iter().flatten());
        }
        let cloned = |v: Option<u32>| v.is_some_and(|v| map.contains_key(&v));
        match m.sources(*i.get(3).unwrap_or(&0)) {
            Some((img, smp)) if COMPARE.contains(&op(&i)) && (cloned(img) || cloned(smp)) => {
                let si = m.get(i[3]).unwrap().to_vec();
                let mut pre = Vec::new();
                let img = m.clone_chain(si[3], &map, &types, &mut pre);
                let smp = m.clone_chain(si[4], &map, &types, &mut pre);
                let new = m.fresh();
                pre.push(inst(OP_SAMPLED_IMAGE, &[*types.get(&si[1]).unwrap_or(&si[1]), new, img, smp]));
                out.extend(pre.iter().flatten());
                out.extend(&i[..3]);
                out.push(new);
                out.extend(&i[4..]);
            }
            _ => out.extend(&i),
        }
        if let Some((_, extra)) = globals.iter().find(|(at, _)| *at == n) {
            out.extend(extra.iter().flatten());
        }
    }
    out[3] = m.bound;
    out
}

/// The clone's pointer type for variable pointer type `ptr`: a depth twin for images (recording
/// every retyped type in `types`), the same type for samplers.
fn retype(m: &mut Module, ptr: u32, types: &mut HashMap<u32, u32>, out: &mut Vec<Vec<u32>>) -> u32 {
    if let Some(&t) = types.get(&ptr) {
        return t;
    }
    let p = m.get(ptr).unwrap().to_vec();
    let (class, pointee) = (p[2], m.get(p[3]).unwrap().to_vec());
    let image = match op(&pointee) {
        OP_TYPE_IMAGE => p[3],
        OP_TYPE_ARRAY | OP_TYPE_RUNTIME_ARRAY => pointee[2],
        _ => return ptr,
    };
    let mut depth = m.get(image).unwrap().to_vec();
    if op(&depth) != OP_TYPE_IMAGE {
        return ptr;
    }
    let depth_id = m.fresh();
    depth[1] = depth_id;
    depth[4] = 1;
    out.push(depth);
    types.insert(image, depth_id);
    let twins: Vec<Vec<u32>> = m
        .insts
        .iter()
        .filter(|i| (op(i) == OP_TYPE_SAMPLED_IMAGE && i[2] == image) || (op(i) == OP_TYPE_POINTER && i[3] == image && i[1] != ptr))
        .cloned()
        .collect();
    for mut t in twins {
        let new = m.fresh();
        types.insert(t[1], new);
        t[1] = new;
        *t.last_mut().unwrap() = depth_id;
        out.push(t);
    }
    let var_pointee = if pointee[1] == image {
        depth_id
    } else {
        let mut a = pointee.clone();
        a[1] = m.fresh();
        a[2] = depth_id;
        out.push(a.clone());
        a[1]
    };
    let new_ptr = m.fresh();
    out.push(inst(OP_TYPE_POINTER, &[new_ptr, class, var_pointee]));
    types.insert(ptr, new_ptr);
    new_ptr
}

/// The result id of the instructions this pass follows.
fn result(i: &[u32]) -> Option<u32> {
    match op(i) {
        OP_TYPE_IMAGE | OP_TYPE_SAMPLED_IMAGE | OP_TYPE_ARRAY | OP_TYPE_RUNTIME_ARRAY | OP_TYPE_POINTER => i.get(1).copied(),
        OP_VARIABLE | OP_LOAD | OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN | OP_COPY_OBJECT | OP_SAMPLED_IMAGE => i.get(2).copied(),
        _ => None,
    }
}
