//! Steam services no platform backend offers yet: the HTML surface (Steam's embedded Chromium),
//! lobbies, peer-to-peer messages, relay utilities, the inventory, screenshots and the server browser.
//! Their accessors return null, or their objects report "nothing" at once, which managed handles
//! (WebSurface logs "unavailable", inventory and votes come back empty, server lists finish empty).
//! The methods behind them answer as an idle, empty service if called anyway.
// TODO: Steam-backend passthroughs; managed already decodes their callbacks from the raw pump.

use abi::callbacks::sandbox_services_server_list as server_list;
use abi::slots::{
    c_server_list as sl, c_steam_inventory_result as inv_result, c_steam_item_instance as item, global_steam as global,
    i_steam_html_surface as html, i_steam_matchmaking as mm, i_steam_networking_messages as msgs, i_steam_networking_utils as nu,
    steam_inventory as inv, steam_screenshots as shots,
};
use abi::structs::SteamNetConnectionRealTimeStatus_t;
use abi::{ManagedObject, NativeRef, RawPtr, Registry, StrRet, bind, class};

/// `EResult::k_EResultNoConnection`: what sending to a peer gives with no network session.
const NO_CONNECTION: i32 = 3;

/// A server-list query: the managed `ServerList` it reports to.
struct ServerList(ManagedObject);

fn server_list_query(l: NativeRef<class::CServerList>) {
    if let Some(list) = objects::get::<_, ServerList>(l) {
        server_list::on_started(list.0);
        server_list::on_finished(list.0);
    }
}

fn html(r: &mut Registry) {
    bind!(r, global::STEAM_HTML_SURFACE, NativeRef::null);
    bind!(r, html::INIT, |_| false);
    bind!(r, html::SHUTDOWN, |_| true);
    // k_uAPICallInvalid: no browser is coming.
    bind!(r, html::CREATE_BROWSER, |_, _, _| 0);
    bind!(r, html::REMOVE_BROWSER, |_, _| {});
    bind!(r, html::LOAD_URL, |_, _, _, _| {});
    bind!(r, html::ADD_HEADER, |_, _, _, _| {});
    bind!(r, html::EXECUTE_JAVASCRIPT, |_, _, _| {});
    bind!(r, html::SET_SIZE, |_, _, _, _| {});
    bind!(r, html::GET_LINK_AT_POSITION, |_, _, _, _| {});
    bind!(r, html::SET_HORIZONTAL_SCROLL, |_, _, _| {});
    bind!(r, html::SET_VERTICAL_SCROLL, |_, _, _| {});
    bind!(r, html::SET_KEY_FOCUS, |_, _, _| {});
    bind!(r, html::ALLOW_START_REQUEST, |_, _, _| {});
    bind!(r, html::JS_DIALOG_RESPONSE, |_, _, _| {});
    bind!(r, html::SET_BACKGROUND_MODE, |_, _, _| {});
    bind!(r, html::SET_DPI_SCALING_FACTOR, |_, _, _| {});
    bind!(r, html::KEY_DOWN, |_, _, _, _, _| {});
    bind!(r, html::KEY_UP, |_, _, _, _| {});
    bind!(r, html::KEY_CHAR, |_, _, _, _| {});
    bind!(r, html::MOUSE_UP, |_, _, _| {});
    bind!(r, html::MOUSE_DOWN, |_, _, _| {});
    bind!(r, html::MOUSE_DOUBLE_CLICK, |_, _, _| {});
    bind!(r, html::MOUSE_MOVE, |_, _, _, _| {});
    bind!(r, html::MOUSE_WHEEL, |_, _, _| {});
    bind!(r, html::SET_COOKIE, |_, _, _, _, _, _, _, _| {});
}

