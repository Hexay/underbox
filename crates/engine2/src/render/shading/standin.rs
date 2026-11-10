//! The stand-in program for core material shaders that ship without source (`complex`,
//! `generic`, `unlit`, ...; sbox-public has only their `.shader.meta`): standin.shader, our own
//! vfx, translated like any other. Its inputs follow complex's conventions (`g_tColor`,
//! `g_vColorTint`, `g_flModelTintAmount`), so typical `.vmat` files keep their colour and tint.

use shaders::{IncludePaths, VfxShader};
use std::path::Path;

/// The key the stand-in program is cached under (never a real resource path).
pub const PATH: &str = "shaders/underbox_standin.shader";

/// Closed core material shaders drawn with the stand-in, and whether the original is unlit.
const MAPPED: &[(&str, bool)] = &[
    ("complex", false),
    ("generic", false),
    ("simple", false),
    ("skin", false),
    ("eyeball", false),
    ("static_overlay", false),
    ("error", false),
    ("tools_generic", false),
    ("tools_solid", false),
    ("unlit", true),
    ("tools_textured_unlit", true),
];

fn stem(shader_path: &str) -> &str {
    let name = shader_path.rsplit(['/', '\\']).next().unwrap_or(shader_path);
    let name = name.strip_suffix("_c").unwrap_or(name);
    name.strip_suffix(".shader").or_else(|| name.strip_suffix(".vfx")).unwrap_or(name)
}

/// `Some(unlit)` when `shader_path` names a mapped closed shader.
pub fn mapped(shader_path: &str) -> Option<bool> {
    let stem = stem(shader_path);
    MAPPED.iter().find(|(name, _)| stem.eq_ignore_ascii_case(name)).map(|(_, unlit)| *unlit)
}

/// Values a draw through the stand-in adds for the original shader (`StandinUnlit`).
pub fn values(unlit: bool) -> [(&'static str, f32); 1] {
    [("StandinUnlit", if unlit { 1.0 } else { 0.0 })]
}

pub fn shader(includes: &IncludePaths) -> Result<VfxShader, String> {
    VfxShader::parse(include_str!("standin.shader"), Path::new(PATH), includes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_closed_material_shaders_by_name() {
        assert_eq!(mapped("shaders/complex.shader"), Some(false));
        assert_eq!(mapped("shaders/complex.shader_c"), Some(false));
        assert_eq!(mapped("unlit.shader"), Some(true));
        assert_eq!(mapped("complex.vfx"), Some(false));
        assert_eq!(mapped("shaders/scene_renderer_mesh.shader"), None);
    }
}
