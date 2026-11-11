use super::*;
use crate::{dxc::Dxc, spirv, translate};

/// CMAA2's pattern (an exchange whose old value is kept, plain stores and loads of the same
/// image) plus the other atomics.
const SRC: &str = r#"
RWTexture2D<uint> Heads : register(u0);
RWStructuredBuffer<uint> Out : register(u1);
[numthreads(1, 1, 1)]
void MainCs(uint3 id : SV_DispatchThreadID)
{
    uint a, b, c, d;
    Heads[id.xy + uint2(5, 0)] = 0xFFFFFFFF;
    InterlockedExchange(Heads[id.xy], 5u, a);
    InterlockedAdd(Heads[int2(id.xy) + int2(1, 0)], 3u, b);
    InterlockedCompareExchange(Heads[id.xy + uint2(2, 0)], 7u, 9u, c);
    InterlockedMax(Heads[id.xy + uint2(3, 0)], 4u, d);
    InterlockedOr(Heads[id.xy + uint2(4, 0)], 0x10u);
    Out[0] = a; Out[1] = b; Out[2] = c; Out[3] = d; Out[4] = Heads[id.xy + uint2(5, 0)];
}
"#;

fn translate(words: &[u32]) -> Result<String, String> {
    translate::spirv_to_wgsl(words).map(|t| t.wgsl).map_err(|e| e.message)
}

fn compiled() -> Option<Vec<u32>> {
    let dxc = Dxc::load().inspect_err(|e| eprintln!("DXC unavailable ({e}); skipped")).ok()?;
    let words = dxc.compile(SRC, std::path::Path::new("image_atomics.hlsl"), "MainCs", "cs_6_0", &[]).unwrap();
    Some(spirv::strip_reflection(&words).0)
}

#[test]
fn texel_atomics_become_atomics_on_a_word_buffer() {
    let Some(words) = compiled() else { return };
    let before = translate(&words).unwrap_err();
    assert!(before.contains("storage class"), "naga's own failure, else the pass is moot: {before}");
    let lowered = lower_image_atomics(&words);
    assert!(!instructions(&lowered).any(|i| op(i) == OP_IMAGE_TEXEL_POINTER), "every texel pointer is gone");
    let wgsl = translate(&lowered).unwrap();
    assert!(wgsl.contains(&format!("Heads{TEXEL_WORDS}")), "{wgsl}");
    for f in ["atomicExchange(", "atomicAdd(", "atomicCompareExchangeWeak(", "atomicMax(", "atomicOr(", "atomicStore(", "atomicLoad("] {
        assert!(wgsl.contains(f), "{f} missing:\n{wgsl}");
    }
    assert!(!wgsl.contains("textureLoad(") && !wgsl.contains("textureStore("), "no texel goes through the image:\n{wgsl}");
}

#[test]
fn modules_without_texel_pointers_are_unchanged() {
    let mut m = vec![0x0723_0203, 0x10000, 0, 4, 0];
    m.extend(inst(OP_TYPE_INT, &[1, 32, 0]));
    assert_eq!(lower_image_atomics(&m), m);
}

#[test]
fn an_image_without_a_name_is_left_alone() {
    let Some(words) = compiled() else { return };
    let unnamed: Vec<u32> = instructions(&words).filter(|i| !(op(i) == OP_NAME && string(&i[2..]) == "Heads")).flatten().copied().collect();
    let unnamed = [&words[..HEADER_WORDS], &unnamed].concat();
    assert_eq!(lower_image_atomics(&unnamed), unnamed);
}
