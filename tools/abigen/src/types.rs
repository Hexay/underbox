//! How one argument or return value crosses: its raw ABI type, the safe type implementations see,
//! and the conversion between them.

use crate::names;
use crate::structs::{Structs, primitive};
use abi_spec::Arg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conv {
    /// Same type on both sides (`#[repr(transparent)]` wrappers included).
    Same,
    /// `int` 0/1 ↔ `bool`.
    Bool,
    /// `const char*` in → `Option<&CStr>`.
    CStrIn,
    /// `const char*` out ← `StrRet` (slots) / `Option<String>` (callbacks).
    StrOut,
    /// `void*` ↔ `NativeRef<C>`.
    NativeRef,
    /// `T*` ↔ `RawPtr<T>`.
    RawPtr,
    /// `T*` holding a value passed by reference ↔ `T` (slots) / `&T` (callbacks).
    ByVal,
    /// `T*` for `out`/`ref` ↔ `&mut T`.
    OutRef,
    /// A buffer pointer whose length is argument `n` ↔ `&mut [T]` (see buffers.rs).
    Slice(usize),
    /// That buffer's length: folded into the slice, absent from the safe signature.
    Len,
}

#[derive(Debug, Clone)]
pub struct Mapped {
    pub abi: String,
    pub safe: String,
    pub conv: Conv,
}

impl Mapped {
    fn new(abi: impl Into<String>, safe: impl Into<String>, conv: Conv) -> Self {
        Self { abi: abi.into(), safe: safe.into(), conv }
    }

    fn same(ty: impl Into<String>) -> Self {
        let ty = ty.into();
        Self::new(ty.clone(), ty, Conv::Same)
    }
}

/// Which side calls: native slots are called by managed; callbacks are called by native.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Slot,
    Callback,
}

pub fn map(arg: &Arg, is_ret: bool, dir: Dir, self_class: Option<&str>, structs: &Structs) -> Result<Mapped, String> {
    let (managed, byref) = strip_byref(arg.managed.trim());
    let byref = byref || arg.has_flag("out") || arg.has_flag("ref");
    let kind = arg.kind();

    if arg.name == "self" && kind == "Pointer" {
        let class = self_class.ok_or("self argument outside a class")?;
        return Ok(Mapped::new("*mut c_void", format!("NativeRef<class::{}>", names::type_name(class)), Conv::NativeRef));
    }
    if arg.name == "m_ObjectId" {
        return Ok(Mapped::same("ManagedObject"));
    }

    Ok(match kind {
        "Void" => Mapped::same("()"),
        "Bool" => Mapped::new(prim(managed)?, "bool", Conv::Bool),
        "Enum" => Mapped::same("i64"),
        "StringToken" => Mapped::same("StringToken"),
        "ManagedClass" => Mapped::same("ManagedObject"),
        "Guid" if byref => Mapped::new("*mut Guid", "&mut Guid", Conv::OutRef),
        "Guid" => Mapped::same("Guid"),
        "String" | "CUtlString" => match (is_ret, dir) {
            (true, Dir::Slot) => Mapped::new("*const c_char", "StrRet", Conv::StrOut),
            (true, Dir::Callback) => Mapped::new("*const c_char", "Option<String>", Conv::StrOut),
            (false, _) => Mapped::new("*const c_char", "Option<&CStr>", Conv::CStrIn),
        },
        "Pointer" | "Delegate" => Mapped::new("*mut c_void", "RawPtr<c_void>", Conv::RawPtr),
        "DefinedClass" => {
            let class = arg.of.as_deref().ok_or("DefinedClass without its class")?;
            let marker = format!("class::{}", names::type_name(class));
            match managed {
                "int" => Mapped::same(format!("HandleId<{marker}>")),
                "IntPtr" => Mapped::new("*mut c_void", format!("NativeRef<{marker}>"), Conv::NativeRef),
                other => return Err(format!("DefinedClass crossing as {other}")),
            }
        }
        "Array" => {
            let elem = prim(managed.trim_end_matches('*'))?;
            Mapped::new(format!("*mut {elem}"), format!("RawPtr<{elem}>"), Conv::RawPtr)
        }
        "DefinedStruct" => defined_struct(arg, managed, byref, structs)?,
        _ => {
            let p = prim(managed)?;
            if byref { Mapped::new(format!("*mut {p}"), format!("&mut {p}"), Conv::OutRef) } else { Mapped::same(p) }
        }
    })
}

fn defined_struct(arg: &Arg, managed: &str, byref: bool, structs: &Structs) -> Result<Mapped, String> {
    if let Some(handle) = ptr_handle(managed) {
        return Ok(Mapped::same(format!("PtrHandle<handle::{}>", names::type_name(handle))));
    }
    let is_ptr = managed.ends_with('*');
    let name = arg.of.as_deref().unwrap_or_else(|| managed.trim_end_matches('*').trim());
    let ty = structs.rust_type(name).ok_or_else(|| format!("no layout for struct {name}"))?.to_string();
    Ok(if byref {
        Mapped::new(format!("*mut {ty}"), format!("&mut {ty}"), Conv::OutRef)
    } else if arg.has_flag("asref") {
        Mapped::new(format!("*mut {ty}"), format!("RawPtr<{ty}>"), Conv::RawPtr)
    } else if is_ptr {
        Mapped::new(format!("*const {ty}"), ty, Conv::ByVal)
    } else {
        Mapped::same(ty)
    })
}

/// `out int` → (`int`, true).
fn strip_byref(managed: &str) -> (&str, bool) {
    for prefix in ["out ", "ref ", "in "] {
        if let Some(rest) = managed.strip_prefix(prefix) {
            return (rest.trim(), true);
        }
    }
    (managed, false)
}

/// `IntPtr /* PtrHandle:RenderBufferHandle_t  */` → `RenderBufferHandle_t`.
pub fn ptr_handle(managed: &str) -> Option<&str> {
    let start = managed.find("PtrHandle:")? + "PtrHandle:".len();
    let rest = &managed[start..];
    Some(rest[..rest.find("*/").unwrap_or(rest.len())].trim())
}

fn prim(managed: &str) -> Result<String, String> {
    match managed {
        "void" => Ok("()".into()),
        "IntPtr" => Ok("*mut c_void".into()),
        m => primitive(m).map(|(r, _)| r.to_string()).ok_or_else(|| format!("unknown primitive {m}")),
    }
}
