//! Word-level SPIR-V passes run before naga: baking specialization constants (as managed
//! `SpirvDump.Specialize` does for shipped combos) and lifting DXC's `-fspv-reflect` semantics
//! out of the module, since naga rejects the GOOGLE extensions that carry them.

const OP_NAME: u32 = 5;
const OP_EXTENSION: u32 = 10;
const OP_CONSTANT_TRUE: u32 = 41;
const OP_CONSTANT_FALSE: u32 = 42;
const OP_CONSTANT: u32 = 43;
const OP_CONSTANT_COMPOSITE: u32 = 44;
const OP_SPEC_CONSTANT_TRUE: u32 = 48;
const OP_SPEC_CONSTANT_FALSE: u32 = 49;
const OP_SPEC_CONSTANT: u32 = 50;
const OP_SPEC_CONSTANT_COMPOSITE: u32 = 51;
const OP_VARIABLE: u32 = 59;
const OP_DECORATE: u32 = 71;
const OP_DECORATE_STRING: u32 = 5632;
const OP_MEMBER_DECORATE_STRING: u32 = 5633;
/// Only `-fspv-reflect`'s `CounterBuffer` (an append/consume buffer's counter) uses it.
const OP_DECORATE_ID: u32 = 332;
const DECORATION_SPEC_ID: u32 = 1;
const DECORATION_LOCATION: u32 = 30;
const DECORATION_USER_SEMANTIC: u32 = 5635;
const STORAGE_INPUT: u32 = 1;
pub(crate) const HEADER_WORDS: usize = 5;

/// A shader input's location with its HLSL semantic (`POSITION`, `TEXCOORD0`) and variable name.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InputSemantic {
    pub location: u32,
    pub semantic: String,
    pub name: String,
}

pub(crate) fn instructions(words: &[u32]) -> impl Iterator<Item = &[u32]> {
    let mut i = HEADER_WORDS.min(words.len());
    std::iter::from_fn(move || {
        let n = (*words.get(i)? >> 16) as usize;
        let inst = words.get(i..i + n.max(1))?;
        i += n.max(1);
        Some(inst)
    })
}

fn rebuild(words: &[u32], mut f: impl FnMut(&[u32]) -> Option<Vec<u32>>) -> Vec<u32> {
    let mut out = words[..HEADER_WORDS.min(words.len())].to_vec();
    for inst in instructions(words) {
        if let Some(new) = f(inst) {
            out.extend(new);
        }
    }
    out
}

/// One instruction: word count and opcode, then the operands.
pub(crate) fn inst(opcode: u32, operands: &[u32]) -> Vec<u32> {
    let mut v = vec![((operands.len() as u32 + 1) << 16) | opcode];
    v.extend_from_slice(operands);
    v
}

pub(crate) fn op(inst: &[u32]) -> u32 {
    inst[0] & 0xffff
}

fn with_op(inst: &[u32], opcode: u32) -> Vec<u32> {
    let mut v = inst.to_vec();
    v[0] = (v[0] & 0xffff_0000) | opcode;
    v
}

