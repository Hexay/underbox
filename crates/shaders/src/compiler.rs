//! One (stage, static combo, dynamic combo) through the whole pipeline: defines → DXC → SPIR-V
//! passes → naga → WGSL + reflection, cached on disk.

use crate::cache::{self, Cache};
use crate::dxc::{self, Dxc};
use crate::reflect::{self, Reflection};
use crate::shader::{Program, VfxShader};
use crate::spirv;
use crate::translate::{self, Error, Phase};
use serde::{Deserialize, Serialize};

const PRELUDE: &str = include_str!("../hlsl/prelude.hlsl");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompiledStage {
    pub wgsl: String,
    pub reflection: Reflection,
}

/// What to compile of a program: its combo ids and the values of its `SpecializedConstant()`
/// globals as `(spec id, value)` (see `Annotated::spec_id`; unlisted ones keep their `Default`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Variant {
    pub static_id: u64,
    pub dynamic_id: u64,
    pub spec: Vec<(u32, u32)>,
}

impl Variant {
    pub fn new(static_id: u64, dynamic_id: u64) -> Variant {
        Variant { static_id, dynamic_id, spec: Vec::new() }
    }
}

pub struct Compiler {
    pub dxc: Dxc,
    pub cache: Option<Cache>,
}

impl Compiler {
    /// DXC from [`Dxc::load`] and the cache in [`Cache::default_dir`].
    pub fn new() -> Result<Compiler, String> {
        Ok(Compiler { dxc: Dxc::load()?, cache: Some(Cache::new(Cache::default_dir())) })
    }

    /// HLSL → specialized SPIR-V (GOOGLE reflection still in) for one variant.
    pub fn spirv(&self, shader: &VfxShader, program: &Program, v: &Variant) -> Result<Vec<u32>, Error> {
        let defines = defines(shader, program, v.static_id, v.dynamic_id);
        let stage = program.stage;
        self.dxc
            .compile(&format!("{PRELUDE}{}", program.source), &shader.path, stage.entry(), stage.profile(), &defines)
            .map(|w| spirv::specialize(&w, &v.spec))
            .map_err(|message| Error { phase: Phase::Dxc, message })
    }

    pub fn compile(&self, shader: &VfxShader, program: &Program, v: &Variant) -> Result<CompiledStage, Error> {
        let defines = defines(shader, program, v.static_id, v.dynamic_id);
        let spec: String = v.spec.iter().map(|(id, val)| format!("{id}={val}\n")).collect();
        let key = self.key(program, &defines, &spec);
        if let Some(hit) = self.cache.as_ref().and_then(|c| c.get(&key)) {
            return Ok(hit);
        }
        let words = self.spirv(shader, program, v)?;
        let (words, semantics) = spirv::strip_reflection(&words);
        let t = translate::spirv_to_wgsl(&words)?;
        let reflection = reflect::reflect(&t.module, &t.info, program.stage, &semantics)
            .map_err(|message| Error { phase: Phase::Validate, message })?;
        let out = CompiledStage { wgsl: t.wgsl, reflection };
        if let Some(c) = &self.cache {
            c.put(&key, &out);
        }
        Ok(out)
    }

    fn key(&self, program: &Program, defines: &[(String, String)], spec: &str) -> String {
        let defs: String = defines.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let args = dxc::ARGS.join(" ");
        cache::key([
            concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION")).as_bytes(),
            cache::NAGA_VERSION.as_bytes(),
            self.dxc.version.as_bytes().as_slice(),
            args.as_bytes(),
            program.stage.profile().as_bytes(),
            program.stage.entry().as_bytes(),
            defs.as_bytes(),
            spec.as_bytes(),
            PRELUDE.as_bytes(),
            program.source.as_bytes(),
        ])
    }
}

/// The defines one combo compiles with: `PROGRAM`, `VULKAN`, each feature (the value of the static
/// combo following it, else its minimum), and every static and dynamic combo value.
pub fn defines(shader: &VfxShader, program: &Program, static_id: u64, dynamic_id: u64) -> Vec<(String, String)> {
    let statics = program.statics.defines(static_id);
    let mut out = vec![
        ("PROGRAM".to_string(), (program.stage as u32).to_string()),
        ("VULKAN".into(), "1".into()),
    ];
    for f in &shader.features {
        let linked = program.statics.combos.iter().zip(&statics).find(|(c, _)| c.feature.as_deref() == Some(&f.name));
        out.push((f.name.clone(), linked.map_or(f.min, |(_, (_, v))| *v).to_string()));
    }
    out.extend(statics.into_iter().chain(program.dynamics.defines(dynamic_id)).map(|(k, v)| (k, v.to_string())));
    out
}
