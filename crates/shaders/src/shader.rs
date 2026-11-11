//! A parsed `.shader`: header, modes, material features and one [`Program`] per stage block,
//! each holding the DXC-ready HLSL for that stage and its combos.
//!
//! A stage's source is what native "masks" for it: COMMON, loose top-level code and the stage's
//! own block, in file order, with includes expanded and vfx statements and annotations removed.

use crate::annotation::{self, Annotated};
use crate::blocks::{self, Item};
use crate::combo::{Combo, ComboSet};
use crate::decl::{self, ComboDecl, ComboRange, Decls, FeatureDecl, RuleDecl};
use crate::include::{Dependency, Expander, IncludePaths};
use crate::scan::{split_args, unquote};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A program type, numbered as `VFX_PROGRAM_*` in system.fxc / `ShaderProgramType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stage {
    Vs = 1,
    Ps = 2,
    Gs = 3,
    Hs = 4,
    Ds = 5,
    Cs = 6,
    Rtx = 8,
}

impl Stage {
    pub const ALL: [Stage; 7] = [Stage::Vs, Stage::Ps, Stage::Gs, Stage::Hs, Stage::Ds, Stage::Cs, Stage::Rtx];

    pub fn block(self) -> &'static str {
        ["", "VS", "PS", "GS", "HS", "DS", "CS", "", "RTX"][self as usize]
    }

    pub fn from_block(name: &str) -> Option<Stage> {
        Stage::ALL.into_iter().find(|s| s.block() == name)
    }

    /// The entry point vfx shaders use for this stage.
    pub fn entry(self) -> &'static str {
        ["", "MainVs", "MainPs", "MainGs", "MainHs", "MainDs", "MainCs", "", "MainRtx"][self as usize]
    }

    /// DXC target profile at SM 6.0 (`VfxCompileTarget_t.SM_6_0_VULKAN`).
    pub fn profile(self) -> &'static str {
        ["", "vs_6_0", "ps_6_0", "gs_6_0", "hs_6_0", "ds_6_0", "cs_6_0", "", "lib_6_3"][self as usize]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mode {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Program {
    pub stage: Stage,
    /// HLSL for DXC; combo, feature and program values come in as defines (see `compile`).
    pub source: String,
    pub statics: ComboSet,
    pub dynamics: ComboSet,
    /// Render state, attributes, material inputs and descriptor sets declared for this stage.
    pub decls: Decls,
    pub annotations: Vec<Annotated>,
    /// Annotations left in `source` because they end in a macro (see `annotation::strip_keeping_macros`).
    #[serde(default)]
    pub macro_annotations: Vec<Annotated>,
    /// The preprocessor skeleton of the expanded text (`directives::collect`).
    #[serde(default)]
    pub directives: Vec<(u32, String)>,
    #[serde(skip)]
    pub deps: Vec<Dependency>,
    pub missing_includes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VfxShader {
    pub path: PathBuf,
    pub header: Vec<(String, String)>,
    pub modes: Vec<Mode>,
    pub features: Vec<FeatureDecl>,
    pub feature_rules: Vec<RuleDecl>,
    pub programs: Vec<Program>,
}

impl VfxShader {
    pub fn parse_file(path: &Path, includes: &IncludePaths) -> Result<VfxShader, String> {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        VfxShader::parse(&src, path, includes)
    }

    pub fn parse(src: &str, path: &Path, includes: &IncludePaths) -> Result<VfxShader, String> {
        let items = blocks::split(src).map_err(|e| format!("{}: {e}", path.display()))?;
        let err = |e: String| format!("{}: {e}", path.display());
        let mut shader = VfxShader {
            path: path.into(),
            header: Vec::new(),
            modes: Vec::new(),
            features: Vec::new(),
            feature_rules: Vec::new(),
            programs: Vec::new(),
        };
        for item in &items {
            let Item::Block { name, body, line } = item else { continue };
            match name.as_str() {
                "HEADER" => shader.header = statements(body, '=').into_iter().map(|(k, v)| (k, unquote(&v).into())).collect(),
                "MODES" => shader.modes = modes(body),
                "FEATURES" => {
                    let mut decls = Decls::default();
                    let text = Expander::new(includes).expand(body, path, *line).map_err(err)?;
                    decl::extract(&text, &mut decls).map_err(err)?;
                    shader.features = decls.features;
                    shader.feature_rules = decls.feature_rules;
                }
                _ => {}
            }
        }
        for stage in Stage::ALL {
            if items.iter().any(|i| matches!(i, Item::Block { name, .. } if name == stage.block())) {
                let program = shader.program(&items, stage, includes).map_err(err)?;
                shader.programs.push(program);
            }
        }
        Ok(shader)
    }

    pub fn program(&self, items: &[Item], stage: Stage, includes: &IncludePaths) -> Result<Program, String> {
        let mut expander = Expander::new(includes);
        let mut text = String::new();
        for item in items {
            let (body, line) = match item {
                Item::Block { name, body, line } if name == "COMMON" || name == stage.block() => (body, *line),
                Item::Code { text, line } => (text, *line),
                _ => continue,
            };
            text.push_str(&expander.expand(body, &self.path, line)?);
        }
        let mut decls = Decls::default();
        let text = decl::extract(&text, &mut decls)?;
        let (mut annotations, mut macro_annotations) = (Vec::new(), Vec::new());
        let stripped = annotation::strip_keeping_macros(&text, &mut annotations, &mut macro_annotations);
        let source = crate::word_buffers::rewrite(&crate::compat::rewrite(&stripped));
        let statics = ComboSet {
            combos: self.combos(&decls.static_combos)?,
            rules: decls.static_rules.iter().chain(&self.feature_rules).cloned().collect(),
        };
        let dynamics = ComboSet { combos: self.combos(&decls.dynamic_combos)?, rules: decls.dynamic_rules.clone() };
        Ok(Program {
            stage,
            source,
            statics,
            dynamics,
            directives: crate::directives::collect(&text),
            decls,
            annotations,
            macro_annotations,
            deps: expander.deps,
            missing_includes: expander.missing,
        })
    }

    fn combos(&self, decls: &[ComboDecl]) -> Result<Vec<Combo>, String> {
        decls
            .iter()
            .map(|d| {
                let (min, max, feature) = match &d.range {
                    ComboRange::Values { min, max } => (*min, *max, None),
                    ComboRange::Feature(f) => {
                        let fd = self.feature(f).ok_or_else(|| format!("{} follows unknown feature {f}", d.name))?;
                        (fd.min, fd.max, Some(f.clone()))
                    }
                };
                Ok(Combo { name: d.name.clone(), min, max, feature, sys: d.sys.clone() })
            })
            .collect()
    }

    pub fn feature(&self, name: &str) -> Option<&FeatureDecl> {
        self.features.iter().find(|f| f.name == name)
    }

    pub fn program_for(&self, stage: Stage) -> Option<&Program> {
        self.programs.iter().find(|p| p.stage == stage)
    }

    pub fn header_value(&self, key: &str) -> Option<&str> {
        self.header.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// `Key <sep> Value;` statements (HEADER).
fn statements(body: &str, sep: char) -> Vec<(String, String)> {
    let body = crate::scan::blank_comments(body);
    body.split(';')
        .filter_map(|s| s.split_once(sep))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// `Name( args );` statements (MODES).
fn modes(body: &str) -> Vec<Mode> {
    let body = crate::scan::blank_comments(body);
    body.split(';')
        .filter_map(|s| {
            let s = s.trim();
            let (name, rest) = s.split_once('(')?;
            let args = rest.trim_end().strip_suffix(')')?;
            Some(Mode { name: name.trim().into(), args: split_args(args).into_iter().map(|a| unquote(a).into()).collect() })
        })
        .collect()
}
