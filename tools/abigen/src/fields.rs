//! Default bindings for field accessors (`Get__X_m_y`/`Set__X_m_y`, a quarter of the table):
//! each binds to a per-object field store (`abi::rt::fields`), so what managed sets reads back
//! and unset fields read as zero. They fill only slots no subsystem bound, so a subsystem
//! overrides a field that's really computed by binding its accessor itself.

use crate::slots::Planned;
use abi_spec::SlotKind;
use std::collections::HashMap;
use std::fmt::Write;

pub fn emit(planned: &[Planned], out: &mut String) {
    let accessors: Vec<_> = planned.iter().filter(|p| matches!(p.slot.kind, SlotKind::VariableGet | SlotKind::VariableSet)).collect();
    // A member's getter and setter share one store key: the getter's slot index.
    let member = |p: &Planned| (p.slot.class.as_ref().map(|c| c.native.clone()), p.slot.member.clone());
    let keys: HashMap<_, _> = accessors.iter().filter(|p| p.slot.kind == SlotKind::VariableGet).map(|p| (member(p), p.slot.index)).collect();

    writeln!(out, "/// Bind every field accessor no subsystem bound to the per-object field store.\npub fn bind_fields(r: &mut crate::Registry) {{\n    use crate::rt::fields::{{get, get_str, set, set_str}};").unwrap();
    for p in accessors {
        let key = keys.get(&member(p)).copied().unwrap_or(p.slot.index);
        let f = match p.slot.kind {
            SlotKind::VariableGet if p.ret.safe == "StrRet" => format!("get_str::<{key}>"),
            SlotKind::VariableGet => format!("get::<{}, {key}>", p.ret.safe),
            _ => match p.args.last().map(|a| a.safe.as_str()) {
                Some("Option<&CStr>") => format!("set_str::<{key}>"),
                Some(t) => format!("set::<{t}, {key}>"),
                None => continue,
            },
        };
        let path = p.module.as_deref().map_or_else(|| p.constant.clone(), |m| format!("{m}::{}", p.constant));
        writeln!(out, "    r.bind_default({path}, {f}, \"abi::fields\");").unwrap();
    }
    writeln!(out, "}}\n").unwrap();
}
