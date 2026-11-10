use super::*;

fn game(path: &str) -> Option<String> {
    let root = std::env::var("SBOX_PUBLIC").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../../sbox-public").into());
    std::fs::read_to_string(format!("{root}/game/{path}")).ok()
}

#[test]
fn vmat_splits_shader_features_values_textures() {
    let src = "Layer0\n{\n\tshader \"shaders/sky.shader\"\n\tF_TEXTURE_FORMAT2 6\n\tg_flBias \"0.200\"\n\tg_vTint \"[1 0.5 0 1]\"\n\tSkyTexture \"materials/skybox/day.exr\"\n}";
    let m = from_vmat(src).unwrap();
    assert_eq!(m.shader, "shaders/sky.shader");
    assert_eq!(m.features, [("F_TEXTURE_FORMAT2".into(), 6)]);
    assert_eq!(m.values, [("g_flBias".into(), [0.2, 0.0, 0.0, 0.0]), ("g_vTint".into(), [1.0, 0.5, 0.0, 1.0])]);
    assert_eq!(bind_textures(&m, None), [("g_tSkyTexture".into(), "materials/skybox/day.exr".into())]);
}

#[test]
fn conventions() {
    assert_eq!(conventional_attribute("TextureColor"), "g_tColor");
    assert_eq!(conventional_attribute("SkyTexture"), "g_tSkyTexture");
    assert_eq!(conventional_attribute("g_tNormal"), "g_tNormal");
}

#[test]
fn line_shader_declares_packed_textures() {
    let Some(src) = game("core/shaders/line.shader") else { return eprintln!("skipped: no sbox-public") };
    let pairs = texture_inputs(&src);
    let has = |i: &str, a: &str| pairs.iter().any(|(x, y)| x == i && y == a);
    assert!(has("TextureColor", "g_tColor"), "{pairs:?}");
    assert!(has("TextureRoughness", "g_tRma") && has("TextureMetalness", "g_tRma") && has("TextureAmbientOcclusion", "g_tRma"), "{pairs:?}");
    let m = Material { textures: vec![("TextureMetalness".into(), "m.tga".into())], ..Default::default() };
    assert_eq!(bind_textures(&m, Some(&pairs)), [("g_tRma".into(), "m.tga".into())], "declarations win over convention");
}

#[test]
fn every_vmat_in_the_corpus_parses() {
    let root = std::env::var("SBOX_PUBLIC").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../../sbox-public").into());
    let mut stack = vec![std::path::PathBuf::from(format!("{root}/game"))];
    let (mut parsed, mut with_shader) = (0, 0);
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "vmat") {
                let text = std::fs::read_to_string(&path).unwrap();
                let m = from_vmat(&text).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
                parsed += 1;
                with_shader += usize::from(!m.shader.is_empty());
            }
        }
    }
    assert!(parsed == 0 || with_shader * 10 >= parsed * 9, "{with_shader}/{parsed} name a shader");
}
