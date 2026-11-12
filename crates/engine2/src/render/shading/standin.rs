//! Stand-in programs for core material shaders that ship without source (`complex`, `generic`,
//! `unlit`, `sky`, ...; sbox-public has only their `.shader.meta`): our own vfx, translated like
//! any other. standin.shader follows complex's conventions (`g_tColor`, `g_vColorTint`,
//! `g_flModelTintAmount`), so typical `.vmat` files keep their colour and tint;
//! standin_sky.shader samples a sky material's `SkyTexture` cube along the view ray.
//! The gizmo stand-ins replace shaders that have source but whose geometry shaders don't
//! translate: they fold the GS into the VS over pre-expanded corners (render/draw/gizmo.rs).

use shaders::{IncludePaths, VfxShader};
use std::path::Path;

/// The keys the stand-in programs are cached under (never real resource paths).
pub const PATH: &str = "shaders/underbox_standin.shader";
pub const SKY_PATH: &str = "shaders/underbox_standin_sky.shader";
pub const GIZMO_LINE_PATH: &str = "shaders/underbox_standin_gizmo_line.shader";
pub const GIZMO_SPRITE_PATH: &str = "shaders/underbox_standin_gizmo_sprite.shader";
pub const ALL_PATHS: [&str; 4] = [PATH, SKY_PATH, GIZMO_LINE_PATH, GIZMO_SPRITE_PATH];

/// Which stand-in draws a closed shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Lit,
    Unlit,
    Sky,
    GizmoLine,
    GizmoSprite,
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
    ("gizmo_line", Kind::GizmoLine),
    ("gizmo_sprite", Kind::GizmoSprite),
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
            Kind::GizmoLine => GIZMO_LINE_PATH,
            Kind::GizmoSprite => GIZMO_SPRITE_PATH,
        }
    }

    /// Whether the stand-in draws even when the shader's source is found (its GS won't translate).
    pub fn replaces_source(self) -> bool {
        matches!(self, Kind::GizmoLine | Kind::GizmoSprite)
    }

    /// Values a draw through the stand-in adds for the original shader (`StandinUnlit`), and the
    /// debug view `UNDERBOX_STANDIN_DEBUG` picks (standin.shader).
    pub fn values(self) -> Vec<(&'static str, f32)> {
        static DEBUG: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| std::env::var("UNDERBOX_STANDIN_DEBUG").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0));
        match self {
            Kind::Lit => vec![("StandinUnlit", 0.0), ("StandinDebug", *DEBUG)],
            Kind::Unlit => vec![("StandinUnlit", 1.0), ("StandinDebug", *DEBUG)],
            Kind::Sky | Kind::GizmoLine | Kind::GizmoSprite => Vec::new(),
        }
    }
}

pub fn is_path(path: &str) -> bool {
    matches!(path, PATH | SKY_PATH | GIZMO_LINE_PATH | GIZMO_SPRITE_PATH)
}

/// The stand-in source cached under `path`, or `None` when `path` isn't a stand-in key.
pub fn shader(path: &str, includes: &IncludePaths) -> Option<Result<VfxShader, String>> {
    let source = match path {
        PATH => include_str!("standin.shader")
            .replace("\t//@sun_shadows\n", include_str!("standin_sun_shadows.hlsl"))
            .replace("\t//@managed_shadows\n", include_str!("standin_managed_shadows.hlsl"))
            .replace("\t//@lighting\n", include_str!("standin_lighting.hlsl"))
            .replace("\t//@decals\n", include_str!("standin_decals.hlsl")),
        SKY_PATH => include_str!("standin_sky.shader").to_owned(),
        GIZMO_LINE_PATH => include_str!("standin_gizmo_line.shader").to_owned(),
        GIZMO_SPRITE_PATH => include_str!("standin_gizmo_sprite.shader").to_owned(),
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
        assert_eq!(mapped("shaders/gizmo/gizmo_line.shader").map(Kind::path), Some(GIZMO_LINE_PATH));
        assert_eq!(mapped("shaders/scene_renderer_mesh.shader"), None);
    }
}
