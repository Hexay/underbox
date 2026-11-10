//! The local user's side of a platform: identity, auth tickets, social, app ownership and cloud
//! files. Only a game client asks for it; a dedicated server is [`crate::Platform`]'s server side.

use crate::{BeginAuth, Event, SteamId};

/// An auth ticket for a server to validate, and the handle that cancels it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ticket {
    pub handle: u32,
    pub bytes: Vec<u8>,
}

pub trait Client: Send + Sync {
    fn user(&self) -> SteamId;
    fn logged_on(&self) -> bool;

    /// A ticket proving who we are to `target` (a server; 0 for anyone).
    fn ticket(&self, target: SteamId) -> Ticket;
    fn cancel_ticket(&self, handle: u32);
    /// Validate a peer's ticket (a listen server's host); the verdict arrives through [`Client::run_callbacks`].
    fn begin_auth(&self, user: SteamId, ticket: &[u8]) -> BeginAuth;
    fn end_auth(&self, user: SteamId);

    /// Social services; `None` makes managed skip friends, names and presence.
    fn friends(&self) -> Option<&dyn Friends> {
        None
    }
    fn apps(&self) -> Option<&dyn Apps> {
        None
    }
    fn storage(&self) -> Option<&dyn CloudStorage> {
        None
    }
    fn text_filter(&self) -> Option<&dyn TextFilter> {
        None
    }

    /// Deliver pending events, then any backend-native callbacks verbatim (`raw(kind, bytes)`, Steam's
    /// callback id and struct). Each raw callback is valid only during its call.
    fn run_callbacks(&self, event: &mut dyn FnMut(Event), raw: &mut dyn FnMut(i32, &[u8]));
}

pub trait Friends: Send + Sync {
    fn persona_name(&self) -> String;
    /// `None` removes the key.
    fn set_rich_presence(&self, key: &str, value: Option<&str>) -> bool;
    fn clear_rich_presence(&self);
    /// Ask for `user`'s equipped profile items; returns the async call id its completion names.
    fn request_profile_items(&self, user: SteamId) -> u64;
    fn profile_item_property(&self, user: SteamId, kind: u32, property: u32) -> String;
}

/// Ownership and install state of apps (the game and anything it mounts).
pub trait Apps: Send + Sync {
    fn is_subscribed(&self) -> bool;
    fn is_subscribed_app(&self, app: u32) -> bool;
    fn is_app_installed(&self, app: u32) -> bool;
    fn is_dlc_installed(&self, app: u32) -> bool;
    fn is_low_violence(&self) -> bool;
    fn is_cybercafe(&self) -> bool;
    fn is_vac_banned(&self) -> bool;
    fn build_id(&self) -> i32;
    /// Comma-separated, as Steam gives them.
    fn available_languages(&self) -> String;
    fn current_language(&self) -> String;
    fn launch_command_line(&self) -> String;
    fn install_dir(&self, app: u32) -> String;
}

/// The user's per-game cloud files.
pub trait CloudStorage: Send + Sync {
    fn exists(&self, name: &str) -> bool;
    fn size(&self, name: &str) -> Option<usize>;
    fn read(&self, name: &str) -> Option<Vec<u8>>;
    fn write(&self, name: &str, data: &[u8]) -> bool;
    /// Returns the async call id its completion names.
    fn write_async(&self, name: &str, data: &[u8]) -> u64;
    fn delete(&self, name: &str) -> bool;
}

pub trait TextFilter: Send + Sync {
    fn init(&self, options: u32) -> bool;
    /// `input` with filtered characters replaced; the second value is how many were.
    fn filter(&self, context: u32, source: SteamId, input: &str) -> (String, i32);
}
