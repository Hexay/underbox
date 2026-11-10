//! SPIR-V → naga IR → WGSL. The WGSL is parsed back and validated, so what we hand wgpu is known
//! to be accepted by the same naga version (30, as wgpu 30 in crates/render uses).

use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
use std::fmt;

/// Where translation stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Phase {
    /// HLSL didn't compile (DXC).
    Dxc,
    /// naga's SPIR-V frontend rejected the module.
    SpvIn,
    /// The IR failed validation.
    Validate,
    /// The WGSL backend failed.
    WgslOut,
    /// The WGSL didn't parse or validate again.
    WgslIn,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Error {
    pub phase: Phase,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.phase, self.message)
    }
}

impl std::error::Error for Error {}

pub struct Translated {
    pub module: naga::Module,
    pub info: ModuleInfo,
    pub wgsl: String,
}

/// What we validate against: everything naga can express. Device limits (binding arrays, push
/// constants) are wgpu's to check when the module is created.
pub fn capabilities() -> Capabilities {
    Capabilities::all()
}

fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        s.push_str(": ");
        s.push_str(&inner.to_string());
        src = inner.source();
    }
    s
}

pub fn validate(module: &naga::Module) -> Result<ModuleInfo, String> {
    Validator::new(ValidationFlags::all(), capabilities()).validate(module).map_err(|e| chain(&e))
}

/// Translates a SPIR-V module (already specialized and stripped, see `spirv`). naga's SPIR-V
/// frontend has `unreachable!`s that DXC output can reach, so a panic becomes an error too.
pub fn spirv_to_wgsl(words: &[u32]) -> Result<Translated, Error> {
    std::panic::catch_unwind(|| translate(words)).unwrap_or_else(|p| {
        let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
        Err(Error { phase: Phase::SpvIn, message: format!("naga panicked: {}", msg.unwrap_or_default()) })
    })
}

fn translate(words: &[u32]) -> Result<Translated, Error> {
    let fail = |phase, message| Error { phase, message };
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    // HLSL positions are already in wgpu's (D3D-style) clip space; don't flip Y.
    let options = naga::front::spv::Options { adjust_coordinate_space: false, strict_capabilities: false, block_ctx_dump_prefix: None };
    let mut module = naga::front::spv::parse_u8_slice(&bytes, &options).map_err(|e| fail(Phase::SpvIn, chain(&e)))?;
    lower_is_nan(&mut module);
    let info = validate(&module).map_err(|m| fail(Phase::Validate, m))?;
    let wgsl = naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
        .map_err(|e| fail(Phase::WgslOut, chain(&e)))?;
    let back = naga::front::wgsl::parse_str(&wgsl).map_err(|e| fail(Phase::WgslIn, e.emit_to_string(&wgsl)))?;
    validate(&back).map_err(|m| fail(Phase::WgslIn, m))?;
    Ok(Translated { module, info, wgsl })
}

/// WGSL has no `isnan`, so `isnan(x)` becomes `x != x`.
// Gotcha: a backend compiling with fast-math may fold `x != x` to false; NaN checks are guards only.
fn lower_is_nan(module: &mut naga::Module) {
    use naga::{BinaryOperator, Expression, RelationalFunction};
    let functions = module.functions.iter_mut().map(|(_, f)| f).chain(module.entry_points.iter_mut().map(|e| &mut e.function));
    for f in functions {
        for (_, e) in f.expressions.iter_mut() {
            if let Expression::Relational { fun: RelationalFunction::IsNan, argument } = *e {
                *e = Expression::Binary { op: BinaryOperator::NotEqual, left: argument, right: argument };
            }
        }
    }
}
