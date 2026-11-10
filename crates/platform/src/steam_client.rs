//! The Steam client (the logged-in user) through steam_api's flat C API. Callbacks are pumped by
//! manual dispatch and handed over raw, as native engine2 does: managed's own Steamworks layer
//! decodes them and reads call results itself, so each one is delivered before it is freed.

use crate::{Apps, BeginAuth, Client, CloudStorage, Event, Friends, SteamId, TextFilter, Ticket};
use std::ffi::{CStr, CString, c_char};
use steamworks::sys;

pub struct SteamClient {
    pipe: sys::HSteamPipe,
}

impl SteamClient {
    /// Start the Steam client API; `None` (with the reason logged) without a running Steam or app id.
    pub fn start() -> Option<Self> {
        let mut err: sys::SteamErrMsg = [0; 1024];
        // SAFETY: InitFlat writes a NUL-terminated message into the buffer it's given.
        let ok = unsafe { sys::SteamAPI_InitFlat(&mut err) } == sys::ESteamAPIInitResult::k_ESteamAPIInitResult_OK;
        if !ok {
            // SAFETY: as above.
            let why = unsafe { CStr::from_ptr(err.as_ptr()) }.to_string_lossy();
            eprintln!("platform: Steam client init failed ({why}); using an offline identity");
            return None;
        }
        // SAFETY: Steam is initialised.
        unsafe { sys::SteamAPI_ManualDispatch_Init() };
        Some(Self { pipe: unsafe { sys::SteamAPI_GetHSteamPipe() } })
    }
}

impl Drop for SteamClient {
    fn drop(&mut self) {
        // SAFETY: started in `start`.
        unsafe { sys::SteamAPI_Shutdown() }
    }
}

fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// A C string Steam owns (valid until its next call on that interface), copied out.
fn owned(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

/// A string Steam writes into a caller buffer.
fn buffered(f: impl FnOnce(*mut c_char, usize)) -> String {
    let mut buf = vec![0 as c_char; 4096];
    f(buf.as_mut_ptr(), buf.len());
    owned(buf.as_ptr())
}

macro_rules! steam {
    ($iface:ident, $f:ident $(, $arg:expr)*) => {
        // SAFETY: the client is initialised for as long as `self` exists, and every argument is valid for the call.
        unsafe { sys::$f(sys::$iface() $(, $arg)*) }
    };
}

impl Client for SteamClient {
    fn user(&self) -> SteamId {
        SteamId(steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_GetSteamID))
    }

    fn logged_on(&self) -> bool {
        steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_BLoggedOn)
    }

    fn ticket(&self, target: SteamId) -> Ticket {
        let mut identity: sys::SteamNetworkingIdentity = unsafe { std::mem::zeroed() };
        let identity = if target.0 == 0 {
            std::ptr::null()
        } else {
            // SAFETY: a zeroed identity is Steam's "invalid" one, which SetSteamID64 overwrites.
            unsafe { sys::SteamAPI_SteamNetworkingIdentity_SetSteamID64(&mut identity, target.0) };
            &identity as *const _
        };
        let mut bytes = vec![0u8; 1024];
        let mut len = 0u32;
        let handle = steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_GetAuthSessionTicket, bytes.as_mut_ptr().cast(), 1024, &mut len, identity);
        bytes.truncate(len as usize);
        Ticket { handle, bytes }
    }

    fn cancel_ticket(&self, handle: u32) {
        steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_CancelAuthTicket, handle)
    }

    fn begin_auth(&self, user: SteamId, ticket: &[u8]) -> BeginAuth {
        use sys::EBeginAuthSessionResult as R;
        match steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_BeginAuthSession, ticket.as_ptr().cast(), ticket.len() as i32, user.0) {
            R::k_EBeginAuthSessionResultOK => BeginAuth::Ok,
            R::k_EBeginAuthSessionResultDuplicateRequest => BeginAuth::DuplicateRequest,
            R::k_EBeginAuthSessionResultInvalidVersion => BeginAuth::InvalidVersion,
            R::k_EBeginAuthSessionResultGameMismatch => BeginAuth::GameMismatch,
            R::k_EBeginAuthSessionResultExpiredTicket => BeginAuth::ExpiredTicket,
            _ => BeginAuth::InvalidTicket,
        }
    }

    fn end_auth(&self, user: SteamId) {
        steam!(SteamAPI_SteamUser_v023, SteamAPI_ISteamUser_EndAuthSession, user.0)
    }

    fn friends(&self) -> Option<&dyn Friends> {
        Some(self)
    }

    fn apps(&self) -> Option<&dyn Apps> {
        Some(self)
    }

    fn storage(&self) -> Option<&dyn CloudStorage> {
        Some(self)
    }

    fn text_filter(&self) -> Option<&dyn TextFilter> {
        Some(self)
    }

    fn run_callbacks(&self, _event: &mut dyn FnMut(Event), raw: &mut dyn FnMut(i32, &[u8])) {
        // SAFETY: manual dispatch on our pipe; each message's buffer lives until FreeLastCallback.
        unsafe {
            sys::SteamAPI_ManualDispatch_RunFrame(self.pipe);
            let mut msg: sys::CallbackMsg_t = std::mem::zeroed();
            while sys::SteamAPI_ManualDispatch_GetNextCallback(self.pipe, &mut msg) {
                let len = msg.m_cubParam.max(0) as usize;
                let data = if msg.m_pubParam.is_null() { &[][..] } else { std::slice::from_raw_parts(msg.m_pubParam, len) };
                raw(msg.m_iCallback, data);
                sys::SteamAPI_ManualDispatch_FreeLastCallback(self.pipe);
            }
        }
    }
}

