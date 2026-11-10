//! The workshop (`Steam::Ugc`): publishing (`CUgcUpdate`), queries, votes and subscriptions. No
//! platform backend publishes or queries yet, so queries and votes return null ("Steam couldn't be
//! asked"), new items fail to create, and an opened item's submit fails at once with
//! `k_EResultNotLoggedOn`; managed logs the code and gives up.
// TODO: a Platform workshop trait with a Steam backend (ISteamUGC create/update/query/vote).

use abi::slots::{
    steam_ugc_c_ugc_query as query, steam_ugc_c_ugc_subscriptions as subs, steam_ugc_c_ugc_update as update, steam_ugc_c_ugc_vote as vote,
};
use abi::{NativeRef, Registry, StrRet, bind, class};

/// `EResult::k_EResultNotLoggedOn`.
const NOT_LOGGED_ON: i32 = 21;

type Update = NativeRef<class::Steam_Ugc_CUgcUpdate>;

/// The item an update edits: its published id, 0 for one being created.
struct Item(u64);

macro_rules! set_field {
    ($ty:ty, $getter:path, $obj:expr, $value:expr) => {
        abi::rt::fields::set::<$ty, { $getter.index }>($obj.as_raw(), $value)
    };
}

/// An update whose item never got created (no workshop to create it in).
fn create_failed() -> Update {
    let u: Update = objects::create(Item(0));
    set_field!(bool, update::GET_M_CREATING, u, false);
    set_field!(bool, update::GET_M_CREATED, u, false);
    set_field!(i32, update::GET_M_RESULT_CODE, u, NOT_LOGGED_ON);
    u
}

fn open(id: u64) -> Update {
    let u: Update = objects::create(Item(id));
    set_field!(bool, update::GET_M_CREATED, u, true);
    u
}

/// Nothing to submit to: report it finished and failed, so managed's wait ends either way.
fn submit(u: Update) -> bool {
    set_field!(bool, update::GET_M_SUBMITTED, u, false);
    set_field!(bool, update::GET_M_COMPLETE, u, true);
    set_field!(bool, update::GET_M_SUCCESS, u, false);
    set_field!(i32, update::GET_M_RESULT_CODE, u, NOT_LOGGED_ON);
    false
}

fn edits(r: &mut Registry) {
    // The edits have nowhere to go (see the module docs); they're accepted and dropped.
    bind!(r, update::SET_TITLE, |_, _| {});
    bind!(r, update::SET_DESCRIPTION, |_, _| {});
    bind!(r, update::SET_LANGUAGE, |_, _| {});
    bind!(r, update::SET_METADATA, |_, _| {});
    bind!(r, update::SET_VISIBILITY, |_, _| {});
    bind!(r, update::SET_TAG, |_, _| {});
    bind!(r, update::SET_CONTENT_FOLDER, |_, _| {});
    bind!(r, update::SET_PREVIEW_IMAGE, |_, _| {});
    bind!(r, update::SET_ALLOW_LEGACY_UPLOAD, |_, _| {});
    bind!(r, update::ADD_KEY_VALUE_TAG, |_, _, _| {});
    bind!(r, update::REMOVE_KEY_VALUE_TAGS, |_, _| {});
    bind!(r, update::REMOVE_ALL_KEY_VALUE_TAGS, |_| {});
    bind!(r, update::ADD_PREVIEW_FILE, |_, _, _| {});
    bind!(r, update::ADD_PREVIEW_VIDEO, |_, _| {});
    bind!(r, update::UPDATE_PREVIEW_FILE, |_, _, _| {});
    bind!(r, update::UPDATE_PREVIEW_VIDEO, |_, _, _| {});
    bind!(r, update::REMOVE_PREVIEW, |_, _| {});
    bind!(r, update::SET_REQUIRED_GAME_VERSIONS, |_, _, _| {});
}

pub fn register(r: &mut Registry) {
    bind!(r, update::CREATE_COMMUNITY_ITEM, create_failed);
    bind!(r, update::CREATE_MTX_ITEM, create_failed);
    bind!(r, update::OPEN_COMMUNITY_ITEM, open);
    bind!(r, update::DISPOSE, |u| {
        objects::destroy(u);
    });
    bind!(r, update::GET_PUBLISHED_FILE_ID, |u| objects::get::<_, Item>(u).map_or(0, |i| i.0));
    edits(r);
    bind!(r, update::SUBMIT, |u, _| submit(u));
    bind!(r, update::GET_PROGRESS_PERCENT, |_| 0.0);
    bind!(r, update::GET_BYTES_PROCESSED, |_| 0);
    bind!(r, update::GET_BYTES_TOTAL, |_| 0);
    // k_EItemUpdateStatusInvalid: no update in flight.
    bind!(r, update::GET_UPDATE_STATUS, |_| 0);

    bind!(r, query::CREATE_QUERY, |_, _| NativeRef::null());
    bind!(r, query::DISPOSE, |q| {
        objects::destroy(q);
    });
    bind!(r, query::GET_RESULT_JSON, |_| StrRet::from(c""));
    bind!(r, vote::GET_VOTE, |_| NativeRef::null());
    bind!(r, vote::SET_VOTE, |_, _| NativeRef::null());
    bind!(r, vote::DISPOSE, |v| {
        objects::destroy(v);
    });

    // Nothing subscribed or downloaded: nothing to download, track or forget either.
    bind!(r, subs::INIT_WORKSHOP_FOR_GAME_SERVER, |_| true);
    bind!(r, subs::GET_SUBSCRIBED_JSON, || StrRet::from(c"[]"));
    bind!(r, subs::GET_DOWNLOADED_JSON, || StrRet::from(c"[]"));
    // "" is managed's "no Steam" for an item.
    bind!(r, subs::GET_ITEM_JSON, |_| StrRet::from(c""));
    bind!(r, subs::DOWNLOAD_ITEM, |_, _| false);
    bind!(r, subs::MARK_DOWNLOADED_ITEM_AS_UNUSED, |_| false);
    bind!(r, subs::SUBSCRIBE_ITEM, |_| {});
    bind!(r, subs::UNSUBSCRIBE_ITEM, |_| {});
    bind!(r, subs::START_PLAYTIME_TRACKING, |_| {});
    bind!(r, subs::STOP_PLAYTIME_TRACKING, |_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<T: Copy + Default + Send + 'static, const K: usize>(u: Update) -> T {
        abi::rt::fields::get::<T, K>(u.as_raw())
    }

    #[test]
    fn creating_fails_and_opened_items_fail_to_submit() {
        let made = create_failed();
        assert!(!field::<bool, { update::GET_M_CREATING.index }>(made) && !field::<bool, { update::GET_M_CREATED.index }>(made));
        assert_eq!(field::<i32, { update::GET_M_RESULT_CODE.index }>(made), NOT_LOGGED_ON);

        let opened = open(1234);
        assert_eq!(objects::get::<_, Item>(opened).map(|i| i.0), Some(1234));
        assert!(!submit(opened));
        assert!(field::<bool, { update::GET_M_COMPLETE.index }>(opened) && !field::<bool, { update::GET_M_SUCCESS.index }>(opened));
        assert!(objects::destroy(made) && objects::destroy(opened));
    }
}
