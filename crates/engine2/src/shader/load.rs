//! `.shader_c` in and out: `CreateFromResourceFile`/`CreateAnonymous` read one (compiled.rs),
//! `WriteCombo`/`WriteProgramToBuffer`/`Serialize` produce the bodies managed frames into one,
//! `GetProgramData` reports which programs came from it.

use super::compile::{byte_code, info_combo};
use super::compiled::{self, FEATURE, Features, Loaded};
use super::native_features;
use super::{GAME_COMPILED, LOADED, Vfx, includes, locate, modified, resolve, resource_path, source_hash, stage, string};
use crate::{resource, utl_buffer};
use abi::slots::{c_vfx as vfx, c_vfx_program_data as pd};
use abi::{NativeRef, RawPtr, Registry, bind, class};
use shaders::{Stage, VfxShader};
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;

/// `VFX_CHECK_MD5_AGAINST_SOURCE` and `VFX_LOAD_STATIC_COMBO_DATA` (Shader.cs).
const CHECK_MD5: u32 = 1 << 0;
const LOAD_COMBOS: u32 = 1 << 1;

fn warn(m: &str) {
    abi::rt::log::log(abi::rt::log::Level::Warn, m);
}

/// The shader a native-compiled file embeds: each program's masked source parsed for its stage.
fn from_embedded(l: &Loaded, path: &Path) -> Result<VfxShader, String> {
    let mut programs: Vec<(Stage, &String)> = l
        .embedded
        .iter()
        .filter_map(|(name, src)| Some((Stage::from_block(name.strip_prefix("VFX_PROGRAM_")?)?, src)))
        .collect();
    programs.sort_by_key(|(st, _)| *st as u32);
    let features = native_features::block(&l.native_features);
    let mut out: Option<VfxShader> = None;
    for (st, src) in programs {
        // Native's masking blanks the block keywords too (MODES, FEATURES, `VS {`), leaving the
        // program's HLSL bare; our own keeps them. Bare HLSL is that stage's block.
        let mut parsed = VfxShader::parse(src, path, &includes());
        if parsed.as_ref().is_ok_and(|p| p.program_for(st).is_none()) || (parsed.is_err() && !l.native_features.is_empty()) {
            parsed = VfxShader::parse(&format!("{features}{}\n{{\n{src}\n}}\n", st.block()), path, &includes());
        }
        let parsed = parsed?;
        let program = parsed.program_for(st).cloned().ok_or_else(|| format!("embedded {} has no {} block", st.block(), st.block()))?;
        out.get_or_insert(VfxShader { programs: Vec::new(), ..parsed }).programs.push(program);
    }
    out.ok_or_else(|| "neither our shader nor embedded source".into())
}

/// Puts what `bytes` (a `.shader_c`, read from `file` if any) holds into `v`. With [`CHECK_MD5`],
/// a file whose source sits beside it with another hash is out of date and doesn't load.
pub(super) fn apply(v: &Vfx, bytes: &[u8], file: Option<&Path>, flags: u32) -> Result<(), String> {
    let l = compiled::read(bytes)?;
    let source = file.and_then(|f| f.to_str()?.strip_suffix("_c").map(std::path::PathBuf::from)).filter(|s| s.is_file());
    if flags & CHECK_MD5 != 0
        && let Some(source) = source
    {
        let text = std::fs::read_to_string(&source).map_err(|e| e.to_string())?;
        let now = VfxShader::parse(&text, &source, &includes()).map(|s| source_hash(&text, &s)).unwrap_or_default();
        if l.features.as_ref().is_none_or(|f| f.source_hash != now) {
            return Err("out of date".into());
        }
    }
    let path = file.map_or_else(|| v.name.clone().into(), Path::to_path_buf);
    let (shader, hash) = match &l.features {
        Some(f) => (f.shader.clone(), f.source_hash.clone()),
        None => (from_embedded(&l, &path)?, String::new()),
    };
    let mut st = v.state.lock().unwrap();
    st.source_hash = hash;
    st.from_compiled = l.programs.clone();
    st.combos = if flags & LOAD_COMBOS != 0 { l.combos } else { Default::default() };
    st.compiled_file = file.map(|f| (f.to_path_buf(), modified(f)));
    *v.shader.write().unwrap() = Some(Arc::new(shader));
    Ok(())
}

/// `x.shader` names its `x.shader_c`.
fn compiled_path(path: &str) -> String {
    if path.to_ascii_lowercase().ends_with(".shader") { format!("{path}_c") } else { path.to_string() }
}

