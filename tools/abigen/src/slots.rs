//! `generated/slots.rs` (a typed `Slot` per table entry, the class markers, the table's facts) and
//! `generated/thunks.rs` (the `extern "C"` function each table entry points at).

use crate::names;
use crate::structs::Structs;
use crate::types::{self, Conv, Dir, Mapped};
use abi_spec::{Abi, Slot, SlotKind};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

pub struct Emitted {
    pub slots: String,
    pub thunks: String,
}

pub(crate) struct Planned<'a> {
    pub slot: &'a Slot,
    pub module: Option<String>,
    pub constant: String,
    pub args: Vec<Mapped>,
    pub ret: Mapped,
}

pub fn emit(abi: &Abi, structs: &Structs, planned: &[Planned]) -> Result<Emitted, String> {
    let mut markers = BTreeSet::new();
    let mut handles = BTreeSet::new();
    for p in planned {
        if let Some(c) = &p.slot.class {
            markers.insert(names::type_name(&c.native));
        }
        for a in p.slot.args.iter().chain([&p.slot.ret]) {
            if let Some(of) = &a.of
                && a.kind() == "DefinedClass" {
                    markers.insert(names::type_name(of));
                }
            if let Some(h) = types::ptr_handle(&a.managed) {
                handles.insert(names::type_name(h));
            }
        }
    }
    for cb in &abi.callbacks {
        for a in cb.args.iter().chain([&cb.ret]) {
            if a.kind() == "DefinedClass" {
                markers.insert(names::type_name(a.of.as_deref().unwrap_or_default()));
            }
        }
    }
    Ok(Emitted { slots: emit_slots(abi, structs, planned, &markers, &handles)?, thunks: emit_thunks(planned)? })
}

pub(crate) fn plan<'a>(abi: &'a Abi, structs: &Structs) -> Result<Vec<Planned<'a>>, String> {
    let mut used: HashMap<Option<String>, BTreeSet<String>> = HashMap::new();
    let mut out = Vec::with_capacity(abi.slots.len());
    for slot in &abi.slots {
        let module = slot.class.as_ref().map(|c| names::module(&names::type_name(&c.native)));
        let member = slot.member.as_deref().unwrap_or_default();
        let mut constant = match slot.kind {
            SlotKind::Error => "ERROR".into(),
            SlotKind::CastFromTo => format!("FROM_{}", names::screaming(slot.base_class.as_deref().unwrap_or_default())),
            SlotKind::CastToFrom => format!("TO_{}", names::screaming(slot.base_class.as_deref().unwrap_or_default())),
            SlotKind::VariableGet => format!("GET_{}", names::screaming(member)),
            SlotKind::VariableSet => format!("SET_{}", names::screaming(member)),
            SlotKind::Function => format!("{}{}", names::screaming(member), overload_suffix(&slot.name, member)),
        };
        let taken = used.entry(module.clone()).or_default();
        while !taken.insert(constant.clone()) {
            constant.push('_');
        }
        let self_class = slot.class.as_ref().map(|c| c.native.as_str());
        let ctx = |e: String| format!("slot {} ({}): {e}", slot.index, slot.name);
        let mut args = slot
            .args
            .iter()
            .map(|a| types::map(a, false, Dir::Slot, self_class, structs).map_err(ctx))
            .collect::<Result<Vec<_>, _>>()?;
        crate::buffers::fold(slot, &mut args).map_err(ctx)?;
        let ret = types::map(&slot.ret, true, Dir::Slot, self_class, structs).map_err(ctx)?;
        if matches!(ret.conv, Conv::ByVal | Conv::OutRef | Conv::CStrIn) {
            return Err(ctx(format!("can't return {:?}", ret.conv)));
        }
        out.push(Planned { slot, module, constant, args, ret });
    }
    Ok(out)
}

/// InteropGen numbers overloads `Name_1`, `Name_2` in its mangled names.
fn overload_suffix(mangled: &str, member: &str) -> String {
    match mangled.rsplit_once('_') {
        Some((head, n)) if n.chars().all(|c| c.is_ascii_digit()) && head.ends_with(member) => format!("_{n}"),
        _ => String::new(),
    }
}

fn safe_fn(p: &Planned) -> String {
    let args: Vec<_> = p.args.iter().filter(|a| a.conv != Conv::Len).map(|a| a.safe.as_str()).collect();
    let ret = if p.ret.safe == "()" { String::new() } else { format!(" -> {}", p.ret.safe) };
    format!("fn({}){ret}", args.join(", "))
}

