//! A shader's `RenderState( DepthEnable / DepthWriteEnable, ... )` declarations (postprocess
//! common.hlsl turns both off), applied over the draw's own raster state. Only literal values
//! count; a combo expression (`F_X ? a : b`) leaves the draw's state.
// TODO: CullMode, DepthFunc and blend states.

use render::RasterState;
use render::wgpu::CompareFunction;
use shaders::VfxShader;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    pub depth_test: Option<bool>,
    pub depth_write: Option<bool>,
}

fn literal(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Every stage's declarations, later ones winning.
pub fn of(shader: &VfxShader) -> Overrides {
    let mut o = Overrides::default();
    for (name, value) in shader.programs.iter().flat_map(|p| &p.decls.render_states) {
        match name.as_str() {
            "DepthEnable" => o.depth_test = literal(value).or(o.depth_test),
            "DepthWriteEnable" => o.depth_write = literal(value).or(o.depth_write),
            _ => {}
        }
    }
    o
}

impl Overrides {
    pub fn apply(&self, s: RasterState) -> RasterState {
        let mut s = s;
        if self.depth_test == Some(false) {
            (s.depth_compare, s.depth_write) = (CompareFunction::Always, false);
        }
        if let Some(write) = self.depth_write {
            s.depth_write = write && self.depth_test != Some(false);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postprocess_states_turn_depth_off() {
        let src = "MODES { Default(); }\nPS {\nRenderState( DepthWriteEnable, false );\nRenderState( DepthEnable, false );\nfloat4 MainPs() : SV_Target0 { return 1; }\n}\n";
        let shader = VfxShader::parse(src, std::path::Path::new("pp.shader"), &shaders::IncludePaths::new(Vec::new())).unwrap();
        let o = of(&shader);
        assert_eq!(o, Overrides { depth_test: Some(false), depth_write: Some(false) });
        let s = o.apply(RasterState::default());
        assert_eq!((s.depth_compare, s.depth_write), (CompareFunction::Always, false));
        let none = Overrides::default().apply(RasterState::default());
        assert_eq!(none, RasterState::default(), "no declarations keep the draw's state");
    }
}
