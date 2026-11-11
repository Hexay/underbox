//! `Steam::Inventory` and its result objects over [`platform::Inventory`], plus screenshots. A
//! dedicated server checks players' serialised inventories through the game server's inventory.
//! Without one (offline), no result object is handed out, so inventories come back empty.

use abi::slots::{c_steam_inventory_result as res, c_steam_item_instance as item, steam_inventory as inv, steam_screenshots as shots};
use abi::{NativeRef, Registry, StrRet, bind, class};
use platform::{Inventory, ItemInstance, PENDING, ResultHandle, SteamId};
use std::ffi::CStr;
use std::sync::Mutex;

type Result_ = NativeRef<class::CSteamInventoryResult>;
type Instance = NativeRef<class::CSteamItemInstance>;

fn inventory() -> Option<&'static dyn Inventory> {
    if crate::app::dict::current_config().dedicated_server {
        platform::get().server_inventory()
    } else {
        super::client::local()?.inventory()
    }
}

fn on<R: Default>(f: impl FnOnce(&dyn Inventory) -> R) -> R {
    inventory().map(f).unwrap_or_default()
}

/// A result handle, and the item instances handed out from it, which live until it's destroyed.
struct Handle(ResultHandle, Mutex<Vec<Instance>>);

fn wrap(h: Option<ResultHandle>) -> Result_ {
    h.map_or(NativeRef::null(), |h| objects::create(Handle(h, Mutex::default())))
}

fn with<R: Default>(r: Result_, f: impl FnOnce(&dyn Inventory, ResultHandle) -> R) -> R {
    let Some(h) = objects::get::<_, Handle>(r) else { return R::default() };
    on(|i| f(i, h.0))
}

fn get(r: Result_, index: i32) -> Instance {
    let Some(h) = objects::get::<_, Handle>(r) else { return NativeRef::null() };
    let Some(found) = on(|i| i.items(h.0).get(index.max(0) as usize).copied()) else { return NativeRef::null() };
    let it: Instance = objects::create(found);
    h.1.lock().unwrap().push(it);
    it
}

fn destroy(r: Result_) {
    if let Some(h) = objects::get::<_, Handle>(r) {
        on(|i| i.destroy(h.0));
        h.1.lock().unwrap().drain(..).for_each(|it| {
            objects::destroy(it);
        });
    }
    objects::destroy(r);
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn price(definition: i32, price: &mut u64, base: &mut u64) -> bool {
    let got = on(|i| i.price(definition));
    (*price, *base) = got.unwrap_or_default();
    got.is_some()
}

pub fn register(r: &mut Registry) {
    bind!(r, inv::GET_ALL_ITEMS, || wrap(on(|i| i.all_items())));
    bind!(r, inv::DESERIALIZE, |data| wrap(on(|i| i.deserialize(data))));
    bind!(r, inv::DEFINITION_COUNT, || on(|i| i.definitions().len() as i32));
    bind!(r, inv::GET_DEFINITION_ID, |index| on(|i| i.definitions().get(index.max(0) as usize).copied().unwrap_or(0)));
    bind!(r, inv::GET_DEFINITION_PROPERTY, |def, name| StrRet::from(on(|i| i.definition_property(def, &text(name)))));
    bind!(r, inv::GET_DEFINITION_PRICE, price);
    bind!(r, inv::HAS_PRICES, || on(|i| i.has_prices()));
    bind!(r, inv::GET_CURRENCY, || StrRet::from(on(|i| i.currency())));
    bind!(r, inv::CHECK_OUT, |defs| on(|i| i.check_out(defs)));
    bind!(r, inv::IS_CHECKING_OUT, || on(|i| i.checking_out()));
    bind!(r, inv::WAS_CHECKOUT_SUCCESSFUL, || on(|i| i.checkout_succeeded()));

    bind!(r, res::DESTROY, destroy);
    bind!(r, res::IS_PENDING, |r| with(r, |i, h| i.status(h) == PENDING));
    bind!(r, res::IS_OK, |r| with(r, |i, h| i.status(h) == 1));
    bind!(r, res::CHECK_STEAM_ID, |r, id| with(r, |i, h| i.check_owner(h, SteamId(id))));
    bind!(r, res::GET_TIMESTAMP, |r| with(r, |i, h| i.timestamp(h)));
    bind!(r, res::COUNT, |r| with(r, |i, h| i.items(h).len() as i32));
    bind!(r, res::GET, get);
    bind!(r, res::GET_SERIALIZED_SIZE, |r| with(r, |i, h| i.serialize(h).len() as u32));
    bind!(r, res::SERIALIZE, |r, out| {
        let data = with(r, |i, h| i.serialize(h));
        let fits = !data.is_empty() && data.len() <= out.len();
        if fits {
            out[..data.len()].copy_from_slice(&data);
        }
        fits
    });
    bind!(r, item::ITEM_ID, |it: Instance| objects::get::<_, ItemInstance>(it).map_or(0, |i| i.id));
    bind!(r, item::DEFINITION_ID, |it: Instance| objects::get::<_, ItemInstance>(it).map_or(0, |i| i.definition));

    bind!(r, shots::WRITE_SCREENSHOT, |rgb, len, w, h| {
        // SAFETY: managed passes its RGB buffer and that buffer's length.
        let rgb = unsafe { rgb.cast::<u8>().as_slice(len as usize) };
        super::client::local().is_some_and(|c| c.write_screenshot(rgb, w, h))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_inventory_is_empty() {
        assert!(inventory().is_none());
        assert!(wrap(on(|i| i.all_items())).is_null());
        let (mut p, mut b) = (1, 1);
        assert!(!price(5, &mut p, &mut b) && (p, b) == (0, 0));
    }
}
