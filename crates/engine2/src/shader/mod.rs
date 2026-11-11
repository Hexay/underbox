//! `CVfx` (HShader): a shader parsed from vfx source (crates/shaders) or read from a `.shader_c`
//! (compiled.rs, load.rs), and what managed's ShaderCompile drives to compile one: IVfx's
//! CompileShader, compile contexts, combo results and bytecode managers (compile.rs), the write
//! slots (load.rs) and ShaderTools (tools.rs). Combo ids are the mixed-radix ids
//! `shaders::ComboSet` documents, so iterators, `DescribeCombo` and the renderer agree.

mod combos;
mod compile;
mod compiled;
mod load;
mod schema;
mod tools;

use crate::bind_resource_handle;
use crate::resource::{self, ByName, Resource};
use abi::slots::c_vfx as vfx;
use abi::{NativeRef, Registry, StrRet, bind, class};
use compiled::Combos;
use shaders::{IncludePaths, Stage, VfxShader};
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

pub struct Vfx {
    name: String,
    /// Set for shaders loaded by resource path (`GetShader`): what `GetFilename` answers.
    resource: Option<String>,
    shader: RwLock<Option<Arc<VfxShader>>>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// What `VFX_CHECK_MD5_AGAINST_SOURCE` compares (`source_hash`).
    source_hash: String,
    /// Compiled combos per `VFX_PROGRAM_*`, read from a `.shader_c` or written by `WriteCombo`.
    combos: HashMap<i64, Combos>,
    /// Programs a `.shader_c` held (`CVfxProgramData::m_bLoadedFromVcsFile`).
    from_compiled: Vec<i64>,
    /// `GetProgramData` objects by program, freed with the shader.
    program_data: HashMap<i64, usize>,
    /// The `.shader_c` a resource load read and its mtime, to pick up a recompile.
    compiled_file: Option<(PathBuf, Option<SystemTime>)>,
}

impl Vfx {
    fn new(name: String, resource: Option<String>) -> Vfx {
        Vfx { name, resource, shader: RwLock::new(None), state: Mutex::default() }
    }

    fn shader(&self) -> Option<Arc<VfxShader>> {
        self.shader.read().unwrap().clone()
    }
}

impl Drop for Vfx {
    fn drop(&mut self) {
        for &addr in self.state.get_mut().unwrap().program_data.values() {
            objects::destroy(NativeRef::<class::CVfxProgramData>::from_raw(addr as *mut _));
        }
    }
}

impl Resource for Vfx {
    fn is_error(&self) -> bool {
        self.shader.read().unwrap().is_none()
    }
}

static LOADED: ByName<Vfx> = ByName::new();

/// GameShaders' prefix: shaders game code compiled, found on GAME as `compiled/<path>.shader_c`.
const GAME_COMPILED: &str = "compiled/";

/// `shaders/x.shader` for `x`, `x.shader`, `x.shader_c` or `shaders/x.shader_c`; `compiled/...`
/// paths stay where they are.
fn resource_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.trim_start_matches('/');
    let p = p.strip_suffix("_c").unwrap_or(p);
    let p = p.strip_suffix(".shader").unwrap_or(p);
    let lower = p.to_ascii_lowercase();
    let dir = if lower.starts_with("shaders/") || lower.starts_with(GAME_COMPILED) { "" } else { "shaders/" };
    format!("{dir}{p}.shader")
}

/// The shader at a resource path, loaded once; `None` when neither its source nor a `.shader_c`
/// we can read is on disk (closed core shaders such as `complex`).
pub fn find(path: &str) -> Option<Arc<VfxShader>> {
    by_path(path).shader()
}

fn by_path(path: &str) -> Arc<Vfx> {
    let path = resource_path(path);
    LOADED.find_or_insert(&path, || {
        let v = Vfx::new(path.clone(), Some(path.clone()));
        load(&v, &path);
        v
    })
}

