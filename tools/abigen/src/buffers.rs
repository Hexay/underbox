//! Pointer + length argument pairs that cross as one safe slice, so slot code never builds a
//! slice from a raw pointer. Typed arrays (`int[]` in a .def) pair with the count InteropGen
//! passes after them; untyped `void*` buffers only pair when listed in `KNOWN`.

use crate::types::{Conv, Mapped};
use abi_spec::Slot;

/// (slot name, pointer arg, length arg) for `void*` buffers the .def files don't mark as arrays.
const KNOWN: &[(&str, &str, &str)] = &[
    ("EngineGlue_ReadCompiledResourceFileJson", "data", "nDataSize"),
    ("EngineGlue_ReadCompiledResourceFileBlock", "pHeader", "nDataSize"),
    ("g_pRenderDevice_FindOrCreateTexture2", "data", "dataSize"),
    ("g_pRenderDevice_AsyncSetTextureData2", "pData", "nDataSize"),
];

fn is_count(kind: &str) -> bool {
    matches!(kind, "Int" | "UInt")
}

/// Rewrite `args` (mapped one-to-one with `slot.args`) so each buffer pair becomes `Slice` + `Len`.
pub fn fold(slot: &Slot, args: &mut [Mapped]) -> Result<(), String> {
    for i in 0..slot.args.len().saturating_sub(1) {
        let (ptr, len) = (&slot.args[i], &slot.args[i + 1]);
        let known = KNOWN.iter().any(|&(s, p, l)| s == slot.name && p == ptr.name && l == len.name);
        let array = ptr.kind() == "Array" && is_count(len.kind());
        if !(known || array) {
            continue;
        }
        if !is_count(len.kind()) {
            return Err(format!("buffer length `{}` isn't an integer", len.name));
        }
        // RawPtr<T> → T for the element type; untyped buffers are bytes.
        let elem = args[i].safe.strip_prefix("RawPtr<").and_then(|s| s.strip_suffix('>')).unwrap_or("u8");
        let elem = if elem == "c_void" { "u8" } else { elem };
        args[i] = Mapped { abi: args[i].abi.clone(), safe: format!("&mut [{elem}]"), conv: Conv::Slice };
        args[i + 1].conv = Conv::Len;
    }
    for &(s, p, _) in KNOWN.iter().filter(|(s, _, _)| *s == slot.name) {
        if !slot.args.iter().any(|a| a.name == p) {
            return Err(format!("buffers.rs lists `{s}`.`{p}`, which no longer exists"));
        }
    }
    Ok(())
}
