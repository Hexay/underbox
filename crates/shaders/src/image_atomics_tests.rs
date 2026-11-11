use super::*;
use crate::{dxc::Dxc, spirv, translate};

/// CMAA2's pattern (an exchange whose old value is kept) plus the other lowered forms.
const SRC: &str = r#"
RWTexture2D<uint> Heads : register(u0);
RWTexture2D<int> Signed : register(u1);
RWStructuredBuffer<uint> Out : register(u2);
[numthreads(1, 1, 1)]
void MainCs(uint3 id : SV_DispatchThreadID)
{
    uint a, b, c, d;
    int e;
    InterlockedExchange(Heads[id.xy], 5u, a);
    InterlockedAdd(Heads[id.xy + uint2(1, 0)], 3u, b);
    InterlockedCompareExchange(Heads[id.xy + uint2(2, 0)], 7u, 9u, c);
    InterlockedMax(Heads[id.xy + uint2(3, 0)], 4u, d);
    InterlockedMin(Signed[id.xy], -2, e);
    InterlockedOr(Heads[id.xy + uint2(4, 0)], 0x10u);
    Out[0] = a; Out[1] = b; Out[2] = c; Out[3] = d; Out[4] = uint(e);
}
"#;

fn translate(words: &[u32]) -> Result<String, String> {
    translate::spirv_to_wgsl(words).map(|t| t.wgsl).map_err(|e| e.message)
}

#[test]
fn texel_atomics_become_image_reads_and_writes() {
    let dxc = match Dxc::load() {
        Ok(d) => d,
        Err(e) => return eprintln!("DXC unavailable ({e}); skipped"),
    };
    let words = dxc.compile(SRC, std::path::Path::new("image_atomics.hlsl"), "MainCs", "cs_6_0", &[]).unwrap();
    let (words, _) = spirv::strip_reflection(&words);
    let before = translate(&words).unwrap_err();
    assert!(before.contains("storage class"), "naga's own failure, else the pass is moot: {before}");
    let lowered = lower_image_atomics(&words);
    assert!(!instructions(&lowered).any(|i| op(i) == OP_IMAGE_TEXEL_POINTER), "every texel pointer is gone");
    let wgsl = translate(&lowered).unwrap();
    assert_eq!(wgsl.matches("textureLoad(").count(), 6, "{wgsl}");
    assert_eq!(wgsl.matches("textureStore(").count(), 6, "{wgsl}");
    assert!(wgsl.contains("select("), "min, max and compare-exchange pick:\n{wgsl}");
}

#[test]
fn modules_without_texel_pointers_are_unchanged() {
    let mut m = vec![0x0723_0203, 0x10000, 0, 4, 0];
    m.extend(inst(OP_TYPE_INT, &[1, 32, 0]));
    assert_eq!(lower_image_atomics(&m), m);
}

/// `OpMemberDecorate %s 0 Offset 0` has 35 where an atomic has its pointer: a texel pointer
/// that happens to be %35 must still lower.
#[test]
fn decorations_never_pass_for_atomics() {
    const OP_MEMBER_DECORATE: u32 = 72;
    const OFFSET: u32 = 35;
    let mut m = vec![0x0723_0203, 0x10000, 0, 40, 0];
    m.extend(inst(OP_MEMBER_DECORATE, &[30, 0, OFFSET, 0]));
    m.extend(inst(OP_TYPE_INT, &[1, 32, 0]));
    m.extend(inst(OP_TYPE_POINTER, &[2, STORAGE_IMAGE, 1]));
    m.extend(inst(OP_TYPE_POINTER, &[3, 0, 4]));
    m.extend(inst(OP_VARIABLE, &[3, 5, 0]));
    m.extend(inst(OP_IMAGE_TEXEL_POINTER, &[2, OFFSET, 5, 6, 7]));
    m.extend(inst(OP_ATOMIC_EXCHANGE, &[1, 8, OFFSET, 7, 7, 9]));
    let out = lower_image_atomics(&m);
    assert!(!instructions(&out).any(|i| op(i) == OP_IMAGE_TEXEL_POINTER), "lowered");
    assert!(instructions(&out).any(|i| op(i) == OP_IMAGE_WRITE));
}

#[test]
fn an_unknown_atomic_leaves_the_module_alone() {
    const OP_ATOMIC_IINCREMENT: u32 = 232;
    let mut m = vec![0x0723_0203, 0x10000, 0, 9, 0];
    m.extend(inst(OP_TYPE_INT, &[1, 32, 0]));
    m.extend(inst(OP_TYPE_POINTER, &[2, STORAGE_IMAGE, 1]));
    m.extend(inst(OP_IMAGE_TEXEL_POINTER, &[2, 3, 4, 5, 6]));
    m.extend(inst(OP_ATOMIC_IINCREMENT, &[1, 7, 3, 8, 8]));
    assert_eq!(lower_image_atomics(&m), m);
}
