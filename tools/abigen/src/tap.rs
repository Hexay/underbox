//! `crates/tap/src/generated/`: a logging trampoline per slot and per callback, and how each struct
//! prints (docs/differential-tap.md).

use crate::slots::Planned;
use crate::structs::Structs;
use crate::types::{self, Conv, Dir, Mapped};
use abi_spec::Abi;
use std::fmt::Write;

/// Flattened structs print at most this many fields; the rest is elided.
const MAX_FIELDS: usize = 24;

pub fn emit(abi: &Abi, structs: &Structs, planned: &[Planned]) -> Result<Vec<(&'static str, String)>, String> {
    let mut tramp = String::from(HEADER);
    writeln!(tramp, "use crate::call::{{Call, Side, managed, native}};\nuse abi::rt::types::*;\nuse abi::slots::{{class, handle}};\nuse abi::structs::*;\nuse std::ffi::{{c_char, c_void}};\n").unwrap();
    for p in planned {
        trampoline(&mut tramp, &format!("s{}", p.slot.index), "Native", p.slot.index, &p.args, &p.ret)?;
    }
    let mut names = Vec::new();
    for cb in &abi.callbacks {
        let ctx = |e: String| format!("callback {} ({}): {e}", cb.index, cb.name);
        let args = cb.args.iter().map(|a| types::map(a, false, Dir::Callback, None, structs).map_err(ctx)).collect::<Result<Vec<_>, _>>()?;
        let ret = types::map(&cb.ret, true, Dir::Callback, None, structs).map_err(ctx)?;
        trampoline(&mut tramp, &format!("c{}", cb.index), "Managed", cb.index, &args, &ret)?;
        names.push((cb.name.as_str(), cb.class.managed.as_str()));
    }
    table(&mut tramp, "SLOT_TRAMPOLINES", "SLOT_COUNT", planned.iter().map(|p| format!("s{}", p.slot.index)));
    table(&mut tramp, "CALLBACK_TRAMPOLINES", "CALLBACK_COUNT", abi.callbacks.iter().map(|c| format!("c{}", c.index)));
    writeln!(tramp, "pub static CALLBACK_NAMES: [&str; abi::CALLBACK_COUNT] = [").unwrap();
    for (name, _) in &names {
        writeln!(tramp, "    {name:?},").unwrap();
    }
    writeln!(tramp, "];\n\n/// Each callback's managed class.\npub static CALLBACK_CLASS: [&str; abi::CALLBACK_COUNT] = [").unwrap();
    for (_, class) in &names {
        writeln!(tramp, "    {class:?},").unwrap();
    }
    writeln!(tramp, "];").unwrap();

    Ok(vec![("trampolines.rs", tramp), ("values.rs", values(structs)), ("mod.rs", MOD_RS.into())])
}

fn trampoline(out: &mut String, name: &str, side: &str, index: usize, args: &[Mapped], ret: &Mapped) -> Result<(), String> {
    let params: Vec<_> = args.iter().enumerate().map(|(n, a)| format!("a{n}: {}", a.abi)).collect();
    let tys: Vec<_> = args.iter().map(|a| a.abi.as_str()).collect();
    let call: Vec<_> = (0..args.len()).map(|n| format!("a{n}")).collect();
    let (call, ret_ty) = (call.join(", "), if ret.abi == "()" { String::new() } else { format!(" -> {}", ret.abi) });
    let table = if side == "Native" { "native" } else { "managed" };
    writeln!(
        out,
        "pub(crate) unsafe extern \"C\" fn {name}({}){ret_ty} {{\n    // SAFETY: the wrapped table's entry {index}, which has this signature.\n    let f: unsafe extern \"C\" fn({}){ret_ty} = unsafe {{ std::mem::transmute::<usize, _>({table}({index})) }};\n    let Some(mut c) = Call::begin(Side::{side}, {index}) else {{\n        return unsafe {{ f({call}) }};\n    }};",
        params.join(", "),
        tys.join(", ")
    )
    .unwrap();
    let mut after = Vec::new();
    for (n, a) in args.iter().enumerate() {
        match a.conv {
            Conv::ByVal => writeln!(out, "    c.by_ref(a{n});").unwrap(),
            Conv::OutRef => after.push(format!("c.by_ref(a{n}.cast_const());")),
            Conv::Slice(len) => {
                let elem = a.safe.strip_prefix("&mut [").and_then(|s| s.strip_suffix(']')).ok_or_else(|| format!("{name}: slice type {}", a.safe))?;
                after.push(format!("c.bytes(a{n}.cast_const().cast(), (a{len} as i64).max(0) as usize * size_of::<{elem}>());"));
            }
            Conv::StrOut => return Err(format!("{name}: string-return conversion on an argument")),
            _ => writeln!(out, "    c.val(&a{n});").unwrap(),
        }
    }
    writeln!(out, "    let r = unsafe {{ f({call}) }};").unwrap();
    if !after.is_empty() {
        writeln!(out, "    c.outs();").unwrap();
        for line in after {
            writeln!(out, "    {line}").unwrap();
        }
    }
    writeln!(out, "    c.ret(&r);\n    r\n}}\n").unwrap();
    Ok(())
}

fn table(out: &mut String, name: &str, count: &str, fns: impl Iterator<Item = String>) {
    writeln!(out, "pub fn {}() -> [usize; abi::{count}] {{\n    [", name.to_lowercase()).unwrap();
    for f in fns {
        writeln!(out, "        {f} as *const () as usize,").unwrap();
    }
    writeln!(out, "    ]\n}}\n").unwrap();
}

fn values(structs: &Structs) -> String {
    let mut out = String::from(HEADER);
    writeln!(out, "use crate::val::{{Line, P, Val}};\nuse abi::structs::*;\n").unwrap();
    for (name, prims) in structs.flattened() {
        let shown: Vec<_> = prims.iter().take(MAX_FIELDS).map(|(off, ty)| format!("({off}, P::{})", prim(ty))).collect();
        let more = prims.len() > MAX_FIELDS;
        writeln!(
            out,
            "impl Val for {name} {{\n    fn put(&self, l: &mut Line) {{\n        l.fields(std::ptr::from_ref(self).cast(), &[{}], {more});\n    }}\n}}\n",
            shown.join(", ")
        )
        .unwrap();
    }
    out
}

fn prim(rust: &str) -> &'static str {
    match rust {
        "u8" => "U8",
        "i8" => "I8",
        "i16" => "I16",
        "u16" => "U16",
        "i32" => "I32",
        "u32" => "U32",
        "i64" => "I64",
        "u64" => "U64",
        "f32" => "F32",
        "f64" => "F64",
        _ => "Ptr",
    }
}

const HEADER: &str = "// @generated by tools/abigen from abi/*.json. Do not edit.\n#![allow(non_camel_case_types, unused_imports, unused_mut, clippy::all)]\n\n";
const MOD_RS: &str = "// @generated by tools/abigen. Do not edit.\npub mod trampolines;\nmod values;\n";
