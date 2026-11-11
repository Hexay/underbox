//! Flat steam_api functions redeclared with plain integers where steamworks-sys has Rust enums.
//! Gotcha: managed hands us raw enum values, and Steam writes them back; a value outside a Rust
//! enum's variants is undefined behaviour, so these cross as `i32` (a C enum's ABI).

use std::ffi::c_char;
use steamworks::sys::{
    ISteamNetworkingMessages, ISteamNetworkingUtils, ISteamUGC, SteamNetConnectionInfo_t, SteamNetConnectionRealTimeStatus_t,
    SteamNetworkingIdentity,
};

unsafe extern "C" {
    #[link_name = "SteamAPI_ISteamUGC_CreateQueryAllUGCRequestCursor"]
    pub fn ugc_query_all(ugc: *mut ISteamUGC, kind: i32, matching: i32, creator: u32, consumer: u32, cursor: *const c_char) -> u64;
    #[link_name = "SteamAPI_ISteamUGC_CreateQueryUserUGCRequest"]
    pub fn ugc_query_user(ugc: *mut ISteamUGC, account: u32, list: i32, matching: i32, sort: i32, creator: u32, consumer: u32, page: u32) -> u64;
    #[link_name = "SteamAPI_ISteamUGC_GetQueryUGCStatistic"]
    pub fn ugc_statistic(ugc: *mut ISteamUGC, query: u64, index: u32, stat: i32, value: *mut u64) -> bool;
    #[link_name = "SteamAPI_ISteamUGC_GetQueryUGCAdditionalPreview"]
    pub fn ugc_additional_preview(
        ugc: *mut ISteamUGC,
        query: u64,
        index: u32,
        preview: u32,
        url: *mut c_char,
        url_len: u32,
        name: *mut c_char,
        name_len: u32,
        kind: *mut i32,
    ) -> bool;
    #[link_name = "SteamAPI_ISteamUGC_CreateItem"]
    pub fn ugc_create_item(ugc: *mut ISteamUGC, app: u32, kind: i32) -> u64;
    #[link_name = "SteamAPI_ISteamUGC_SetItemVisibility"]
    pub fn ugc_set_visibility(ugc: *mut ISteamUGC, update: u64, visibility: i32) -> bool;
    #[link_name = "SteamAPI_ISteamUGC_AddItemPreviewFile"]
    pub fn ugc_add_preview_file(ugc: *mut ISteamUGC, update: u64, file: *const c_char, kind: i32) -> bool;
    #[link_name = "SteamAPI_ISteamUGC_GetItemUpdateProgress"]
    pub fn ugc_update_progress(ugc: *mut ISteamUGC, update: u64, processed: *mut u64, total: *mut u64) -> i32;

    #[link_name = "SteamAPI_ISteamNetworkingUtils_SetGlobalConfigValueInt32"]
    pub fn net_set_int(utils: *mut ISteamNetworkingUtils, key: i32, value: i32) -> bool;
    #[link_name = "SteamAPI_ISteamNetworkingUtils_SetGlobalConfigValueString"]
    pub fn net_set_str(utils: *mut ISteamNetworkingUtils, key: i32, value: *const c_char) -> bool;

    #[link_name = "SteamAPI_ISteamNetworkingMessages_GetSessionConnectionInfo"]
    pub fn peer_session_info(
        msgs: *mut ISteamNetworkingMessages,
        remote: *const SteamNetworkingIdentity,
        info: *mut SteamNetConnectionInfo_t,
        status: *mut SteamNetConnectionRealTimeStatus_t,
    ) -> i32;
    #[link_name = "SteamAPI_ISteamNetworkingMessages_SendMessageToUser"]
    pub fn peer_send(msgs: *mut ISteamNetworkingMessages, remote: *const SteamNetworkingIdentity, data: *const u8, len: u32, flags: i32, channel: i32)
    -> i32;
}

/// A Steam-written struct as bytes, for reading with [`super::pump::field`] (no enum is ever materialised).
pub fn zeroed_bytes<T>() -> Vec<u8> {
    // Over-allocate by the alignment so the struct pointer can be aligned inside.
    vec![0u8; size_of::<T>() + align_of::<T>()]
}

/// The aligned start of a [`zeroed_bytes`] buffer, as `T`'s pointer, and the bytes from there.
pub fn aligned<T>(buf: &mut [u8]) -> (*mut T, std::ops::Range<usize>) {
    let start = buf.as_ptr().align_offset(align_of::<T>());
    // SAFETY: zeroed_bytes left room for the alignment offset.
    (unsafe { buf.as_mut_ptr().add(start) }.cast(), start..start + size_of::<T>())
}
