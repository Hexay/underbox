//! Steam callback structs the platform reports itself, decoded from their raw bytes.

use super::pump::{field, read_at};
use crate::{AuthResponse, Event, LobbyEvent, SteamId};
use std::mem::offset_of;
use steamworks::sys;

/// The callback ids we decode (`<struct>_k_iCallback`).
pub(crate) mod ids {
    use steamworks::sys;
    pub const SERVERS_CONNECTED: i32 = sys::SteamServersConnected_t_k_iCallback as i32;
    pub const SERVER_CONNECT_FAILURE: i32 = sys::SteamServerConnectFailure_t_k_iCallback as i32;
    pub const SERVERS_DISCONNECTED: i32 = sys::SteamServersDisconnected_t_k_iCallback as i32;
    pub const VALIDATE_AUTH_TICKET: i32 = sys::ValidateAuthTicketResponse_t_k_iCallback as i32;
    pub const LOBBY_INVITE: i32 = sys::LobbyInvite_t_k_iCallback as i32;
    pub const LOBBY_ENTER: i32 = sys::LobbyEnter_t_k_iCallback as i32;
    pub const LOBBY_DATA_UPDATE: i32 = sys::LobbyDataUpdate_t_k_iCallback as i32;
    pub const LOBBY_CHAT_UPDATE: i32 = sys::LobbyChatUpdate_t_k_iCallback as i32;
    pub const LOBBY_CHAT_MSG: i32 = sys::LobbyChatMsg_t_k_iCallback as i32;
    pub const LOBBY_CREATED: i32 = sys::LobbyCreated_t_k_iCallback as i32;
    pub const SESSION_REQUEST: i32 = sys::SteamNetworkingMessagesSessionRequest_t_k_iCallback as i32;
    pub const WEB_TICKET: i32 = sys::GetTicketForWebApiResponse_t_k_iCallback as i32;
    pub const INVENTORY_DEFINITIONS: i32 = sys::SteamInventoryDefinitionUpdate_t_k_iCallback as i32;
    pub const MICRO_TXN: i32 = sys::MicroTxnAuthorizationResponse_t_k_iCallback as i32;
}

/// `EAuthSessionResponse` → ours; unknown codes are an invalid ticket.
fn auth_response(code: i32) -> AuthResponse {
    use AuthResponse::*;
    const ALL: [AuthResponse; 11] = [
        Ok,
        UserNotConnectedToSteam,
        NoLicenseOrExpired,
        VacBanned,
        LoggedInElsewhere,
        VacCheckTimedOut,
        TicketCanceled,
        TicketAlreadyUsed,
        TicketInvalid,
        PublisherIssuedBan,
        NetworkIdentityFailure,
    ];
    ALL.into_iter().find(|r| *r as i32 == code).unwrap_or(TicketInvalid)
}

/// A game-server callback the [`crate::Platform`] reports as an [`Event`] (the rest go to managed raw).
pub(crate) fn server_event(kind: i32, b: &[u8]) -> Option<Event> {
    Some(match kind {
        ids::SERVERS_CONNECTED => Event::ServerConnected,
        ids::SERVER_CONNECT_FAILURE => Event::ServerConnectFailure {
            result: field!(b, sys::SteamServerConnectFailure_t, m_eResult: i32),
            retrying: field!(b, sys::SteamServerConnectFailure_t, m_bStillRetrying: bool),
        },
        ids::SERVERS_DISCONNECTED => Event::ServersDisconnected { result: field!(b, sys::SteamServersDisconnected_t, m_eResult: i32) },
        ids::VALIDATE_AUTH_TICKET => Event::AuthResult {
            user: SteamId(field!(b, sys::ValidateAuthTicketResponse_t, m_SteamID: u64)),
            owner: SteamId(field!(b, sys::ValidateAuthTicketResponse_t, m_OwnerSteamID: u64)),
            response: auth_response(field!(b, sys::ValidateAuthTicketResponse_t, m_eAuthSessionResponse: i32)),
        },
        _ => return None,
    })
}

/// `k_EChatMemberStateChangeEntered`; every other change is someone leaving.
const ENTERED: u32 = 1;
/// `k_EChatRoomEnterResponseSuccess`.
const ENTER_OK: u32 = 1;

