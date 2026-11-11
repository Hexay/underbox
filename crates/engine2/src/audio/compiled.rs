//! Compiled `.vsnd_c` sounds (see `assets::compiled::sound`) decoded to `Pcm`, keeping the
//! resource's loop points.

use audio::pcm::Pcm;

/// The compiled sound for resource `stem` (`sounds/x`), if the game folders have one.
pub fn path(stem: &str) -> Option<String> {
    let path = format!("{stem}.vsnd_c");
    crate::fs::resolve(&path, "GAME").map(|_| path)
}

pub fn decode(path: &str) -> Option<Pcm> {
    let warn = |e: &dyn std::fmt::Display| abi::rt::log::warn(&format!("sound {path}: {e}"));
    let file = crate::fs::read(path, "GAME")?;
    let sound = assets::compiled::sound::read(&file).inspect_err(|e| warn(e)).ok()?;
    let (bytes, ext) = sound.playable();
    let mut pcm = audio::decode::decode(&bytes, Some(ext)).inspect_err(|e| warn(e)).ok()?;
    pcm.loop_start = usize::try_from(sound.loop_start).ok();
    pcm.loop_end = usize::try_from(sound.loop_end).ok().filter(|&e| e > 0);
    Some(pcm)
}