pub(super) fn from_resource_file(v: &Vfx, path: &str, flags: u32) -> bool {
    let compiled = compiled_path(path);
    let Some(file) = locate(&compiled) else { return false };
    let bytes = std::fs::read(&file).inspect(|b| crate::resource_loaded::read(&compiled, b));
    let result = bytes.map_err(|e| e.to_string()).and_then(|b| apply(v, &b, Some(&file), flags));
    result.inspect_err(|e| if e != "out of date" { warn(&format!("{}: {e}", file.display())) }).is_ok()
}

/// An anonymous shader from `.shader_c` bytes. The renderer finds shaders by name, so it is
/// registered under its (generated, unique) name.
pub(super) fn anonymous(name: &str, bytes: &[u8]) -> Option<Arc<Vfx>> {
    let path = resource_path(name);
    let v = Arc::new(Vfx::new(name.to_string(), Some(path.clone())));
    apply(&v, bytes, None, LOAD_COMBOS).inspect_err(|e| warn(&format!("{name}: {e}"))).ok()?;
    if path.to_ascii_lowercase().starts_with(GAME_COMPILED) {
        LOADED.insert(&path, v.clone());
    }
    Some(v)
}

fn features(v: &Vfx) -> Option<Features> {
    Some(Features { source_hash: v.state.lock().unwrap().source_hash.clone(), shader: (*v.shader()?).clone() })
}

/// A program's body: FEATURE is the shader, a stage its combos (from `manager` when given).
fn program_body(v: &Vfx, program: i64, manager: Option<compiled::Combos>) -> Option<Vec<u8>> {
    if program == FEATURE {
        return features(v).map(|f| compiled::features_body(&f));
    }
    let combos = manager.or_else(|| v.state.lock().unwrap().combos.get(&program).cloned()).unwrap_or_default();
    Some(compiled::combos_body(&combos))
}

/// Every program the shader has, framed as SPRV's payload (what native `Serialize` returns).
pub(super) fn serialize(v: &Vfx) -> Option<Vec<u8>> {
    let shader = v.shader()?;
    let present = |p: i64| p == FEATURE || stage(p).is_some_and(|s| shader.program_for(s).is_some());
    let bodies = (0..compiled::PROGRAM_MAX as i64).filter(|p| present(*p)).filter_map(|p| Some((p as usize, program_body(v, p, None)?)));
    Some(compiled::sprv_payload(&bodies.collect::<Vec<_>>()))
}

fn program_data(v: &Vfx, program: i64) -> NativeRef<class::CVfxProgramData> {
    let mut st = v.state.lock().unwrap();
    let addr = *st.program_data.entry(program).or_insert_with(|| objects::create::<class::CVfxProgramData, _>(()).addr());
    abi::rt::fields::set::<bool, { pd::GET_M_B_LOADED_FROM_VCS_FILE.index }>(addr as *mut c_void, st.from_compiled.contains(&program));
    NativeRef::from_raw(addr as *mut _)
}

pub fn register(r: &mut Registry) {
    bind!(r, vfx::CREATE_FROM_RESOURCE_FILE, |h, path, _target, flags, _silent| {
        resolve(h).is_some_and(|v| from_resource_file(&v, &string(path), flags))
    });
    bind!(r, vfx::CREATE_ANONYMOUS, |name, data: RawPtr<c_void>, size| {
        // SAFETY: managed pins `size` bytes at `data` for the call (GameShaders.CreateNative).
        let bytes = unsafe { data.cast::<u8>().as_slice(size.max(0) as usize) };
        anonymous(&string(name), bytes).map_or(NativeRef::NULL, resource::new_handle)
    });
    bind!(r, vfx::GET_PROGRAM_DATA, |h, program| resolve(h).map_or(NativeRef::NULL, |v| program_data(&v, program)));
    bind!(r, vfx::INITIALIZE_WRITE, |h| resolve(h).is_some_and(|v| v.shader().is_some()));
    bind!(r, vfx::FINALIZE_COMPILE, |h| resolve(h).is_some_and(|v| v.shader().is_some()));
    bind!(r, vfx::WRITE_COMBO, |h, program, s, d, info| {
        let (Some(v), Some((_, blob, failed))) = (resolve(h), info_combo(info)) else { return false };
        if !failed {
            v.state.lock().unwrap().combos.entry(program).or_default().insert((s, d), blob);
        }
        !failed
    });
    bind!(r, vfx::WRITE_PROGRAM_TO_BUFFER, |h, program, manager, out| {
        let body = resolve(h).and_then(|v| program_body(&v, program, byte_code(manager)));
        body.is_some_and(|b| utl_buffer::put(out, b))
    });
    bind!(r, vfx::SERIALIZE, |h| resolve(h).and_then(|v| serialize(&v)).map_or(utl_buffer::Ref::NULL, utl_buffer::create));
}
