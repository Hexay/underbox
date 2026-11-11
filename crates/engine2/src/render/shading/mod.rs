//! Material shading: a material's vfx translated per stage into a wgpu program (crates/shaders),
//! compiled lazily per (shader, combo) and cached, plus per-draw resource binding by reflected
//! name (bind.rs). A program that fails to translate or validate is cached as `None` and draws
//! fall back to the placeholder, with one warning per shader. Closed core shaders without source
//! draw with the stand-in (standin.rs).

pub mod bind;
mod bind_log;
mod defaults;
mod shadow_slots;
pub mod layout;
pub mod program;
pub mod render_state;
mod stages;
pub mod standin;
pub mod vertex;
pub mod view;
#[cfg(test)]
mod coverage;
#[cfg(test)]
mod fog_tests;
#[cfg(test)]
mod gizmo_tests;
#[cfg(test)]
mod sky_tests;
#[cfg(test)]
mod tonemap_tests;
#[cfg(test)]
pub(crate) mod tests;

pub use program::Shading;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

static COMPILER: LazyLock<Option<shaders::Compiler>> = LazyLock::new(|| {
    shaders::Compiler::new().inspect_err(|e| abi::rt::log::log(abi::rt::log::Level::Warn, &format!("shader compiler: {e}"))).ok()
});

/// The path and each program's combo ids ([`variants`]).
type Key = (String, Vec<(u64, u64)>);
type Attrs = crate::render::attributes::Ref;
/// Programs by key, with the [`generation`] of the source they were built from.
type Cached = (Generation, Option<Arc<Shading>>);
static PROGRAMS: LazyLock<Mutex<HashMap<Key, Cached>>> = LazyLock::new(Default::default);

/// The loaded source a shader path has: `GetShader` reloading a changed `.shader_c` swaps the
/// shader's source `Arc`, so a cached program built from another one is stale. Held (not its
/// address) so a freed source's address can't come back as a match. Stand-ins never change.
#[derive(Clone, Default)]
struct Generation(Option<Arc<shaders::VfxShader>>);

impl PartialEq for Generation {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        }
    }
}

fn generation(shader_path: &str) -> Generation {
    Generation((!standin::is_path(shader_path)).then(|| crate::shader::find(shader_path)).flatten())
}

/// Values a draw adds on top of its material's (the stand-in's `StandinUnlit`).
pub type Extra = Vec<(&'static str, f32)>;

/// The program a material's shader draws with, and its [`Extra`] values; `None` when neither the
/// shader nor a stand-in builds.
pub fn for_shader(gpu: &render::Gpu, shader_path: &str, attrs: &[Attrs]) -> Option<(Arc<Shading>, Extra)> {
    if let Some(kind) = standin::mapped(shader_path)
        && (kind.replaces_source() || crate::shader::find(shader_path).is_none())
    {
        return Some((program(gpu, kind.path(), attrs)?, kind.values()));
    }
    Some((program(gpu, shader_path, attrs)?, Vec::new()))
}

fn source(shader_path: &str) -> Result<Arc<shaders::VfxShader>, String> {
    if let Some(standin) = standin::shader(shader_path, &shaders::IncludePaths::new(crate::fs::search_roots())) {
        return standin.map(Arc::new);
    }
    crate::shader::find(shader_path).ok_or_else(|| "shader not found".into())
}

/// Each program's `(static, dynamic)` combo ids at the combos `attrs` set (`SetComboValue`, most
/// specific first); unset combos take their minimum.
fn variants(shader: &shaders::VfxShader, attrs: &[Attrs]) -> Vec<(u64, u64)> {
    shader.programs.iter().map(|p| combo_ids(p, attrs)).collect()
}

fn combo_ids(p: &shaders::Program, attrs: &[Attrs]) -> (u64, u64) {
    let value = |name: &str| attrs.iter().find_map(|a| crate::render::attributes::combo(*a, name));
    (p.statics.encode(value), p.dynamics.encode(value))
}

fn variant_of(attrs: &[Attrs]) -> impl Fn(&shaders::Program) -> shaders::Variant + '_ {
    move |p| {
        let (statics, dynamics) = combo_ids(p, attrs);
        shaders::Variant::new(statics, dynamics)
    }
}

