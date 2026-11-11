//! Workshop queries, votes and what's on this machine (`Steam::Ugc::CUgcQuery`, `CUgcVote`,
//! `CUgcSubscriptions`). With no workshop, queries and votes return null ("Steam couldn't be
//! asked") and nothing is subscribed or downloaded.

use abi::slots::{steam_ugc_c_ugc_query as query, steam_ugc_c_ugc_subscriptions as subs, steam_ugc_c_ugc_vote as vote};
use abi::{NativeRef, Registry, StrRet, bind, class};
use platform::Workshop;
use platform::workshop::{Done, UgcQuery};
use std::ffi::CStr;
use std::sync::{Arc, Mutex};

/// The workshop this process uses: the game server's on a dedicated server, else the user's.
pub(super) fn workshop() -> Option<&'static dyn Workshop> {
    if crate::app::dict::current_config().dedicated_server {
        platform::get().server_workshop()
    } else {
        super::client::local()?.workshop()
    }
}

/// A completion that runs `f` only while the object at `r` is still `state`: managed may dispose it
/// before Steam answers, and its address may by then belong to another object.
pub(super) fn completed<C: 'static, T: Send + Sync + 'static, R: 'static>(
    r: NativeRef<C>,
    state: Arc<T>,
    f: impl FnOnce(NativeRef<C>, Arc<T>, R) + Send + 'static,
) -> Done<R> {
    Box::new(move |result| {
        if objects::get::<C, T>(r).is_some_and(|s| Arc::ptr_eq(&s, &state)) {
            f(r, state, result)
        }
    })
}

macro_rules! set {
    ($ty:ty, $getter:path, $obj:expr, $value:expr) => {
        abi::rt::fields::set::<$ty, { $getter.index }>($obj.as_raw(), $value)
    };
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

type Query = NativeRef<class::Steam_Ugc_CUgcQuery>;
type Vote = NativeRef<class::Steam_Ugc_CUgcVote>;

/// A query's answer as JSON, once it has come.
#[derive(Default)]
struct Answer(Mutex<String>);

fn create_query(json: &str, cursor: &str) -> Query {
    let (Some(w), Some(request)) = (workshop(), UgcQuery::parse(json)) else { return NativeRef::null() };
    let answer = Arc::new(Answer::default());
    let q: Query = objects::create_shared(answer.clone());
    let done = completed(q, answer, |q, answer, r: Result<platform::workshop::QueryPage, i32>| {
        if let Ok(page) = &r {
            *answer.0.lock().unwrap() = serde_json::to_string(page).unwrap_or_default();
        }
        set!(bool, query::GET_M_SUCCESS, q, r.is_ok());
        set!(i32, query::GET_M_RESULT_CODE, q, r.as_ref().err().copied().unwrap_or(1));
        set!(bool, query::GET_M_COMPLETE, q, true);
    });
    if w.query(&request, cursor, done) {
        q
    } else {
        objects::destroy(q);
        NativeRef::null()
    }
}

#[derive(Default)]
struct Ballot;

fn vote(item: u64, up: Option<bool>) -> Vote {
    let Some(w) = workshop() else { return NativeRef::null() };
    let state = Arc::new(Ballot);
    let v: Vote = objects::create_shared(state.clone());
    let done = completed(v, state, |v, _, r: Result<platform::workshop::Vote, i32>| {
        let got = r.unwrap_or_default();
        set!(bool, vote::GET_M_VOTED_UP, v, got.up);
        set!(bool, vote::GET_M_VOTED_DOWN, v, got.down);
        set!(bool, vote::GET_M_SUCCESS, v, r.is_ok());
        set!(i32, vote::GET_M_RESULT_CODE, v, r.err().unwrap_or(1));
        set!(bool, vote::GET_M_COMPLETE, v, true);
    });
    let started = match up {
        Some(up) => w.set_vote(item, up, done),
        None => w.get_vote(item, done),
    };
    if started {
        v
    } else {
        objects::destroy(v);
        NativeRef::null()
    }
}

fn items_json(ids: impl FnOnce(&dyn Workshop) -> Vec<u64>) -> String {
    let Some(w) = workshop() else { return "[]".into() };
    let items: Vec<_> = ids(w).into_iter().map(|id| w.item(id)).collect();
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
}

fn on<R: Default>(f: impl FnOnce(&dyn Workshop) -> R) -> R {
    workshop().map(f).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    bind!(r, query::CREATE_QUERY, |json, cursor| create_query(&text(json), &text(cursor)));
    bind!(r, query::DISPOSE, |q| {
        objects::destroy(q);
    });
    bind!(r, query::GET_RESULT_JSON, |q| StrRet::from(objects::get::<_, Answer>(q).map(|a| a.0.lock().unwrap().clone())));
    bind!(r, vote::GET_VOTE, |item| vote(item, None));
    bind!(r, vote::SET_VOTE, |item, up| vote(item, Some(up)));
    bind!(r, vote::DISPOSE, |v| {
        objects::destroy(v);
    });

    bind!(r, subs::GET_SUBSCRIBED_JSON, || StrRet::from(items_json(|w| w.subscribed())));
    bind!(r, subs::GET_DOWNLOADED_JSON, || StrRet::from(items_json(|w| w.downloaded())));
    // "" is managed's "no Steam" for an item.
    bind!(r, subs::GET_ITEM_JSON, |id| StrRet::from(on(|w| serde_json::to_string(&w.item(id)).unwrap_or_default())));
    bind!(r, subs::DOWNLOAD_ITEM, |id, high| on(|w| w.download(id, high)));
    bind!(r, subs::MARK_DOWNLOADED_ITEM_AS_UNUSED, |id| on(|w| w.mark_unused(id)));
    bind!(r, subs::SUBSCRIBE_ITEM, |id| on(|w| w.subscribe(id)));
    bind!(r, subs::UNSUBSCRIBE_ITEM, |id| on(|w| w.unsubscribe(id)));
    bind!(r, subs::START_PLAYTIME_TRACKING, |id| on(|w| w.start_playtime(id)));
    bind!(r, subs::STOP_PLAYTIME_TRACKING, |id| on(|w| w.stop_playtime(id)));
    // Without a game-server workshop there's nothing to set up, and nothing to fail.
    bind!(r, subs::INIT_WORKSHOP_FOR_GAME_SERVER, |folder| workshop().is_none_or(|w| w.init_for_server(&text(folder))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_has_no_workshop() {
        assert!(workshop().is_none());
        assert!(create_query(r#"{"FileIds":[1]}"#, "*").is_null());
        assert!(vote(1, Some(true)).is_null());
        assert_eq!(items_json(|w| w.subscribed()), "[]");
    }

    #[test]
    fn completions_skip_disposed_objects() {
        let answer = Arc::new(Answer::default());
        let q: Query = objects::create_shared(answer.clone());
        let ran = Arc::new(Mutex::new(false));
        let r2 = ran.clone();
        let done: Done<()> = completed(q, answer, move |_, _, ()| *r2.lock().unwrap() = true);
        objects::destroy(q);
        done(());
        assert!(!*ran.lock().unwrap());
    }
}
