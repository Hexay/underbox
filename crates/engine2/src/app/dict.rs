//! `CMaterialSystem2AppSystemDict`: the app framework's startup configuration, which the managed
//! `AppSystem` creates first and fills in before the engine's init.

use abi::slots::c_material_system2_app_system_dict as dict;
use abi::structs::MaterialSystem2AppSystemDictCreateInfo;
use abi::{NativeRef, RawPtr, Registry, StrRet, bind, class};
use std::ffi::{CStr, c_void};
use std::sync::{Arc, Mutex, RwLock};

pub type Ref = NativeRef<class::CMaterialSystem2AppSystemDict>;

#[derive(Debug, Default, Clone)]
pub struct Config {
    pub flags: i32,
    pub steam_app_id: u32,
    pub dedicated_server: bool,
    pub tools_mode: bool,
    pub test_mode: bool,
    pub standalone: bool,
    pub suppress_startup_manifest: bool,
    pub initialization_phase: i32,
    pub module_search_path: String,
    pub mod_game_subdir: String,
    pub mod_file_name: String,
    pub default_render_system: String,
}

#[derive(Default)]
pub struct AppSystemDict {
    config: Mutex<Config>,
}

// The app the process runs: global queries such as AppIsDedicatedServer read it.
static CURRENT: RwLock<Option<Arc<AppSystemDict>>> = RwLock::new(None);

/// The running app's configuration (defaults before the managed AppSystem made one).
pub fn current_config() -> Config {
    CURRENT.read().unwrap().as_ref().map(|a| a.config.lock().unwrap().clone()).unwrap_or_default()
}

fn create(info: MaterialSystem2AppSystemDictCreateInfo) -> Ref {
    let app = Arc::new(AppSystemDict::default());
    {
        let mut c = app.config.lock().unwrap();
        c.flags = info.i_flags;
        c.steam_app_id = info.n_steam_app_id;
    }
    *CURRENT.write().unwrap() = Some(app.clone());
    objects::create_shared(app)
}

fn destroy(r: Ref) {
    if let Some(app) = objects::get::<_, AppSystemDict>(r) {
        let mut current = CURRENT.write().unwrap();
        if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &app)) {
            *current = None;
        }
    }
    objects::destroy(r);
}

fn with<R: Default>(r: Ref, f: impl FnOnce(&mut Config) -> R) -> R {
    objects::get::<_, AppSystemDict>(r).map(|a| f(&mut a.config.lock().unwrap())).unwrap_or_default()
}

fn string(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, dict::CREATE, create);
    bind!(r, dict::DESTROY, destroy);

    // Engine subsystems initialise on first use, so the app framework's phases have nothing to wait for.
    bind!(r, dict::INIT, |_| true);
    bind!(r, dict::INIT_WITHOUT_MATERIAL_SYSTEM, |_| true);
    bind!(r, dict::INIT_FINISH_SETUP_MATERIAL_SYSTEM, |_| true);
    bind!(r, dict::START_BACKGROUND_SYSTEMS, |_| {});
    bind!(r, dict::PRE_SHUTDOWN, |_| {});
    bind!(r, dict::SUPPRESS_COM_INITIALIZATION, |_| {});
    bind!(r, dict::DISABLE_MOD_PATH_CHECK, |_| {});
    // SDL starts per subsystem (app/sdl/system.rs) and with the first window (window crate).
    bind!(r, dict::INIT_SDL, |_, _flags| true);
    bind!(r, dict::SHUTDOWN_SDL, |_| {});
    bind!(r, dict::ADD_SYSTEM, add_system);

    bind!(r, dict::SET_DEDICATED_SERVER, |a, v| with(a, |c| c.dedicated_server = v));
    bind!(r, dict::IS_DEDICATED_SERVER, |a| with(a, |c| c.dedicated_server));
    bind!(r, dict::SET_IN_TOOLS_MODE, |a| with(a, |c| c.tools_mode = true));
    bind!(r, dict::IS_IN_TOOLS_MODE, |a| with(a, |c| c.tools_mode));
    bind!(r, dict::SET_IN_TEST_MODE, |a| with(a, |c| c.test_mode = true));
    bind!(r, dict::IS_IN_TEST_MODE, |a| with(a, |c| c.test_mode));
    bind!(r, dict::SET_IN_STANDALONE_APP, |a| with(a, |c| c.standalone = true));
    bind!(r, dict::IS_STANDALONE_APP, |a| with(a, |c| c.standalone));
    bind!(r, dict::SUPPRESS_STARTUP_MANIFEST_LOAD, |a, v| with(a, |c| c.suppress_startup_manifest = v));
    bind!(r, dict::IS_IN_DEVELOPER_MODE, |_| false);
    bind!(r, dict::IS_IN_VR_MODE, |_| false);
    bind!(r, dict::IS_RUNNING_ON_CUSTOMER_MACHINE, |_| false);
    bind!(r, dict::IS_CONSOLE_APP, |a| with(a, |c| c.flags & APP_FLAG_CONSOLE != 0));
    bind!(r, dict::IS_GAME_APP, |a| with(a, |c| c.flags & APP_FLAG_GAME != 0));

    bind!(r, dict::SET_INITIALIZATION_PHASE, |a, p| with(a, |c| c.initialization_phase = p));
    bind!(r, dict::GET_INITIALIZATION_PHASE, |a| with(a, |c| c.initialization_phase));
    bind!(r, dict::SET_STEAM_APP_ID, |a, id| with(a, |c| c.steam_app_id = id));
    bind!(r, dict::GET_STEAM_APP_ID, |a| with(a, |c| c.steam_app_id));

    bind!(r, dict::SET_MODULE_SEARCH_PATH, |a, s| with(a, |c| c.module_search_path = string(s)));
    bind!(r, dict::SET_MOD_GAME_SUBDIR, |a, s| with(a, |c| c.mod_game_subdir = string(s)));
    bind!(r, dict::SET_MOD_FROM_FILE_NAME, |a, s, _no_exe_check| with(a, |c| c.mod_file_name = string(s)));
    bind!(r, dict::SET_DEFAULT_RENDER_SYSTEM_OPTION, |a, s| with(a, |c| c.default_render_system = string(s)));
    bind!(r, dict::GET_MOD_GAME_SUBDIR, |a| StrRet::from(with(a, |c| c.mod_game_subdir.clone())));
    bind!(r, dict::GET_CONTENT_PATH, |_| StrRet::from(crate::app::platform::game_root()));
}

// `iFlags` is managed's AppSystemFlags cast as is (Sandbox.AppSystem/AppSystemFlags.cs).
const APP_FLAG_CONSOLE: i32 = 1 << 0;
const APP_FLAG_GAME: i32 = 1 << 1;

/// Source loads a module by name and registers its interface. Ours are built in, so there's nothing to load.
fn add_system(_: Ref, dll: Option<&CStr>, interface: Option<&CStr>) -> RawPtr<c_void> {
    abi::rt::log::warn(&format!("AddSystem({}, {}): modules are built in; nothing added", string(dll), string(interface)));
    RawPtr::NULL
}
