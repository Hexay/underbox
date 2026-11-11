//! The workshop over `ISteamUGC`: the client's (`SteamUGC`) or the game server's (`SteamGameServerUGC`).
//! Async calls complete through [`Calls`] while that side's callbacks are pumped.

use super::ffi;
use super::pump::{Calls, field};
use super::strings::{c, written};
use super::ugc_query::{self, FAIL};
use crate::SteamResult;
use crate::workshop::{Done, Edit, ItemEdits, ItemKind, LocalItem, Published, QueryPage, UgcQuery, UpdateProgress, Vote, Workshop};
use std::ffi::c_char;
use std::sync::Arc;
use steamworks::sys;

/// `k_UGCUpdateHandleInvalid`.
const NO_UPDATE: u64 = u64::MAX;

type Accessor<T> = unsafe extern "C" fn() -> *mut T;

pub(crate) struct SteamUgc {
    ugc: Accessor<sys::ISteamUGC>,
    utils: Accessor<sys::ISteamUtils>,
    calls: Arc<Calls>,
    /// Owner names come from the user's friends cache; a game server has none.
    names: bool,
}

impl SteamUgc {
    pub fn client(calls: Arc<Calls>) -> Self {
        Self { ugc: sys::SteamAPI_SteamUGC_v021, utils: sys::SteamAPI_SteamUtils_v010, calls, names: true }
    }

    pub fn server(calls: Arc<Calls>) -> Self {
        Self { ugc: sys::SteamAPI_SteamGameServerUGC_v021, utils: sys::SteamAPI_SteamGameServerUtils_v010, calls, names: false }
    }

    fn raw(&self) -> *mut sys::ISteamUGC {
        // SAFETY: the owner only hands this out while its side of Steam is initialised.
        unsafe { (self.ugc)() }
    }

    fn app(&self) -> u32 {
        // SAFETY: as `raw`.
        unsafe { sys::SteamAPI_ISteamUtils_GetAppID((self.utils)()) }
    }

    /// Wait on `call`, then hand `done` what `read` makes of its result (`None` if the call failed).
    fn expect<T: 'static>(&self, call: u64, read: fn(&[u8]) -> Result<T, SteamResult>, done: Done<Result<T, SteamResult>>) -> bool {
        self.calls.expect(call, move |b| done(b.map_or(Err(FAIL), read)))
    }

    fn ids(&self, count: u32, fill: impl FnOnce(*mut u64, u32) -> u32) -> Vec<u64> {
        let mut ids = vec![0u64; count as usize];
        let n = fill(ids.as_mut_ptr(), count);
        ids.truncate(n as usize);
        ids
    }
}

fn result_of(r: i32) -> Result<(), SteamResult> {
    if r == 1 { Ok(()) } else { Err(r) }
}

fn vote_read(b: &[u8]) -> Result<Vote, SteamResult> {
    result_of(field!(b, sys::GetUserItemVoteResult_t, m_eResult: i32))?;
    Ok(Vote { up: field!(b, sys::GetUserItemVoteResult_t, m_bVotedUp: bool), down: field!(b, sys::GetUserItemVoteResult_t, m_bVotedDown: bool) })
}

fn set_vote_read(b: &[u8]) -> Result<Vote, SteamResult> {
    result_of(field!(b, sys::SetUserItemVoteResult_t, m_eResult: i32))?;
    let up = field!(b, sys::SetUserItemVoteResult_t, m_bVoteUp: bool);
    Ok(Vote { up, down: !up })
}

fn created_read(b: &[u8]) -> Result<Published, SteamResult> {
    result_of(field!(b, sys::CreateItemResult_t, m_eResult: i32))?;
    Ok(Published {
        item: field!(b, sys::CreateItemResult_t, m_nPublishedFileId: u64),
        needs_legal_agreement: field!(b, sys::CreateItemResult_t, m_bUserNeedsToAcceptWorkshopLegalAgreement: bool),
    })
}

fn submitted_read(b: &[u8]) -> Result<Published, SteamResult> {
    result_of(field!(b, sys::SubmitItemUpdateResult_t, m_eResult: i32))?;
    Ok(Published {
        item: field!(b, sys::SubmitItemUpdateResult_t, m_nPublishedFileId: u64),
        needs_legal_agreement: field!(b, sys::SubmitItemUpdateResult_t, m_bUserNeedsToAcceptWorkshopLegalAgreement: bool),
    })
}

