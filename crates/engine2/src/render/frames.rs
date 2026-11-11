//! Frame capture for headless runs: with `UNDERBOX_FRAMES` set, presented frames are read back,
//! their hash logged (`frame: <swap chain> #<n> <w>x<h> <blake3>`) and, when the value is a
//! directory rather than `log`, written there as PPM. `UNDERBOX_FRAMES_EVERY=n` keeps every n-th;
//! `UNDERBOX_FRAMES_LAST=1` keeps only the latest (`<swap chain>.ppm`).

use std::path::PathBuf;
use std::sync::LazyLock;

struct Config {
    dir: Option<PathBuf>,
    every: u64,
    /// One file per swap chain, overwritten each frame (`UNDERBOX_FRAMES_LAST=1`).
    last_only: bool,
}

static CONFIG: LazyLock<Option<Config>> = LazyLock::new(|| {
    let value = std::env::var("UNDERBOX_FRAMES").ok().filter(|v| !v.is_empty())?;
    let every = std::env::var("UNDERBOX_FRAMES_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(1u64).max(1);
    let last_only = std::env::var("UNDERBOX_FRAMES_LAST").is_ok_and(|v| v == "1");
    Some(Config { dir: (value != "log").then(|| PathBuf::from(value)), every, last_only })
});

pub fn on_present(gpu: &render::Gpu, name: &str, n: u64, color: &render::Texture) {
    let Some(config) = CONFIG.as_ref() else { return };
    if !n.is_multiple_of(config.every) {
        return;
    }
    let Some(mut frame) = color.read_region(gpu, 0, [0, 0, color.desc.width, color.desc.height]) else { return };
    // A window's surface is often BGRA; dumps and hashes are RGBA.
    if matches!(color.desc.format, render::wgpu::TextureFormat::Bgra8Unorm | render::wgpu::TextureFormat::Bgra8UnormSrgb) {
        frame.pixels.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
    }
    let hash = frame.hash();
    abi::rt::log::log(abi::rt::log::Level::Info, &format!("frame: {name} #{n} {}x{} {hash}", frame.width, frame.height));
    if let Some(dir) = &config.dir {
        let name = name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let file = if config.last_only { dir.join(format!("{name}.ppm")) } else { dir.join(format!("{name}-{n:05}.ppm")) };
        // Written aside and renamed: a boot killed mid-write must not leave a truncated frame.
        let partial = file.with_extension("ppm.partial");
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| frame.write_ppm(&partial)).and_then(|_| std::fs::rename(&partial, &file)) {
            abi::rt::log::log(abi::rt::log::Level::Warn, &format!("frame dump {}: {e}", file.display()));
        }
    }
}
