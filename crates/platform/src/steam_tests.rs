use super::*;

#[test]
fn maps_begin_auth_results() {
    assert_eq!(begin_result(Ok(())), BeginAuth::Ok);
    assert_eq!(begin_result(Err(AuthSessionError::DuplicateRequest)), BeginAuth::DuplicateRequest);
    assert_eq!(begin_result(Err(AuthSessionError::ExpiredTicket)) as i64, 5);
}

#[test]
fn maps_auth_ticket_response_with_reason() {
    let r = |response| ValidateAuthTicketResponse {
        steam_id: steamworks::SteamId::from_raw(1),
        owner_steam_id: steamworks::SteamId::from_raw(2),
        response,
    };
    let (user, owner) = (SteamId(1), SteamId(2));
    assert_eq!(auth_event(&r(Ok(()))), Event::AuthResult { user, owner, response: AuthResponse::Ok });
    let banned = auth_event(&r(Err(V::VACBanned)));
    assert_eq!(banned, Event::AuthResult { user, owner, response: AuthResponse::VacBanned });
    assert_eq!(AuthResponse::VacBanned as i32, 3);
}

#[test]
fn inverts_steam_errors_to_result_codes() {
    assert_eq!(result_code(SteamError::Generic), 2);
    assert_eq!(result_code(SteamError::NoConnection), 3);
    assert_eq!(result_code(SteamError::from(sys::EResult::k_EResultTimeout)), sys::EResult::k_EResultTimeout as i32);
}

#[test]
fn formats_public_ip() {
    let v4 = |ip: u32| sys::SteamIPAddress_t {
        __bindgen_anon_1: sys::SteamIPAddress_t__bindgen_ty_1 { m_unIPv4: ip },
        m_eType: sys::ESteamIPType::k_ESteamIPTypeIPv4,
    };
    assert_eq!(ip_string(v4(0x7f00_0001)).as_deref(), Some("127.0.0.1"));
    assert_eq!(ip_string(v4(0)), None);
}

#[test]
fn acts_offline_before_init() {
    let p = Steam::default();
    p.server_info(&mut |i| i.max_players = 8);
    p.server_log_on(None);
    assert!(p.server_logged_on());
    assert_eq!(p.poll(), [Event::ServerConnected]);
    assert_eq!(p.server_public_ip(), None);
    p.server_shutdown();
    assert!(!p.server_logged_on());
}

#[test]
#[ignore = "calls the real SteamGameServer_Init; passes only where no Steam client is installed"]
fn falls_back_when_steam_init_fails() {
    let p = Steam::default();
    p.server_init(27015, 27016, "1.0.0.0");
    assert!(p.with(|_| ()).is_none());
    p.server_log_on(None);
    assert_eq!(p.poll(), [Event::ServerConnected]);
}
