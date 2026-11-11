//! Compiling one combo, as managed's ShaderCompile drives it (ProgramSource.CompileCore): IVfx's
//! `CompileShader` on a shared `IShaderCompileContext` holding the masked source, giving a
//! `VfxCompiledShaderInfo_t` per combo, gathered by static then dynamic id into a
//! `CVfxByteCodeManager`. A combo compiles HLSL → SPIR-V through DXC (crates/shaders); the
//! renderer translates to WGSL itself, from the shader the `.shader_c` carries.
//!
//! The context objects are made by `CreateSharedContext` (render/vfx.rs); their masked source
//! lives in [`CONTEXTS`] by address.

use super::{Vfx, compiled, includes, resolve, stage};
use abi::slots::{c_vfx_byte_code_manager as bcm, i_shader_compile_context as ctx, i_vfx, vfx_compiled_shader_info_t as info};
use abi::{NativeRef, Registry, bind, class};
use shaders::{Compiler, Variant, VfxShader};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

static COMPILER: LazyLock<Result<Compiler, String>> = LazyLock::new(Compiler::new);

/// A context's masked source, parsed once by the first combo that uses it.
struct Masked {
    code: String,
    parsed: OnceLock<Result<Arc<VfxShader>, String>>,
}

static CONTEXTS: LazyLock<Mutex<HashMap<usize, Arc<Masked>>>> = LazyLock::new(Default::default);

/// One combo's result: the blob managed reads with `GetCompiledShader`, and what it said.
pub(super) struct Outcome {
    pub static_id: u64,
    pub dynamic_id: u64,
    pub blob: Vec<u8>,
    pub failed: bool,
    pub output: String,
}

struct Info {
    outcome: Outcome,
    /// The `CUtlBuffer` handed out by `GetCompiledShader`, owned by this info.
    buffer: OnceLock<usize>,
}

impl Drop for Info {
    fn drop(&mut self) {
        if let Some(&addr) = self.buffer.get() {
            objects::destroy(crate::utl_buffer::Ref::from_raw(addr as *mut _));
        }
    }
}

/// Compiles combo `(s, d)` of `program` in `shader`. A program without bytecode
/// (PS_RENDER_STATE) succeeds empty.
pub(super) fn compile_combo(shader: &VfxShader, program: i64, s: u64, d: u64) -> Outcome {
    let done = |blob: Vec<u8>, failed: bool, output: String| Outcome { static_id: s, dynamic_id: d, blob, failed, output };
    let Some(st) = stage(program) else { return done(compiled::combo_blob(&[]), false, String::new()) };
    let Some(p) = shader.program_for(st) else { return done(Vec::new(), true, format!("{}: no {} block", shader.path.display(), st.block())) };
    if !p.statics.is_valid(s) || !p.dynamics.is_valid(d) {
        return done(Vec::new(), true, format!("{}: combo {s}/{d} is outside the program's valid combos", shader.path.display()));
    }
    let compiler = match COMPILER.as_ref() {
        Ok(c) => c,
        Err(e) => return done(Vec::new(), true, format!("shader compiler unavailable: {e}")),
    };
    match compiler.spirv(shader, p, &Variant::new(s, d)) {
        Ok(words) => done(compiled::combo_blob(&words), false, String::new()),
        Err(e) => done(Vec::new(), true, e.message),
    }
}

/// The masked source a context holds, parsed as `like`'s file, else `like` itself.
fn source_for(context: NativeRef<class::IShaderCompileContext>, like: &Vfx) -> Result<Arc<VfxShader>, String> {
    let vfx = like.shader();
    let masked = CONTEXTS.lock().unwrap().get(&context.addr()).cloned();
    let Some(m) = masked else { return vfx.ok_or_else(|| format!("{}: shader isn't loaded", like.name)) };
    let path = vfx.map_or_else(|| like.name.clone().into(), |s| s.path.clone());
    m.parsed.get_or_init(|| VfxShader::parse(&m.code, &path, &includes()).map(Arc::new)).clone()
}

