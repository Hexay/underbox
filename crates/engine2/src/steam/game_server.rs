//! `ISteamGameServer` and the `SteamGameServer_*` globals: logon, server-browser info, auth sessions.

use super::interface;
use abi::slots::{global_steam as global, i_steam_game_server as gs};
use abi::structs::HAuthTicket;
use abi::{NativeRef, Registry, StrRet, bind, class};
use platform::{ServerInfo, SteamId};
use std::ffi::CStr;
use std::sync::OnceLock;

/// Managed's ticket buffer, as for the client's tickets.
const TICKET_CAPACITY: usize = 1024;

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn info(edit: impl FnOnce(&mut ServerInfo)) {
    let mut edit = Some(edit);
    platform::get().server_info(&mut |i| edit.take().unwrap()(i));
}

use platform::get as p;

pub fn register(r: &mut Registry) {
    static SERVER: OnceLock<usize> = OnceLock::new();
    bind!(r, global::STEAM_GAME_SERVER_INIT, |port, query, version| p().server_init(port as u16, query as u16, &text(version)));
    bind!(r, global::STEAM_GAME_SERVER, || -> NativeRef<class::ISteamGameServer> { interface(&SERVER) });
    bind!(r, global::STEAM_GAME_SERVER_GET_STEAM_ID, || p().server_id().0);
    // Secure (VAC) means Steam vouches for the server; only a real Steam logon can.
    bind!(r, global::STEAM_GAME_SERVER_B_SECURE, || false);
    bind!(r, global::STEAM_GAME_SERVER_SHUTDOWN, || p().server_shutdown());
    bind!(r, global::STEAM_GAME_SERVER_RELEASE_CURRENT_THREAD_MEMORY, || {});

    bind!(r, gs::LOG_ON, |_, token| p().server_log_on(Some(&text(token))));
    bind!(r, gs::LOG_ON_ANONYMOUS, |_| p().server_log_on(None));
    bind!(r, gs::LOG_OFF, |_| p().server_log_off());
    bind!(r, gs::B_LOGGED_ON, |_| p().server_logged_on());
    bind!(r, gs::GET_PUBLIC_IP, |_| StrRet::from(p().server_public_ip()));

    bind!(r, gs::SET_PRODUCT, |_, s| info(|i| i.product = text(s)));
    bind!(r, gs::SET_MOD_DIR, |_, s| info(|i| i.mod_dir = text(s)));
    bind!(r, gs::SET_GAME_DESCRIPTION, |_, s| info(|i| i.description = text(s)));
    bind!(r, gs::SET_SERVER_NAME, |_, s| info(|i| i.name = text(s)));
    bind!(r, gs::SET_MAP_NAME, |_, s| info(|i| i.map = text(s)));
    bind!(r, gs::SET_GAME_TAGS, |_, s| info(|i| i.tags = text(s)));
    bind!(r, gs::SET_MAX_PLAYER_COUNT, |_, n| info(|i| i.max_players = n));
    bind!(r, gs::SET_DEDICATED_SERVER, |_, b| info(|i| i.dedicated = b));
    bind!(r, gs::SET_PASSWORD_PROTECTED, |_, b| info(|i| i.password_protected = b));
    bind!(r, gs::SET_ADVERTISE_SERVER_ACTIVE, |_, b| info(|i| i.advertise = b));

    bind!(r, gs::BEGIN_AUTH_SESSION, |_, user, ticket, len| {
        // SAFETY: managed passes its ticket buffer and that buffer's length.
        let ticket = if ticket.is_null() || len <= 0 { &[][..] } else { unsafe { ticket.cast::<u8>().as_slice(len as usize) } };
        p().begin_auth(SteamId(user), ticket) as i64
    });
    bind!(r, gs::END_AUTH_SESSION, |_, user| p().end_auth(SteamId(user)));
    bind!(r, gs::GET_AUTH_SESSION_TICKET, |_, target, buffer, len| {
        let t = p().server_ticket(SteamId(target));
        let n = t.bytes.len().min(TICKET_CAPACITY);
        // SAFETY: managed passes a TICKET_CAPACITY-byte buffer.
        unsafe { buffer.cast::<u8>().as_mut_slice(n) }.copy_from_slice(&t.bytes[..n]);
        *len = n as u32;
        HAuthTicket { value: t.handle }
    });
    bind!(r, gs::CANCEL_AUTH_TICKET, |_, h| p().cancel_server_ticket(h.value));
}
