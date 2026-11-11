use super::*;

/// A `T`-sized buffer with `(offset, bytes)` written in.
fn raw<T>(set: &[(usize, &[u8])]) -> Vec<u8> {
    let mut b = vec![0u8; size_of::<T>()];
    for (at, v) in set {
        b[*at..*at + v.len()].copy_from_slice(v);
    }
    b
}

#[test]
fn server_callbacks_become_events() {
    type A = sys::ValidateAuthTicketResponse_t;
    let b = raw::<A>(&[
        (offset_of!(A, m_SteamID), &1u64.to_le_bytes()),
        (offset_of!(A, m_eAuthSessionResponse), &3i32.to_le_bytes()),
        (offset_of!(A, m_OwnerSteamID), &2u64.to_le_bytes()),
    ]);
    let banned = Event::AuthResult { user: SteamId(1), owner: SteamId(2), response: AuthResponse::VacBanned };
    assert_eq!(server_event(ids::VALIDATE_AUTH_TICKET, &b), Some(banned));
    assert_eq!(auth_response(99), AuthResponse::TicketInvalid);

    type F = sys::SteamServerConnectFailure_t;
    let b = raw::<F>(&[(offset_of!(F, m_eResult), &3i32.to_le_bytes()), (offset_of!(F, m_bStillRetrying), &[1])]);
    assert_eq!(server_event(ids::SERVER_CONNECT_FAILURE, &b), Some(Event::ServerConnectFailure { result: 3, retrying: true }));
    assert_eq!(server_event(ids::SERVERS_CONNECTED, &[0]), Some(Event::ServerConnected));
    assert_eq!(server_event(ids::LOBBY_ENTER, &[]), None);
    assert_eq!((ids::SERVERS_CONNECTED, ids::VALIDATE_AUTH_TICKET), (101, 143));
}

#[test]
fn lobby_callbacks_become_lobby_events() {
    let (lobby, user) = (SteamId(109775240000000001), SteamId(76561197960265729));
    type U = sys::LobbyChatUpdate_t;
    let update = |change: u32| {
        raw::<U>(&[
            (offset_of!(U, m_ulSteamIDLobby), &lobby.0.to_le_bytes()),
            (offset_of!(U, m_ulSteamIDUserChanged), &user.0.to_le_bytes()),
            (offset_of!(U, m_rgfChatMemberStateChange), &change.to_le_bytes()),
        ])
    };
    assert_eq!(lobby_event(ids::LOBBY_CHAT_UPDATE, &update(1)), Some(LobbyEvent::MemberEntered { lobby, member: user }));
    assert_eq!(lobby_event(ids::LOBBY_CHAT_UPDATE, &update(4)), Some(LobbyEvent::MemberLeft { lobby, member: user }));

    type E = sys::LobbyEnter_t;
    let enter = |response: u32| raw::<E>(&[(0, &lobby.0.to_le_bytes()), (offset_of!(E, m_EChatRoomEnterResponse), &response.to_le_bytes())]);
    assert_eq!(lobby_event(ids::LOBBY_ENTER, &enter(1)), Some(LobbyEvent::Entered { lobby }));
    assert_eq!(lobby_event(ids::LOBBY_ENTER, &enter(5)), None, "a refused entry isn't entering");

    type C = sys::LobbyCreated_t;
    let created = raw::<C>(&[(offset_of!(C, m_eResult), &1i32.to_le_bytes()), (offset_of!(C, m_ulSteamIDLobby), &lobby.0.to_le_bytes())]);
    assert_eq!(lobby_event(ids::LOBBY_CREATED, &created), Some(LobbyEvent::Created { lobby }));

    type M = sys::LobbyChatMsg_t;
    let msg = raw::<M>(&[(0, &lobby.0.to_le_bytes()), (offset_of!(M, m_iChatID), &7u32.to_le_bytes())]);
    assert_eq!(chat_message(ids::LOBBY_CHAT_MSG, &msg), Some((lobby, 7)));
}

#[test]
fn session_requests_tickets_and_purchases() {
    type R = sys::SteamNetworkingMessagesSessionRequest_t;
    let at = offset_of!(sys::SteamNetworkingIdentity, __bindgen_anon_1);
    let b = raw::<R>(&[(0, &16i32.to_le_bytes()), (at, &42u64.to_le_bytes())]);
    assert_eq!(session_request(ids::SESSION_REQUEST, &b), Some(SteamId(42)));

    type W = sys::GetTicketForWebApiResponse_t;
    let b = raw::<W>(&[
        (offset_of!(W, m_hAuthTicket), &5u32.to_le_bytes()),
        (offset_of!(W, m_eResult), &1i32.to_le_bytes()),
        (offset_of!(W, m_cubTicket), &3i32.to_le_bytes()),
        (offset_of!(W, m_rgubTicket), &[9, 8, 7, 6]),
    ]);
    assert_eq!(web_ticket(ids::WEB_TICKET, &b), Some((5, 1, vec![9, 8, 7])));

    type T = sys::MicroTxnAuthorizationResponse_t;
    let b = raw::<T>(&[(offset_of!(T, m_ulOrderID), &11u64.to_le_bytes()), (offset_of!(T, m_bAuthorized), &[1])]);
    assert_eq!(micro_txn(ids::MICRO_TXN, &b), Some((11, true)));
}