/// A lobby callback as managed's `LobbyManager` hears it. Chat needs fetching: see [`chat_message`].
pub(crate) fn lobby_event(kind: i32, b: &[u8]) -> Option<LobbyEvent> {
    let id = |at| SteamId(read_at::<u64>(b, at));
    match kind {
        ids::LOBBY_INVITE => Some(LobbyEvent::Invite {
            lobby: id(offset_of!(sys::LobbyInvite_t, m_ulSteamIDLobby)),
            from: id(offset_of!(sys::LobbyInvite_t, m_ulSteamIDUser)),
        }),
        ids::LOBBY_CREATED => (field!(b, sys::LobbyCreated_t, m_eResult: i32) == 1)
            .then(|| LobbyEvent::Created { lobby: id(offset_of!(sys::LobbyCreated_t, m_ulSteamIDLobby)) }),
        ids::LOBBY_ENTER => (field!(b, sys::LobbyEnter_t, m_EChatRoomEnterResponse: u32) == ENTER_OK)
            .then(|| LobbyEvent::Entered { lobby: id(offset_of!(sys::LobbyEnter_t, m_ulSteamIDLobby)) }),
        ids::LOBBY_DATA_UPDATE => (field!(b, sys::LobbyDataUpdate_t, m_bSuccess: u8) != 0).then(|| LobbyEvent::DataChanged {
            lobby: id(offset_of!(sys::LobbyDataUpdate_t, m_ulSteamIDLobby)),
            target: id(offset_of!(sys::LobbyDataUpdate_t, m_ulSteamIDMember)),
        }),
        ids::LOBBY_CHAT_UPDATE => {
            let lobby = id(offset_of!(sys::LobbyChatUpdate_t, m_ulSteamIDLobby));
            let member = id(offset_of!(sys::LobbyChatUpdate_t, m_ulSteamIDUserChanged));
            let entered = field!(b, sys::LobbyChatUpdate_t, m_rgfChatMemberStateChange: u32) & ENTERED != 0;
            Some(if entered { LobbyEvent::MemberEntered { lobby, member } } else { LobbyEvent::MemberLeft { lobby, member } })
        }
        _ => None,
    }
}

/// A chat message's lobby and entry id, to fetch with `GetLobbyChatEntry`.
pub(crate) fn chat_message(kind: i32, b: &[u8]) -> Option<(SteamId, i32)> {
    (kind == ids::LOBBY_CHAT_MSG)
        .then(|| (SteamId(field!(b, sys::LobbyChatMsg_t, m_ulSteamIDLobby: u64)), field!(b, sys::LobbyChatMsg_t, m_iChatID: u32) as i32))
}

/// Who asked to open a peer-message session (a Steam-id identity's id).
pub(crate) fn session_request(kind: i32, b: &[u8]) -> Option<SteamId> {
    let at = offset_of!(sys::SteamNetworkingMessagesSessionRequest_t, m_identityRemote) + offset_of!(sys::SteamNetworkingIdentity, __bindgen_anon_1);
    (kind == ids::SESSION_REQUEST).then(|| SteamId(read_at::<u64>(b, at))).filter(|id| id.0 != 0)
}

/// A web API ticket: its handle, `EResult`, and the ticket bytes.
pub(crate) fn web_ticket(kind: i32, b: &[u8]) -> Option<(u32, i32, Vec<u8>)> {
    (kind == ids::WEB_TICKET).then(|| {
        let len = field!(b, sys::GetTicketForWebApiResponse_t, m_cubTicket: i32).clamp(0, 2560) as usize;
        let at = offset_of!(sys::GetTicketForWebApiResponse_t, m_rgubTicket);
        let ticket = b.get(at..at + len).unwrap_or_default().to_vec();
        (
            field!(b, sys::GetTicketForWebApiResponse_t, m_hAuthTicket: u32),
            field!(b, sys::GetTicketForWebApiResponse_t, m_eResult: i32),
            ticket,
        )
    })
}

/// A microtransaction's order id and whether the user authorised it.
pub(crate) fn micro_txn(kind: i32, b: &[u8]) -> Option<(u64, bool)> {
    (kind == ids::MICRO_TXN).then(|| {
        (field!(b, sys::MicroTxnAuthorizationResponse_t, m_ulOrderID: u64), field!(b, sys::MicroTxnAuthorizationResponse_t, m_bAuthorized: u8) != 0)
    })
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
