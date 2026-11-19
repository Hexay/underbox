use super::*;

pub(super) const SRC: &str = "HEADER { Description = \"t\"; }\n\
    FEATURES { Feature( F_A, 0..1, \"Group A\" ); }\n\
    COMMON { }\n\
    VS { float4 MainVs() : SV_Position { return 0; } }\n\
    PS { StaticCombo( S_A, F_A, Sys( ALL ) ); DynamicCombo( D_B, 0..2, Sys( ALL ) );\n\
    float g_flScale < Attribute( \"Scale\" ); UiGroup( \"Look\" ); Range( 0.5, 4.0 ); Default( 2.0 ); >;\n\
    float4 MainPs() : SV_Target0 { return g_flScale; } }\n";

/// `src` in a fresh temp dir (one per call: tests run in parallel) as `name`; the caller removes the dir.
pub(super) fn write_temp(name: &str, src: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("underbox-vfx-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, src).unwrap();
    path
}

pub(super) fn loaded() -> Vfx {
    let path = write_temp("t.shader", SRC);
    let v = Vfx::new("test".into(), None);
    assert!(load(&v, &path.to_string_lossy()));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    v
}

#[test]
fn loads_source_and_describes_combos() {
    let v = loaded();
    let s = v.shader().unwrap();
    assert!(!v.is_error());
    assert_eq!(combos::describe(&s, Stage::Ps as i64, 1, 2), "S_A=1 D_B=2");
    assert_eq!(combos::describe(&s, Stage::Ps as i64, 0, 0), "");
    assert_eq!(combos::describe(&s, Stage::Vs as i64, 1, 1), "");
    assert_eq!(v.state.lock().unwrap().source_hash.len(), 64, "a source hash for VFX_CHECK_MD5_AGAINST_SOURCE");
}

#[test]
fn missing_file_is_an_error() {
    let v = Vfx::new("missing".into(), None);
    assert!(!load(&v, "/nonexistent/x.shader"));
    assert!(v.is_error());
}

#[test]
fn resource_paths_normalise() {
    for p in ["postprocess/cmaa2_cs", "shaders/postprocess/cmaa2_cs.shader", "/shaders/postprocess/cmaa2_cs.shader_c", "postprocess/cmaa2_cs.shader"] {
        assert_eq!(resource_path(p), "shaders/postprocess/cmaa2_cs.shader", "{p}");
    }
    assert_eq!(resource_path("compiled/effects/water.shader_c"), "compiled/effects/water.shader");
}

#[test]
fn masking_keeps_shared_blocks_and_the_programs_own() {
    let ps = tools::mask(SRC, Stage::Ps as i64, false);
    assert_eq!(ps.lines().count(), SRC.lines().count(), "lines stay where they were");
    assert!(ps.contains("FEATURES {") && ps.contains("COMMON {") && ps.contains("MainPs") && !ps.contains("MainVs"));
    let features = tools::mask(SRC, compiled::FEATURE, false);
    assert!(features.contains("HEADER {") && !features.contains("MainPs") && !features.contains("MainVs"));
    let parsed = VfxShader::parse(&ps, Path::new("t.shader"), &IncludePaths::default()).unwrap();
    assert!(parsed.program_for(Stage::Ps).is_some() && parsed.program_for(Stage::Vs).is_none());
}

#[test]
fn schema_lists_features_and_attributes() {
    let s = loaded().shader().unwrap();
    let json: serde_json::Value = serde_json::from_str(&schema::properties_json(&s)).unwrap();
    let combo = &json["Combos"][0];
    assert_eq!((combo["Name"].as_str(), combo["Group"].as_str(), combo["Min"].as_u64(), combo["Max"].as_u64()), (Some("F_A"), Some("Group A"), Some(0), Some(1)));
    let var = json["Variables"].as_array().unwrap().iter().find(|v| v["Name"] == "g_flScale").unwrap();
    assert_eq!(var["AttributeName"], "Scale");
    assert_eq!(var["UiGroup"], "Look");
    assert_eq!(var["FloatMin"][0].as_f64(), Some(0.5));
    assert_eq!(var["FloatMax"][0].as_f64(), Some(4.0));
    assert_eq!(var["FloatDefault"][0].as_f64(), Some(2.0));
}

#[test]
fn ui_variables_need_a_valid_feature_value() {
    let s = loaded().shader().unwrap();
    assert!(schema::ui_variable_active(&s, "g_flScale", "F_A", 1));
    assert!(!schema::ui_variable_active(&s, "g_flScale", "F_A", 2), "out of the feature's range");
    assert!(!schema::ui_variable_active(&s, "g_flScale", "F_MISSING", 0));
    assert!(!schema::ui_variable_active(&s, "g_tNothing", "F_A", 0));
}

#[test]
fn combo_objects_name_their_feature() {
    let s = loaded().shader().unwrap();
    let c = schema::combos(&s).remove(0);
    assert_eq!((c.name.as_str(), c.group.as_str()), ("F_A", "Group A"));
    let r = objects::create::<class::CVfxCombo, _>(c);
    assert_eq!(objects::get::<_, schema::ComboInfo>(r).map(|c| c.labels.len()), Some(2));
    objects::destroy(r);
}
