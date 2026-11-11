//! A workshop query as managed sends it (`Storage.Query` / `Workshop.Query.ToRequest` serialised by
//! System.Text.Json), and the Steam request it becomes.

use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

/// Steam's `kNumUGCResultsPerPage`.
pub const PAGE_SIZE: u32 = 50;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct UgcQuery {
    #[serde(deserialize_with = "nullable")]
    pub file_ids: Vec<u64>,
    #[serde(deserialize_with = "nullable")]
    pub tags_required: Vec<String>,
    #[serde(deserialize_with = "nullable")]
    pub tags_excluded: Vec<String>,
    #[serde(deserialize_with = "nullable")]
    pub key_values: BTreeMap<String, String>,
    pub search_text: Option<String>,
    pub max_cache_age: u32,
    /// Steam's `EUGCQuery`.
    pub sort_order: i32,
    pub author: u64,
    /// "favorited" asks for what `author` favourited rather than published.
    pub user_list: Option<String>,
    pub rank_trend_days: u32,
    /// The JSON names `FileIds` (null or absent means it isn't an id lookup).
    #[serde(skip)]
    pub by_id: bool,
}

/// Managed serialises unset collections as `null`.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Steam's `EUserUGCList` values a query can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserList {
    Published = 0,
    Favorited = 5,
}

/// Which Steam request a query is.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryPlan {
    /// No ids to ask about: an empty page, without asking.
    Nothing,
    /// `CreateQueryUGCDetailsRequest`.
    Details(Vec<u64>),
    /// `CreateQueryUserUGCRequest`; user lists page by number, so the cursor is the page.
    User { account: u32, list: UserList, sort: i32, page: u32 },
    /// `CreateQueryAllUGCRequestCursor`.
    All { kind: i32, cursor: String },
}

/// `EUGCQuery`s ranked over a window of days, the only ones `SetRankedByTrendDays` takes.
const TREND_SORTS: [i32; 4] = [3, 13, 15, 17];

impl UgcQuery {
    pub fn parse(json: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(json).ok()?;
        let by_id = value.as_object().is_some_and(|o| o.iter().any(|(k, v)| k.eq_ignore_ascii_case("FileIds") && v.is_array()));
        Some(Self { by_id, ..serde_json::from_value(value).ok()? })
    }

    /// An id lookup with no ids asks for nothing.
    pub fn plan(&self, cursor: &str) -> QueryPlan {
        if self.by_id {
            return if self.file_ids.is_empty() { QueryPlan::Nothing } else { QueryPlan::Details(self.file_ids.clone()) };
        }
        if self.author != 0 {
            let list = if self.user_list.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("favorited")) {
                UserList::Favorited
            } else {
                UserList::Published
            };
            let page = cursor.parse().ok().filter(|&p| p > 0).unwrap_or(1);
            return QueryPlan::User { account: self.author as u32, list, sort: user_sort(self.sort_order), page };
        }
        let cursor = if cursor.is_empty() { "*" } else { cursor };
        QueryPlan::All { kind: self.sort_order, cursor: cursor.into() }
    }

    pub fn trend_days(&self) -> Option<u32> {
        (TREND_SORTS.contains(&self.sort_order) && self.rank_trend_days > 0).then_some(self.rank_trend_days)
    }

    pub fn search(&self) -> Option<&str> {
        self.search_text.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
}

/// The nearest `EUserUGCListSortOrder` to an `EUGCQuery`.
fn user_sort(query: i32) -> i32 {
    match query {
        // RankedByPublicationDate → CreationOrderDesc
        1 => 0,
        // RankedByLastUpdatedDate → LastUpdatedDesc
        19 => 3,
        // RankedByVote, RankedByTrend, RankedByVotesUp, RankedByTextSearch → VoteScoreDesc
        0 | 3 | 10 | 11 => 5,
        _ => 0,
    }
}

/// Where the page after this one starts, or `None` for the last page.
pub fn next_cursor(plan: &QueryPlan, steam_cursor: &str, returned: u32, total: u32) -> Option<String> {
    match plan {
        QueryPlan::Nothing | QueryPlan::Details(_) => None,
        QueryPlan::User { page, .. } => (returned > 0 && page * PAGE_SIZE < total).then(|| (page + 1).to_string()),
        QueryPlan::All { cursor, .. } => (returned > 0 && !steam_cursor.is_empty() && steam_cursor != cursor).then(|| steam_cursor.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_storage_query_with_nulls() {
        let json = r#"{"FileIds":null,"TagsRequired":["map"],"TagsExcluded":[],"KeyValues":{"package":"a.b"},
            "SearchText":null,"MaxCacheAge":0,"SortOrder":3,"Author":0,"RankTrendDays":30}"#;
        let q = UgcQuery::parse(json).unwrap();
        assert_eq!(q.tags_required, ["map"]);
        assert_eq!(q.key_values["package"], "a.b");
        assert_eq!(q.trend_days(), Some(30));
        assert!(!q.by_id);
        assert_eq!(q.plan("*"), QueryPlan::All { kind: 3, cursor: "*".into() });
    }

    #[test]
    fn id_lookups_and_user_lists() {
        let q = UgcQuery::parse(r#"{"FileIds":[5,6],"MaxCacheAge":600}"#).unwrap();
        assert_eq!((q.plan("*"), q.max_cache_age), (QueryPlan::Details(vec![5, 6]), 600));
        assert_eq!(UgcQuery::parse(r#"{"fileIds":[]}"#).unwrap().plan("*"), QueryPlan::Nothing);

        let q = UgcQuery::parse(r#"{"Author":76561197960265770,"UserList":"favorited","SortOrder":1}"#).unwrap();
        assert_eq!(q.plan("*"), QueryPlan::User { account: 42, list: UserList::Favorited, sort: 0, page: 1 });
        assert_eq!(q.plan("3"), QueryPlan::User { account: 42, list: UserList::Favorited, sort: 0, page: 3 });
        assert_eq!(q.trend_days(), None);
    }

    #[test]
    fn paging() {
        let user = QueryPlan::User { account: 1, list: UserList::Published, sort: 0, page: 1 };
        assert_eq!(next_cursor(&user, "", 50, 120).as_deref(), Some("2"));
        let last = QueryPlan::User { account: 1, list: UserList::Published, sort: 0, page: 3 };
        assert_eq!(next_cursor(&last, "", 20, 120), None);
        let all = QueryPlan::All { kind: 0, cursor: "*".into() };
        assert_eq!(next_cursor(&all, "AoJ", 50, 900).as_deref(), Some("AoJ"));
        assert_eq!(next_cursor(&all, "*", 50, 900), None, "the same cursor again is the end");
        assert_eq!(next_cursor(&all, "AoJ", 0, 900), None);
    }
}
