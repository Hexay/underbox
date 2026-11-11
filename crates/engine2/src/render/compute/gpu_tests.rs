use crate::render::shading::{program, tests::core_dir};

/// The compute shaders managed's scene renderer dispatches, and whether they run on the GPU.
const SHADERS: [(&str, bool); 5] =
    [("clustered_light_culling_cs", true), ("depthresolve_cs", true), ("postprocess/cmaa2_cs", true), ("downsample_spd", true), ("occlusion_cull_cs", true)];

#[test]
fn compute_shaders_translate_where_naga_can() {
    let (Some(core), Some(gpu)) = (core_dir(), crate::render::device::create()) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let includes = shaders::IncludePaths::new([core.clone()]);
    let compiler = shaders::Compiler::new().unwrap();
    for (name, gpu_ok) in SHADERS {
        let shader = shaders::VfxShader::parse_file(&core.join(format!("shaders/{name}.shader")), &includes).unwrap();
        let built = program::build_compute(gpu, &compiler, &shader, name, &shaders::Variant::default());
        eprintln!("{name}: {}", built.as_ref().map_or_else(|e| e.clone(), |s| format!("ok, workgroup {:?}", s.program.workgroup_size)));
        // Only a regression fails: one that starts translating should move to the GPU list.
        assert!(!gpu_ok || built.is_ok(), "{name} no longer builds");
    }
}
