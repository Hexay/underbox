//! Material coverage over sbox-public's `.vmat` files: for each shader they name, whether it
//! draws with its own translated program, with the stand-in, or falls back to the placeholder
//! (and why). Ignored by default (it runs DXC on every shader): `sh tools/test.sh engine2
//! material_coverage -- --ignored`, table in `target/material-coverage.tsv`.

use super::{bind, program, standin, vertex, view::View};
use crate::mesh::layout::Layout;
use glam::Mat4;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

fn shader_of(vmat: &Path) -> Option<String> {
    let m = assets::material::from_vmat(&std::fs::read_to_string(vmat).ok()?).ok()?;
    let s = m.shader.replace('\\', "/").to_ascii_lowercase();
    let s = s.strip_prefix("shaders/").unwrap_or(&s);
    let s = s.strip_suffix("_c").unwrap_or(s);
    Some(s.strip_suffix(".shader").or_else(|| s.strip_suffix(".vfx")).unwrap_or(s).to_owned())
}

/// "program", "stand-in" or "placeholder: <reason>" for one shader stem.
fn verdict(gpu: &render::Gpu, compiler: &shaders::Compiler, core: &Path, stem: &str) -> String {
    let includes = shaders::IncludePaths::new([core.to_path_buf()]);
    let file = core.join("shaders").join(format!("{stem}.shader"));
    let standin = standin::mapped(stem).filter(|k| k.replaces_source() || !file.is_file());
    let (shader, kind) = if let Some(kind) = standin {
        (standin::shader(kind.path(), &includes).expect("a stand-in key"), "stand-in")
    } else if file.is_file() {
        (shaders::VfxShader::parse_file(&file, &includes), "program")
    } else {
        return "placeholder: no source, not mapped".into();
    };
    built_verdict(gpu, compiler, shader, stem, kind)
}

fn built_verdict(gpu: &render::Gpu, compiler: &shaders::Compiler, shader: Result<shaders::VfxShader, String>, stem: &str, kind: &str) -> String {
    let built = shader.and_then(|sh| program::build(gpu, compiler, &sh, stem, &shaders::Variant::default()));
    let s = match built {
        Ok(s) => s,
        // wgpu's first line is only "Validation Error"; the cause follows.
        Err(e) => return format!("placeholder: {}", e.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")),
    };
    if vertex::inputs(&s.vertex_inputs, &Layout::sbox_vertex()).is_none() {
        return "placeholder: mesh layout can't feed its vertex inputs".into();
    }
    let view = View { viewport: [0.0, 0.0, 64.0, 64.0], target: [64.0, 64.0], ..Default::default() };
    let instances = [bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }];
    let (textures, values) = (HashMap::new(), HashMap::new());
    let lighting = crate::render::lighting::Lighting::fallback();
    let inputs = bind::Inputs { view: &view, lighting: &lighting, instances: &instances, textures: &textures, values: &[&values], attributes: &[], slots: &[], targets: &[] };
    match gpu.checked(|| bind::bind_groups(gpu, &s, &inputs)).and_then(|r| r) {
        Ok(_) => kind.into(),
        Err(e) => format!("placeholder: bind: {}", e.lines().next().unwrap_or_default()),
    }
}

/// Every `.shader` source under the `UNDERBOX_REAL_SHADERS` folders (game checkouts, read only)
/// through the same build and bind: one verdict line each. Never asserts.
#[test]
fn real_shader_sources() {
    let Ok(dirs) = std::env::var("UNDERBOX_REAL_SHADERS") else { return eprintln!("skipped: UNDERBOX_REAL_SHADERS unset") };
    let (Some(core), Some(gpu)) = (super::tests::core_dir(), crate::render::device::create()) else { return eprintln!("skipped: no sbox-public checkout or GPU") };
    let compiler = shaders::Compiler::new().unwrap();
    let only = std::env::var("UNDERBOX_REAL_ONLY").unwrap_or_default();
    let mut found = Vec::new();
    dirs.split(':').filter(|d| !d.is_empty()).for_each(|d| files(Path::new(d), "shader", &mut found));
    for file in found.iter().filter(|f| only.split(',').any(|o| f.to_string_lossy().contains(o))) {
        // The content root is the folder holding `shaders/`.
        let root = file.ancestors().find(|a| a.file_name().is_some_and(|n| n == "shaders")).and_then(Path::parent).unwrap_or(Path::new("/"));
        let includes = shaders::IncludePaths::new([root.to_path_buf(), core.clone()]);
        let stem = file.file_stem().unwrap().to_string_lossy();
        eprintln!("{}\t{}", file.display(), built_verdict(gpu, &compiler, shaders::VfxShader::parse_file(file, &includes), &stem, "program"));
    }
}

#[test]
#[ignore = "runs DXC over every shader the corpus materials name"]
fn material_coverage() {
    let (Some(core), Some(gpu)) = (super::tests::core_dir(), crate::render::device::create()) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let mut found = Vec::new();
    files(core.parent().unwrap(), "vmat", &mut found);
    let mut by_shader: BTreeMap<String, usize> = BTreeMap::new();
    found.iter().filter_map(|f| shader_of(f)).for_each(|s| *by_shader.entry(s).or_default() += 1);
    let compiler = shaders::Compiler::new().unwrap();
    let mut rows = Vec::new();
    let mut real = 0;
    for (stem, n) in &by_shader {
        let v = verdict(gpu, &compiler, &core, stem);
        if !v.starts_with("placeholder") {
            real += n;
        }
        rows.push(format!("{stem}\t{n}\t{v}"));
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/material-coverage.tsv");
    std::fs::write(&out, rows.join("\n") + "\n").unwrap();
    eprintln!("{}\n== {real} of {} materials draw with a program or the stand-in ({})", rows.join("\n"), found.len(), out.display());
}
