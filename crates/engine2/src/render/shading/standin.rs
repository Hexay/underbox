//! Stand-in programs for core material shaders that ship without source (`complex`, `generic`,
//! `unlit`, `sky`, ...; sbox-public has only their `.shader.meta`): our own vfx, translated like
//! any other. standin.shader follows complex's conventions (`g_tColor`, `g_vColorTint`,
//! `g_flModelTintAmount`), so typical `.vmat` files keep their colour and tint;
//! standin_sky.shader samples a sky material's `SkyTexture` cube along the view ray.
//! standin_gizmo.shader stands in for the gizmo shaders, which have source but whose geometry
//! shaders don't translate ([`Kind::Gizmo`] maps even when the shader is found).

use shaders::{IncludePaths, VfxShader};
use std::path::Path;

/// The keys the stand-in programs are cached under (never real resource paths).
pub const PATH: &str = "shaders/underbox_standin.shader";
pub const SKY_PATH: &str = "shaders/underbox_standin_sky.shader";
pub const GIZMO_PATH: &str = "shaders/underbox_standin_gizmo.shader";

/// Which stand-in draws a closed shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Lit,
    Unlit,
    Sky,
    Gizmo,
}

const MAPPED: &[(&str, Kind)] = &[
    ("complex", Kind::Lit),
    ("generic", Kind::Lit),
    ("simple", Kind::Lit),
    ("skin", Kind::Lit),
    ("eyeball", Kind::Lit),
    ("static_overlay", Kind::Lit),
    ("error", Kind::Lit),
    ("tools_generic", Kind::Lit),
    ("tools_solid", Kind::Lit),
    ("unlit", Kind::Unlit),
    ("tools_textured_unlit", Kind::Unlit),
    ("tools_sprite", Kind::Unlit),
    ("tools_wireframe", Kind::Unlit),
    ("tools_2d_generic", Kind::Unlit),
    ("ui", Kind::Unlit),
    ("sky", Kind::Sky),
    ("gizmo_line", Kind::Gizmo),
    ("gizmo_sprite", Kind::Gizmo),
];

fn stem(shader_path: &str) -> &str {
    let name = shader_path.rsplit(['/', '\\']).next().unwrap_or(shader_path);
    let name = name.strip_suffix("_c").unwrap_or(name);
    name.strip_suffix(".shader").or_else(|| name.strip_suffix(".vfx")).unwrap_or(name)
}

/// The stand-in for `shader_path`, when it names a mapped closed shader.
pub fn mapped(shader_path: &str) -> Option<Kind> {
    let stem = stem(shader_path);
    MAPPED.iter().find(|(name, _)| stem.eq_ignore_ascii_case(name)).map(|(_, kind)| *kind)
}

impl Kind {
    /// The program's cache key.
    pub fn path(self) -> &'static str {
        match self {
            Kind::Lit | Kind::Unlit => PATH,
            Kind::Sky => SKY_PATH,
            Kind::Gizmo => GIZMO_PATH,
        }
    }

    /// Values a draw through the stand-in adds for the original shader (`StandinUnlit`).
    pub fn values(self) -> Vec<(&'static str, f32)> {
        match self {
            Kind::Lit => vec![("StandinUnlit", 0.0)],
            Kind::Unlit => vec![("StandinUnlit", 1.0)],
            Kind::Sky | Kind::Gizmo => Vec::new(),
        }
    }
}

/// The stand-in source cached under `path`, or `None` when `path` isn't a stand-in key.
pub fn shader(path: &str, includes: &IncludePaths) -> Option<Result<VfxShader, String>> {
    let source = match path {
        PATH => include_str!("standin.shader").replace("\t//@managed_shadows\n", include_str!("standin_managed_shadows.hlsl")),
        SKY_PATH => include_str!("standin_sky.shader").to_owned(),
        GIZMO_PATH => include_str!("standin_gizmo.shader").to_owned(),
        _ => return None,
    };
    Some(VfxShader::parse(&source, Path::new(path), includes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_closed_material_shaders_by_name() {
        assert_eq!(mapped("shaders/complex.shader"), Some(Kind::Lit));
        assert_eq!(mapped("shaders/complex.shader_c"), Some(Kind::Lit));
        assert_eq!(mapped("unlit.shader"), Some(Kind::Unlit));
        assert_eq!(mapped("complex.vfx"), Some(Kind::Lit));
        assert_eq!(mapped("shaders/sky.shader").map(Kind::path), Some(SKY_PATH));
        assert_eq!(mapped("shaders/gizmo/gizmo_line.shader").map(Kind::path), Some(GIZMO_PATH));
        assert_eq!(mapped("shaders/scene_renderer_mesh.shader"), None);
    }
}
