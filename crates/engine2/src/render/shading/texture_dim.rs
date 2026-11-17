//! `Source( TextureDim )` and `Source( InvTextureDim )` globals: the size of the texture their
//! `SourceArg( ... )` names, as the draw binds it (the UI shaders step their blurs over
//! `g_tColor`'s texels). `(w, h, depth, 0)` and its reciprocal; a volume's depth, else 1. An
//! unbound texture measures as the 1×1 default that stands in for it.

use super::bind::{self, Inputs};
use super::program::{Attr, Shading};
use glam::Vec4;

pub(super) fn value<P>(s: &Shading<P>, inputs: &Inputs, a: &Attr) -> Option<Vec4> {
    let inverse = match a.source.as_deref()? {
        "TextureDim" => false,
        "InvTextureDim" => true,
        _ => return None,
    };
    let arg = a.source_arg.as_deref()?;
    let bound = s.bindings.iter().find(|b| b.name == arg).and_then(|b| bind::bound_texture(s, inputs, b));
    let size = bound.as_ref().and_then(|t| t.gpu.as_ref()).map_or([1, 1, 1], |g| [g.desc.width, g.desc.height, if g.desc.volume { g.desc.layers } else { 1 }]);
    let size = size.map(|v| v.max(1) as f32);
    Some(if inverse { Vec4::new(1.0 / size[0], 1.0 / size[1], 1.0 / size[2], 0.0) } else { Vec4::new(size[0], size[1], size[2], 0.0) })
}

#[cfg(test)]
mod tests {
    use super::super::{bind::uniform_value, layout::Value, view::View};
    use super::*;
    use crate::render::{lighting::Lighting, texture};
    use render::wgpu;
    use std::collections::HashMap;

    fn shading() -> Shading<()> {
        let ty = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let color = shaders::reflect::Binding { group: 0, binding: 0, name: "g_tColor".into(), ty, count: None };
        let dim = |source: &str| Attr { source: Some(source.into()), source_arg: Some("g_tColor".into()), ..Default::default() };
        let attributes = [("g_vTextureDim", dim("TextureDim")), ("g_vInvTextureDim", dim("InvTextureDim"))].map(|(n, a)| (n.to_owned(), a)).into_iter().collect();
        let render_state = Default::default();
        Shading { label: "dim".into(), program: (), bindings: vec![color], vertex_inputs: Vec::new(), blocks: Default::default(), storage: Default::default(), attributes, render_state }
    }

    #[test]
    fn texture_dims_measure_the_paired_texture() {
        crate::render::device::create().expect("a wgpu adapter (lavapipe on headless Linux)");
        let s = shading();
        let t = texture::create_target("dim", 64, 32, 0);
        let (view, lighting) = (View::default(), Lighting::fallback());
        let value = |textures: &HashMap<String, texture::Ref>, name: &str| {
            let inputs = Inputs { view: &view, lighting: &lighting, instances: &[], textures, values: &[], attributes: &[], slots: &[], targets: &[] };
            match uniform_value(&s, &inputs, name) {
                Some(Value::Vec(v)) => v,
                other => panic!("{name}: {other:?}"),
            }
        };
        let bound = HashMap::from([("g_tColor".to_owned(), t)]);
        assert_eq!(value(&bound, "g_vTextureDim"), Vec4::new(64.0, 32.0, 1.0, 0.0));
        assert_eq!(value(&bound, "g_vInvTextureDim"), Vec4::new(1.0 / 64.0, 1.0 / 32.0, 1.0, 0.0));
        assert_eq!(value(&HashMap::new(), "g_vInvTextureDim"), Vec4::new(1.0, 1.0, 1.0, 0.0), "the 1x1 default");
        crate::resource::destroy_handle(t);
    }
}
