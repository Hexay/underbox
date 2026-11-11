use super::*;

#[test]
fn maps_begin_auth_results() {
    assert_eq!(begin_result(Ok(())), BeginAuth::Ok);
    assert_eq!(begin_result(Err(AuthSessionError::DuplicateRequest)), BeginAuth::DuplicateRequest);
    assert_eq!(begin_result(Err(AuthSessionError::ExpiredTicket)) as i64, 5);
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
fn tickets_take_what_steam_wrote() {
    let t = auth_ticket(SteamId(0), |p, cap, len, id| {
        assert!(id.is_null() && cap == 1024);
        // SAFETY: auth_ticket's buffer holds `cap` bytes.
        unsafe { p.cast::<u8>().write(7) };
        // SAFETY: as above.
        unsafe { *len = 1 };
        3
    });
    assert_eq!(t, Ticket { handle: 3, bytes: vec![7] });
}

#[test]
fn acts_offline_before_init() {
    let p = Steam::default();
    p.server_info(&mut |i| i.max_players = 8);
    p.server_log_on(None);
    assert!(p.server_logged_on());
    assert_eq!(crate::events(|e, r| p.run_callbacks(e, r)), [Event::ServerConnected]);
    assert_eq!(p.server_public_ip(), None);
    assert!(p.server_workshop().is_none() && p.server_ticket(SteamId(1)).bytes.is_empty());
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
    assert_eq!(crate::events(|e, r| p.run_callbacks(e, r)), [Event::ServerConnected]);
}
