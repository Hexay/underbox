//! Material shading: a material's vfx translated per stage into a wgpu program (crates/shaders),
//! compiled lazily per (shader, combo) and cached, plus per-draw resource binding by reflected
//! name (bind.rs). A program that fails to translate or validate is cached as `None` and draws
//! fall back to the placeholder, with one warning per shader. Closed core shaders without source
//! draw with the stand-in (standin.rs).

pub mod bind;
mod defaults;
mod shadow_slots;
pub mod layout;
pub mod program;
mod stages;
pub mod standin;
pub mod vertex;
pub mod view;
#[cfg(test)]
mod coverage;
#[cfg(test)]
mod sky_tests;
#[cfg(test)]
pub(crate) mod tests;

pub use program::Shading;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

static COMPILER: LazyLock<Option<shaders::Compiler>> = LazyLock::new(|| {
    shaders::Compiler::new().inspect_err(|e| abi::rt::log::log(abi::rt::log::Level::Warn, &format!("shader compiler: {e}"))).ok()
});

type Key = (String, u64, u64);
static PROGRAMS: LazyLock<Mutex<HashMap<Key, Option<Arc<Shading>>>>> = LazyLock::new(Default::default);

/// Values a draw adds on top of its material's (the stand-in's `StandinUnlit`).
pub type Extra = Vec<(&'static str, f32)>;

/// The program a material's shader draws with, and its [`Extra`] values; `None` when neither the
/// shader nor a stand-in builds.
pub fn for_shader(gpu: &render::Gpu, shader_path: &str) -> Option<(Arc<Shading>, Extra)> {
    if crate::shader::find(shader_path).is_none()
        && let Some(kind) = standin::mapped(shader_path)
    {
        return Some((program(gpu, kind.path())?, kind.values()));
    }
    Some((program(gpu, shader_path)?, Vec::new()))
}

fn source(shader_path: &str) -> Result<Arc<shaders::VfxShader>, String> {
    if let Some(standin) = standin::shader(shader_path, &shaders::IncludePaths::new(crate::fs::search_roots())) {
        return standin.map(Arc::new);
    }
    crate::shader::find(shader_path).ok_or_else(|| "shader not found".into())
}

/// The program for a shader resource path at a combo, compiled on first use.
// TODO: static/dynamic combos from material features and render state; always 0, 0 for now.
pub fn program(gpu: &render::Gpu, shader_path: &str) -> Option<Arc<Shading>> {
    let key = (shader_path.to_owned(), 0, 0);
    if let Some(cached) = PROGRAMS.lock().unwrap().get(&key) {
        return cached.clone();
    }
    let built = (|| {
        let compiler = COMPILER.as_ref().ok_or("no shader compiler")?;
        let shader = source(shader_path)?;
        program::build(gpu, compiler, &shader, shader_path, &shaders::Variant::new(key.1, key.2))
    })();
    let built = built.inspect_err(|e| abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{shader_path}: {e}"))).ok().map(Arc::new);
    PROGRAMS.lock().unwrap().insert(key, built.clone());
    built
}

pub type Compute = Shading<render::ComputeProgram>;

/// A shader's compute program, compiled on first use; `Err` (cached too) when it doesn't translate.
// TODO: combos from the dispatch's attributes (D_MSAA, ...); always 0, 0 for now.
pub fn compute(gpu: &render::Gpu, shader_path: &str) -> Result<Arc<Compute>, String> {
    type Built = Result<Arc<Compute>, String>;
    static COMPUTE: LazyLock<Mutex<HashMap<String, Built>>> = LazyLock::new(Default::default);
    if let Some(cached) = COMPUTE.lock().unwrap().get(shader_path) {
        return cached.clone();
    }
    let built = (|| {
        let compiler = COMPILER.as_ref().ok_or("no shader compiler")?;
        let shader = source(shader_path)?;
        program::build_compute(gpu, compiler, &shader, shader_path, &shaders::Variant::new(0, 0))
    })()
    .map(Arc::new);
    COMPUTE.lock().unwrap().insert(shader_path.to_owned(), built.clone());
    built
}
