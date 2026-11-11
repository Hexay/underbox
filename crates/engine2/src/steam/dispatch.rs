//! `SteamAPI_RunCallbacks`/`SteamGameServer_RunCallbacks`: platform events become Steam callback
//! structs handed to managed's `Steamworks.Dispatch`, laid out as managed's `[StructLayout]` reads them,
//! or calls into managed's `LobbyManager` and `Services.Inventory`.

use abi::RawPtr;
use abi::callbacks::steamworks_dispatch::on_client_callback;
use abi::callbacks::{sandbox_lobby_manager as lobby, sandbox_services_inventory as inventory};
use abi::slots::global_steam as global;
use abi::{Registry, bind};
use platform::{Event, InventoryEvent, LobbyEvent};

const SERVERS_CONNECTED: i32 = 101;
const SERVER_CONNECT_FAILURE: i32 = 102;
const SERVERS_DISCONNECTED: i32 = 103;
const VALIDATE_AUTH_TICKET_RESPONSE: i32 = 143;

/// Managed's `Platform.StructPlatformPackSize`: Steam packs callback structs to 4 on Linux/macOS, 8 on Windows.
const PACK: usize = if cfg!(windows) { 8 } else { 4 };

/// A callback's struct bytes, laid out field by field with `PACK` alignment.
#[derive(Default)]
struct Packed {
    bytes: Vec<u8>,
    align: usize,
}

impl Packed {
    fn field(mut self, bytes: &[u8]) -> Self {
        let align = bytes.len().min(PACK);
        self.bytes.resize(self.bytes.len().next_multiple_of(align), 0);
        self.bytes.extend_from_slice(bytes);
        self.align = self.align.max(align);
        self
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bytes.is_empty() {
            // C# gives an empty struct size 1.
            return vec![0];
        }
        self.bytes.resize(self.bytes.len().next_multiple_of(self.align), 0);
        self.bytes
    }
}

/// The Steam callback an event stands for; `None` for the ones that aren't Steam callbacks.
fn encode(e: &Event) -> Option<(i32, Vec<u8>)> {
    Some(match *e {
        Event::ServerConnected => (SERVERS_CONNECTED, Packed::default().finish()),
        // Managed marshals StillRetrying as I1: one byte.
        Event::ServerConnectFailure { result, retrying } => {
            (SERVER_CONNECT_FAILURE, Packed::default().field(&result.to_le_bytes()).field(&[retrying as u8]).finish())
        }
        Event::ServersDisconnected { result } => (SERVERS_DISCONNECTED, Packed::default().field(&result.to_le_bytes()).finish()),
        Event::AuthResult { user, owner, response } => {
            let response = response as i32;
            let data = Packed::default().field(&user.0.to_le_bytes()).field(&response.to_le_bytes()).field(&owner.0.to_le_bytes());
            (VALIDATE_AUTH_TICKET_RESPONSE, data.finish())
        }
        Event::Lobby(_) | Event::Inventory(_) => return None,
    })
}

fn deliver_lobby(e: &LobbyEvent) {
    match e {
        LobbyEvent::Invite { lobby, from } => lobby::on_lobby_invite(lobby.0, from.0),
        LobbyEvent::Created { lobby } => lobby::on_created(lobby.0),
        LobbyEvent::Entered { lobby } => lobby::on_entered(lobby.0),
        LobbyEvent::MemberEntered { lobby, member } => lobby::on_member_entered(lobby.0, member.0),
        LobbyEvent::MemberLeft { lobby, member } => lobby::on_member_leave(lobby.0, member.0),
        LobbyEvent::DataChanged { lobby, target } => lobby::on_data_update(lobby.0, target.0),
        // Managed only reads the message; the pointer is mutable because the callback's signature says so.
        LobbyEvent::Chat { lobby, member, data } => {
            lobby::on_chat_message(lobby.0, member.0, RawPtr::from_raw(data.as_ptr().cast_mut().cast()), data.len() as i32)
        }
    }
}

fn deliver_inventory(e: &InventoryEvent) {
    match e {
        InventoryEvent::DefinitionsUpdated => inventory::on_definition_update(),
        InventoryEvent::Prices { success, currency } => {
            let currency = std::ffi::CString::new(currency.as_str()).unwrap_or_default();
            inventory::on_prices_update(*success, Some(&currency))
        }
        InventoryEvent::Purchase { success, order, transaction } => inventory::on_purchase_result(*success, *order, *transaction),
    }
}

fn deliver(e: &Event, is_server: bool) {
    match e {
        Event::Lobby(l) => deliver_lobby(l),
        Event::Inventory(i) => deliver_inventory(i),
        _ => {
            let Some((kind, data)) = encode(e) else { return };
            deliver_raw(kind, &data, is_server);
        }
    }
}

/// Managed only reads the struct; the pointer is mutable because the callback's signature says so.
fn deliver_raw(kind: i32, data: &[u8], is_server: bool) {
    on_client_callback(kind, RawPtr::from_raw(data.as_ptr().cast_mut().cast()), data.len() as i32, is_server);
}

/// Deliver pending platform events, then the backend's own callbacks verbatim (managed decodes
/// those); runs managed code, so no engine lock may be held.
fn run_server_callbacks() {
    platform::get().run_callbacks(&mut |e| deliver(&e, true), &mut |kind, data| deliver_raw(kind, data, true));
}

fn run_client_callbacks() {
    let Some(client) = super::client::local() else { return };
    client.run_callbacks(&mut |e| deliver(&e, false), &mut |kind, data| deliver_raw(kind, data, false));
    super::server_list::poll();
}

pub fn register(r: &mut Registry) {
    bind!(r, global::STEAM_GAME_SERVER_RUN_CALLBACKS, run_server_callbacks);
    bind!(r, global::STEAM_API_RUN_CALLBACKS, run_client_callbacks);
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::{AuthResponse, SteamId};

    #[test]
    fn auth_response_matches_managed_layout() {
        let (kind, data) = encode(&Event::AuthResult { user: SteamId(1), owner: SteamId(2), response: AuthResponse::VacBanned }).unwrap();
        assert_eq!(kind, 143);
        // ulong, AuthResponse (int), ulong: Pack=4 gives 0/8/12 (20 bytes), Pack=8 gives 0/8/16 (24).
        let owner_at = if cfg!(windows) { 16 } else { 12 };
        assert_eq!(data.len(), owner_at + 8);
        assert_eq!((data[8], data[owner_at]), (3, 2));
        assert_eq!(encode(&Event::ServerConnected).unwrap().1, [0]);
        // EResult (int) + I1 bool, padded to the int's alignment: 8 bytes on both packs.
        assert_eq!(encode(&Event::ServerConnectFailure { result: 3, retrying: true }), Some((102, vec![3, 0, 0, 0, 1, 0, 0, 0])));
        assert_eq!(encode(&Event::ServersDisconnected { result: 3 }), Some((103, vec![3, 0, 0, 0])));
        assert_eq!(encode(&Event::Lobby(LobbyEvent::Entered { lobby: SteamId(1) })), None);
    }
}