/// The info object, its `compileFailed`/`compilerOutput` fields set from `outcome`.
pub(super) fn make_info(outcome: Outcome) -> NativeRef<class::VfxCompiledShaderInfo_t> {
    let output = CString::new(outcome.output.replace('\0', " ")).unwrap_or_default();
    let failed = outcome.failed;
    let r = objects::create(Info { outcome, buffer: OnceLock::new() });
    abi::rt::fields::set::<bool, { info::GET_COMPILE_FAILED.index }>(r.as_raw(), failed);
    abi::rt::fields::set_str::<{ info::GET_COMPILER_OUTPUT.index }>(r.as_raw(), Some(&output));
    r
}

/// A combo result's ids and blob; `compileFailed` is managed's to read (and set) as a field.
pub(super) fn info_combo(r: NativeRef<class::VfxCompiledShaderInfo_t>) -> Option<((u64, u64), Vec<u8>, bool)> {
    let i = objects::get::<_, Info>(r)?;
    let failed = abi::rt::fields::get::<bool, { info::GET_COMPILE_FAILED.index }>(r.as_raw());
    Some(((i.outcome.static_id, i.outcome.dynamic_id), i.outcome.blob.clone(), failed))
}

/// What a bytecode manager gathers: each static combo's dynamic combos, in the order managed adds them.
#[derive(Default)]
pub(super) struct ByteCode {
    current: Option<u64>,
    pub combos: compiled::Combos,
}

impl ByteCode {
    pub fn on_static(&mut self, id: u64) {
        self.current = Some(id);
    }

    /// Filed under the static combo last announced, as native's manager groups them.
    pub fn on_dynamic(&mut self, static_id: u64, dynamic_id: u64, blob: Vec<u8>) {
        self.combos.insert((self.current.unwrap_or(static_id), dynamic_id), blob);
    }
}

pub(super) fn byte_code(r: NativeRef<class::CVfxByteCodeManager>) -> Option<compiled::Combos> {
    objects::get::<_, Mutex<ByteCode>>(r).map(|b| b.lock().unwrap().combos.clone())
}

pub fn register(r: &mut Registry) {
    bind!(r, i_vfx::COMPILE_SHADER, |_, context, s, d, h, _target, program, _cache, _flags| {
        let outcome = match resolve(h).ok_or_else(|| "no shader".to_string()).and_then(|v| source_for(context, &v)) {
            Ok(shader) => compile_combo(&shader, program, s, d),
            Err(e) => Outcome { static_id: s, dynamic_id: d, blob: Vec::new(), failed: true, output: e },
        };
        make_info(outcome)
    });

    bind!(r, ctx::SET_MASKED_CODE, |c, code| {
        let code = code.map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
        CONTEXTS.lock().unwrap().insert(c.addr(), Arc::new(Masked { code, parsed: OnceLock::new() }));
    });
    bind!(r, ctx::DELETE, |c| {
        CONTEXTS.lock().unwrap().remove(&c.addr());
        objects::destroy(c);
    });

    bind!(r, info::DELETE, |i| {
        objects::destroy(i);
    });
    bind!(r, info::GET_COMPILED_SHADER, |i| {
        let Some(info) = objects::get::<_, Info>(i) else { return crate::utl_buffer::Ref::NULL };
        let addr = *info.buffer.get_or_init(|| crate::utl_buffer::create(info.outcome.blob.clone()).addr());
        crate::utl_buffer::Ref::from_raw(addr as *mut _)
    });

    bind!(r, bcm::CREATE, || objects::create(Mutex::new(ByteCode::default())));
    bind!(r, bcm::DELETE, |b| {
        objects::destroy(b);
    });
    bind!(r, bcm::RESET, |b| {
        if let Some(b) = objects::get::<_, Mutex<ByteCode>>(b) {
            *b.lock().unwrap() = ByteCode::default();
        }
    });
    bind!(r, bcm::ON_STATIC_COMBO, |b, id| {
        if let Some(b) = objects::get::<_, Mutex<ByteCode>>(b) {
            b.lock().unwrap().on_static(id);
        }
    });
    bind!(r, bcm::ON_DYNAMIC_COMBO, |b, data| {
        let (Some(b), Some(((s, d), blob, _))) = (objects::get::<_, Mutex<ByteCode>>(b), info_combo(data)) else { return };
        b.lock().unwrap().on_dynamic(s, d, blob);
    });
}
