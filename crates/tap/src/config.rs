//! What to log, from the environment once at the handshake (docs/differential-tap.md, "Filters").

use crate::call::Side;
use crate::generated::trampolines::{CALLBACK_CLASS, CALLBACK_NAMES};
use std::path::PathBuf;
use std::sync::OnceLock;

pub struct Config {
    pub dir: PathBuf,
    native: Vec<bool>,
    managed: Vec<bool>,
    pub tick: Option<(Side, usize)>,
    pub ticks: (u64, u64),
    pub max_bytes: u64,
    /// The game dir (the server's cwd), lowercased: strings under it print as `$GAME`.
    pub game: Option<String>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get() -> Option<&'static Config> {
    CONFIG.get()
}

#[cfg(test)]
pub fn install(cfg: Config) -> &'static Config {
    CONFIG.get_or_init(|| cfg)
}

/// Read the environment; `None` (log nothing) without `UNDERBOX_TAP_DIR`.
pub fn init() -> Option<&'static Config> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let dir = PathBuf::from(var("UNDERBOX_TAP_DIR")?);
    let game = std::env::current_dir().ok().map(|d| d.to_string_lossy().to_ascii_lowercase());
    let cfg = Config::new(
        dir,
        &var("UNDERBOX_TAP_SLOTS").unwrap_or_else(|| "*".into()),
        &var("UNDERBOX_TAP_CLASSES").unwrap_or_else(|| "*".into()),
        &var("UNDERBOX_TAP_TICK").unwrap_or_else(|| "global_SourceEngineFrame".into()),
        var("UNDERBOX_TAP_TICKS").as_deref(),
        var("UNDERBOX_TAP_MAX_MB").and_then(|v| v.parse().ok()).unwrap_or(2048),
        game,
    );
    Some(CONFIG.get_or_init(|| cfg))
}

impl Config {
    pub fn new(dir: PathBuf, slots: &str, classes: &str, tick: &str, ticks: Option<&str>, max_mb: u64, game: Option<String>) -> Self {
        let on = |name: &str, class: &str| selected(slots, name) && selected(classes, class);
        let native = (0..abi::SLOT_COUNT).map(|i| on(abi::NAMES[i], abi::SLOT_CLASS[i])).collect();
        let managed = (0..abi::CALLBACK_COUNT).map(|i| on(CALLBACK_NAMES[i], CALLBACK_CLASS[i])).collect();
        let tick = abi::NAMES
            .iter()
            .position(|n| *n == tick)
            .map(|i| (Side::Native, i))
            .or_else(|| CALLBACK_NAMES.iter().position(|n| *n == tick).map(|i| (Side::Managed, i)));
        let game = game.filter(|g| g.len() > 1);
        Self { dir, native, managed, tick, ticks: ticks.map_or((0, u64::MAX), tick_range), max_bytes: max_mb << 20, game }
    }

    pub fn enabled(&self, side: Side, index: usize) -> bool {
        match side {
            Side::Native => self.native[index],
            Side::Managed => self.managed[index],
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.native.iter().filter(|&&b| b).count(), self.managed.iter().filter(|&&b| b).count())
    }
}

/// Comma-separated globs, `!` excludes; the last pattern that matches decides, none matching means no.
fn selected(patterns: &str, name: &str) -> bool {
    let mut on = false;
    for p in patterns.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match p.strip_prefix('!') {
            Some(p) if glob(p.as_bytes(), name.as_bytes()) => on = false,
            None if glob(p.as_bytes(), name.as_bytes()) => on = true,
            _ => {}
        }
    }
    on
}

/// `*` matches any run, `?` one byte.
fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

/// `a-b`, `a-` or `a`, inclusive.
fn tick_range(s: &str) -> (u64, u64) {
    let (lo, hi) = s.split_once('-').unwrap_or((s, s));
    (lo.trim().parse().unwrap_or(0), hi.trim().parse().unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob(b"IPhysics*", b"IPhysicsBody_SetMass"));
        assert!(glob(b"*_Get?", b"CModel_GetX"));
        assert!(!glob(b"IPhysics*", b"CModel_GetX"));
        assert!(glob(b"*", b""));
        assert!(selected("IPhysics*,!*_GetMass", "IPhysicsBody_SetMass"));
        assert!(!selected("IPhysics*,!*_GetMass", "IPhysicsBody_GetMass"));
        assert!(!selected("", "x"));
    }

    #[test]
    fn ranges() {
        assert_eq!(tick_range("5-9"), (5, 9));
        assert_eq!(tick_range("5-"), (5, u64::MAX));
        assert_eq!(tick_range("7"), (7, 7));
    }

    #[test]
    fn filters_select_slots_and_the_tick() {
        let c = Config::new(PathBuf::new(), "IPhysicsWorld_*", "*", "global_SourceEngineFrame", None, 1, None);
        let step = abi::NAMES.iter().position(|n| *n == "IPhysicsWorld_StepSimulation").unwrap();
        assert!(c.enabled(Side::Native, step));
        assert!(!c.enabled(Side::Native, 0));
        assert!(!c.enabled(Side::Managed, 0));
        assert!(matches!(c.tick, Some((Side::Native, _))));
        let c = Config::new(PathBuf::new(), "*", "!IPhysicsWorld", "Sandbox_RealTime_Update", None, 1, None);
        assert!(!c.enabled(Side::Native, step));
        assert!(matches!(c.tick, Some((Side::Managed, 33))));
    }
}
