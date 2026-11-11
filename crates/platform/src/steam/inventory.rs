//! Steam Inventory over `ISteamInventory`: the client's (items, definitions, prices, checkout) or the
//! game server's (checking players' serialised inventories).

use super::pump::{Calls, c_str_at, field};
use super::strings::c;
use crate::{Event, Inventory, InventoryEvent, ItemInstance, ResultHandle, SteamId};
use std::mem::offset_of;
use std::sync::{Arc, Mutex};
use steamworks::sys;

/// `k_SteamInventoryResultInvalid`.
const INVALID: ResultHandle = -1;

unsafe extern "C" {
    // Gotcha: returns an EResult that may be outside steamworks-sys's enum; see ffi.rs.
    #[link_name = "SteamAPI_ISteamInventory_GetResultStatus"]
    fn result_status(inv: *mut sys::ISteamInventory, result: ResultHandle) -> i32;
}

/// Prices and the checkout in flight, filled in by call results and callbacks.
#[derive(Default)]
struct Shop {
    currency: Option<String>,
    order: Option<u64>,
    checking_out: bool,
    succeeded: bool,
}

pub(crate) struct SteamInventory {
    inv: unsafe extern "C" fn() -> *mut sys::ISteamInventory,
    calls: Arc<Calls>,
    shop: Arc<Mutex<Shop>>,
    events: Arc<Mutex<Vec<Event>>>,
}

impl SteamInventory {
    /// The user's inventory: definitions load, and prices are asked for, at once.
    pub fn client(calls: Arc<Calls>, events: Arc<Mutex<Vec<Event>>>) -> Self {
        let inv = Self { inv: sys::SteamAPI_SteamInventory_v003, calls, shop: Arc::default(), events };
        // SAFETY: the client is initialised.
        unsafe { sys::SteamAPI_ISteamInventory_LoadItemDefinitions(inv.raw()) };
        inv.request_prices();
        inv
    }

    pub fn server(calls: Arc<Calls>) -> Self {
        Self { inv: sys::SteamAPI_SteamGameServerInventory_v003, calls, shop: Arc::default(), events: Arc::default() }
    }

    fn raw(&self) -> *mut sys::ISteamInventory {
        // SAFETY: the owner only hands this out while its side of Steam is initialised.
        unsafe { (self.inv)() }
    }

    fn request_prices(&self) {
        let (shop, events) = (self.shop.clone(), self.events.clone());
        // SAFETY: a live interface.
        let call = unsafe { sys::SteamAPI_ISteamInventory_RequestPrices(self.raw()) };
        self.calls.expect(call, move |b| {
            type R = sys::SteamInventoryRequestPricesResult_t;
            let success = b.is_some_and(|b| field!(b, R, m_result: i32) == 1);
            let currency = b.map(|b| c_str_at(b, offset_of!(R, m_rgchCurrency), 4)).unwrap_or_default();
            shop.lock().unwrap().currency = success.then(|| currency.clone());
            events.lock().unwrap().push(Event::Inventory(InventoryEvent::Prices { success, currency }));
        });
    }

    /// A microtransaction was authorised or declined: our checkout, if it's that order.
    pub fn authorized(&self, order: u64, ok: bool) {
        let mut shop = self.shop.lock().unwrap();
        if shop.checking_out && shop.order == Some(order) {
            (shop.checking_out, shop.succeeded) = (false, ok);
        }
    }

    /// Handle from `f(&mut handle)`, when it says it made one.
    fn handle(f: impl FnOnce(&mut ResultHandle) -> bool) -> Option<ResultHandle> {
        let mut h = INVALID;
        (f(&mut h) && h != INVALID).then_some(h)
    }
}

// SAFETY (every unsafe block below): `raw()` is live while `self` is offered, and each pointer is valid for its call.
impl Inventory for SteamInventory {
    fn all_items(&self) -> Option<ResultHandle> {
        Self::handle(|h| unsafe { sys::SteamAPI_ISteamInventory_GetAllItems(self.raw(), h) })
    }

    fn deserialize(&self, data: &[u8]) -> Option<ResultHandle> {
        Self::handle(|h| unsafe { sys::SteamAPI_ISteamInventory_DeserializeResult(self.raw(), h, data.as_ptr().cast(), data.len() as u32, false) })
    }

    fn status(&self, result: ResultHandle) -> i32 {
        unsafe { result_status(self.raw(), result) }
    }

    fn items(&self, result: ResultHandle) -> Vec<ItemInstance> {
        let mut n = 0u32;
        if !unsafe { sys::SteamAPI_ISteamInventory_GetResultItems(self.raw(), result, std::ptr::null_mut(), &mut n) } {
            return vec![];
        }
        let mut items = vec![sys::SteamItemDetails_t { m_itemId: 0, m_iDefinition: 0, m_unQuantity: 0, m_unFlags: 0 }; n as usize];
        if !unsafe { sys::SteamAPI_ISteamInventory_GetResultItems(self.raw(), result, items.as_mut_ptr(), &mut n) } {
            return vec![];
        }
        items.truncate(n as usize);
        items.iter().map(|i| ItemInstance { id: i.m_itemId, definition: i.m_iDefinition as u32 }).collect()
    }

