//! Schema of `abi/engine.json` (tools/interop-dump) and `abi/layouts.json` (tools/layout-dump).

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Abi {
    pub ident: String,
    pub native_dll: String,
    pub hash: i32,
    pub structs: Vec<StructDecl>,
    pub slots: Vec<Slot>,
    pub callbacks: Vec<Callback>,
}

#[derive(Debug, Deserialize)]
pub struct StructDecl {
    pub native: String,
    pub managed: String,
    pub kind: StructKind,
    pub small: bool,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StructKind {
    Struct,
    Enum,
    Pointer,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Class {
    pub native: String,
    pub managed: String,
    #[serde(rename = "static")]
    pub is_static: bool,
    pub accessor: bool,
    pub handle: Option<String>,
    pub resource_handle: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Arg {
    pub name: String,
    /// The type in the managed `delegate* unmanaged<...>` signature (`int`, `IntPtr`, `Vector3*`, `out int`).
    pub managed: String,
    /// The type in the native thunk signature (`void*`, `HModelStrong*`, `const char*`).
    pub native: String,
    pub flags: Option<Vec<String>>,
    /// InteropGen's Arg class without the prefix: `Int`, `String`, `DefinedClass`, `DefinedStruct`, `Array`, ...
    pub kind: Option<String>,
    /// `DefinedClass`: the class's native name. `DefinedStruct`: the struct's managed name. `Array`: the element's kind.
    pub of: Option<String>,
}

impl Arg {
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().flatten().any(|f| f == flag)
    }

    pub fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Error,
    CastFromTo,
    CastToFrom,
    Function,
    VariableGet,
    VariableSet,
}

#[derive(Debug, Deserialize)]
pub struct Slot {
    pub index: usize,
    pub kind: SlotKind,
    pub class: Option<Class>,
    pub name: String,
    pub base_class: Option<String>,
    pub args: Vec<Arg>,
    pub ret: Arg,
    pub no_gc: bool,
    #[serde(rename = "static", default)]
    pub is_static: bool,
    pub member: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Callback {
    pub index: usize,
    pub class: Class,
    pub name: String,
    pub member: String,
    pub args: Vec<Arg>,
    pub ret: Arg,
}

/// A managed struct's layout as the CLR computes it (`sizeof`, field offsets).
#[derive(Debug, Deserialize)]
pub struct Layout {
    pub managed: String,
    /// The names the .def files declare this struct under (`Vector3`, `PhysicsTrace.Result`).
    pub declared: Vec<String>,
    /// Enums: the underlying primitive (`int`, `uint`, `byte`); their `fields` are empty.
    pub underlying: Option<String>,
    pub size: usize,
    pub fields: Vec<Field>,
}

#[derive(Debug, Deserialize)]
pub struct Field {
    pub name: String,
    pub offset: usize,
    /// Managed type name; primitives use C# keywords, nested structs their full name.
    #[serde(rename = "type")]
    pub ty: String,
    /// Element count for fixed buffers, else 1.
    #[serde(default = "one")]
    pub count: usize,
}

fn one() -> usize {
    1
}

pub fn load_abi(path: &Path) -> Result<Abi, String> {
    load(path)
}

pub fn load_layouts(path: &Path) -> Result<Vec<Layout>, String> {
    load(path)
}

fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
