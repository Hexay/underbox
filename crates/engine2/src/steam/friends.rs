//! `ISteamFriends` without a Steam client. Unlike the other client interfaces it can't be null:
//! the game instance sets rich presence every frame without checking (SteamRichPresenceSystem).
//! The offline user has a fixed name, no profile items, and rich presence goes nowhere.

use abi::slots::{global_steam as global, i_steam_friends as friends};
use abi::{Registry, StrRet, bind, class};
use std::sync::OnceLock;

const PERSONA: &std::ffi::CStr = c"Player";

pub fn register(r: &mut Registry) {
    static FRIENDS: OnceLock<usize> = OnceLock::new();
    bind!(r, global::STEAM_FRIENDS, || super::interface::<class::ISteamFriends>(&FRIENDS));
    bind!(r, friends::GET_PERSONA_NAME, |_| StrRet::from(PERSONA));
    bind!(r, friends::GET_PROFILE_ITEM_PROPERTY_STRING, |_, _, _, _| StrRet::from(c""));
    bind!(r, friends::REQUEST_EQUIPPED_PROFILE_ITEMS, |_, _| 0);
    bind!(r, friends::SET_RICH_PRESENCE, |_, _, _| true);
    bind!(r, friends::CLEAR_RICH_PRESENCE, |_| {});
}
