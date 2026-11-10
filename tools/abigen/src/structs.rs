//! `#[repr(C)]` Rust structs from the CLR's layouts, with every offset and size asserted at compile time.

use crate::names;
use abi_spec::Layout;
use std::collections::HashMap;
use std::fmt::Write;

pub struct Structs<'a> {
    by_managed: HashMap<&'a str, &'a Layout>,
    rust_names: HashMap<&'a str, String>,
    /// A .def's declared managed name → the layout's full managed name.
    declared: HashMap<&'a str, &'a str>,
}

impl<'a> Structs<'a> {
    pub fn new(layouts: &'a [Layout]) -> Self {
        let by_managed: HashMap<_, _> = layouts.iter().map(|l| (l.managed.as_str(), l)).collect();
        let declared = layouts.iter().flat_map(|l| l.declared.iter().map(|d| (d.as_str(), l.managed.as_str()))).collect();

        let mut short: HashMap<String, Vec<&str>> = HashMap::new();
        for l in layouts {
            short.entry(names::type_name(last_segment(&l.managed))).or_default().push(&l.managed);
        }
        let mut rust_names = HashMap::new();
        for (name, owners) in short {
            let keeper = owners.iter().min_by_key(|m| (m.len(), **m)).copied();
            for m in owners {
                let n = if Some(m) == keeper { name.clone() } else { names::type_name(m) };
                rust_names.insert(m, n);
            }
        }
        Self { by_managed, rust_names, declared }
    }

    /// The Rust type for a struct named as a .def or a signature names it.
    pub fn rust_type(&self, declared_name: &str) -> Option<&str> {
        let managed = self.declared.get(declared_name).copied().unwrap_or(declared_name);
        self.rust_names.get(managed).map(String::as_str)
    }

    /// The managed `sizeof` of a declared struct, enum or pointer handle.
    pub fn size_of_declared(&self, declared_name: &str) -> Option<usize> {
        let managed = self.declared.get(declared_name)?;
        self.by_managed.get(managed).map(|l| l.size)
    }

    pub fn emit(&self) -> Result<String, String> {
        let mut out = String::from(HEADER);
        let mut layouts: Vec<_> = self.by_managed.values().filter(|l| l.underlying.is_none()).collect();
        layouts.sort_by_key(|l| &l.managed);
        for l in layouts {
            self.emit_one(&mut out, l)?;
        }
        Ok(out)
    }

    fn emit_one(&self, out: &mut String, l: &Layout) -> Result<(), String> {
        let mut sorted: Vec<_> = l.fields.iter().collect();
        sorted.sort_by_key(|f| f.offset);
        let overlaps = sorted.windows(2).any(|w| {
            self.field_type(&w[0].ty).is_some_and(|(_, size, _)| w[0].offset + size * w[0].count > w[1].offset)
        });
        if overlaps { self.emit_union(out, l) } else { self.emit_struct(out, l) }
    }

    /// `[StructLayout(Explicit)]` with overlapping fields: the bytes, and a getter/setter per field.
    fn emit_union(&self, out: &mut String, l: &Layout) -> Result<(), String> {
        let name = &self.rust_names[l.managed.as_str()];
        let natural = self.align_of(l);
        // Pack=1 unions (AnimVariant is 17 bytes) can't carry their fields' alignment.
        let align = if l.size.is_multiple_of(natural) { natural } else { 1 };
        writeln!(out, "/// `{}` ({} bytes, overlapping fields)\n#[repr(C, align({align}))]\n#[derive(Clone, Copy, Debug)]\npub struct {name} {{\n    bytes: [u8; {}],\n}}\n", l.managed, l.size, l.size).unwrap();
        writeln!(out, "impl Default for {name} {{\n    fn default() -> Self {{\n        Self {{ bytes: [0; {}] }}\n    }}\n}}\n\nimpl {name} {{", l.size).unwrap();
        for f in &l.fields {
            let (ty, size, _) = self.field_type(&f.ty).ok_or_else(|| format!("{}.{}: unknown type {}", l.managed, f.name, f.ty))?;
            let ty = if f.count > 1 { format!("[{ty}; {}]", f.count) } else { ty };
            let id = names::ident(&f.name);
            let id = match id.strip_prefix("r#") {
                Some(kw) => format!("{kw}_"),
                None => id,
            };
            let end = f.offset + size * f.count;
            if end > l.size {
                return Err(format!("{}.{}: field ends past the struct", l.managed, f.name));
            }
            writeln!(out, "    pub fn {id}(&self) -> {ty} {{\n        // SAFETY: in bounds (checked by abigen); plain data, read unaligned.\n        unsafe {{ self.bytes.as_ptr().add({}).cast::<{ty}>().read_unaligned() }}\n    }}\n", f.offset).unwrap();
            writeln!(out, "    pub fn set_{id}(&mut self, v: {ty}) {{\n        // SAFETY: as above.\n        unsafe {{ self.bytes.as_mut_ptr().add({}).cast::<{ty}>().write_unaligned(v) }}\n    }}\n", f.offset).unwrap();
        }
        writeln!(out, "}}\n\nconst _: () = assert!(size_of::<{name}>() == {});\n", l.size).unwrap();
        Ok(())
    }