pub(crate) fn string(words: &[u32]) -> String {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).take_while(|&b| b != 0).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Replaces spec constants by constants: `values` maps SpecId to a value (the low word for
/// 64-bit types), others keep their defaults. `OpSpecConstantOp` is left for naga.
pub fn specialize(words: &[u32], values: &[(u32, u32)]) -> Vec<u32> {
    let spec_ids: Vec<(u32, u32)> = instructions(words)
        .filter(|i| op(i) == OP_DECORATE && i.len() >= 4 && i[2] == DECORATION_SPEC_ID)
        .map(|i| (i[1], i[3]))
        .collect();
    let value_of = |id: u32| {
        let spec = spec_ids.iter().find(|(target, _)| *target == id)?.1;
        values.iter().find(|(s, _)| *s == spec).map(|(_, v)| *v)
    };
    rebuild(words, |inst| match op(inst) {
        OP_DECORATE if inst.len() >= 4 && inst[2] == DECORATION_SPEC_ID => None,
        OP_SPEC_CONSTANT_TRUE | OP_SPEC_CONSTANT_FALSE => {
            let on = value_of(inst[2]).map_or(op(inst) == OP_SPEC_CONSTANT_TRUE, |v| v != 0);
            Some(with_op(inst, if on { OP_CONSTANT_TRUE } else { OP_CONSTANT_FALSE }))
        }
        OP_SPEC_CONSTANT => {
            let mut v = with_op(inst, OP_CONSTANT);
            if let (Some(val), Some(w)) = (value_of(inst[2]), v.get_mut(3)) {
                *w = val;
            }
            Some(v)
        }
        OP_SPEC_CONSTANT_COMPOSITE => Some(with_op(inst, OP_CONSTANT_COMPOSITE)),
        _ => Some(inst.to_vec()),
    })
}

/// Drops `OpDecorateString`/`OpMemberDecorateString`/`OpDecorateId` and the GOOGLE extensions
/// (naga reads none of them), returning the stage inputs' semantics from the `UserSemantic`
/// decorations.
pub fn strip_reflection(words: &[u32]) -> (Vec<u32>, Vec<InputSemantic>) {
    let mut names = Vec::new();
    let mut locations = Vec::new();
    let mut semantics = Vec::new();
    let mut inputs = Vec::new();
    for i in instructions(words) {
        match op(i) {
            OP_NAME if i.len() >= 3 => names.push((i[1], string(&i[2..]))),
            OP_DECORATE if i.len() >= 4 && i[2] == DECORATION_LOCATION => locations.push((i[1], i[3])),
            OP_DECORATE_STRING if i.len() >= 4 && i[2] == DECORATION_USER_SEMANTIC => semantics.push((i[1], string(&i[3..]))),
            OP_VARIABLE if i.len() >= 4 && i[3] == STORAGE_INPUT => inputs.push(i[2]),
            _ => {}
        }
    }
    let find = |list: &[(u32, String)], id: u32| list.iter().find(|(t, _)| *t == id).map(|(_, s)| s.clone());
    let mut out: Vec<InputSemantic> = inputs
        .iter()
        .filter_map(|&id| {
            let location = locations.iter().find(|(t, _)| *t == id)?.1;
            Some(InputSemantic {
                location,
                semantic: find(&semantics, id).unwrap_or_default(),
                name: find(&names, id).unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by_key(|s| s.location);
    let stripped = rebuild(words, |inst| match op(inst) {
        OP_DECORATE_STRING | OP_MEMBER_DECORATE_STRING | OP_DECORATE_ID => None,
        OP_EXTENSION if string(&inst[1..]).starts_with("SPV_GOOGLE_") => None,
        _ => Some(inst.to_vec()),
    });
    (stripped, out)
}

const OP_EXECUTION_MODE: u32 = 16;
const OP_CAPABILITY: u32 = 17;
/// `ComputeDerivativeGroupQuadsKHR`, `ComputeDerivativeGroupLinearKHR`.
const DERIVATIVE_CAPABILITIES: [u32; 2] = [5288, 5350];
/// `DerivativeGroupQuadsKHR`, `DerivativeGroupLinearKHR`.
const DERIVATIVE_MODES: [u32; 2] = [5289, 5290];

/// Drops `SPV_KHR_compute_shader_derivatives` (DXC adds it for quad ops in compute, e.g.
/// downsample_spd), which naga rejects. Quads become consecutive invocations, the layout the
/// extension's linear mode names; shaders that swizzle their own thread ids (SPD) don't mind.
pub fn strip_compute_derivatives(words: &[u32]) -> Vec<u32> {
    rebuild(words, |inst| match op(inst) {
        OP_EXTENSION if string(&inst[1..]).ends_with("_compute_shader_derivatives") => None,
        OP_CAPABILITY if inst.len() >= 2 && DERIVATIVE_CAPABILITIES.contains(&inst[1]) => None,
        OP_EXECUTION_MODE if inst.len() >= 3 && DERIVATIVE_MODES.contains(&inst[2]) => None,
        _ => Some(inst.to_vec()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<u32> {
        let mut b = s.as_bytes().to_vec();
        b.resize((b.len() / 4 + 1) * 4, 0);
        b.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
    }

    #[test]
    fn bakes_spec_constants_and_strips_semantics() {
        let mut m = vec![0x0723_0203, 0x10000, 0, 100, 0];
        m.extend(inst(OP_EXTENSION, &words("SPV_GOOGLE_hlsl_functionality1")));
        m.extend(inst(OP_NAME, &[7].into_iter().chain(words("in.var.POSITION")).collect::<Vec<_>>()));
        m.extend(inst(OP_DECORATE, &[5, DECORATION_SPEC_ID, 3]));
        m.extend(inst(OP_DECORATE, &[6, DECORATION_SPEC_ID, 4]));
        m.extend(inst(OP_DECORATE, &[7, DECORATION_LOCATION, 2]));
        m.extend(inst(OP_DECORATE_STRING, &[7, DECORATION_USER_SEMANTIC].into_iter().chain(words("POSITION")).collect::<Vec<_>>()));
        m.extend(inst(OP_DECORATE_ID, &[7, 5634, 8]));
        m.extend(inst(OP_SPEC_CONSTANT, &[1, 5, 9]));
        m.extend(inst(OP_SPEC_CONSTANT_TRUE, &[2, 6]));
        m.extend(inst(OP_VARIABLE, &[3, 7, STORAGE_INPUT]));

        let s = specialize(&m, &[(3, 42), (4, 0)]);
        let ops: Vec<_> = instructions(&s).map(|i| (op(i), i[1..].to_vec())).collect();
        assert!(ops.contains(&(OP_CONSTANT, vec![1, 5, 42])));
        assert!(ops.contains(&(OP_CONSTANT_FALSE, vec![2, 6])));
        assert!(!ops.iter().any(|(o, a)| *o == OP_DECORATE && a[1] == DECORATION_SPEC_ID));

        let (stripped, inputs) = strip_reflection(&s);
        assert_eq!(inputs, [InputSemantic { location: 2, semantic: "POSITION".into(), name: "in.var.POSITION".into() }]);
        assert!(instructions(&stripped).all(|i| ![OP_DECORATE_STRING, OP_DECORATE_ID, OP_EXTENSION].contains(&op(i))));
    }

    #[test]
    fn strips_compute_derivatives_only() {
        let mut m = vec![0x0723_0203, 0x10000, 0, 100, 0];
        m.extend(inst(OP_CAPABILITY, &[1]));
        m.extend(inst(OP_CAPABILITY, &[5288]));
        m.extend(inst(OP_EXTENSION, &words("SPV_KHR_compute_shader_derivatives")));
        m.extend(inst(OP_EXECUTION_MODE, &[4, 17, 256, 1, 1]));
        m.extend(inst(OP_EXECUTION_MODE, &[4, 5289]));
        let ops: Vec<_> = instructions(&strip_compute_derivatives(&m)).map(<[u32]>::to_vec).collect();
        assert_eq!(ops, [inst(OP_CAPABILITY, &[1]), inst(OP_EXECUTION_MODE, &[4, 17, 256, 1, 1])]);
    }
}
