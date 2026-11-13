//! Render states per family: what each declaration does to a draw's state (parse and evaluate,
//! no GPU), then the same states drawn headless: stencil write and test, alpha blending, colour
//! masks and back-face culling. GPU tests skip without a device.

use super::*;
use crate::mesh::layout::{Field, Layout, format};
use crate::render::draw::{self, Draw, Target, shaded::Shaded};
use crate::render::lighting::Lighting;
use crate::render::shading::{program, vertex};
use crate::render::{device, material, texture};
use glam::Mat4;
use render::BufferKind;
use render::pass::{DrawUniforms, Load};
use std::sync::Arc;

fn parse(ps: &str) -> VfxShader {
    let src = format!(
        "MODES {{ Default(); }}\nFEATURES {{ Feature( F_RENDER_BACKFACES, 0..1, \"x\" ); }}\nCOMMON {{\nStaticCombo( S_BACK, F_RENDER_BACKFACES, Sys( ALL ) );\nDynamicCombo( D_PASS, 0..3, Sys( ALL ) );\n}}\n\
         VS {{\nstruct VsIn {{ float3 pos : POSITION; }};\nfloat4 MainVs( VsIn i ) : SV_Position {{ return float4( i.pos.xy, 0.5, 1 ); }}\n}}\n\
         PS {{\n{ps}\nfloat4 MainPs() : SV_Target0 {{ return D_PASS == 0 ? float4( 1, 0, 0, 1 ) : D_PASS == 1 ? float4( 0, 1, 0, 0.5 ) : float4( 1, 1, 1, 1 ); }}\n}}\n"
    );
    VfxShader::parse(&src, std::path::Path::new("states.shader"), &shaders::IncludePaths::new(Vec::new())).unwrap()
}

fn applied(ps: &str, statics: u64, dynamics: u64) -> RasterState {
    of(&parse(ps), &|_| Variant::new(statics, dynamics)).apply(RasterState::default())
}

#[test]
fn depth_states() {
    let s = applied("RenderState( DepthWriteEnable, false );\nRenderState( DepthEnable, false );", 0, 0);
    assert_eq!((s.depth_compare, s.depth_write), (C::Always, false), "postprocess turns depth off");
    assert_eq!(Overrides::default().apply(RasterState::default()), RasterState::default(), "no declarations keep the draw's state");
    let func = "RenderState( DepthFunc, D_PASS == 0 ? LESS_EQUAL : EQUAL );";
    assert_eq!(applied(func, 0, 0).depth_compare, C::LessEqual, "taken as declared: every view is reverse-Z");
    assert_eq!(applied(func, 0, 1).depth_compare, C::Equal);
    let guarded = "RenderState( DepthWriteEnable, true );\n#if D_PASS == 1\nRenderState( DepthWriteEnable, false );\n#endif";
    assert!(applied(guarded, 0, 0).depth_write && !applied(guarded, 0, 1).depth_write, "#if picks the combo's state");
}

#[test]
fn cull_states() {
    let cull = "RenderState( CullMode, F_RENDER_BACKFACES ? NONE : DEFAULT );";
    assert_eq!(applied(cull, 0, 0).cull, Some(Face::Back), "the feature's static combo at 0");
    assert_eq!(applied(cull, 1, 0).cull, None, "F_RENDER_BACKFACES = 1 through S_BACK");
    assert_eq!(applied("RenderState( CullMode, FRONT );", 0, 0).cull, Some(Face::Front));
}

#[test]
fn blend_states() {
    let additive = "#if D_PASS == 1\nRenderState( BlendEnable, true );\nRenderState( SrcBlend, SRC_ALPHA );\nRenderState( DstBlend, ONE );\n#endif";
    assert_eq!(applied(additive, 0, 0).blend, None);
    let b = applied(additive, 0, 1).blend.unwrap();
    assert_eq!((b.color.src_factor, b.color.dst_factor, b.color.operation), (F::SrcAlpha, F::One, Op::Add));
    assert_eq!((b.alpha.src_factor, b.alpha.dst_factor), (F::One, F::Zero), "D3D's alpha defaults");
    let full = "RenderState( BlendEnable, true );\nRenderState( SrcBlend, DEST_COLOR );\nRenderState( DstBlend, SRC_COLOR );\nRenderState( BlendOp, REV_SUBTRACT );\nRenderState( SrcBlendAlpha, ZERO );\nRenderState( DstBlendAlpha, INV_SRC_ALPHA );\nRenderState( BlendOpAlpha, MAX );";
    let b = applied(full, 0, 0).blend.unwrap();
    assert_eq!(b.color, BlendComponent { src_factor: F::Dst, dst_factor: F::Src, operation: Op::ReverseSubtract });
    assert_eq!(b.alpha, BlendComponent { src_factor: F::Zero, dst_factor: F::OneMinusSrcAlpha, operation: Op::Max });
    assert_eq!(applied("RenderState( BlendEnable, false );", 0, 0).blend, None);
}

#[test]
fn color_write_and_coverage_states() {
    assert_eq!(applied("RenderState( ColorWriteEnable0, RGB );", 0, 0).write_mask, ColorWrites::COLOR);
    assert_eq!(applied("RenderState( ColorWriteEnable0, 0 );", 0, 0).write_mask, ColorWrites::empty());
    let a2c = "RenderState( AlphaToCoverageEnable, S_BACK == 0 );";
    assert!(applied(a2c, 0, 0).alpha_to_coverage && !applied(a2c, 1, 0).alpha_to_coverage);
}