impl Friends for SteamClient {
    fn persona_name(&self) -> String {
        owned(steam!(SteamAPI_SteamFriends_v018, SteamAPI_ISteamFriends_GetPersonaName))
    }

    fn set_rich_presence(&self, key: &str, value: Option<&str>) -> bool {
        let (key, value) = (c(key), value.map(c));
        let value = value.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
        steam!(SteamAPI_SteamFriends_v018, SteamAPI_ISteamFriends_SetRichPresence, key.as_ptr(), value)
    }

    fn clear_rich_presence(&self) {
        steam!(SteamAPI_SteamFriends_v018, SteamAPI_ISteamFriends_ClearRichPresence)
    }

    fn request_profile_items(&self, user: SteamId) -> u64 {
        steam!(SteamAPI_SteamFriends_v018, SteamAPI_ISteamFriends_RequestEquippedProfileItems, user.0)
    }

    fn profile_item_property(&self, user: SteamId, kind: u32, property: u32) -> String {
        if kind > 4 || property > 11 {
            return String::new();
        }
        // SAFETY: both enums are repr(u32) and the values are in their ranges.
        let (kind, property) = unsafe {
            (std::mem::transmute::<u32, sys::ECommunityProfileItemType>(kind), std::mem::transmute::<u32, sys::ECommunityProfileItemProperty>(property))
        };
        owned(steam!(SteamAPI_SteamFriends_v018, SteamAPI_ISteamFriends_GetProfileItemPropertyString, user.0, kind, property))
    }
}

impl Apps for SteamClient {
    fn is_subscribed(&self) -> bool {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_BIsSubscribed)
    }
    fn is_subscribed_app(&self, app: u32) -> bool {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_BIsSubscribedApp, app)
    }
    fn is_app_installed(&self, app: u32) -> bool {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_BIsAppInstalled, app)
    }
    fn is_dlc_installed(&self, app: u32) -> bool {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_BIsDlcInstalled, app)
    }
    fn is_low_violence(&self) -> bool {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_BIsLowViolence)
    }
    fn is_cybercafe(&self) -> bool {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_BIsCybercafe)
    }
    fn is_vac_banned(&self) -> bool {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_BIsVACBanned)
    }
    fn build_id(&self) -> i32 {
        steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_GetAppBuildId)
    }
    fn available_languages(&self) -> String {
        owned(steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_GetAvailableGameLanguages))
    }
    fn current_language(&self) -> String {
        owned(steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_GetCurrentGameLanguage))
    }
    fn launch_command_line(&self) -> String {
        buffered(|p, n| {
            steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_GetLaunchCommandLine, p, n as i32);
        })
    }
    fn install_dir(&self, app: u32) -> String {
        buffered(|p, n| {
            steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_GetAppInstallDir, app, p, n as u32);
        })
    }
}

impl CloudStorage for SteamClient {
    fn exists(&self, name: &str) -> bool {
        steam!(SteamAPI_SteamRemoteStorage_v016, SteamAPI_ISteamRemoteStorage_FileExists, c(name).as_ptr())
    }
    fn size(&self, name: &str) -> Option<usize> {
        self.exists(name).then(|| steam!(SteamAPI_SteamRemoteStorage_v016, SteamAPI_ISteamRemoteStorage_GetFileSize, c(name).as_ptr()).max(0) as usize)
    }
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        let mut data = vec![0u8; self.size(name)?];
        let n = steam!(SteamAPI_SteamRemoteStorage_v016, SteamAPI_ISteamRemoteStorage_FileRead, c(name).as_ptr(), data.as_mut_ptr().cast(), data.len() as i32);
        data.truncate(n.max(0) as usize);
        Some(data)
    }
    fn write(&self, name: &str, data: &[u8]) -> bool {
        steam!(SteamAPI_SteamRemoteStorage_v016, SteamAPI_ISteamRemoteStorage_FileWrite, c(name).as_ptr(), data.as_ptr().cast(), data.len() as i32)
    }
    fn write_async(&self, name: &str, data: &[u8]) -> u64 {
        steam!(SteamAPI_SteamRemoteStorage_v016, SteamAPI_ISteamRemoteStorage_FileWriteAsync, c(name).as_ptr(), data.as_ptr().cast(), data.len() as u32)
    }
    fn delete(&self, name: &str) -> bool {
        steam!(SteamAPI_SteamRemoteStorage_v016, SteamAPI_ISteamRemoteStorage_FileDelete, c(name).as_ptr())
    }
}

impl TextFilter for SteamClient {
    fn init(&self, options: u32) -> bool {
        steam!(SteamAPI_SteamUtils_v010, SteamAPI_ISteamUtils_InitFilterText, options)
    }

    fn filter(&self, context: u32, source: SteamId, input: &str) -> (String, i32) {
        use sys::ETextFilteringContext as T;
        let context = match context {
            1 => T::k_ETextFilteringContextGameContent,
            2 => T::k_ETextFilteringContextChat,
            3 => T::k_ETextFilteringContextName,
            _ => T::k_ETextFilteringContextUnknown,
        };
        let input = c(input);
        let mut out = vec![0 as c_char; input.as_bytes().len() + 1];
        let n = steam!(SteamAPI_SteamUtils_v010, SteamAPI_ISteamUtils_FilterText, context, source.0, input.as_ptr(), out.as_mut_ptr(), out.len() as u32);
        (owned(out.as_ptr()), n)
    }
}