    fn check_owner(&self, result: ResultHandle, user: SteamId) -> bool {
        unsafe { sys::SteamAPI_ISteamInventory_CheckResultSteamID(self.raw(), result, user.0) }
    }

    fn timestamp(&self, result: ResultHandle) -> u32 {
        unsafe { sys::SteamAPI_ISteamInventory_GetResultTimestamp(self.raw(), result) }
    }

    fn serialize(&self, result: ResultHandle) -> Vec<u8> {
        let mut n = 0u32;
        unsafe { sys::SteamAPI_ISteamInventory_SerializeResult(self.raw(), result, std::ptr::null_mut(), &mut n) };
        let mut data = vec![0u8; n as usize];
        let ok = n > 0 && unsafe { sys::SteamAPI_ISteamInventory_SerializeResult(self.raw(), result, data.as_mut_ptr().cast(), &mut n) };
        if ok { data } else { vec![] }
    }

    fn destroy(&self, result: ResultHandle) {
        unsafe { sys::SteamAPI_ISteamInventory_DestroyResult(self.raw(), result) }
    }

    fn definitions(&self) -> Vec<i32> {
        let mut n = 0u32;
        if !unsafe { sys::SteamAPI_ISteamInventory_GetItemDefinitionIDs(self.raw(), std::ptr::null_mut(), &mut n) } {
            return vec![];
        }
        let mut ids = vec![0i32; n as usize];
        let ok = unsafe { sys::SteamAPI_ISteamInventory_GetItemDefinitionIDs(self.raw(), ids.as_mut_ptr(), &mut n) };
        ids.truncate(if ok { n as usize } else { 0 });
        ids
    }

    fn definition_property(&self, definition: i32, name: &str) -> String {
        let name = (!name.is_empty()).then(|| c(name));
        let name = name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr());
        let mut n = 0u32;
        unsafe { sys::SteamAPI_ISteamInventory_GetItemDefinitionProperty(self.raw(), definition, name, std::ptr::null_mut(), &mut n) };
        let mut buf = vec![0 as std::ffi::c_char; n.max(1) as usize];
        if !unsafe { sys::SteamAPI_ISteamInventory_GetItemDefinitionProperty(self.raw(), definition, name, buf.as_mut_ptr(), &mut n) } {
            return String::new();
        }
        *buf.last_mut().unwrap() = 0;
        super::strings::owned(buf.as_ptr())
    }

    fn price(&self, definition: i32) -> Option<(u64, u64)> {
        let (mut price, mut base) = (0, 0);
        unsafe { sys::SteamAPI_ISteamInventory_GetItemPrice(self.raw(), definition, &mut price, &mut base) }.then_some((price, base))
    }

    fn has_prices(&self) -> bool {
        self.shop.lock().unwrap().currency.is_some()
    }

    fn currency(&self) -> String {
        self.shop.lock().unwrap().currency.clone().unwrap_or_default()
    }

    fn check_out(&self, definitions: &[i32]) {
        let quantities = vec![1u32; definitions.len()];
        let call = unsafe { sys::SteamAPI_ISteamInventory_StartPurchase(self.raw(), definitions.as_ptr(), quantities.as_ptr(), definitions.len() as u32) };
        {
            let mut s = self.shop.lock().unwrap();
            (s.order, s.checking_out, s.succeeded) = (None, true, false);
        }
        let (shop, events) = (self.shop.clone(), self.events.clone());
        let started = self.calls.expect(call, move |b| {
            type R = sys::SteamInventoryStartPurchaseResult_t;
            let ok = b.is_some_and(|b| field!(b, R, m_result: i32) == 1);
            let (order, transaction) = b.map_or((0, 0), |b| (field!(b, R, m_ulOrderID: u64), field!(b, R, m_ulTransID: u64)));
            let mut s = shop.lock().unwrap();
            // The overlay is open; the authorisation callback finishes it.
            if ok { s.order = Some(order) } else { (s.checking_out, s.succeeded) = (false, false) }
            events.lock().unwrap().push(Event::Inventory(InventoryEvent::Purchase { success: ok, order, transaction }));
        });
        if !started {
            self.shop.lock().unwrap().checking_out = false;
        }
    }

    fn checking_out(&self) -> bool {
        self.shop.lock().unwrap().checking_out
    }

    fn checkout_succeeded(&self) -> bool {
        self.shop.lock().unwrap().succeeded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_ends_on_its_own_authorisation() {
        let inv = SteamInventory::server(Arc::default());
        *inv.shop.lock().unwrap() = Shop { order: Some(5), checking_out: true, ..Shop::default() };
        inv.authorized(6, true);
        assert!(inv.checking_out(), "another order's answer");
        inv.authorized(5, true);
        assert!(!inv.checking_out() && inv.checkout_succeeded());
        assert!(!inv.has_prices() && inv.currency().is_empty());
    }
}
