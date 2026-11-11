//! Workshop queries: a [`UgcQuery`] becomes an `ISteamUGC` query request, and the completed query's
//! results become a [`QueryPage`].

use super::ffi;
use super::pump::{c_str_at, field};
use super::strings::{c, owned, written};
use crate::SteamResult;
use crate::workshop::{ItemDetails, Preview, QueryPage, QueryPlan, UgcQuery, next_cursor, owner};
use std::collections::BTreeMap;
use std::mem::offset_of;
use steamworks::sys;

/// `k_UGCQueryHandleInvalid`.
const INVALID: u64 = u64::MAX;
/// `k_EUGCMatchingUGCType_Items`: every kind of item.
const ITEMS: i32 = 0;
/// `k_EItemStatistic_NumSubscriptions`.
const NUM_SUBSCRIPTIONS: i32 = 0;
/// `k_EResultFail`, for a call Steam says failed without saying why.
pub(crate) const FAIL: SteamResult = 2;

/// Make the query request (unsent); `None` if Steam won't.
pub(crate) fn create(ugc: *mut sys::ISteamUGC, app: u32, query: &UgcQuery, plan: &QueryPlan) -> Option<u64> {
    // SAFETY: a live interface; every pointer is valid for the call.
    let handle = unsafe {
        match plan {
            QueryPlan::Nothing => return None,
            QueryPlan::Details(ids) => {
                let mut ids = ids.clone();
                sys::SteamAPI_ISteamUGC_CreateQueryUGCDetailsRequest(ugc, ids.as_mut_ptr(), ids.len() as u32)
            }
            QueryPlan::User { account, list, sort, page } => ffi::ugc_query_user(ugc, *account, *list as i32, ITEMS, *sort, app, app, *page),
            QueryPlan::All { kind, cursor } => ffi::ugc_query_all(ugc, *kind, ITEMS, app, app, c(cursor).as_ptr()),
        }
    };
    if handle == INVALID {
        return None;
    }
    configure(ugc, handle, query, plan);
    Some(handle)
}

fn configure(ugc: *mut sys::ISteamUGC, h: u64, q: &UgcQuery, plan: &QueryPlan) {
    // SAFETY: a live interface and an unsent query handle.
    unsafe {
        sys::SteamAPI_ISteamUGC_SetReturnLongDescription(ugc, h, true);
        sys::SteamAPI_ISteamUGC_SetReturnMetadata(ugc, h, true);
        sys::SteamAPI_ISteamUGC_SetReturnKeyValueTags(ugc, h, true);
        sys::SteamAPI_ISteamUGC_SetReturnAdditionalPreviews(ugc, h, true);
        if q.max_cache_age > 0 {
            sys::SteamAPI_ISteamUGC_SetAllowCachedResponse(ugc, h, q.max_cache_age);
        }
        if matches!(plan, QueryPlan::Details(_)) {
            return;
        }
        for tag in &q.tags_required {
            sys::SteamAPI_ISteamUGC_AddRequiredTag(ugc, h, c(tag).as_ptr());
        }
        for tag in &q.tags_excluded {
            sys::SteamAPI_ISteamUGC_AddExcludedTag(ugc, h, c(tag).as_ptr());
        }
        for (k, v) in &q.key_values {
            sys::SteamAPI_ISteamUGC_AddRequiredKeyValueTag(ugc, h, c(k).as_ptr(), c(v).as_ptr());
        }
        // Steam searches and ranks by trend only on all-items queries; user lists are filtered by managed.
        if let QueryPlan::All { .. } = plan {
            if let Some(text) = q.search() {
                sys::SteamAPI_ISteamUGC_SetSearchText(ugc, h, c(text).as_ptr());
            }
            if let Some(days) = q.trend_days() {
                sys::SteamAPI_ISteamUGC_SetRankedByTrendDays(ugc, h, days);
            }
        }
    }
}

