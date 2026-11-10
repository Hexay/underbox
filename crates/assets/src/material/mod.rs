//! Materials from `.vmat` source (KV1, one `Layer0` block). Texture inputs are named the way
//! the material editor shows them (`TextureColor`); [`bind_textures`] turns them into the
//! shader attributes (`g_tColor`) the engine binds.

mod shader;

pub use shader::texture_inputs;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Material {
    pub shader: String,
    /// Static combo features (`F_*`).
    pub features: Vec<(String, i32)>,
    /// Texture inputs: (input name, image or .vtex path).
    pub textures: Vec<(String, String)>,
    /// Numeric parameters (`g_fl*`, `g_v*`, `g_n*`, `g_b*`); scalars fill x only.
    pub values: Vec<(String, [f32; 4])>,
}

pub fn from_vmat(src: &str) -> Result<Material, kv::Error> {
    let root = kv::kv1::parse(src)?;
    let layer = root.0.iter().find_map(|(_, v)| v.as_object()).cloned().unwrap_or_default();
    let mut material = Material::default();
    for (key, value) in &layer.0 {
        let Some(text) = value.as_str() else { continue };
        if key.eq_ignore_ascii_case("shader") {
            material.shader = text.to_owned();
        } else if key.starts_with("F_") {
            material.features.push((key.clone(), text.trim().parse().unwrap_or(0)));
        } else if let Some(v) = numbers(text) {
            material.values.push((key.clone(), v));
        } else if !text.is_empty() {
            material.textures.push((key.clone(), text.to_owned()));
        }
    }
    Ok(material)
}

/// `"0.5"` or `"[1 2 3 4]"` as a vector; `None` for anything else (paths).
fn numbers(text: &str) -> Option<[f32; 4]> {
    let inner = text.trim().trim_start_matches('[').trim_end_matches(']');
    let parsed: Vec<f32> = inner.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
    (1..=4).contains(&parsed.len()).then(|| [0, 1, 2, 3].map(|i| parsed.get(i).copied().unwrap_or(0.0)))
}

/// The attribute a texture input binds to when its shader's declarations aren't available
/// (closed shaders): `TextureColor` → `g_tColor`, `SkyTexture` → `g_tSkyTexture`. A guess at
/// Valve's convention; declarations win wherever we have them.
pub fn conventional_attribute(input: &str) -> String {
    if input.starts_with("g_t") {
        return input.to_owned();
    }
    format!("g_t{}", input.strip_prefix("Texture").filter(|s| !s.is_empty()).unwrap_or(input))
}

/// (attribute, texture path) for each texture input, through `declared` (input → attributes
/// from the shader source) when given, else [`conventional_attribute`].
pub fn bind_textures(material: &Material, declared: Option<&[(String, String)]>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (input, path) in &material.textures {
        let attrs: Vec<String> = match declared {
            Some(d) => d.iter().filter(|(i, _)| i.eq_ignore_ascii_case(input)).map(|(_, a)| a.clone()).collect(),
            None => Vec::new(),
        };
        if attrs.is_empty() {
            out.push((conventional_attribute(input), path.clone()));
        }
        out.extend(attrs.into_iter().map(|a| (a, path.clone())));
    }
    out
}

#[cfg(test)]
mod tests;
