//! The vfx parser against sbox-public's core shaders (skips without a checkout).

mod common;

use shaders::{Stage, VfxShader};

macro_rules! core_or_skip {
    () => {
        match common::core_dir() {
            Some(c) => c,
            None => return eprintln!("no sbox-public checkout; skipped"),
        }
    };
}

fn parse(rel: &str) -> Option<VfxShader> {
    let core = common::core_dir()?;
    Some(VfxShader::parse_file(&core.join("shaders").join(rel), &common::includes(&core)).unwrap())
}

#[test]
fn every_core_shader_parses() {
    let core = core_or_skip!();
    let files = common::shader_files(&core.join("shaders"));
    let includes = common::includes(&core);
    let mut failed = Vec::new();
    let mut missing = std::collections::BTreeSet::new();
    for (rel, path) in &files {
        match VfxShader::parse_file(path, &includes) {
            Ok(s) => s.programs.iter().flat_map(|p| &p.missing_includes).for_each(|m| {
                missing.insert(format!("{rel}: {m}"));
            }),
            Err(e) => failed.push(format!("{rel}: {e}")),
        }
    }
    eprintln!("parsed {}/{} core shaders", files.len() - failed.len(), files.len());
    for m in &missing {
        eprintln!("unresolved include {m}");
    }
    assert_eq!(files.len(), 85, "core shader count changed");
    assert!(failed.is_empty(), "failed to parse:\n{}", failed.join("\n"));
}

#[test]
fn vertex_color_blocks_and_combos() {
    let _ = core_or_skip!();
    let s = parse("vertex_color.shader").unwrap();
    assert_eq!(s.header_value("Description"), Some("Simple shader that makes use of vertex colors"));
    let modes: Vec<_> = s.modes.iter().map(|m| (m.name.as_str(), m.args.clone())).collect();
    assert_eq!(modes[1], ("Depth", vec!["depth_only.shader".to_string()]));
    assert!(s.feature("F_DO_NOT_CAST_SHADOWS").is_some() && s.feature("F_MORPH_SUPPORTED").is_some());
    let stages: Vec<_> = s.programs.iter().map(|p| p.stage).collect();
    assert_eq!(stages, [Stage::Vs, Stage::Ps]);
    let ps = s.program_for(Stage::Ps).unwrap();
    assert!(ps.dynamics.find("D_OPAQUE_FADE").is_some());
    assert!(ps.source.contains("MainPs") && !ps.source.contains("MainVs"));
    assert!(!ps.source.contains("D_OPAQUE_FADE, 0..1") && !ps.source.contains("Attribute( CanBatch"));
    let vs = s.program_for(Stage::Vs).unwrap();
    assert!(vs.dynamics.find("D_BAKED_LIGHTING_FROM_LIGHTMAP").is_some(), "combo from an included file");
    assert!(vs.source.contains("MainVs") && !vs.source.contains("MainPs"));
    assert!(vs.decls.descriptor_sets.iter().any(|(n, slot)| n == "g_globalLateBoundBindlessSet" && *slot == 1));
}

#[test]
fn compute_shader_combos_and_samplers() {
    let _ = core_or_skip!();
    let s = parse("downsample_cs.shader").unwrap();
    let cs = s.program_for(Stage::Cs).unwrap();
    let d = cs.dynamics.find("D_DOWNSAMPLE_METHOD").unwrap();
    assert_eq!((d.min, d.max), (1, 6));
    let border = cs.annotations.iter().find(|a| a.name == "BilinearBorder").unwrap();
    assert_eq!(border.get("AddressU"), Some("BORDER"));
    let size = cs.annotations.iter().find(|a| a.name == "TextureSize").unwrap();
    assert_eq!(size.get("Attribute"), Some("\"TextureSize\""));
}

#[test]
fn feature_linked_static_combos_and_render_state() {
    let _ = core_or_skip!();
    let s = parse("glass.shader").unwrap();
    let q = s.feature("F_GLASS_QUALITY").unwrap();
    assert_eq!((q.min, q.max, q.labels.len()), (0, 2, 3));
    let ps = s.program_for(Stage::Ps).unwrap();
    let combo = ps.statics.find("S_GLASS_QUALITY").unwrap();
    assert_eq!((combo.min, combo.max, combo.feature.as_deref()), (0, 2, Some("F_GLASS_QUALITY")));
    assert!(!ps.decls.render_states.is_empty());
}