/// The page a completed query (`SteamUGCQueryCompleted_t`, `None` if the call failed) found. Releases the handle.
pub(crate) fn page(ugc: *mut sys::ISteamUGC, h: u64, plan: &QueryPlan, done: Option<&[u8]>, names: bool) -> Result<QueryPage, SteamResult> {
    let result = done.map_or(FAIL, |b| field!(b, sys::SteamUGCQueryCompleted_t, m_eResult: i32));
    let page = (result == 1).then(|| {
        let b = done.unwrap_or_default();
        let returned = field!(b, sys::SteamUGCQueryCompleted_t, m_unNumResultsReturned: u32);
        let total = field!(b, sys::SteamUGCQueryCompleted_t, m_unTotalMatchingResults: u32);
        let cursor = c_str_at(b, offset_of!(sys::SteamUGCQueryCompleted_t, m_rgchNextCursor), 256);
        let items: Vec<_> = (0..returned).filter_map(|i| item(ugc, h, i, names)).collect();
        QueryPage { total_count: total, result_count: items.len() as u32, next_cursor: next_cursor(plan, &cursor, returned, total), items }
    });
    // SAFETY: a live interface; the handle is released once, here.
    unsafe { sys::SteamAPI_ISteamUGC_ReleaseQueryUGCRequest(ugc, h) };
    page.ok_or(result)
}

fn item(ugc: *mut sys::ISteamUGC, h: u64, i: u32, names: bool) -> Option<ItemDetails> {
    let mut buf = ffi::zeroed_bytes::<sys::SteamUGCDetails_t>();
    let (details, range) = ffi::aligned::<sys::SteamUGCDetails_t>(&mut buf);
    // SAFETY: a live interface and a completed query; `details` is a struct-sized, aligned buffer.
    if !unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCResult(ugc, h, i, details) } {
        return None;
    }
    let d = &buf[range];
    macro_rules! get {
        ($f:ident: $t:ty) => {
            field!(d, sys::SteamUGCDetails_t, $f: $t)
        };
    }
    macro_rules! text {
        ($f:ident, $n:expr) => {
            c_str_at(d, offset_of!(sys::SteamUGCDetails_t, $f), $n)
        };
    }
    let owner_id = get!(m_ulSteamIDOwner: u64);
    let file_size = get!(m_nFileSize: i32).max(0) as u64;
    let total_size = get!(m_ulTotalFilesSize: u64);
    // SAFETY (all below): a live interface, a completed query, and buffers of the sizes passed.
    let tags = (0..unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCNumTags(ugc, h, i) })
        .filter_map(|t| written(256, |p, n| unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCTag(ugc, h, i, t, p, n) }))
        .collect();
    let mut subscriptions = 0u64;
    unsafe { ffi::ugc_statistic(ugc, h, i, NUM_SUBSCRIPTIONS, &mut subscriptions) };
    let previews: Vec<_> = (0..unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCNumAdditionalPreviews(ugc, h, i) })
        .filter_map(|p| {
            let mut kind = 0;
            let mut name = [0; 260];
            let url = written(1024, |u, n| unsafe { ffi::ugc_additional_preview(ugc, h, i, p, u, n, name.as_mut_ptr(), 260, &mut kind) })?;
            Some(Preview { url, kind })
        })
        .collect();
    let key_values: BTreeMap<_, _> = (0..unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCNumKeyValueTags(ugc, h, i) })
        .filter_map(|k| {
            let mut value = [0; 256];
            let key = written(256, |p, n| unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCKeyValueTag(ugc, h, i, k, p, n, value.as_mut_ptr(), 256) })?;
            value[255] = 0;
            Some((key, owned(value.as_ptr())))
        })
        .collect();
    let name = names.then(|| owned(unsafe { sys::SteamAPI_ISteamFriends_GetFriendPersonaName(sys::SteamAPI_SteamFriends_v018(), owner_id) }));
    Some(ItemDetails {
        id: get!(m_nPublishedFileId: u64),
        title: text!(m_rgchTitle, 129),
        description: text!(m_rgchDescription, 8000),
        preview: written(1024, |p, n| unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCPreviewURL(ugc, h, i, p, n) }).unwrap_or_default(),
        additional_previews: previews.len() as u32,
        previews,
        owner: owner(crate::SteamId(owner_id), name),
        size: if total_size > 0 { total_size } else { file_size },
        votes_up: get!(m_unVotesUp: u32),
        votes_down: get!(m_unVotesDown: u32),
        score: get!(m_flScore: f32),
        num_subscriptions: subscriptions,
        created: get!(m_rtimeCreated: u32),
        updated: get!(m_rtimeUpdated: u32),
        tags,
        key_values,
        metadata: written(5000, |p, n| unsafe { sys::SteamAPI_ISteamUGC_GetQueryUGCMetadata(ugc, h, i, p, n) }).unwrap_or_default(),
        visibility: get!(m_eVisibility: i32),
        banned: get!(m_bBanned: bool),
        accepted: get!(m_bAcceptedForUse: bool),
        file_handle: get!(m_hFile: u64),
        filename: text!(m_pchFileName, 260),
        url: text!(m_rgchURL, 256),
    })
}
