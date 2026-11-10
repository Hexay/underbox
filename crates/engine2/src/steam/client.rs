//! The local user's interfaces (`ISteamUser`, `ISteamFriends`, `ISteamApps`, `ISteamRemoteStorage`,
//! `ISteamUtils`) over [`platform::Client`]. A dedicated server gets null for all of them, as does a
//! client whose platform lacks the service; managed checks `IsValid` and skips the feature.

use super::interface;
use abi::slots::{
    global_steam as global, i_steam_apps as apps, i_steam_friends as friends, i_steam_remote_storage as rs, i_steam_user as user,
    i_steam_utils as utils,
};
use abi::structs::HAuthTicket;
use abi::{NativeClass, NativeRef, RawPtr, Registry, StrRet, bind, class};
use platform::{Apps, Client, CloudStorage, Friends, SteamId, TextFilter};
use std::ffi::{CStr, c_void};
use std::sync::OnceLock;

/// Managed's ticket buffer (`Auth.GetAuthTicket` stackallocs 1024 bytes).
const TICKET_CAPACITY: usize = 1024;

/// The local user's services; `None` on a dedicated server, which has no user.
pub(super) fn local() -> Option<&'static dyn Client> {
    (!crate::app::dict::current_config().dedicated_server).then(|| platform::get().client())
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn friend<R: Default>(f: impl FnOnce(&dyn Friends) -> R) -> R {
    local().and_then(|c| c.friends()).map(f).unwrap_or_default()
}

fn app<R: Default>(f: impl FnOnce(&dyn Apps) -> R) -> R {
    local().and_then(|c| c.apps()).map(f).unwrap_or_default()
}

fn storage<R: Default>(f: impl FnOnce(&dyn CloudStorage) -> R) -> R {
    local().and_then(|c| c.storage()).map(f).unwrap_or_default()
}

fn filter<R: Default>(f: impl FnOnce(&dyn TextFilter) -> R) -> R {
    local().and_then(|c| c.text_filter()).map(f).unwrap_or_default()
}

fn user<R: Default>(f: impl FnOnce(&dyn Client) -> R) -> R {
    local().map(f).unwrap_or_default()
}

/// The interface singleton when `available`, else null.
fn offered<C: NativeClass>(cell: &'static OnceLock<usize>, available: bool) -> NativeRef<C> {
    if available { interface(cell) } else { NativeRef::null() }
}

fn ticket(target: u64, buffer: RawPtr<c_void>, len: &mut u32) -> HAuthTicket {
    let t = local().map(|c| c.ticket(SteamId(target))).unwrap_or_default();
    let n = t.bytes.len().min(TICKET_CAPACITY);
    // SAFETY: managed passes a TICKET_CAPACITY-byte buffer.
    unsafe { buffer.cast::<u8>().as_mut_slice(n) }.copy_from_slice(&t.bytes[..n]);
    *len = n as u32;
    HAuthTicket { value: t.handle }
}

fn begin_auth(sender: u64, ticket: RawPtr<c_void>, len: i32) -> i64 {
    // SAFETY: managed passes the ticket buffer and its length.
    let ticket = unsafe { ticket.cast::<u8>().as_slice(len.max(0) as usize) };
    local().map_or(platform::BeginAuth::InvalidTicket, |c| c.begin_auth(SteamId(sender), ticket)) as i64
}

fn read_file(name: Option<&CStr>, out: RawPtr<c_void>, cap: i32) -> i32 {
    let Some(data) = storage(|s| s.read(&text(name))) else { return 0 };
    let n = data.len().min(cap.max(0) as usize);
    // SAFETY: managed passes a buffer of `cap` bytes.
    unsafe { out.cast::<u8>().as_mut_slice(n) }.copy_from_slice(&data[..n]);
    n as i32
}

fn bytes<'a>(data: RawPtr<c_void>, len: usize) -> &'a [u8] {
    // SAFETY: managed passes its buffer and that buffer's length.
    unsafe { data.cast::<u8>().as_slice(len) }
}

fn filter_text(context: i64, source: u64, input: Option<&CStr>, out: RawPtr<c_void>, cap: u32) -> i32 {
    let input = text(input);
    let (filtered, n) = filter(|f| Some(f.filter(context as u32, SteamId(source), &input))).unwrap_or((input, 0));
    // NUL-terminated, cut to fit.
    let len = filtered.len().min((cap as usize).saturating_sub(1));
    // SAFETY: managed passes a buffer of `cap` bytes.
    let dst = unsafe { out.cast::<u8>().as_mut_slice(cap as usize) };
    if !dst.is_empty() {
        dst[..len].copy_from_slice(&filtered.as_bytes()[..len]);
        dst[len] = 0;
    }
    n
}

