//! The workshop's publishing side (`Steam::Ugc::CUgcUpdate`) over [`platform::Workshop`]. Managed
//! polls the update's fields (`m_creating`, `m_complete`, ...), which the platform's completions set
//! while callbacks run. With no workshop (offline, or Steam not up) new items fail to create and
//! submits fail at once with `k_EResultNotLoggedOn`; managed logs the code and gives up.

use super::ugc_query::{completed, workshop};
use abi::slots::steam_ugc_c_ugc_update as update;
use abi::{NativeRef, Registry, bind, class};
use platform::workshop::{Edit, ItemEdits, ItemKind};
use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// `EResult::k_EResultNotLoggedOn`.
const NOT_LOGGED_ON: i32 = 21;
/// `EResult::k_EResultFail`.
const FAIL: i32 = 2;

type Update = NativeRef<class::Steam_Ugc_CUgcUpdate>;

/// The item an update edits (0 until a new one is created), its edits so far, and its upload.
#[derive(Default)]
struct Item {
    id: AtomicU64,
    edits: Mutex<ItemEdits>,
    upload: Mutex<Option<u64>>,
}

macro_rules! set_field {
    ($ty:ty, $getter:path, $obj:expr, $value:expr) => {
        abi::rt::fields::set::<$ty, { $getter.index }>($obj.as_raw(), $value)
    };
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn finish(u: Update, item: &Item, result: Result<platform::workshop::Published, i32>) {
    if let Ok(p) = result {
        item.id.store(p.item, Ordering::Relaxed);
    }
    set_field!(bool, update::GET_M_B_NEEDS_LEGAL_AGREEMENT, u, result.is_ok_and(|p| p.needs_legal_agreement));
    set_field!(i32, update::GET_M_RESULT_CODE, u, result.err().unwrap_or(1));
}

fn create(kind: ItemKind) -> Update {
    let item = Arc::new(Item::default());
    let u: Update = objects::create_shared(item.clone());
    let Some(w) = workshop() else {
        set_field!(i32, update::GET_M_RESULT_CODE, u, NOT_LOGGED_ON);
        return u;
    };
    set_field!(bool, update::GET_M_CREATING, u, true);
    let done = completed(u, item, |u, item, r| {
        finish(u, &item, r);
        set_field!(bool, update::GET_M_CREATED, u, r.is_ok());
        set_field!(bool, update::GET_M_CREATING, u, false);
    });
    if !w.create_item(kind, done) {
        set_field!(bool, update::GET_M_CREATING, u, false);
        set_field!(i32, update::GET_M_RESULT_CODE, u, FAIL);
    }
    u
}

fn open(id: u64) -> Update {
    let u: Update = objects::create(Item { id: AtomicU64::new(id), ..Item::default() });
    set_field!(bool, update::GET_M_CREATED, u, true);
    u
}

fn edit(u: Update, f: impl FnOnce(&mut ItemEdits)) {
    if let Some(item) = objects::get::<_, Item>(u) {
        f(&mut item.edits.lock().unwrap());
    }
}

/// Start the upload; false (and finished, failed) when there's nothing to submit to.
fn submit(u: Update, note: &str) -> bool {
    let Some(item) = objects::get::<_, Item>(u) else { return false };
    let fail = |code| {
        set_field!(bool, update::GET_M_COMPLETE, u, true);
        set_field!(bool, update::GET_M_SUCCESS, u, false);
        set_field!(i32, update::GET_M_RESULT_CODE, u, code);
        false
    };
    let Some(w) = workshop() else { return fail(NOT_LOGGED_ON) };
    let edits = item.edits.lock().unwrap().clone();
    let id = item.id.load(Ordering::Relaxed);
    let done = completed(u, item.clone(), |u, item, r| {
        finish(u, &item, r);
        set_field!(bool, update::GET_M_SUCCESS, u, r.is_ok());
        set_field!(bool, update::GET_M_COMPLETE, u, true);
    });
    let Some(upload) = w.submit(id, &edits, note, done) else { return fail(FAIL) };
    *item.upload.lock().unwrap() = Some(upload);
    set_field!(bool, update::GET_M_SUBMITTED, u, true);
    true
}

fn progress(u: Update) -> platform::workshop::UpdateProgress {
    let upload = objects::get::<_, Item>(u).and_then(|i| *i.upload.lock().unwrap());
    upload.zip(workshop()).map(|(h, w)| w.update_progress(h)).unwrap_or_default()
}

fn edits(r: &mut Registry) {
    bind!(r, update::SET_TITLE, |u, s| edit(u, |e| e.push(Edit::Title(text(s)))));
    bind!(r, update::SET_DESCRIPTION, |u, s| edit(u, |e| e.push(Edit::Description(text(s)))));
    bind!(r, update::SET_LANGUAGE, |u, s| edit(u, |e| e.push(Edit::Language(text(s)))));
    bind!(r, update::SET_METADATA, |u, s| edit(u, |e| e.push(Edit::Metadata(text(s)))));
    bind!(r, update::SET_VISIBILITY, |u, v| edit(u, |e| e.push(Edit::Visibility(v))));
    bind!(r, update::SET_TAG, |u, s| edit(u, |e| e.add_tag(&text(s))));
    bind!(r, update::SET_CONTENT_FOLDER, |u, s| edit(u, |e| e.push(Edit::Content(text(s)))));
    bind!(r, update::SET_PREVIEW_IMAGE, |u, s| edit(u, |e| e.push(Edit::Preview(text(s)))));
    bind!(r, update::SET_ALLOW_LEGACY_UPLOAD, |u, b| edit(u, |e| e.push(Edit::AllowLegacyUpload(b))));
    bind!(r, update::ADD_KEY_VALUE_TAG, |u, k, v| edit(u, |e| e.push(Edit::AddKeyValue(text(k), text(v)))));
    bind!(r, update::REMOVE_KEY_VALUE_TAGS, |u, k| edit(u, |e| e.push(Edit::RemoveKeyValues(text(k)))));
    bind!(r, update::REMOVE_ALL_KEY_VALUE_TAGS, |u| edit(u, |e| e.push(Edit::RemoveAllKeyValues)));
    bind!(r, update::ADD_PREVIEW_FILE, |u, s, kind| edit(u, |e| e.push(Edit::AddPreviewFile(text(s), kind))));
    bind!(r, update::ADD_PREVIEW_VIDEO, |u, s| edit(u, |e| e.push(Edit::AddPreviewVideo(text(s)))));
    bind!(r, update::UPDATE_PREVIEW_FILE, |u, i, s| edit(u, |e| e.push(Edit::UpdatePreviewFile(i, text(s)))));
    bind!(r, update::UPDATE_PREVIEW_VIDEO, |u, i, s| edit(u, |e| e.push(Edit::UpdatePreviewVideo(i, text(s)))));
    bind!(r, update::REMOVE_PREVIEW, |u, i| edit(u, |e| e.push(Edit::RemovePreview(i))));
    bind!(r, update::SET_REQUIRED_GAME_VERSIONS, |u, min, max| edit(u, |e| e.push(Edit::GameVersions(text(min), text(max)))));
}

pub fn register(r: &mut Registry) {
    bind!(r, update::CREATE_COMMUNITY_ITEM, || create(ItemKind::Community));
    bind!(r, update::CREATE_MTX_ITEM, || create(ItemKind::Microtransaction));
    bind!(r, update::OPEN_COMMUNITY_ITEM, open);
    bind!(r, update::DISPOSE, |u| {
        objects::destroy(u);
    });
    bind!(r, update::GET_PUBLISHED_FILE_ID, |u| objects::get::<_, Item>(u).map_or(0, |i| i.id.load(Ordering::Relaxed)));
    edits(r);
    bind!(r, update::SUBMIT, |u, note| submit(u, &text(note)));
    bind!(r, update::GET_PROGRESS_PERCENT, |u| progress(u).percent());
    bind!(r, update::GET_BYTES_PROCESSED, |u| progress(u).processed);
    bind!(r, update::GET_BYTES_TOTAL, |u| progress(u).total);
    // k_EItemUpdateStatusInvalid (0) until an upload is in flight.
    bind!(r, update::GET_UPDATE_STATUS, |u| progress(u).status);
    super::ugc_query::register(r);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<T: Copy + Default + Send + 'static, const K: usize>(u: Update) -> T {
        abi::rt::fields::get::<T, K>(u.as_raw())
    }

    #[test]
    fn without_a_workshop_creating_fails_and_submits_fail() {
        let made = create(ItemKind::Community);
        assert!(!field::<bool, { update::GET_M_CREATING.index }>(made) && !field::<bool, { update::GET_M_CREATED.index }>(made));
        assert_eq!(field::<i32, { update::GET_M_RESULT_CODE.index }>(made), NOT_LOGGED_ON);

        let opened = open(1234);
        edit(opened, |e| e.add_tag("map"));
        assert_eq!(objects::get::<_, Item>(opened).unwrap().edits.lock().unwrap().tags.as_deref(), Some(&["map".to_string()][..]));
        assert!(!submit(opened, "note"));
        assert!(field::<bool, { update::GET_M_COMPLETE.index }>(opened) && !field::<bool, { update::GET_M_SUCCESS.index }>(opened));
        assert_eq!(progress(opened).status, 0);
        assert!(objects::destroy(made) && objects::destroy(opened));
    }
}