fn emit_slots(abi: &Abi, structs: &Structs, planned: &[Planned], markers: &BTreeSet<String>, handles: &BTreeSet<String>) -> Result<String, String> {
    let mut out = String::from(HEADER);
    writeln!(out, "use crate::rt::registry::Slot;\nuse crate::rt::strings::StrRet;\nuse crate::rt::types::*;\nuse crate::generated::structs::*;\nuse std::ffi::{{CStr, c_void}};\n").unwrap();
    writeln!(out, "pub const HASH: i32 = {};\npub const SLOT_COUNT: usize = {};\npub const CALLBACK_COUNT: usize = {};\n", abi.hash, abi.slots.len(), abi.callbacks.len()).unwrap();

    writeln!(out, "/// Marker types naming native classes in `NativeRef<C>` / `HandleId<C>`.\npub mod class {{\n    use crate::rt::casts::NativeClass;").unwrap();
    for m in markers {
        writeln!(out, "    pub enum {m} {{}}\n    impl NativeClass for {m} {{ const NAME: &'static str = {m:?}; }}").unwrap();
    }
    writeln!(out, "}}\n\n/// Marker types naming pointer handles in `PtrHandle<H>`.\npub mod handle {{").unwrap();
    for h in handles {
        writeln!(out, "    pub enum {h} {{}}").unwrap();
    }
    writeln!(out, "}}\n").unwrap();

    let mut by_module: BTreeMap<Option<&str>, Vec<&Planned>> = BTreeMap::new();
    for p in planned {
        by_module.entry(p.module.as_deref()).or_default().push(p);
    }
    for (module, slots) in by_module {
        if let Some(m) = module {
            let class = slots[0].slot.class.as_ref().unwrap();
            writeln!(out, "/// `{}` (managed `{}`)\npub mod {m} {{\n    use super::*;", class.native, class.managed).unwrap();
        }
        for p in slots {
            let indent = if module.is_some() { "    " } else { "" };
            writeln!(out, "{indent}/// `{}`\n{indent}pub const {}: Slot<{}> = Slot::new({});", p.slot.name, p.constant, safe_fn(p), p.slot.index).unwrap();
        }
        if module.is_some() {
            writeln!(out, "}}\n").unwrap();
        }
    }

    let casts: Vec<_> = planned.iter().filter(|p| matches!(p.slot.kind, SlotKind::CastFromTo | SlotKind::CastToFrom)).collect();
    writeln!(out, "/// (class, direct base class), by marker name.\npub static CLASS_BASES: &[(&str, &str)] = &[").unwrap();
    for p in casts.iter().filter(|p| p.slot.kind == SlotKind::CastFromTo) {
        let class = names::type_name(&p.slot.class.as_ref().unwrap().native);
        writeln!(out, "    ({class:?}, {:?}),", names::type_name(p.slot.base_class.as_deref().unwrap())).unwrap();
    }
    writeln!(out, "];\n\n/// Bind every cast slot: up-casts keep the pointer, down-casts check the object's class (`dynamic_cast`).\npub fn bind_casts(r: &mut crate::Registry) {{\n    use crate::rt::casts::{{down, up}};").unwrap();
    for p in casts {
        let class = names::type_name(&p.slot.class.as_ref().unwrap().native);
        let base = names::type_name(p.slot.base_class.as_deref().unwrap());
        let module = p.module.as_deref().unwrap();
        let f = match p.slot.kind {
            SlotKind::CastFromTo => format!("down::<class::{base}, class::{class}>"),
            _ => format!("up::<class::{class}, class::{base}>"),
        };
        writeln!(out, "    r.bind({module}::{}, {f}, \"abi::casts\");", p.constant).unwrap();
    }
    writeln!(out, "}}\n").unwrap();
    crate::fields::emit(planned, &mut out);

    writeln!(out, "pub static NAMES: [&str; SLOT_COUNT] = [").unwrap();
    for p in planned {
        writeln!(out, "    {:?},", p.slot.name).unwrap();
    }
    writeln!(out, "];\n\n/// Each slot's native class (`\"\"` for slot 0).\npub static SLOT_CLASS: [&str; SLOT_COUNT] = [").unwrap();
    for p in planned {
        writeln!(out, "    {:?},", p.slot.class.as_ref().map_or("", |c| c.native.as_str())).unwrap();
    }
    writeln!(out, "];\n\n/// (inherited slot, the declaring class's slot): an unbound inherited slot takes its base's binding.\npub static INHERITS: &[(usize, usize)] = &[").unwrap();
    for (derived, base) in inherits(abi) {
        writeln!(out, "    ({derived}, {base}),").unwrap();
    }
    writeln!(out, "];\n\n/// Managed's `sizeof` of each declared struct, enum and pointer handle, in the order `igen_engine` receives them.\npub static STRUCT_SIZES: &[(&str, usize)] = &[").unwrap();
    for s in &abi.structs {
        let size = structs.size_of_declared(&s.managed).ok_or_else(|| format!("no size for {}", s.managed))?;
        writeln!(out, "    ({:?}, {size}),", s.managed).unwrap();
    }
    writeln!(out, "];").unwrap();
    Ok(out)
}