fn lobbies_and_peers(r: &mut Registry) {
    bind!(r, global::STEAM_MATCHMAKING, NativeRef::null);
    bind!(r, mm::LEAVE_LOBBY, |_, _| {});
    bind!(r, mm::GET_NUM_LOBBY_MEMBERS, |_, _| 0);
    bind!(r, mm::GET_LOBBY_MEMBER_BY_INDEX, |_, _, _| 0);
    // Steam returns "" (never null) for a missing key.
    bind!(r, mm::GET_LOBBY_DATA, |_, _, _| RawPtr::from_raw(c"".as_ptr().cast_mut().cast()));
    bind!(r, mm::SET_LOBBY_DATA, |_, _, _, _| false);
    bind!(r, mm::GET_LOBBY_DATA_COUNT, |_, _| 0);
    bind!(r, mm::DELETE_LOBBY_DATA, |_, _, _| {});
    bind!(r, mm::GET_LOBBY_DATA_BY_INDEX, |_, _, _, _, _, _, _| false);

    bind!(r, global::STEAM_NETWORKING_MESSAGES, NativeRef::null);
    // k_ESteamNetworkingConnectionState_None.
    bind!(r, msgs::GET_SESSION_STATE, |_, _, _, _| 0);
    bind!(r, msgs::SEND_MESSAGE_TO_USER, |_, _, _, _, _, _| NO_CONNECTION);
    bind!(r, msgs::RECEIVE_MESSAGES_ON_CHANNEL, |_, _, _, _| 0);
    bind!(r, msgs::ACCEPT_SESSION_WITH_USER, |_, _| false);
    bind!(r, msgs::CLOSE_CHANNEL_WITH_USER, |_, _, _| false);
    bind!(r, msgs::RELEASE_MESSAGE, |_, _| {});
    bind!(r, msgs::GET_CONNECTION_INFO, |_, _| SteamNetConnectionRealTimeStatus_t::default());

    // Without it managed skips relay bootstrap and fake-lag settings.
    bind!(r, global::STEAM_NETWORKING_UTILS, NativeRef::null);
    bind!(r, nu::SET_CONFIG, |_, _, _| {});
    bind!(r, nu::SET_CONFIG_1, |_, _, _| {});
    bind!(r, nu::INITIALIZE_RELAY_NETWORK, |_| {});
    bind!(r, nu::GET_LOCAL_PING_LOCATION_STRING, |_| StrRet::from(c""));
    // Negative: no ping data.
    bind!(r, nu::ESTIMATE_PING_FROM_LOCATION_STRING, |_, _| -1);
}

fn inventory(r: &mut Registry) {
    bind!(r, inv::GET_ALL_ITEMS, NativeRef::null);
    bind!(r, inv::DESERIALIZE, |_| NativeRef::null());
    bind!(r, inv::DEFINITION_COUNT, || 0);
    bind!(r, inv::GET_DEFINITION_ID, |_| 0);
    bind!(r, inv::GET_DEFINITION_PROPERTY, |_, _| StrRet::from(c""));
    bind!(r, inv::GET_DEFINITION_PRICE, |_, price, base| {
        (*price, *base) = (0, 0);
        false
    });
    bind!(r, inv::HAS_PRICES, || false);
    bind!(r, inv::GET_CURRENCY, || StrRet::from(c""));
    bind!(r, inv::CHECK_OUT, |_| {});
    bind!(r, inv::IS_CHECKING_OUT, || false);
    bind!(r, inv::WAS_CHECKOUT_SUCCESSFUL, || false);
    // No result object is ever handed out, so these only see null.
    bind!(r, inv_result::DESTROY, |_| {});
    bind!(r, inv_result::IS_PENDING, |_| false);
    bind!(r, inv_result::IS_OK, |_| false);
    bind!(r, inv_result::CHECK_STEAM_ID, |_, _| false);
    bind!(r, inv_result::GET_TIMESTAMP, |_| 0);
    bind!(r, inv_result::COUNT, |_| 0);
    bind!(r, inv_result::GET, |_, _| NativeRef::null());
    bind!(r, inv_result::GET_SERIALIZED_SIZE, |_| 0);
    bind!(r, inv_result::SERIALIZE, |_, _| false);
    bind!(r, item::ITEM_ID, |_| 0);
    bind!(r, item::DEFINITION_ID, |_| 0);
}

pub fn register(r: &mut Registry) {
    html(r);
    lobbies_and_peers(r);
    inventory(r);
    bind!(r, shots::WRITE_SCREENSHOT, |_, _, _, _| false);

    bind!(r, sl::CREATE, |owner| objects::create(ServerList(owner)));
    bind!(r, sl::DESTROY, |l| {
        objects::destroy(l);
    });
    bind!(r, sl::ADD_FILTER, |_, _, _| {});
    bind!(r, sl::START_QUERY, server_list_query);
    bind!(r, sl::START_LAN_QUERY, server_list_query);
}
