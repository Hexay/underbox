//! The logged-in user's social, app, cloud and text-filter services over steam_api's flat C API.

use super::strings::{buffered, c, owned};
use crate::steam_client::SteamClient;
use crate::{Apps, CloudStorage, Friends, SteamId, TextFilter};
use std::ffi::c_char;
use steamworks::sys;

macro_rules! steam {
    ($iface:ident, $f:ident $(, $arg:expr)*) => {
        // SAFETY: the client is initialised for as long as a SteamClient exists, and every argument is valid for the call.
        unsafe { sys::$f(sys::$iface() $(, $arg)*) }
    };
}
pub(crate) use steam;

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
        buffered(4096, |p, n| {
            steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_GetLaunchCommandLine, p, n as i32);
        })
    }
    fn install_dir(&self, app: u32) -> String {
        buffered(4096, |p, n| {
            steam!(SteamAPI_SteamApps_v009, SteamAPI_ISteamApps_GetAppInstallDir, app, p, n);
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
