//! A self-contained vfx shader through the whole pipeline (needs DXC: tools/fetch-dxc.sh;
//! skips without it). Checks combo defines, spec-constant baking, reflection and layout merging.

use shaders::{Compiler, IncludePaths, Stage, Variant, VfxShader, bind_group_layouts};
use std::path::Path;
use wgpu_types as wgt;

const SRC: &str = r#"
HEADER { Description = "pipeline test"; }
MODES { Forward(); }
COMMON
{
    cbuffer PerView { float4x4 g_matWorldToProjection; float4 g_vTint; };
}
struct VS_INPUT { float3 vPositionOs : POSITION < Semantic( PosXyz ); >; float2 vUv : TEXCOORD0; };
struct PS_INPUT { float4 vPositionPs : SV_Position; float2 vUv : TEXCOORD0; };
VS
{
    PS_INPUT MainVs( VS_INPUT i )
    {
        PS_INPUT o;
        o.vPositionPs = mul( g_matWorldToProjection, float4( i.vPositionOs, 1 ) );
        o.vUv = i.vUv;
        return o;
    }
}
PS
{
    DynamicCombo( D_RED, 0..1, Sys( ALL ) );
    Texture2D g_tColor < Attribute( "Color" ); >;
    SamplerState g_sColor < Filter( BILINEAR ); >;
    int Mode < Attribute( "Mode" ); Default( 3 ); SpecializedConstant(); >;
    RenderState( CullMode, NONE );
    float4 MainPs( PS_INPUT i ) : SV_Target0
    {
        float4 c = g_tColor.Sample( g_sColor, i.vUv ) * g_vTint * Mode;
    #if D_RED
        c.r = 1;
    #endif
        return c;
    }
}
"#;

fn setup() -> Option<(Compiler, VfxShader)> {
    let compiler = match Compiler::new() {
        Ok(mut c) => {
            c.cache = None;
            c
        }
        Err(e) => {
            eprintln!("DXC unavailable ({e}); run tools/fetch-dxc.sh. skipped");
            return None;
        }
    };
    Some((compiler, VfxShader::parse(SRC, Path::new("test.shader"), &IncludePaths::default()).unwrap()))
}

#[test]
fn vertex_and_pixel_to_wgsl() {
    let Some((compiler, shader)) = setup() else { return };
    let vs = compiler.compile(&shader, shader.program_for(Stage::Vs).unwrap(), &Variant::default()).unwrap();
    let ps_program = shader.program_for(Stage::Ps).unwrap();
    let ps = compiler.compile(&shader, ps_program, &Variant::default()).unwrap();

    assert_eq!(vs.reflection.entry, "MainVs");
    let inputs: Vec<_> = vs.reflection.vertex_inputs.iter().map(|v| (v.semantic.as_str(), v.format)).collect();
    assert_eq!(inputs, [("POSITION", Some(wgt::VertexFormat::Float32x3)), ("TEXCOORD0", Some(wgt::VertexFormat::Float32x2))]);

    let groups = bind_group_layouts(&[&vs.reflection, &ps.reflection], 1024);
    let all: Vec<_> = groups.iter().flatten().collect();
    let cbuffer = all.iter().find(|e| matches!(e.ty, wgt::BindingType::Buffer { .. })).unwrap();
    assert_eq!(cbuffer.visibility, wgt::ShaderStages::VERTEX | wgt::ShaderStages::FRAGMENT);
    assert!(all.iter().any(|e| matches!(e.ty, wgt::BindingType::Texture { .. })));
    assert!(all.iter().any(|e| matches!(e.ty, wgt::BindingType::Sampler(_))));

    assert_eq!(ps_program.decls.render_states, [("CullMode".to_string(), "NONE".to_string())]);
    let mode = ps_program.annotations.iter().find(|a| a.name == "Mode").unwrap();
    assert_eq!(mode.spec_id, Some(0));
    assert!(ps.wgsl.contains("3i") || ps.wgsl.contains("3.0") || ps.wgsl.contains("f32(3)"), "default baked:\n{}", ps.wgsl);
}

#[test]
fn combos_and_spec_constants_change_the_module() {
    let Some((compiler, shader)) = setup() else { return };
    let ps = shader.program_for(Stage::Ps).unwrap();
    let red = ps.dynamics.encode(|n| (n == "D_RED").then_some(1));
    let base = compiler.compile(&shader, ps, &Variant::default()).unwrap().wgsl;
    let with_red = compiler.compile(&shader, ps, &Variant::new(0, red)).unwrap().wgsl;
    let spec = Variant { spec: vec![(0, 7)], ..Variant::default() };
    let with_spec = compiler.compile(&shader, ps, &spec).unwrap().wgsl;
    assert_ne!(base, with_red);
    assert_ne!(base, with_spec);
}
