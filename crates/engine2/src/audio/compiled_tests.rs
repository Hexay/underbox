//! `PrecacheSound` on a compiled `.vsnd_c` (no source file beside it), as `Sound.Play` reaches it.

use abi::slots::{c_sfx_table as sfx, g_p_sound_system_internal as system};
use assets::compiled::{fixture, sound};
use std::ffi::{CString, c_char, c_void};

type P = *mut c_void;

fn thunk<F: Copy>(index: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    // SAFETY: F is the thunk's own extern "C" signature, spelled out at each use.
    unsafe { std::mem::transmute_copy(&crate::igen::test_table()[index]) }
}

#[test]
fn compiled_sounds_precache_decode_and_loop() {
    let dir = std::env::temp_dir().join(format!("underbox_vsnd_{}", std::process::id()));
    let file = dir.join(format!("{}_c", fixture::SOUND));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, sound::write_v5(&fixture::sound())).unwrap();
    crate::fs::add_search_path("GAME", dir.to_str().unwrap(), true);

    let name = CString::new(fixture::SOUND).unwrap();
    let table = unsafe { thunk::<unsafe extern "C" fn(*const c_char) -> P>(system::PRECACHE_SOUND.index)(name.as_ptr()) };
    let sfx = super::sound::sfx(abi::NativeRef::from_raw(table)).unwrap();
    let Some(audio::sampler::Source::Pcm(pcm)) = sfx.source() else { panic!("decoded") };
    assert_eq!((pcm.rate, pcm.channel_count(), pcm.frames()), (22050, 2, 16));
    assert_eq!((pcm.loop_start, pcm.loop_end), (Some(4), Some(12)));
    assert!((pcm.channels[0][3] - 3.0 * 1024.0 / 32768.0).abs() < 1e-4, "{:?}", &pcm.channels[0][..4]);
    assert!(unsafe { thunk::<unsafe extern "C" fn(P) -> i32>(sfx::IS_VALID_FOR_PLAYBACK.index)(table) } != 0);
    let _ = std::fs::remove_dir_all(dir);
}
