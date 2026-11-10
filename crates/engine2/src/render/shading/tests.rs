//! The material path end to end on a real device: translate sbox-public's scene_renderer_mesh,
//! build its program, match a mesh layout, and bind every reflected resource without a
//! validation error. Skips without a GPU or an sbox-public checkout.

use super::{bind, program, vertex, view::View};
use crate::mesh::layout::Layout;
use glam::Mat4;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn core_dir() -> Option<PathBuf> {
    let env = std::env::var_os("SBOX_PUBLIC").map(PathBuf::from);
    let beside = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().map(|a| a.join("sbox-public")).find(|p| p.join("game/core/shaders").is_dir());
    env.or(beside).map(|p| p.join("game/core")).filter(|p| p.is_dir())
}

#[test]
fn scene_renderer_mesh_builds_and_binds() {
    let (Some(core), Ok(gpu)) = (core_dir(), render::Gpu::headless()) else {
        eprintln!("skipped: no sbox-public checkout or GPU");
        return;
    };
    let includes = shaders::IncludePaths::new([core.clone()]);
    let shader = shaders::VfxShader::parse_file(&core.join("shaders/scene_renderer_mesh.shader"), &includes).unwrap();
    let compiler = shaders::Compiler::new().unwrap();
    let s = program::build(&gpu, &compiler, &shader, "scene_renderer_mesh", &shaders::Variant::default()).unwrap();

    let inputs = vertex::inputs(&s.vertex_inputs, &Layout::sbox_vertex()).expect("mesh layout feeds the shader");
    assert!(!inputs.keys.is_empty());

    let view = View { view_proj: Mat4::IDENTITY, viewport: [0.0, 0.0, 64.0, 64.0], target: [64.0, 64.0] };
    let instances = [bind::Instance { world: Mat4::IDENTITY, tint: [1.0; 4] }];
    let (textures, values) = (HashMap::new(), HashMap::new());
    let draw = bind::Inputs { view: &view, instances: &instances, textures: &textures, values: &[&values] };
    let groups = gpu.checked(|| bind::bind_groups(&gpu, &s, &draw)).unwrap().unwrap();
    assert_eq!(groups.len(), s.program.bind_group_layouts.len());
}
