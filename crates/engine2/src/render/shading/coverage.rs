//! Material coverage over sbox-public's `.vmat` files: for each shader they name, whether it
//! draws with its own translated program, with the stand-in, or falls back to the placeholder
//! (and why). Ignored by default (it runs DXC on every shader): `sh tools/test.sh engine2
//! material_coverage -- --ignored`, table in `target/material-coverage.tsv`.

use super::{bind, program, standin, vertex, view::View};
use crate::mesh::layout::Layout;
use glam::Mat4;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

fn vmats(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            vmats(&p, out);
        } else if p.extension().is_some_and(|x| x == "vmat") {
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
    let (shader, kind) = if file.is_file() {
        (shaders::VfxShader::parse_file(&file, &includes), "program")
    } else if standin::mapped(stem).is_some() {
        (standin::shader(&includes), "stand-in")
    } else {
        return "placeholder: no source, not mapped".into();
    };
    let built = shader.and_then(|sh| program::build(gpu, compiler, &sh, stem, &shaders::Variant::default()));
    let s = match built {
        Ok(s) => s,
        // wgpu's first line is only "Validation Error"; the cause follows.
        Err(e) => return format!("placeholder: {}", e.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")),
    };
    if vertex::inputs(&s.vertex_inputs, &Layout::sbox_vertex()).is_none() {
        return "placeholder: mesh layout can't feed its vertex inputs".into();
    }
    let view = View { view_proj: Mat4::IDENTITY, viewport: [0.0, 0.0, 64.0, 64.0], target: [64.0, 64.0] };
    let instances = [bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }];
    let (textures, values) = (HashMap::new(), HashMap::new());
    let lighting = crate::render::lighting::Lighting::fallback();
    let inputs = bind::Inputs { view: &view, lighting: &lighting, instances: &instances, textures: &textures, values: &[&values], attributes: &[], slots: &[], targets: &[] };
    match gpu.checked(|| bind::bind_groups(gpu, &s, &inputs)).and_then(|r| r) {
        Ok(_) => kind.into(),
        Err(e) => format!("placeholder: bind: {}", e.lines().next().unwrap_or_default()),
    }
}

#[test]
#[ignore = "runs DXC over every shader the corpus materials name"]
fn material_coverage() {
    let (Some(core), Some(gpu)) = (super::tests::core_dir(), crate::render::device::create()) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let mut files = Vec::new();
    vmats(core.parent().unwrap(), &mut files);
    let mut by_shader: BTreeMap<String, usize> = BTreeMap::new();
    files.iter().filter_map(|f| shader_of(f)).for_each(|s| *by_shader.entry(s).or_default() += 1);
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
    eprintln!("{}\n== {real} of {} materials draw with a program or the stand-in ({})", rows.join("\n"), files.len(), out.display());
}