pub fn register(r: &mut Registry) {
    static USER: OnceLock<usize> = OnceLock::new();
    static FRIENDS: OnceLock<usize> = OnceLock::new();
    static APPS: OnceLock<usize> = OnceLock::new();
    static STORAGE: OnceLock<usize> = OnceLock::new();
    static UTILS: OnceLock<usize> = OnceLock::new();
    bind!(r, global::STEAM_USER, || -> NativeRef<class::ISteamUser> { offered(&USER, local().is_some()) });
    // Never null: the game instance sets rich presence every frame unchecked (SteamRichPresenceSystem).
    bind!(r, global::STEAM_FRIENDS, || -> NativeRef<class::ISteamFriends> { interface(&FRIENDS) });
    bind!(r, global::STEAM_APPS, || -> NativeRef<class::ISteamApps> { offered(&APPS, app(|_| true)) });
    bind!(r, global::STEAM_REMOTE_STORAGE, || -> NativeRef<class::ISteamRemoteStorage> { offered(&STORAGE, storage(|_| true)) });
    bind!(r, global::STEAM_UTILS, || -> NativeRef<class::ISteamUtils> { offered(&UTILS, filter(|_| true)) });

    bind!(r, user::B_LOGGED_ON, |_| user(|c| c.logged_on()));
    bind!(r, user::GET_STEAM_ID, |_| user(|c| c.user().0));
    bind!(r, user::GET_AUTH_SESSION_TICKET, |_, target, buffer, len| ticket(target, buffer, len));
    bind!(r, user::BEGIN_AUTH_SESSION, |_, sender, t, len| begin_auth(sender, t, len));
    bind!(r, user::CANCEL_AUTH_TICKET, |_, h| user(|c| c.cancel_ticket(h.value)));
    bind!(r, user::END_AUTH_SESSION, |_, id| user(|c| c.end_auth(SteamId(id))));

    bind!(r, friends::GET_PERSONA_NAME, |_| StrRet::from(friend(|f| Some(f.persona_name())).unwrap_or_else(|| "Player".into())));
    bind!(r, friends::SET_RICH_PRESENCE, |_, k, v| friend(|f| f.set_rich_presence(&text(k), v.map(|v| v.to_string_lossy()).as_deref())));
    bind!(r, friends::CLEAR_RICH_PRESENCE, |_| friend(|f| f.clear_rich_presence()));
    bind!(r, friends::REQUEST_EQUIPPED_PROFILE_ITEMS, |_, id| friend(|f| f.request_profile_items(SteamId(id))));
    bind!(r, friends::GET_PROFILE_ITEM_PROPERTY_STRING, |_, id, kind, prop| {
        StrRet::from(friend(|f| Some(f.profile_item_property(SteamId(id), kind as u32, prop as u32))))
    });

    bind!(r, apps::B_IS_SUBSCRIBED, |_| app(|a| a.is_subscribed()));
    bind!(r, apps::B_IS_SUBSCRIBED_APP, |_, id| app(|a| a.is_subscribed_app(id as u32)));
    bind!(r, apps::B_IS_APP_INSTALLED, |_, id| app(|a| a.is_app_installed(id as u32)));
    bind!(r, apps::B_IS_DLC_INSTALLED, |_, id| app(|a| a.is_dlc_installed(id as u32)));
    bind!(r, apps::B_IS_LOW_VIOLENCE, |_| app(|a| a.is_low_violence()));
    bind!(r, apps::B_IS_CYBERCAFE, |_| app(|a| a.is_cybercafe()));
    bind!(r, apps::B_IS_VAC_BANNED, |_| app(|a| a.is_vac_banned()));
    bind!(r, apps::GET_APP_BUILD_ID, |_| app(|a| a.build_id()));
    bind!(r, apps::GET_AVAILABLE_GAME_LANGUAGES, |_| StrRet::from(app(|a| Some(a.available_languages()))));
    bind!(r, apps::GET_CURRENT_GAME_LANGUAGE, |_| StrRet::from(app(|a| Some(a.current_language()))));
    bind!(r, apps::GET_COMMAND_LINE, |_| StrRet::from(app(|a| Some(a.launch_command_line()))));
    bind!(r, apps::GET_APP_INSTALL_DIR, |_, id| StrRet::from(app(|a| Some(a.install_dir(id as u32)))));

    bind!(r, rs::FILE_EXISTS, |_, name| storage(|s| s.exists(&text(name))));
    bind!(r, rs::GET_FILE_SIZE, |_, name| storage(|s| s.size(&text(name)).map_or(0, |n| n as i32)));
    bind!(r, rs::FILE_READ, |_, name, out, cap| read_file(name, out, cap));
    bind!(r, rs::FILE_WRITE, |_, name, data, len| storage(|s| s.write(&text(name), bytes(data, len.max(0) as usize))));
    bind!(r, rs::FILE_WRITE_ASYNC, |_, name, data, len| storage(|s| s.write_async(&text(name), bytes(data, len as usize))));
    bind!(r, rs::FILE_DELETE, |_, name| storage(|s| s.delete(&text(name))));

    bind!(r, utils::INIT_FILTER_TEXT, |_, options| filter(|f| f.init(options)));
    bind!(r, utils::FILTER_TEXT, |_, context, source, input, out, cap| filter_text(context, source, input, out, cap));
}
