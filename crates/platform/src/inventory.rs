//! Steam Inventory: the user's items, item definitions and their prices, checkout, and serialised
//! inventory proofs a server checks. Results are polled; definitions, prices and purchases also
//! arrive as [`crate::InventoryEvent`]s.

use crate::SteamId;

/// Steam's `SteamInventoryResult_t`.
pub type ResultHandle = i32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemInstance {
    pub id: u64,
    pub definition: u32,
}

pub trait Inventory: Send + Sync {
    fn all_items(&self) -> Option<ResultHandle>;
    fn deserialize(&self, data: &[u8]) -> Option<ResultHandle>;
    /// Steam's `EResult` for the result: 22 (Pending) until it's ready, 1 when it's good.
    fn status(&self, result: ResultHandle) -> i32;
    fn items(&self, result: ResultHandle) -> Vec<ItemInstance>;
    fn check_owner(&self, result: ResultHandle, user: SteamId) -> bool;
    fn timestamp(&self, result: ResultHandle) -> u32;
    fn serialize(&self, result: ResultHandle) -> Vec<u8>;
    fn destroy(&self, result: ResultHandle);

    fn definitions(&self) -> Vec<i32>;
    /// `name` "" lists the property names, comma-separated.
    fn definition_property(&self, definition: i32, name: &str) -> String;
    /// `(price, base price)` in the currency's smallest unit, once prices have arrived.
    fn price(&self, definition: i32) -> Option<(u64, u64)>;
    fn has_prices(&self) -> bool;
    fn currency(&self) -> String;

    /// Open Steam's checkout for one of each of `definitions`.
    fn check_out(&self, definitions: &[i32]);
    fn checking_out(&self) -> bool;
    fn checkout_succeeded(&self) -> bool;
}

/// `EResult::k_EResultPending`.
pub const PENDING: i32 = 22;
