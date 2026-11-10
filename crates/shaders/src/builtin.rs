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

builtins!["math_general.fxc"];

pub fn get(name: &str) -> Option<&'static str> {
    FILES.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}
