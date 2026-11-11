use super::*;
use crate::{dxc::Dxc, spirv, translate};

const SRC: &str = r#"
Texture3D g_bindless_Texture3D[] : register(t0, space1);
RWStructuredBuffer<float4> Out : register(u0);
cbuffer C { uint g_nIndex; };
[numthreads(1, 1, 1)]
void MainCs(uint3 id : SV_DispatchThreadID)
{
    Texture3D t = g_bindless_Texture3D[NonUniformResourceIndex(g_nIndex)];
    Out[0] = t.Load(int4(id, 0)) + t.Load(int4(id, 1), int3(1, 0, 0));
}
"#;

fn translate(words: &[u32]) -> Result<String, String> {
    translate::spirv_to_wgsl(words).map(|t| t.wgsl).map_err(|e| e.message)
}

#[test]
fn fetches_from_a_bindless_element_translate() {
    let dxc = match Dxc::load() {
        Ok(d) => d,
        Err(e) => return eprintln!("DXC unavailable ({e}); skipped"),
    };
    let words = dxc.compile(SRC, std::path::Path::new("bindless_fetch.hlsl"), "MainCs", "cs_6_0", &[]).unwrap();
    let (words, _) = spirv::strip_reflection(&words);
    let before = translate(&words).unwrap_err();
    assert!(before.contains("invalid image"), "naga's own failure, else the pass is moot: {before}");
    let wgsl = translate(&outline_bindless_fetches(&words)).unwrap();
    assert_eq!(wgsl.matches("textureLoad(").count(), 2, "{wgsl}");
    assert!(wgsl.contains("param_1: texture_3d<f32>"), "the image is a parameter:\n{wgsl}");
}

#[test]
fn modules_without_bindless_fetches_are_unchanged() {
    let m = vec![0x0723_0203, 0x10000, 0, 4, 0, inst(OP_LABEL, &[1])[0], 1];
    assert_eq!(outline_bindless_fetches(&m), m);
}
