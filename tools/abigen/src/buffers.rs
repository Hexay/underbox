//! Pointer + length argument pairs that cross as one safe slice, so slot code never builds a
//! slice from a raw pointer. Typed arrays (`int[]` in a .def) pair with the count InteropGen
//! passes after them; untyped `void*` buffers only pair when listed in `KNOWN`, with the count
//! on either side.

use crate::types::{Conv, Mapped};
use abi_spec::Slot;

/// (slot name, pointer arg, length arg, element type) for `void*` buffers the .def files don't
/// mark as arrays. The element type is `u8` for raw bytes.
const KNOWN: &[(&str, &str, &str, &str)] = &[
    ("EngineGlue_ReadCompiledResourceFileJson", "data", "nDataSize", "u8"),
    ("EngineGlue_ReadCompiledResourceFileBlock", "pHeader", "nDataSize", "u8"),
    ("g_pRenderDevice_FindOrCreateTexture2", "data", "dataSize", "u8"),
    ("g_pRenderDevice_AsyncSetTextureData2", "pData", "nDataSize", "u8"),
    ("IPhysicsBody_AddHullShape", "vertices", "numVertices", "Vector3"),
    ("IPhysicsBody_AddMeshShape", "vertices", "numVertices", "Vector3"),
    ("IPhysicsBody_AddMeshShape", "indices", "numIndices", "i32"),
    ("IPhysicsShape_UpdateHullShape", "pvVertexBase", "nVertexCount", "Vector3"),
    ("IPhysicsShape_UpdateMeshShape", "vertices", "numVertices", "Vector3"),
    ("IPhysicsShape_UpdateMeshShape", "indices", "numIndices", "i32"),
];

fn is_count(kind: &str) -> bool {
    matches!(kind, "Int" | "UInt")
}

fn fold_pair(args: &mut [Mapped], ptr: usize, len: usize, elem: &str) {
    args[ptr] = Mapped { abi: args[ptr].abi.clone(), safe: format!("&mut [{elem}]"), conv: Conv::Slice(len) };
    args[len].conv = Conv::Len;
}

/// Rewrite `args` (mapped one-to-one with `slot.args`) so each buffer pair becomes `Slice` + `Len`.
pub fn fold(slot: &Slot, args: &mut [Mapped]) -> Result<(), String> {
    let index = |name: &str| slot.args.iter().position(|a| a.name == name);
    for &(_, p, l, elem) in KNOWN.iter().filter(|(s, ..)| *s == slot.name) {
        let (Some(ptr), Some(len)) = (index(p), index(l)) else {
            return Err(format!("buffers.rs lists `{}`.`{p}`/`{l}`, which no longer exist", slot.name));
        };
        if !is_count(slot.args[len].kind()) {
            return Err(format!("buffer length `{l}` isn't an integer"));
        }
        fold_pair(args, ptr, len, elem);
    }
    for i in 0..slot.args.len().saturating_sub(1) {
        let (ptr, len) = (&slot.args[i], &slot.args[i + 1]);
        if ptr.kind() == "Array" && is_count(len.kind()) && args[i].conv != Conv::Len {
            // RawPtr<T> → T for the element type.
            let elem = args[i].safe.strip_prefix("RawPtr<").and_then(|s| s.strip_suffix('>')).unwrap_or("u8").to_owned();
            fold_pair(args, i, i + 1, if elem == "c_void" { "u8" } else { &elem });
        }
    }
    Ok(())
}