/// Apply `edits` to update `u`, in order.
fn apply(ugc: *mut sys::ISteamUGC, u: u64, edits: &ItemEdits) {
    // SAFETY: a live interface and an unsubmitted update handle; every string outlives its call.
    unsafe {
        if let Some(tags) = &edits.tags {
            let tags: Vec<_> = tags.iter().map(|t| c(t)).collect();
            let mut ptrs: Vec<*const c_char> = tags.iter().map(|t| t.as_ptr()).collect();
            let array = sys::SteamParamStringArray_t { m_ppStrings: ptrs.as_mut_ptr(), m_nNumStrings: ptrs.len() as i32 };
            sys::SteamAPI_ISteamUGC_SetItemTags(ugc, u, &array, false);
        }
        for e in &edits.edits {
            match e {
                Edit::Title(s) => sys::SteamAPI_ISteamUGC_SetItemTitle(ugc, u, c(s).as_ptr()),
                Edit::Description(s) => sys::SteamAPI_ISteamUGC_SetItemDescription(ugc, u, c(s).as_ptr()),
                Edit::Language(s) => sys::SteamAPI_ISteamUGC_SetItemUpdateLanguage(ugc, u, c(s).as_ptr()),
                Edit::Metadata(s) => sys::SteamAPI_ISteamUGC_SetItemMetadata(ugc, u, c(s).as_ptr()),
                Edit::Visibility(v) => ffi::ugc_set_visibility(ugc, u, *v),
                Edit::Content(s) => sys::SteamAPI_ISteamUGC_SetItemContent(ugc, u, c(s).as_ptr()),
                Edit::Preview(s) => sys::SteamAPI_ISteamUGC_SetItemPreview(ugc, u, c(s).as_ptr()),
                Edit::AllowLegacyUpload(b) => sys::SteamAPI_ISteamUGC_SetAllowLegacyUpload(ugc, u, *b),
                Edit::AddKeyValue(k, v) => sys::SteamAPI_ISteamUGC_AddItemKeyValueTag(ugc, u, c(k).as_ptr(), c(v).as_ptr()),
                Edit::RemoveKeyValues(k) => sys::SteamAPI_ISteamUGC_RemoveItemKeyValueTags(ugc, u, c(k).as_ptr()),
                Edit::RemoveAllKeyValues => sys::SteamAPI_ISteamUGC_RemoveAllItemKeyValueTags(ugc, u),
                Edit::AddPreviewFile(s, kind) => ffi::ugc_add_preview_file(ugc, u, c(s).as_ptr(), *kind),
                Edit::AddPreviewVideo(s) => sys::SteamAPI_ISteamUGC_AddItemPreviewVideo(ugc, u, c(s).as_ptr()),
                Edit::UpdatePreviewFile(i, s) => sys::SteamAPI_ISteamUGC_UpdateItemPreviewFile(ugc, u, *i, c(s).as_ptr()),
                Edit::UpdatePreviewVideo(i, s) => sys::SteamAPI_ISteamUGC_UpdateItemPreviewVideo(ugc, u, *i, c(s).as_ptr()),
                Edit::RemovePreview(i) => sys::SteamAPI_ISteamUGC_RemoveItemPreview(ugc, u, *i),
                Edit::GameVersions(min, max) => sys::SteamAPI_ISteamUGC_SetRequiredGameVersions(ugc, u, c(min).as_ptr(), c(max).as_ptr()),
            };
        }
    }
}

// SAFETY (every unsafe block below): `raw()` is live while `self` is offered, and each pointer is valid for its call.
impl Workshop for SteamUgc {
    fn query(&self, query: &UgcQuery, cursor: &str, done: Done<Result<QueryPage, SteamResult>>) -> bool {
        let plan = query.plan(cursor);
        if plan == crate::workshop::QueryPlan::Nothing {
            done(Ok(QueryPage::default()));
            return true;
        }
        let (ugc, names) = (self.raw(), self.names);
        let Some(handle) = ugc_query::create(ugc, self.app(), query, &plan) else { return false };
        let call = unsafe { sys::SteamAPI_ISteamUGC_SendQueryUGCRequest(ugc, handle) };
        let accessor = self.ugc;
        let started = self.calls.expect(call, move |b| done(ugc_query::page(unsafe { accessor() }, handle, &plan, b, names)));
        if !started {
            unsafe { sys::SteamAPI_ISteamUGC_ReleaseQueryUGCRequest(ugc, handle) };
        }
        started
    }

    fn get_vote(&self, item: u64, done: Done<Result<Vote, SteamResult>>) -> bool {
        self.expect(unsafe { sys::SteamAPI_ISteamUGC_GetUserItemVote(self.raw(), item) }, vote_read, done)
    }

    fn set_vote(&self, item: u64, up: bool, done: Done<Result<Vote, SteamResult>>) -> bool {
        self.expect(unsafe { sys::SteamAPI_ISteamUGC_SetUserItemVote(self.raw(), item, up) }, set_vote_read, done)
    }

    fn create_item(&self, kind: ItemKind, done: Done<Result<Published, SteamResult>>) -> bool {
        self.expect(unsafe { ffi::ugc_create_item(self.raw(), self.app(), kind as i32) }, created_read, done)
    }

    fn submit(&self, item: u64, edits: &ItemEdits, note: &str, done: Done<Result<Published, SteamResult>>) -> Option<u64> {
        let ugc = self.raw();
        let update = unsafe { sys::SteamAPI_ISteamUGC_StartItemUpdate(ugc, self.app(), item) };
        if update == NO_UPDATE {
            return None;
        }
        apply(ugc, update, edits);
        let call = unsafe { sys::SteamAPI_ISteamUGC_SubmitItemUpdate(ugc, update, c(note).as_ptr()) };
        self.expect(call, submitted_read, done).then_some(update)
    }

