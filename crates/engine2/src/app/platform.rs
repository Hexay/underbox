//! The process (Source's `Plat_*`) and the engine's lifecycle (`SourceEngine*`), from `EngineGlobal`.

use crate::app::dict;
use abi::slots::global;
use abi::{Registry, StrRet, bind};
use std::ffi::CStr;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

static GAME_ROOT: RwLock<String> = RwLock::new(String::new());
static MODULE_FILENAME: RwLock<String> = RwLock::new(String::new());
static COMMAND_LINE: RwLock<String> = RwLock::new(String::new());
static FRAME: AtomicU64 = AtomicU64::new(0);

/// The game folder (s&box's `game/`), as managed set it at startup; `.` before that.
pub fn game_root() -> String {
    let root = GAME_ROOT.read().unwrap();
    if root.is_empty() { ".".into() } else { root.clone() }
}

/// The command line managed handed `SourceEnginePreInit`.
pub fn command_line() -> String {
    COMMAND_LINE.read().unwrap().clone()
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, global::PLAT_SET_MODULE_FILENAME, |s| *MODULE_FILENAME.write().unwrap() = string(s));
    bind!(r, global::PLAT_SET_CURRENT_DIRECTORY, |s| *GAME_ROOT.write().unwrap() = string(s));
    bind!(r, global::GET_GAME_ROOT_FOLDER, || StrRet::from(game_root()));
    bind!(r, global::GET_GAME_SEARCH_PATH, || StrRet::from(game_root()));

    bind!(r, global::PLAT_GET_CURRENT_FRAME, || FRAME.load(Ordering::Relaxed));
    bind!(r, global::PLAT_SET_CURRENT_FRAME, |f| FRAME.store(f, Ordering::Relaxed));
    bind!(r, global::PLAT_CHANGE_CURRENT_FRAME, |d| {
        FRAME.fetch_add(d as u64, Ordering::Relaxed);
    });

    // A build of the public source is a development build, never Facepunch's retail one.
    bind!(r, global::IS_RETAIL, || false);
    bind!(r, global::PLAT_IS_RUNNING_ON_CUSTOMER_MACHINE, || false);
    bind!(r, global::PLAT_SET_NO_ASSERT, || {});
    bind!(r, global::HAS_LAUNCH_PARAMETER, |p| has_launch_parameter(&string(p)));
    bind!(r, global::PLAT_MESSAGE_BOX, |title, text| {
        abi::rt::log::warn(&format!("message box: {}: {}", string(title), string(text)))
    });
    bind!(r, global::PLAT_SAFE_REMOVE_FILE, |p| std::fs::remove_file(string(p)).is_ok());
    bind!(r, global::PLAT_EXIT_PROCESS, |code| std::process::exit(code));
    bind!(r, global::TOOLS_STALL_MONITOR_INDICATE_ACTIVITY, || {});

    bind!(r, global::APP_IS_DEDICATED_SERVER, || dict::current_config().dedicated_server);
    bind!(r, global::SOURCE_ENGINE_UNIT_TEST_INIT, || true);
    // Managed asks only with a game window, then creates the window itself through globalSdl; we
    // only need video up (SDL's offscreen driver when headless, which renders offscreen).
    bind!(r, global::SOURCE_ENGINE_INIT_STARTUP_WINDOW, |_| window::video::ensure());
    bind!(r, global::SOURCE_ENGINE_PRE_INIT, |cmd, _app| {
        *COMMAND_LINE.write().unwrap() = string(cmd);
        crate::fs::compile_core();
        // Managed's SteamClient.Init reads the flat interfaces right after this returns.
        let config = dict::current_config();
        if !config.dedicated_server {
            platform::get().start_client(config.steam_app_id);
        }
        true
    });
    bind!(r, global::SOURCE_ENGINE_INIT, |_| {
        crate::render::device::report_to_managed();
        true
    });
    // False asks managed's EngineLoop to quit. A client renders and presents its frame here.
    bind!(r, global::SOURCE_ENGINE_FRAME, |_, _time, _previous| {
        crate::render::game_window::frame();
        !crate::engine_glue::exit_requested()
    });
    bind!(r, global::SOURCE_ENGINE_SHUTDOWN, |_, _| SHUTTING_DOWN.store(true, std::sync::atomic::Ordering::Relaxed));
}

static SHUTTING_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether managed has begun shutting the engine down (SourceEngineShutdown).
pub fn shutting_down() -> bool {
    SHUTTING_DOWN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Source's launch parameters are the process arguments (`-dev`, `+game x`), matched case-blind.
fn has_launch_parameter(name: &str) -> bool {
    std::env::args().any(|a| a.eq_ignore_ascii_case(name))
}
