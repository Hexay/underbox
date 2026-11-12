//! World-panel UI variants compile from sbox-public's source: `ui/scissor.hlsl` reads
//! `ui/pixel.hlsl`'s `BoxSize` above its declaration (hoist.rs). Needs DXC and a checkout; skips
//! without either.

mod common;

use shaders::{Compiler, Stage, Variant, VfxShader};

#[test]
fn ui_world_panel_pixel_shaders_compile() {
    let (Some(core), Ok(mut compiler)) = (common::core_dir(), Compiler::new()) else {
        eprintln!("no sbox-public checkout or DXC; skipped");
        return;
    };
    compiler.cache = None;
    for name in ["ui_text", "ui_cssbox", "ui_basic"] {
        let shader = VfxShader::parse_file(&core.join(format!("shaders/{name}.shader")), &common::includes(&core)).unwrap();
        let ps = shader.program_for(Stage::Ps).unwrap();
        let world = ps.dynamics.encode(|n| (n == "D_WORLDPANEL").then_some(1));
        if let Err(e) = compiler.compile(&shader, ps, &Variant::new(0, world)) {
            panic!("{name} with D_WORLDPANEL: {e}");
        }
    }
}