/// The source a path's programs are keyed by, parsed once per [`generation`].
fn keyed_source(shader_path: &str, generation: &Generation) -> Option<Arc<shaders::VfxShader>> {
    type Sources = HashMap<String, (Generation, Option<Arc<shaders::VfxShader>>)>;
    static SOURCES: LazyLock<Mutex<Sources>> = LazyLock::new(Default::default);
    if let Some((from, s)) = SOURCES.lock().unwrap().get(shader_path)
        && from == generation
    {
        return s.clone();
    }
    let s = source(shader_path).ok();
    SOURCES.lock().unwrap().insert(shader_path.to_owned(), (generation.clone(), s.clone()));
    s
}

/// The program for a shader resource path at the combos `attrs` select, compiled on first use.
pub fn program(gpu: &render::Gpu, shader_path: &str, attrs: &[Attrs]) -> Option<Arc<Shading>> {
    let generation = generation(shader_path);
    let ids = keyed_source(shader_path, &generation).map(|s| variants(&s, attrs)).unwrap_or_default();
    let key = (shader_path.to_owned(), ids);
    if let Some((built_from, cached)) = PROGRAMS.lock().unwrap().get(&key)
        && *built_from == generation
    {
        return cached.clone();
    }
    let built = (|| -> Result<Shading, String> {
        let compiler = COMPILER.as_ref().ok_or("no shader compiler")?;
        let variant = variant_of(attrs);
        source(shader_path).and_then(|s| program::build_with(gpu, compiler, &s, shader_path, &variant)).or_else(|e| {
            // A source newer than its shipped .shader_c may not compile; native game runs load the .shader_c.
            let shipped = crate::shader::shipped(shader_path).ok_or_else(|| e.clone())?;
            let built = program::build_with(gpu, compiler, &shipped, shader_path, &variant).map_err(|e2| format!("{e}; its .shader_c: {e2}"))?;
            abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{shader_path}: drawing with its .shader_c, the source fails ({e})"));
            Ok(built)
        })
    })();
    let built = built.inspect_err(|e| abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{shader_path}: {e}"))).ok().map(Arc::new);
    PROGRAMS.lock().unwrap().insert(key, (generation, built.clone()));
    built
}

pub type Compute = Shading<render::ComputeProgram>;

/// A shader's compute program at the combos the dispatch's attributes set (`SetComboValue`, first
/// layer first), compiled on first use; `Err` (cached too) when it doesn't translate. Storage
/// images take the formats of the textures bound to them (`bind::storage_mismatches`).
pub fn compute(gpu: &render::Gpu, shader_path: &str, attrs: &[crate::render::attributes::Ref]) -> Result<Arc<Compute>, String> {
    let declared = compute_for(gpu, shader_path, attrs, &[])?;
    let storage = bind::storage_mismatches(&declared, attrs);
    if storage.is_empty() { Ok(declared) } else { compute_for(gpu, shader_path, attrs, &storage) }
}

fn compute_for(gpu: &render::Gpu, shader_path: &str, attrs: &[crate::render::attributes::Ref], storage: &[program::StorageFormat]) -> Result<Arc<Compute>, String> {
    type Built = Result<Arc<Compute>, String>;
    static COMPUTE: LazyLock<Mutex<HashMap<Key, (Generation, Built)>>> = LazyLock::new(Default::default);
    let generation = generation(shader_path);
    let shader = source(shader_path)?;
    let cs = shader.programs.iter().find(|p| p.stage == shaders::Stage::Cs).ok_or("no CS program")?;
    let name = if storage.is_empty() { shader_path.to_owned() } else { format!("{shader_path} {storage:?}") };
    let (statics, dynamics) = combo_ids(cs, attrs);
    let key = (name, vec![(statics, dynamics)]);
    if let Some((built_from, cached)) = COMPUTE.lock().unwrap().get(&key)
        && *built_from == generation
    {
        return cached.clone();
    }
    let built = (|| {
        let compiler = COMPILER.as_ref().ok_or("no shader compiler")?;
        program::build_compute_for(gpu, compiler, &shader, shader_path, &shaders::Variant::new(statics, dynamics), storage)
    })()
    .map(Arc::new);
    COMPUTE.lock().unwrap().insert(key, (generation, built.clone()));
    built
}