    fn update_progress(&self, update: u64) -> UpdateProgress {
        let (mut processed, mut total) = (0, 0);
        let status = unsafe { ffi::ugc_update_progress(self.raw(), update, &mut processed, &mut total) };
        UpdateProgress { status, processed, total }
    }

    fn subscribed(&self) -> Vec<u64> {
        let ugc = self.raw();
        let n = unsafe { sys::SteamAPI_ISteamUGC_GetNumSubscribedItems(ugc, false) };
        self.ids(n, |p, n| unsafe { sys::SteamAPI_ISteamUGC_GetSubscribedItems(ugc, p, n, false) })
    }

    fn downloaded(&self) -> Vec<u64> {
        let ugc = self.raw();
        let n = unsafe { sys::SteamAPI_ISteamUGC_GetNumDownloadedItems(ugc) };
        self.ids(n, |p, n| unsafe { sys::SteamAPI_ISteamUGC_GetDownloadedItems(ugc, p, n) })
    }

    fn item(&self, item: u64) -> LocalItem {
        let ugc = self.raw();
        let mut out = LocalItem { published_file_id: item, ..Default::default() };
        out.state = unsafe { sys::SteamAPI_ISteamUGC_GetItemState(ugc, item) };
        let (mut size, mut stamp) = (0, 0);
        out.install_folder =
            written(4096, |p, n| unsafe { sys::SteamAPI_ISteamUGC_GetItemInstallInfo(ugc, item, &mut size, p, n, &mut stamp) }).unwrap_or_default();
        (out.size_on_disk, out.timestamp) = (size, stamp);
        unsafe { sys::SteamAPI_ISteamUGC_GetItemDownloadInfo(ugc, item, &mut out.bytes_downloaded, &mut out.bytes_total) };
        out
    }

    fn download(&self, item: u64, high_priority: bool) -> bool {
        unsafe { sys::SteamAPI_ISteamUGC_DownloadItem(self.raw(), item, high_priority) }
    }

    fn mark_unused(&self, item: u64) -> bool {
        unsafe { sys::SteamAPI_ISteamUGC_MarkDownloadedItemAsUnused(self.raw(), item) }
    }

    // Steam broadcasts RemoteStoragePublishedFile(Un)Subscribed_t, which managed listens for.
    fn subscribe(&self, item: u64) {
        unsafe { sys::SteamAPI_ISteamUGC_SubscribeItem(self.raw(), item) };
    }

    fn unsubscribe(&self, item: u64) {
        unsafe { sys::SteamAPI_ISteamUGC_UnsubscribeItem(self.raw(), item) };
    }

    fn start_playtime(&self, mut item: u64) {
        unsafe { sys::SteamAPI_ISteamUGC_StartPlaytimeTracking(self.raw(), &mut item, 1) };
    }

    fn stop_playtime(&self, mut item: u64) {
        unsafe { sys::SteamAPI_ISteamUGC_StopPlaytimeTracking(self.raw(), &mut item, 1) };
    }

    /// The workshop depot is the app's own.
    fn init_for_server(&self, folder: &str) -> bool {
        unsafe { sys::SteamAPI_ISteamUGC_BInitWorkshopForGameServer(self.raw(), self.app(), c(folder).as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    fn bytes<T>(set: &[(usize, &[u8])]) -> Vec<u8> {
        let mut b = vec![0u8; size_of::<T>()];
        for (at, v) in set {
            b[*at..*at + v.len()].copy_from_slice(v);
        }
        b
    }

    #[test]
    fn reads_call_results() {
        type C = sys::CreateItemResult_t;
        let ok = bytes::<C>(&[
            (offset_of!(C, m_eResult), &1i32.to_le_bytes()),
            (offset_of!(C, m_nPublishedFileId), &99u64.to_le_bytes()),
            (offset_of!(C, m_bUserNeedsToAcceptWorkshopLegalAgreement), &[1]),
        ]);
        assert_eq!(created_read(&ok), Ok(Published { item: 99, needs_legal_agreement: true }));
        let denied = bytes::<C>(&[(offset_of!(C, m_eResult), &15i32.to_le_bytes())]);
        assert_eq!(created_read(&denied), Err(15));

        type V = sys::GetUserItemVoteResult_t;
        let down = bytes::<V>(&[(offset_of!(V, m_eResult), &1i32.to_le_bytes()), (offset_of!(V, m_bVotedDown), &[1])]);
        assert_eq!(vote_read(&down), Ok(Vote { up: false, down: true }));
        type S = sys::SetUserItemVoteResult_t;
        let up = bytes::<S>(&[(offset_of!(S, m_eResult), &1i32.to_le_bytes()), (offset_of!(S, m_bVoteUp), &[1])]);
        assert_eq!(set_vote_read(&up), Ok(Vote { up: true, down: false }));
    }
}