    fn emit_struct(&self, out: &mut String, l: &Layout) -> Result<(), String> {
        let name = &self.rust_names[l.managed.as_str()];
        let mut fields = Vec::new();
        let mut cursor = 0;
        let mut aligned = true;
        let mut align = 1;
        let mut used = std::collections::HashSet::new();
        let mut sorted: Vec<_> = l.fields.iter().collect();
        sorted.sort_by_key(|f| f.offset);
        for f in sorted {
            let (ty, size, a) = self.field_type(&f.ty).ok_or_else(|| format!("{}.{}: unknown type {}", l.managed, f.name, f.ty))?;
            if f.offset < cursor {
                return Err(format!("{}.{}: overlapping field (explicit layout unions aren't supported yet)", l.managed, f.name));
            }
            if f.offset > cursor {
                fields.push((format!("_pad{cursor}"), format!("[u8; {}]", f.offset - cursor), cursor));
            }
            aligned &= f.offset % a == 0;
            align = align.max(a);
            let mut id = names::ident(&f.name);
            while !used.insert(id.clone()) {
                id.push('_');
            }
            let ty = if f.count > 1 { format!("[{ty}; {}]", f.count) } else { ty };
            fields.push((id, ty, f.offset));
            cursor = f.offset + size * f.count;
        }
        if l.size > cursor {
            fields.push((format!("_pad{cursor}"), format!("[u8; {}]", l.size - cursor), cursor));
        }
        let packed = !aligned || !l.size.is_multiple_of(align);

        let repr = if packed { "#[repr(C, packed)]" } else { "#[repr(C)]" };
        let derives = if packed { "#[derive(Clone, Copy)]" } else { "#[derive(Clone, Copy, Debug)]" };
        writeln!(out, "/// `{}` ({} bytes)\n{repr}\n{derives}\npub struct {name} {{", l.managed, l.size).unwrap();
        for (id, ty, _) in &fields {
            let vis = if id.starts_with("_pad") { "" } else { "pub " };
            writeln!(out, "    {vis}{id}: {ty},").unwrap();
        }
        writeln!(out, "}}\n").unwrap();
        writeln!(out, "impl Default for {name} {{\n    fn default() -> Self {{\n        // SAFETY: plain data; all-zero is a valid value of every field.\n        unsafe {{ std::mem::zeroed() }}\n    }}\n}}\n").unwrap();
        write!(out, "const _: () = {{\n    assert!(size_of::<{name}>() == {});\n", l.size).unwrap();
        for (id, _, off) in fields.iter().filter(|(id, ..)| !id.starts_with("_pad")) {
            writeln!(out, "    assert!(std::mem::offset_of!({name}, {id}) == {off});").unwrap();
        }
        writeln!(out, "}};\n").unwrap();
        Ok(())
    }

    /// (Rust type, size, alignment) of a field's managed type.
    fn field_type(&self, ty: &str) -> Option<(String, usize, usize)> {
        let ty = ty.strip_prefix("enum:").unwrap_or(ty);
        if let Some((rust, size)) = primitive(ty) {
            return Some((rust.into(), size, size));
        }
        let l = self.by_managed.get(ty)?;
        Some((self.rust_names[ty].clone(), l.size, self.align_of(l)))
    }

    fn align_of(&self, l: &Layout) -> usize {
        l.fields.iter().filter_map(|f| self.field_type(&f.ty)).map(|(_, _, a)| a).max().unwrap_or(1)
    }
}

/// A managed primitive's Rust type and size (= alignment). `bool` is one byte in a CLR struct.
pub fn primitive(ty: &str) -> Option<(&'static str, usize)> {
    Some(match ty {
        "bool" | "byte" => ("u8", 1),
        "sbyte" => ("i8", 1),
        "short" => ("i16", 2),
        "ushort" | "char" => ("u16", 2),
        "int" => ("i32", 4),
        "uint" => ("u32", 4),
        "float" => ("f32", 4),
        "long" => ("i64", 8),
        "ulong" => ("u64", 8),
        "double" => ("f64", 8),
        "IntPtr" => ("usize", 8),
        _ => return None,
    })
}

fn last_segment(managed: &str) -> &str {
    managed.rsplit('.').next().unwrap_or(managed)
}

const HEADER: &str = "// @generated by tools/abigen from abi/layouts.json. Do not edit.\n#![allow(non_camel_case_types, clippy::all)]\n\n";
