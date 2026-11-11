//! naga's SPIR-V frontend types an `OpImageFetch`/`OpImageRead` image by its variable, so a fetch
//! from a bindless array element (DDGI's `Bindless::GetTexture3D(i).Load`, particle_lighting_cs)
//! sees the binding array's type and fails with "invalid image". This pass moves each such fetch
//! into its own function taking the image as a parameter, which naga types correctly; the call
//! passes the array element. Ids defined outside functions (constants, `ConstOffset`) stay direct.

use crate::spirv::{HEADER_WORDS, inst, instructions, op};
use std::collections::{HashMap, HashSet};

const OP_UNDEF: u32 = 1;
/// OpConstantTrue … OpSpecConstantOp.
const OP_CONSTANT_FIRST: u32 = 41;
const OP_CONSTANT_LAST: u32 = 52;
const OP_TYPE_FIRST: u32 = 19;
const OP_TYPE_LAST: u32 = 39;
const OP_TYPE_RUNTIME_ARRAY: u32 = 29;
const OP_TYPE_POINTER: u32 = 32;
const OP_TYPE_FUNCTION: u32 = 33;
const OP_VARIABLE: u32 = 59;
const OP_LOAD: u32 = 61;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_IN_BOUNDS_ACCESS_CHAIN: u32 = 66;
const OP_FUNCTION: u32 = 54;
const OP_FUNCTION_PARAMETER: u32 = 55;
const OP_FUNCTION_END: u32 = 56;
const OP_FUNCTION_CALL: u32 = 57;
const OP_IMAGE_FETCH: u32 = 95;
const OP_IMAGE_READ: u32 = 98;
const OP_LABEL: u32 = 248;
const OP_RETURN_VALUE: u32 = 254;


struct Module<'a> {
    /// Result id → result type, for value instructions (first definition wins).
    types: HashMap<u32, u32>,
    /// Ids defined outside any function.
    globals: HashSet<u32>,
    /// Images loaded from an element of a runtime-array variable.
    bindless_images: HashSet<u32>,
    insts: Vec<&'a [u32]>,
}

impl<'a> Module<'a> {
    fn read(words: &'a [u32]) -> Self {
        let insts: Vec<&[u32]> = instructions(words).collect();
        let (mut types, mut globals) = (HashMap::new(), HashSet::new());
        let mut pointee = HashMap::new();
        let mut var_type = HashMap::new();
        let mut chains = HashMap::new();
        let mut in_function = false;
        let mut bindless_images = HashSet::new();
        for i in &insts {
            let o = op(i);
            match o {
                OP_FUNCTION => in_function = true,
                OP_FUNCTION_END => in_function = false,
                _ => {}
            }
            if (OP_TYPE_FIRST..=OP_TYPE_LAST).contains(&o) {
                if o == OP_TYPE_POINTER && i.len() >= 4 {
                    pointee.insert(i[1], i[3]);
                }
                globals.insert(i[1]);
                continue;
            }
            let global_value = o == OP_UNDEF || (OP_CONSTANT_FIRST..=OP_CONSTANT_LAST).contains(&o) || o == OP_VARIABLE;
            if i.len() >= 3 && (in_function || global_value) {
                types.entry(i[2]).or_insert(i[1]);
                if !in_function {
                    globals.insert(i[2]);
                }
            }
            match o {
                OP_VARIABLE => {
                    var_type.insert(i[2], i[1]);
                }
                OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN if i.len() >= 4 => {
                    chains.insert(i[2], i[3]);
                }
                OP_LOAD if i.len() >= 4 => {
                    if let Some(var) = chains.get(&i[3]) {
                        bindless_images.insert((i[2], *var));
                    }
                }
                _ => {}
            }
        }
        let is_runtime_array = |var: &u32| {
            let ptr = var_type.get(var).and_then(|t| pointee.get(t));
            ptr.is_some_and(|t| insts.iter().any(|i| op(i) == OP_TYPE_RUNTIME_ARRAY && i[1] == *t))
        };
        let bindless_images = bindless_images.into_iter().filter(|(_, v)| is_runtime_array(v)).map(|(id, _)| id).collect();
        Module { types, globals, bindless_images, insts }
    }
}

pub fn outline_bindless_fetches(words: &[u32]) -> Vec<u32> {
    if words.len() < HEADER_WORDS {
        return words.to_vec();
    }
    let m = Module::read(words);
    let is_fetch = |i: &[u32]| matches!(op(i), OP_IMAGE_FETCH | OP_IMAGE_READ) && i.len() >= 5 && m.bindless_images.contains(&i[3]);
    if !m.insts.iter().any(|i| is_fetch(i)) {
        return words.to_vec();
    }
    let mut bound = words[3];
    let mut fresh = || {
        bound += 1;
        bound - 1
    };
    let (mut fn_types, mut functions, mut body) = (Vec::new(), Vec::new(), Vec::new());
    for i in &m.insts {
        if !is_fetch(i) {
            body.push(i.to_vec());
            continue;
        }
        let (result_type, result) = (i[1], i[2]);
        // Operands: image, coordinate, then (after the image-operands mask at index 5) their ids.
        let operand_ids: Vec<(usize, u32)> = (3..i.len()).filter(|&k| k != 5).map(|k| (k, i[k])).collect();
        let passed: Vec<(usize, u32)> = operand_ids.into_iter().filter(|(_, id)| !m.globals.contains(id)).collect();
        let fn_type = fresh();
        let mut sig = vec![fn_type, result_type];
        sig.extend(passed.iter().map(|(_, id)| m.types[id]));
        fn_types.push(inst(OP_TYPE_FUNCTION, &sig));
        let func = fresh();
        functions.push(inst(OP_FUNCTION, &[result_type, func, 0, fn_type]));
        let mut inner = i.to_vec();
        for (k, id) in &passed {
            let param = fresh();
            functions.push(inst(OP_FUNCTION_PARAMETER, &[m.types[id], param]));
            inner[*k] = param;
        }
        let inner_result = fresh();
        inner[2] = inner_result;
        functions.push(inst(OP_LABEL, &[fresh()]));
        functions.push(inner);
        functions.push(inst(OP_RETURN_VALUE, &[inner_result]));
        functions.push(inst(OP_FUNCTION_END, &[]));
        let mut call = vec![result_type, result, func];
        call.extend(passed.iter().map(|(_, id)| *id));
        body.push(inst(OP_FUNCTION_CALL, &call));
    }
    let first_function = body.iter().position(|i| op(i) == OP_FUNCTION).unwrap_or(body.len());
    body.splice(first_function..first_function, fn_types);
    let mut out = words[..HEADER_WORDS].to_vec();
    out.extend(body.into_iter().flatten());
    out.extend(functions.into_iter().flatten());
    out[3] = bound;
    out
}

#[cfg(test)]
#[path = "bindless_fetch_tests.rs"]
mod tests;
