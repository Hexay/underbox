//! The workshop (Steam UGC): queries, votes, publishing and what's on this machine. Results are
//! shaped as managed deserialises them (`Workshop.Page`/`Storage.QueryResult`, `Ugc.ItemInfo`).

pub use crate::workshop_query::{PAGE_SIZE, QueryPlan, UgcQuery, UserList, next_cursor};
use crate::{SteamId, SteamResult};
use serde::Serialize;
use std::collections::BTreeMap;

/// A completion, run on the thread that pumps callbacks.
pub type Done<T> = Box<dyn FnOnce(T) + Send>;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct QueryPage {
    pub total_count: u32,
    pub result_count: u32,
    pub next_cursor: Option<String>,
    pub items: Vec<ItemDetails>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ItemDetails {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub preview: String,
    pub additional_previews: u32,
    pub previews: Vec<Preview>,
    pub owner: Owner,
    pub size: u64,
    pub votes_up: u32,
    pub votes_down: u32,
    pub score: f32,
    pub num_subscriptions: u64,
    /// Unix seconds.
    pub created: u32,
    pub updated: u32,
    pub tags: Vec<String>,
    pub key_values: BTreeMap<String, String>,
    pub metadata: String,
    pub visibility: i32,
    pub banned: bool,
    pub accepted: bool,
    pub file_handle: u64,
    pub filename: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Preview {
    pub url: String,
    /// Steam's `EItemPreviewType`.
    #[serde(rename = "Type")]
    pub kind: i32,
}

/// Managed's `Profile`: who published an item.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Owner {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// What Steam knows about an item on this machine (managed's `Ugc.ItemInfo`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct LocalItem {
    pub published_file_id: u64,
    /// Steam's `EItemState` flags.
    pub state: u32,
    pub install_folder: String,
    pub size_on_disk: u64,
    pub timestamp: u32,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

/// One change to an item, applied in order when it's submitted.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Title(String),
    Description(String),
    Language(String),
    Metadata(String),
    Visibility(i32),
    Content(String),
    Preview(String),
    AllowLegacyUpload(bool),
    AddKeyValue(String, String),
    RemoveKeyValues(String),
    RemoveAllKeyValues,
    AddPreviewFile(String, i32),
    AddPreviewVideo(String),
    UpdatePreviewFile(u32, String),
    UpdatePreviewVideo(u32, String),
    RemovePreview(u32),
    GameVersions(String, String),
}

/// The edits an update has collected. Steam takes tags as one list, so they gather here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemEdits {
    pub edits: Vec<Edit>,
    pub tags: Option<Vec<String>>,
}

impl ItemEdits {
    pub fn push(&mut self, edit: Edit) {
        self.edits.push(edit);
    }

    pub fn add_tag(&mut self, tag: &str) {
        let tags = self.tags.get_or_insert_default();
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            tags.push(tag.into());
        }
    }
}

/// `EWorkshopFileType` of a new item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Community = 0,
    Microtransaction = 1,
}

/// A created or updated item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Published {
    pub item: u64,
    pub needs_legal_agreement: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Vote {
    pub up: bool,
    pub down: bool,
}

/// An upload in flight: Steam's `EItemUpdateStatus` and bytes so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdateProgress {
    pub status: i32,
    pub processed: u64,
    pub total: u64,
}

impl UpdateProgress {
    pub fn percent(&self) -> f32 {
        if self.total == 0 { 0.0 } else { (self.processed as f64 * 100.0 / self.total as f64) as f32 }
    }
}

/// Starting an async call returns false when it couldn't start, and `done` is then never run.
pub trait Workshop: Send + Sync {
    fn query(&self, query: &UgcQuery, cursor: &str, done: Done<Result<QueryPage, SteamResult>>) -> bool;
    fn get_vote(&self, item: u64, done: Done<Result<Vote, SteamResult>>) -> bool;
    fn set_vote(&self, item: u64, up: bool, done: Done<Result<Vote, SteamResult>>) -> bool;

    fn create_item(&self, kind: ItemKind, done: Done<Result<Published, SteamResult>>) -> bool;
    /// Upload `edits` to `item`; the update's handle (for [`Workshop::update_progress`]), or `None` if it didn't start.
    fn submit(&self, item: u64, edits: &ItemEdits, note: &str, done: Done<Result<Published, SteamResult>>) -> Option<u64>;
    fn update_progress(&self, update: u64) -> UpdateProgress;

    fn subscribed(&self) -> Vec<u64>;
    fn downloaded(&self) -> Vec<u64>;
    fn item(&self, item: u64) -> LocalItem;
    fn download(&self, item: u64, high_priority: bool) -> bool;
    fn mark_unused(&self, item: u64) -> bool;
    fn subscribe(&self, item: u64);
    fn unsubscribe(&self, item: u64);
    fn start_playtime(&self, item: u64);
    fn stop_playtime(&self, item: u64);
    /// A game server keeps its items in `folder`.
    fn init_for_server(&self, folder: &str) -> bool;
}

/// What a query's owner id serialises as when Steam knows the name.
pub fn owner(id: SteamId, name: Option<String>) -> Owner {
    Owner { id: id.0, name: name.filter(|n| !n.is_empty() && n != "[unknown]") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_json_matches_managed_names() {
        let item = ItemDetails {
            id: 7,
            title: "Arena".into(),
            previews: vec![Preview { url: "u".into(), kind: 1 }],
            owner: owner(SteamId(9), Some("[unknown]".into())),
            num_subscriptions: 3,
            ..Default::default()
        };
        let page = QueryPage { total_count: 1, result_count: 1, next_cursor: None, items: vec![item] };
        let v: serde_json::Value = serde_json::to_value(&page).unwrap();
        assert_eq!(v["TotalCount"], 1);
        assert!(v["NextCursor"].is_null());
        let i = &v["Items"][0];
        assert_eq!((i["Id"].as_u64(), i["NumSubscriptions"].as_u64()), (Some(7), Some(3)));
        assert_eq!(i["Previews"][0]["Type"], 1);
        assert_eq!(i["Owner"], serde_json::json!({ "Id": 9 }));
    }

    #[test]
    fn edits_gather_tags_once() {
        let mut e = ItemEdits::default();
        e.add_tag("map");
        e.add_tag("Map");
        e.push(Edit::Title("t".into()));
        assert_eq!(e.tags.as_deref(), Some(&["map".to_string()][..]));
        assert_eq!(UpdateProgress { status: 3, processed: 1, total: 4 }.percent(), 25.0);
        let item = serde_json::to_value(LocalItem { published_file_id: 1, state: 5, ..Default::default() }).unwrap();
        assert_eq!((item["PublishedFileId"].as_u64(), item["State"].as_u64()), (Some(1), Some(5)));
    }
}