/// `GetShader`: as [`by_path`], reloading a shader whose `.shader_c` changed since (a recompile
/// managed would announce with `mat_reloadshaders`).
fn get_shader(path: &str) -> Arc<Vfx> {
    let v = by_path(path);
    let file = v.state.lock().unwrap().compiled_file.clone();
    if let Some((file, mtime)) = file
        && modified(&file) != mtime
    {
        load::from_resource_file(&v, &file.to_string_lossy(), 0);
    }
    v
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn resolve(h: NativeRef<class::CVfx>) -> Option<Arc<Vfx>> {
    resource::resolve::<_, Vfx>(h)
}

fn shader(h: NativeRef<class::CVfx>) -> Option<Arc<VfxShader>> {
    resolve(h).and_then(|v| v.shader())
}

/// `VfxProgram_t` → stage (`ShaderProgramType`: VS = 1 … CS = 6, RTX = 8).
fn stage(program: i64) -> Option<Stage> {
    Stage::ALL.into_iter().find(|s| *s as i64 == program)
}

fn includes() -> IncludePaths {
    IncludePaths::new(crate::fs::search_roots())
}

/// An absolute path, else a path relative to the mounted roots (projects, cloud, core).
fn locate(path: &str) -> Option<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        return p.is_file().then_some(p);
    }
    crate::fs::resolve(path, "GAME")
}

/// What `VFX_CHECK_MD5_AGAINST_SOURCE` compares: the source, every include it read, our version.
fn source_hash(text: &str, shader: &VfxShader) -> String {
    let deps = shader.programs.iter().flat_map(|p| &p.deps).map(|d| d.hash.as_bytes().as_slice());
    shaders::cache::key([env!("CARGO_PKG_VERSION").as_bytes(), text.as_bytes()].into_iter().chain(deps))
}

/// Parses vfx source `path` into `v` (`CreateFromShaderFile`).
fn load_source(v: &Vfx, path: &str) -> Result<(), String> {
    let file = locate(path).ok_or_else(|| format!("{path}: not found"))?;
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let s = VfxShader::parse(&text, &file, &includes())?;
    v.state.lock().unwrap().source_hash = source_hash(&text, &s);
    *v.shader.write().unwrap() = Some(Arc::new(s));
    Ok(())
}

/// A resource path's source, else its `.shader_c`; logs why when neither loads.
fn load(v: &Vfx, path: &str) -> bool {
    let Err(source) = load_source(v, path) else { return true };
    let compiled = format!("{path}_c");
    if locate(&compiled).is_some() && load::from_resource_file(v, &compiled, 0) {
        return true;
    }
    abi::rt::log::log(abi::rt::log::Level::Warn, &format!("{}: {source}", v.name));
    false
}

#[cfg(test)]
mod compiled_tests;
#[cfg(test)]
mod tests;

pub fn register(r: &mut Registry) {
    bind_resource_handle!(r, abi::slots::c_vfx, Vfx);
    bind!(r, vfx::CREATE, |name| resource::new_handle(Arc::new(Vfx::new(string(name), None))));
    bind!(r, abi::slots::glue_resources::GET_SHADER, |path, _guid| resource::new_handle(get_shader(&string(path))));
    bind!(r, vfx::CREATE_FROM_SHADER_FILE, |h, path, _target, _flags| {
        let path = string(path);
        resolve(h).is_some_and(|v| {
            load_source(&v, &path).inspect_err(|e| abi::rt::log::log(abi::rt::log::Level::Warn, e)).is_ok()
        })
    });
    bind!(r, vfx::GET_FILENAME, |h| {
        let v = resolve(h);
        let file = || shader(h).map(|s| s.path.to_string_lossy().into_owned());
        StrRet::from(v.as_ref().and_then(|v| v.resource.clone()).or_else(file).or_else(|| v.map(|v| v.name.clone())).unwrap_or_default())
    });
    // FEATURE is every parsed shader's: ShaderSource.Serialize writes it first.
    bind!(r, vfx::HAS_SHADER_PROGRAM, |h, program| {
        shader(h).is_some_and(|s| program == compiled::FEATURE || stage(program).is_some_and(|st| s.program_for(st).is_some()))
    });
    combos::register(r);
    compile::register(r);
    load::register(r);
    schema::register(r);
    tools::register(r);
}
