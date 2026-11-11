//! A compile as managed's ShaderCompile runs it, and what the `.shader_c` it writes loads back as.

use super::compiled::{self, Combos, FEATURE};
use super::tests::{SRC, loaded, write_temp};
use super::*;
use assets::container::Container;

const PS: i64 = Stage::Ps as i64;
const SPIRV_MAGIC: u32 = 0x0723_0203;

/// What `ShaderSource.WriteResourceFile` writes: empty DATA landing on SPRV, then HLSL if any.
fn resource_file(sprv: Vec<u8>, hlsl: Option<&str>) -> Vec<u8> {
    let mut blocks = vec![(*b"DATA", Vec::new()), (*b"SPRV", sprv)];
    blocks.extend(hlsl.map(|h| (*b"HLSL", h.as_bytes().to_vec())));
    Container { version: compiled::RESOURCE_VERSION as u16, blocks }.write()
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Every PS combo compiled, as ProgramSource gathers them, then Serialize's payload.
fn compile_all(v: &Vfx) -> Vec<u8> {
    let shader = v.shader().unwrap();
    let ps = shader.program_for(Stage::Ps).unwrap();
    let mut combos = Combos::new();
    for s in ps.statics.valid_ids() {
        for d in ps.dynamics.valid_ids() {
            let out = compile::compile_combo(&shader, PS, s, d);
            assert!(!out.failed, "{}", out.output);
            combos.insert((s, d), out.blob);
        }
    }
    v.state.lock().unwrap().combos.insert(PS, combos);
    load::serialize(v).unwrap()
}

#[test]
fn a_combo_compiles_to_the_blob_managed_reads() {
    let s = loaded().shader().unwrap();
    let out = compile::compile_combo(&s, PS, 1, 2);
    assert!(!out.failed, "{}", out.output);
    assert_eq!((out.static_id, out.dynamic_id), (1, 2));
    assert_eq!(u32_at(&out.blob, 0), 4, "CVfxShaderFileVulkan version");
    assert_eq!(u32_at(&out.blob, 4) as usize, out.blob.len() - 8);
    assert_eq!(u32_at(&out.blob, 8), SPIRV_MAGIC);
    let bad = compile::compile_combo(&s, PS, 2, 0);
    assert!(bad.failed && bad.output.contains("outside"), "static 2 isn't a valid id: {}", bad.output);
    let render_state = compile::compile_combo(&s, 7, 0, 0);
    assert!(!render_state.failed && u32_at(&render_state.blob, 4) == 0, "PS_RENDER_STATE has no bytecode");
}

/// sbox-public's material template as ProgramSource feeds it: masked per program, then the
/// preprocessor's header in front (`#line` and `system.fxc`), core includes left to us.
#[test]
fn the_material_template_compiles_through_masked_source() {
    let Some(core) = crate::render::shading::tests::core_dir() else { return eprintln!("skipped: no sbox-public checkout") };
    let path = core.parent().unwrap().join("templates/material.shader");
    let src = std::fs::read_to_string(&path).unwrap();
    for stage in [Stage::Vs, Stage::Ps] {
        let masked = format!("#line 1 \"templates/material.shader\"\n#include \"system.fxc\"\n{}", tools::mask(&src, stage as i64, false));
        let shader = VfxShader::parse(&masked, &path, &IncludePaths::new([core.clone()])).unwrap();
        let out = compile::compile_combo(&shader, stage as i64, 0, 0);
        assert!(!out.failed, "{stage:?}: {}", out.output);
        assert_eq!(u32_at(&out.blob, 8), SPIRV_MAGIC);
    }
}

#[test]
fn compile_errors_reach_the_output() {
    let broken = SRC.replace("return g_flScale;", "return undefined_thing;");
    let s = VfxShader::parse(&broken, Path::new("broken.shader"), &IncludePaths::default()).unwrap();
    let out = compile::compile_combo(&s, PS, 0, 0);
    assert!(out.failed && out.output.contains("undefined_thing"), "{}", out.output);
}

#[test]
fn compiled_shader_round_trips() {
    let v = loaded();
    let payload = compile_all(&v);
    // Each program body starts with the VCS header ShaderUpgrade.GetState checks.
    let ps = u32_at(&payload, PS as usize * 8) as usize;
    assert_eq!(&payload[ps..ps + 4], b"vcs2");
    assert_eq!((u32_at(&payload, ps + 4) as i32, u32_at(&payload, ps + 8) as i32), (compiled::VCS_VERSION, compiled::ABI_VERSION));
    assert_ne!(u32_at(&payload, 0), 0, "FEATURE is written");
    assert_eq!(u32_at(&payload, 3 * 8), 0, "no GS");

    let back = Vfx::new("back".into(), None);
    load::apply(&back, &resource_file(payload, None), None, 2).unwrap();
    let (a, b) = (v.shader().unwrap(), back.shader().unwrap());
    assert_eq!(a.programs.len(), b.programs.len());
    assert_eq!(a.program_for(Stage::Ps).unwrap().source, b.program_for(Stage::Ps).unwrap().source);
    let st = back.state.lock().unwrap();
    assert_eq!(st.from_compiled, [FEATURE, Stage::Vs as i64, PS]);
    assert_eq!(st.combos[&PS].len(), 2 * 3);
    assert_eq!(st.combos[&PS], v.state.lock().unwrap().combos[&PS]);
}

#[test]
fn a_changed_source_makes_the_compiled_file_out_of_date() {
    let path = write_temp("md5.shader", SRC);
    let v = Vfx::new("md5".into(), None);
    load_source(&v, &path.to_string_lossy()).unwrap();
    let compiled_file = PathBuf::from(format!("{}_c", path.display()));
    std::fs::write(&compiled_file, resource_file(load::serialize(&v).unwrap(), None)).unwrap();
    let check = |p: &Path| load::apply(&Vfx::new("c".into(), None), &std::fs::read(p).unwrap(), Some(p), 1);
    assert_eq!(check(&compiled_file), Ok(()));
    std::fs::write(&path, SRC.replace("4.0", "5.0")).unwrap();
    assert_eq!(check(&compiled_file), Err("out of date".into()));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn native_compiled_files_load_from_their_embedded_source() {
    // A program body native wrote: the VCS header, then a layout we don't read.
    let body = [b"vcs2".as_slice(), &68i32.to_le_bytes(), &2i32.to_le_bytes(), &[9; 16]].concat();
    let sprv = compiled::sprv_payload(&[(FEATURE as usize, body.clone()), (PS as usize, body)]);
    let masked = tools::mask(SRC, PS, false);
    let hlsl = serde_json::json!({ "Programs": { "VFX_PROGRAM_PS": masked } }).to_string();
    let v = Vfx::new("native".into(), None);
    load::apply(&v, &resource_file(sprv, Some(&hlsl)), None, 0).unwrap();
    let s = v.shader().unwrap();
    assert_eq!(s.features[0].name, "F_A");
    let ps = s.program_for(Stage::Ps).unwrap();
    assert_eq!((ps.statics.count(), ps.dynamics.count()), (2, 3));
    assert!(s.program_for(Stage::Vs).is_none(), "only what was embedded");
    assert_eq!(v.state.lock().unwrap().from_compiled, [FEATURE, PS]);
    assert!(load::apply(&v, &resource_file(compiled::sprv_payload(&[]), None), None, 0).is_err(), "no shader at all");
}

/// A program body in the layout real files have (docs/real-games.md): VCS header (65 or 68, ABI 2),
/// program hash and shared hash, reflection text, then combos as `-3` and a zstd frame needing
/// dictionary 0x2bc2fa87, which no file carries.
fn real_layout_body(vcs: i32) -> Vec<u8> {
    let mut b = [b"vcs2".as_slice(), &vcs.to_le_bytes(), &2i32.to_le_bytes(), &[0xA5; 32], b"g_flScale\0"].concat();
    b.extend((-3i32).to_le_bytes());
    b.extend([0x28, 0xB5, 0x2F, 0xFD, 0x63, 0x87, 0xFA, 0xC2, 0x2B, 0x01, 0x02, 0x03]);
    b
}

/// Real files put GUID and REDI before the empty DATA, and resource versions run 2..=4.
fn real_layout_file(blocks: Vec<([u8; 4], Vec<u8>)>, version: u16) -> Vec<u8> {
    let mut all = vec![(*b"GUID", vec![7; 16]), (*b"REDI", vec![0; 64]), (*b"DATA", Vec::new())];
    all.extend(blocks);
    Container { version, blocks: all }.write()
}

#[test]
fn real_layout_loads_from_embedded_source_without_decoding_combos() {
    for (vcs, version) in [(68, 4), (65, 3)] {
        let sprv = compiled::sprv_payload(&[(FEATURE as usize, real_layout_body(vcs)), (PS as usize, real_layout_body(vcs))]);
        let hlsl = serde_json::json!({ "Programs": { "VFX_PROGRAM_PS": tools::mask(SRC, PS, false) } }).to_string();
        let file = real_layout_file(vec![(*b"SPRV", sprv), (*b"HLSL", hlsl.into_bytes())], version);
        let v = Vfx::new("real".into(), None);
        load::apply(&v, &file, None, 2).unwrap();
        assert!(v.shader().unwrap().program_for(Stage::Ps).is_some());
        let st = v.state.lock().unwrap();
        assert_eq!(st.from_compiled, [FEATURE, PS]);
        assert!(st.combos.is_empty(), "dictionary-compressed combos stay undecoded");
    }
}

/// A FEATURE body with one record as real files lay it out (see native_features.rs).
fn real_feature_body(vcs: i32, name: &str, max: i32) -> Vec<u8> {
    let field = |s: &str| [s.as_bytes(), &vec![0; 64 - s.len()]].concat();
    let ints: &[i32] = if vcs >= 68 { &[0, 0, 0, max, 0, -1] } else { &[0, 0, max, 0, -1, 0] };
    let mut b = [real_layout_body(vcs), vec![0], field(name), field("Rendering")].concat();
    ints.iter().for_each(|i| b.extend(i.to_le_bytes()));
    b
}

#[test]
fn natively_blanked_sources_take_features_from_the_feature_body() {
    let src = "\n\n\nStaticCombo( S_PATTERN, F_PATTERN, Sys( ALL ) );\nfloat4 MainPs() : SV_Target0 { return S_PATTERN; }\n";
    for (vcs, max) in [(65, 1), (68, 3)] {
        let sprv = compiled::sprv_payload(&[(FEATURE as usize, real_feature_body(vcs, "F_PATTERN", max)), (PS as usize, real_layout_body(vcs))]);
        let hlsl = serde_json::json!({ "Programs": { "VFX_PROGRAM_PS": src } }).to_string();
        let v = Vfx::new("blanked".into(), None);
        load::apply(&v, &real_layout_file(vec![(*b"SPRV", sprv), (*b"HLSL", hlsl.into_bytes())], 4), None, 0).unwrap();
        let s = v.shader().unwrap();
        assert_eq!((s.features[0].name.as_str(), s.features[0].min, s.features[0].max), ("F_PATTERN", 0, max as u32));
        let combo = &s.program_for(Stage::Ps).unwrap().statics.combos[0];
        assert_eq!((combo.name.as_str(), combo.max), ("S_PATTERN", max as u32));
    }
}

#[test]
fn real_files_without_usable_programs_fail_cleanly() {
    let v = Vfx::new("real".into(), None);
    let dxbc_only = real_layout_file(vec![(*b"DXBC", vec![1; 32])], 2);
    assert!(load::apply(&v, &dxbc_only, None, 0).unwrap_err().contains("no SPRV"));
    let no_hlsl = real_layout_file(vec![(*b"SPRV", compiled::sprv_payload(&[(PS as usize, real_layout_body(68))]))], 3);
    assert!(load::apply(&v, &no_hlsl, None, 0).is_err());
    assert!(v.shader().is_none());
}

#[test]
fn game_compiled_shaders_load_without_their_source() {
    let file = resource_file(load::serialize(&loaded()).unwrap(), None);
    // GameShaders links only `compiled/x.shader_c` onto GAME; the cache's source is gone.
    let path = write_temp("game.shader_c", "");
    std::fs::write(&path, &file).unwrap();
    let v = Vfx::new("game".into(), None);
    assert!(load(&v, path.to_string_lossy().strip_suffix("_c").unwrap()));
    assert!(v.shader().unwrap().program_for(Stage::Ps).is_some());
    assert!(v.state.lock().unwrap().compiled_file.is_some(), "remembered, to reload after a recompile");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();

    // An anonymous one is found by its generated name, as the renderer looks shaders up.
    let name = format!("compiled/anonymous/test{}.shader", std::process::id());
    assert!(load::anonymous(&name, &file).is_some());
    assert!(find(&name).is_some_and(|s| s.features[0].name == "F_A"));
    assert!(load::anonymous("compiled/anonymous/bad.shader", b"not a resource").is_none());
}

#[test]
fn byte_code_managers_group_by_the_announced_static_combo() {
    let mut b = compile::ByteCode::default();
    b.on_static(3);
    b.on_dynamic(0, 1, vec![1]);
    b.on_dynamic(0, 2, vec![2]);
    assert_eq!(b.combos.keys().copied().collect::<Vec<_>>(), [(3, 1), (3, 2)]);
}

#[test]
fn combo_results_carry_their_fields() {
    let s = loaded().shader().unwrap();
    let r = compile::make_info(compile::compile_combo(&s, PS, 2, 0));
    assert!(abi::rt::fields::get::<bool, { abi::slots::vfx_compiled_shader_info_t::GET_COMPILE_FAILED.index }>(r.as_raw()));
    let output = abi::rt::fields::get_str::<{ abi::slots::vfx_compiled_shader_info_t::GET_COMPILER_OUTPUT.index }>(r.as_raw());
    assert!(matches!(output, abi::StrRet::Owned(o) if o.to_string_lossy().contains("outside")));
    assert_eq!(compile::info_combo(r).map(|(ids, _, failed)| (ids, failed)), Some(((2, 0), true)));
    objects::destroy(r);
}

/// A game's source edited after its `.shader_c` was built, and broken against core (grubs'
/// new_water reads a VS-only interpolant in PS): the renderer draws with the shipped file.
#[test]
fn a_source_that_fails_to_compile_draws_with_its_shipped_file() {
    let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
    let good = "HEADER { }\nFEATURES { }\nCOMMON { struct PsIn { float4 vPositionPs : SV_Position; }; }\n\
        VS { PsIn MainVs( float3 p : POSITION ) { PsIn o; o.vPositionPs = float4( p, 1 ); return o; } }\n\
        PS { float4 MainPs( PsIn i ) : SV_Target0 { return float4( 1, 0, 0, 1 ); } }\n";
    let broken = good.replace("float4( 1, 0, 0, 1 )", "i.vMissing");
    let name = format!("ubx_shipped_{}", std::process::id());
    let root = write_temp("root", "").parent().unwrap().to_path_buf();
    let path = root.join(format!("shaders/{name}.shader"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &broken).unwrap();
    let body = [b"vcs2".as_slice(), &68i32.to_le_bytes(), &2i32.to_le_bytes(), &[9; 16]].concat();
    let vs = Stage::Vs as i64;
    let sprv = compiled::sprv_payload(&[(FEATURE as usize, body.clone()), (vs as usize, body.clone()), (PS as usize, body)]);
    let hlsl = serde_json::json!({ "Programs": { "VFX_PROGRAM_VS": tools::mask(good, vs, false), "VFX_PROGRAM_PS": tools::mask(good, PS, false) } });
    std::fs::write(format!("{}_c", path.display()), resource_file(sprv, Some(&hlsl.to_string()))).unwrap();
    let root = root.to_string_lossy().into_owned();
    crate::fs::add_search_path("GAME", &root, false);

    let resource = format!("shaders/{name}.shader");
    assert!(find(&resource).is_some(), "the broken source still parses");
    assert!(crate::render::shading::program(gpu, &resource).is_some(), "built from the shipped file's HLSL");
    crate::fs::remove_search_path("GAME", &root);
    std::fs::remove_dir_all(&root).unwrap();
}