#[test]
fn stencil_states() {
    let one_sided = "RenderState( StencilEnable, true );\nRenderState( StencilPassOp, REPLACE );\nRenderState( StencilRef, 3 );\nRenderState( StencilWriteMask, 254 );";
    let s = applied(one_sided, 0, 0).stencil.unwrap();
    assert_eq!((s.front.pass_op, s.front.compare, s.reference, s.write_mask, s.read_mask), (S::Replace, C::Always, 3, 254, 0xFF));
    assert_eq!(s.back, s.front, "back faces follow the front without BackStencil states");
    let two_sided = "RenderState( StencilEnable, true );\nRenderState( StencilFunc, NOT_EQUAL );\nRenderState( BackStencilFunc, F_RENDER_BACKFACES ? NOT_EQUAL : ALWAYS );";
    let s = applied(two_sided, 0, 0).stencil.unwrap();
    assert_eq!((s.front.compare, s.back.compare), (C::NotEqual, C::Always));
}

/// What the GPU passes draw: 0 red writing stencil 1 (left half only), 1 green at half alpha
/// blended where stencil is 1, 2 white through a blue-only mask, 3 a triangle culled per winding.
const PASSES: &str = "RenderState( DepthEnable, false );\n\
    #if D_PASS == 0\nRenderState( StencilEnable, true );\nRenderState( StencilPassOp, REPLACE );\nRenderState( StencilRef, 1 );\n\
    #elif D_PASS == 1\nRenderState( StencilEnable, true );\nRenderState( StencilFunc, EQUAL );\nRenderState( StencilRef, 1 );\n\
    RenderState( BlendEnable, true );\nRenderState( SrcBlend, SRC_ALPHA );\nRenderState( DstBlend, INV_SRC_ALPHA );\n\
    #elif D_PASS == 2\nRenderState( ColorWriteEnable0, B );\n\
    #else\nRenderState( CullMode, BACK );\n#endif";

fn pass_draw(gpu: &render::Gpu, shader: &VfxShader, mat: material::Ref, pass: u64, xy: &[[f32; 2]]) -> Draw {
    let s = Arc::new(program::build(gpu, &shaders::Compiler::new().unwrap(), shader, "states", &Variant::new(0, pass)).unwrap());
    let fields = vec![Field { semantic: "position".into(), index: 0, format: format::RGB32F, offset: 0 }];
    let inputs = vertex::inputs(&s.vertex_inputs, &Layout { name: "xyz".into(), stride: 12, fields }).unwrap();
    let bytes: Vec<u8> = xy.iter().flat_map(|p| [p[0], p[1], 0.0]).flat_map(f32::to_le_bytes).collect();
    let instance = super::super::bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] };
    let shaded = Shaded { shading: s, vertex: inputs.keys, material: material::resolve(mat).unwrap(), values: Default::default(), instance, view_proj: Mat4::IDENTITY, context: Default::default(), cached_mesh: None };
    Draw {
        vertices: Arc::new(render::Buffer::with_data(gpu, "states", BufferKind::Vertex, &bytes)),
        vertex_offset: 0,
        vertex: Vec::new(),
        indices: None,
        range: 0..xy.len() as u32,
        base_vertex: 0,
        instances: 1,
        uniforms: DrawUniforms { mvp: Mat4::IDENTITY, model: Mat4::IDENTITY, tint: [1.0; 4] },
        state: RasterState::default(),
        shaded: Some(shaded),
        caster: None,
        cull: Default::default(),
    }
}

#[test]
fn states_draw_on_the_gpu() {
    let Some(gpu) = device::create() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let shader = parse(PASSES);
    let mat = material::create_raw(Some(c"states test"), Some(c"shaders/states.shader"), true);
    let full = [[-1.0, -1.0], [3.0, -1.0], [-1.0, 3.0]];
    let left = [[-1.0, -1.0], [0.0, -1.0], [-1.0, 1.0], [0.0, -1.0], [0.0, 1.0], [-1.0, 1.0]];
    let draws = [pass_draw(gpu, &shader, mat, 0, &left), pass_draw(gpu, &shader, mat, 1, &full), pass_draw(gpu, &shader, mat, 2, &full)];
    // D24S8: the depth target needs a stencil aspect.
    let (color, depth) = (texture::create_target("states", 8, 8, 0), texture::create_target("states depth", 8, 8, 63));
    let target = Target { color, depth, ..Default::default() };
    let clear = Load { color: Some(render::wgpu::Color::BLACK), depth: Some(0.0) };
    draw::execute(&target, &Lighting::default(), clear, &draws);
    let frame = texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu);
    let (l, r) = (frame.texel(2, 4), frame.texel(6, 4));
    assert!(l[0].abs_diff(128) <= 1 && l[1].abs_diff(128) <= 1 && l[2] == 255, "red, half green over it, then blue only: {l:?}");
    assert_eq!(r[..3], [0, 0, 255][..], "stencil kept green off the right half; the mask let blue through: {r:?}");

    // Counter-clockwise on screen (y up) is front-facing, as wgpu's default and our meshes wind.
    let (ccw, cw) = ([[-1.0, -1.0], [3.0, -1.0], [-1.0, 3.0]], [[-1.0, -1.0], [-1.0, 3.0], [3.0, -1.0]]);
    let covered = |tri: &[[f32; 2]]| {
        draw::execute(&target, &Lighting::default(), clear, &[pass_draw(gpu, &shader, mat, 3, tri)]);
        texture::resolve(color).unwrap().gpu.as_ref().unwrap().read_back(gpu).texel(4, 4)[0] == 255
    };
    assert!(covered(&ccw) && !covered(&cw), "CullMode BACK drops the clockwise triangle only");
    [color, depth].into_iter().for_each(crate::resource::destroy_handle);
    crate::resource::destroy_handle(mat);
}
