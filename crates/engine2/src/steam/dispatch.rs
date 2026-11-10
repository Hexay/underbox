//! `SteamAPI_RunCallbacks`/`SteamGameServer_RunCallbacks`: platform events become Steam callback
//! structs handed to managed's `Steamworks.Dispatch`, laid out as managed's `[StructLayout]` reads them.

use abi::callbacks::steamworks_dispatch::on_client_callback;
use abi::slots::global_steam as global;
use abi::{RawPtr, Registry, bind};
use platform::Event;

const SERVERS_CONNECTED: i32 = 101;
const VALIDATE_AUTH_TICKET_RESPONSE: i32 = 143;
const AUTH_OK: i32 = 0;
const AUTH_TICKET_INVALID: i32 = 8;

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

fn encode(e: &Event) -> (i32, Vec<u8>) {
    match *e {
        Event::ServerConnected => (SERVERS_CONNECTED, Packed::default().finish()),
        Event::AuthResult { user, owner, ok } => {
            let response = if ok { AUTH_OK } else { AUTH_TICKET_INVALID };
            let data = Packed::default().field(&user.0.to_le_bytes()).field(&response.to_le_bytes()).field(&owner.0.to_le_bytes());
            (VALIDATE_AUTH_TICKET_RESPONSE, data.finish())
        }
    }
}

/// Deliver pending platform events; runs managed code, so no engine lock may be held.
fn run_server_callbacks() {
    for e in platform::get().poll() {
        let (kind, mut data) = encode(&e);
        on_client_callback(kind, RawPtr::from_raw(data.as_mut_ptr().cast()), data.len() as i32, true);
    }
}

pub fn register(r: &mut Registry) {
    bind!(r, global::STEAM_GAME_SERVER_RUN_CALLBACKS, run_server_callbacks);
    // Client callbacks come from a Steam client, which the offline platform has none of.
    bind!(r, global::STEAM_API_RUN_CALLBACKS, || {});
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::SteamId;

    #[test]
    fn auth_response_matches_managed_layout() {
        let (kind, data) = encode(&Event::AuthResult { user: SteamId(1), owner: SteamId(2), ok: true });
        assert_eq!(kind, 143);
        // ulong, AuthResponse (int), ulong: Pack=4 gives 0/8/12 (20 bytes), Pack=8 gives 0/8/16 (24).
        let owner_at = if cfg!(windows) { 16 } else { 12 };
        assert_eq!(data.len(), owner_at + 8);
        assert_eq!(data[owner_at], 2);
        assert_eq!(encode(&Event::ServerConnected).1, [0]);
    }
}
