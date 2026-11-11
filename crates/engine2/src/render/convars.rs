//! Render convars engine2 defines natively, so managed's quality profiles
//! (`core/cfg/quality_profiles.json`, `RenderQualityProfiles.ApplyAll`) can set them. Names and
//! defaults are real libengine2's static `ConVar`s.

use crate::convar::{self, Spec};
use std::sync::Once;

const DEFAULTS: &[(&str, &str)] = &[
    ("r_3d_skybox", "1"),
    ("r_enable_volume_fog", "1"),
    ("volume_fog_width", "240"),
    ("volume_fog_height", "160"),
    ("volume_fog_depth", "64"),
];

/// Define every render convar (once per process); managed sees them after [`convar::announce_all`].
pub fn define() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        for (name, default) in DEFAULTS {
            convar::define(Spec { name: (*name).into(), default: (*default).into(), ..Spec::default() });
        }
    });
}

fn int(name: &str) -> Option<i32> {
    convar::find(name).map(|c| c.get_int())
}

/// `r_enable_volume_fog`: the quality profile's Off level clears it, and no fog volume draws.
pub fn volume_fog_enabled() -> bool {
    int("r_enable_volume_fog").is_none_or(|v| v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defined_with_real_defaults() {
        define();
        assert_eq!(int("volume_fog_width"), Some(240));
        assert!(volume_fog_enabled());
    }
}
