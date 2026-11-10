//! Our replacements for Valve's closed core shader headers, which the s&box core shaders include
//! but sbox-public doesn't ship (crates/shaders/hlsl/README.md). Used only when an include
//! resolves nowhere on disk, so a real header always wins.

/// Path prefix resolved built-ins carry (`#line` markers and dependency lists show it).
pub const PREFIX: &str = "<underbox>";

macro_rules! builtins {
    ($($name:literal),* $(,)?) => {
        const FILES: &[(&str, &str)] = &[$(($name, include_str!(concat!("../hlsl/builtin/", $name)))),*];
    };
}

builtins![
    "ambient_cube.fxc",
    "baked_lighting_constants.fxc",
    "encoded_normals.fxc",
    "ffd.fxc",
    "instancing.fxc",
    "light_probe_volume.fxc",
    "math_general.fxc",
    "morph.fxc",
    "octohedral_encoding.fxc",
    "sheet_sampling.fxc",
    "transform_buffer.fxc",
    "volumetric_fog.fxc",
    "vs_decompress.fxc",
];

pub fn get(name: &str) -> Option<&'static str> {
    FILES.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}