fn inherits(abi: &Abi) -> Vec<(usize, usize)> {
    let sig = |s: &Slot| s.args.iter().filter(|a| a.name != "self").map(|a| a.managed.clone()).collect::<Vec<_>>();
    let mut declared = HashMap::new();
    for s in abi.slots.iter().filter(|s| s.kind == SlotKind::Function && s.base_class.is_none()) {
        if let (Some(c), Some(m)) = (&s.class, &s.member) {
            declared.insert((c.native.as_str(), m.as_str(), sig(s), s.ret.managed.as_str()), s.index);
        }
    }
    abi.slots
        .iter()
        .filter(|s| s.kind == SlotKind::Function)
        .filter_map(|s| {
            let base = s.base_class.as_deref()?;
            declared.get(&(base, s.member.as_deref()?, sig(s), s.ret.managed.as_str())).map(|&b| (s.index, b))
        })
        .collect()
}

fn emit_thunks(planned: &[Planned]) -> Result<String, String> {
    let mut out = String::from(HEADER);
    writeln!(out, "use crate::generated::slots::{{self, *}};\nuse crate::generated::structs::*;\nuse crate::rt::thunk::*;\nuse crate::rt::types::*;\nuse std::ffi::{{c_char, c_void}};\n").unwrap();
    for p in planned {
        let i = p.slot.index;
        let params: Vec<_> = p.args.iter().enumerate().map(|(n, a)| format!("a{n}: {}", a.abi)).collect();
        let ret = if p.ret.abi == "()" { String::new() } else { format!(" -> {}", p.ret.abi) };
        let path = match &p.module {
            Some(m) => format!("slots::{m}::{}", p.constant),
            None => format!("slots::{}", p.constant),
        };
        writeln!(out, "pub(crate) unsafe extern \"C\" fn s{i}({}){ret} {{\n    match bound({path}) {{\n        Some(f) => {{", params.join(", ")).unwrap();
        let mut call_args = Vec::new();
        for (n, a) in p.args.iter().enumerate() {
            let expr = match a.conv {
                Conv::Same => format!("a{n}"),
                Conv::Bool => format!("a{n} != 0"),
                Conv::CStrIn => format!("unsafe {{ cstr_arg(a{n}) }}"),
                Conv::NativeRef => format!("NativeRef::from_raw(a{n})"),
                Conv::RawPtr => format!("RawPtr::from_raw(a{n})"),
                Conv::ByVal => format!("unsafe {{ by_val(a{n}) }}"),
                Conv::OutRef => {
                    writeln!(out, "            let mut s{n} = Default::default();").unwrap();
                    format!("unsafe {{ out_ref(a{n}, &mut s{n}) }}")
                }
                Conv::Slice(len) => format!("unsafe {{ slice_arg(a{n}, a{len}) }}"),
                Conv::Len => continue,
                Conv::StrOut => return Err(format!("slot {i}: string-return conversion on an argument")),
            };
            call_args.push(expr);
        }
        let call = format!("f({})", call_args.join(", "));
        let body = match p.ret.conv {
            Conv::Bool => format!("{call} as {}", p.ret.abi),
            Conv::StrOut => format!("{call}.into_return()"),
            Conv::NativeRef => format!("{call}.as_raw()"),
            Conv::RawPtr => format!("{call}.as_ptr()"),
            _ => call,
        };
        writeln!(out, "            {body}\n        }}\n        None => unbound({i}),\n    }}\n}}\n").unwrap();
    }
    writeln!(out, "/// Every thunk's address, in slot order: what `igen_engine` copies into managed's table.\npub fn native_table() -> [usize; SLOT_COUNT] {{\n    [").unwrap();
    for p in planned {
        writeln!(out, "        s{} as *const () as usize,", p.slot.index).unwrap();
    }
    writeln!(out, "    ]\n}}").unwrap();
    Ok(out)
}

const HEADER: &str = "// @generated by tools/abigen from abi/engine.json. Do not edit.\n#![allow(non_camel_case_types, non_upper_case_globals, unused_imports, clippy::all)]\n\n";
