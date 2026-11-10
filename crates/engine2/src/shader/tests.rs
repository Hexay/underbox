use super::*;

const SRC: &str = "FEATURES { Feature( F_A, 0..1, \"G\" ); }\n\
    COMMON { }\n\
    PS { StaticCombo( S_A, F_A, Sys( ALL ) ); DynamicCombo( D_B, 0..2, Sys( ALL ) ); float4 MainPs() : SV_Target0 { return 0; } }\n";

fn loaded() -> Vfx {
    let path = std::env::temp_dir().join(format!("underbox-vfx-{}.shader", std::process::id()));
    std::fs::write(&path, SRC).unwrap();
    let v = Vfx { name: "test".into(), resource: None, shader: RwLock::new(None) };
    assert!(load(&v, &path.to_string_lossy()));
    let _ = std::fs::remove_file(path);
    v
}

#[test]
fn loads_source_and_describes_combos() {
    let v = loaded();
    let s = v.shader.read().unwrap().clone().unwrap();
    assert!(!v.is_error());
    assert_eq!(describe(&s, Stage::Ps as i64, 1, 2), "S_A=1 D_B=2");
    assert_eq!(describe(&s, Stage::Ps as i64, 0, 0), "");
    assert_eq!(describe(&s, Stage::Vs as i64, 1, 1), "");
}

#[test]
fn missing_file_is_an_error() {
    let v = Vfx { name: "missing".into(), resource: None, shader: RwLock::new(None) };
    assert!(!load(&v, "/nonexistent/x.shader"));
    assert!(v.is_error());
}

#[test]
fn resource_paths_normalise() {
    for p in ["postprocess/cmaa2_cs", "shaders/postprocess/cmaa2_cs.shader", "/shaders/postprocess/cmaa2_cs.shader_c", "postprocess/cmaa2_cs.shader"] {
        assert_eq!(resource_path(p), "shaders/postprocess/cmaa2_cs.shader", "{p}");
    }
}
