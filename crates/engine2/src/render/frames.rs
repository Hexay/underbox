//! Frame capture for headless runs: with `UNDERBOX_FRAMES` set, presented frames are read back,
//! their hash logged (`frame: <swap chain> #<n> <w>x<h> <blake3>`) and, when the value is a
//! directory rather than `log`, written there as PPM. `UNDERBOX_FRAMES_EVERY=n` keeps every n-th.

use std::path::PathBuf;
use std::sync::LazyLock;

struct Config {
    dir: Option<PathBuf>,
    every: u64,
}

static CONFIG: LazyLock<Option<Config>> = LazyLock::new(|| {
    let value = std::env::var("UNDERBOX_FRAMES").ok().filter(|v| !v.is_empty())?;
    let every = std::env::var("UNDERBOX_FRAMES_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(1u64).max(1);
    Some(Config { dir: (value != "log").then(|| PathBuf::from(value)), every })
});

pub fn on_present(gpu: &render::Gpu, name: &str, n: u64, color: &render::Texture) {
    let Some(config) = CONFIG.as_ref() else { return };
    if n % config.every != 0 {
        return;
    }
    let Some(frame) = color.read_region(gpu, 0, [0, 0, color.desc.width, color.desc.height]) else { return };
    let hash = frame.hash();
    abi::rt::log::log(abi::rt::log::Level::Info, &format!("frame: {name} #{n} {}x{} {hash}", frame.width, frame.height));
    if let Some(dir) = &config.dir {
        let file = dir.join(format!("{}-{n:05}.ppm", name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| frame.write_ppm(&file)) {
            abi::rt::log::log(abi::rt::log::Level::Warn, &format!("frame dump {}: {e}", file.display()));
        }
    }
}
