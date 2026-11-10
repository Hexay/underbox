use abi::structs::{NativeEngine_Sdl_Event, Rect};

#[test]
fn delay_precise_waits_at_least_the_duration() {
    let start = std::time::Instant::now();
    super::delay_precise(3_000_000);
    let took = start.elapsed();
    // No upper bound: under parallel builds and boots the scheduler can oversleep arbitrarily.
    assert!(took >= std::time::Duration::from_millis(3), "{took:?}");
}

#[test]
fn poll_event_writes_sdl_events_into_managed_layout() {
    let events = 0x4000; // SDL_INIT_EVENTS
    assert!(window::video::init_subsystem(events), "{}", window::error());
    let mut managed = NativeEngine_Sdl_Event::default();
    while super::input::poll_event(&mut managed) {}

    let mut wheel = window::events::new(0x403);
    wheel.wheel.x = 1.5;
    wheel.wheel.integer_x = 3;
    wheel.wheel.mouse_y = 20.0;
    assert!(window::events::push(&mut wheel), "{}", window::error());

    assert!(super::input::poll_event(&mut managed));
    let w = managed.wheel();
    assert_eq!((managed.type_(), w.x, w.integer_x, w.mouse_y), (0x403, 1.5, 3, 20.0));
    assert!(!super::input::poll_event(&mut managed));
    window::video::quit_subsystem(events);
}

#[test]
fn null_window_handles_fail_softly() {
    let null = abi::RawPtr::NULL;
    let (mut w, mut h) = (7, 7);
    assert!(!super::windows::out_pair(window::windows::size(super::win(null)), &mut w, &mut h));
    assert_eq!((w, h), (0, 0), "outs are zeroed on failure");
    assert!(!window::windows::show(super::win(null)));
    let area = Rect { x: 1, y: 2, width: 3, height: 4 };
    assert!(!window::keyboard::set_text_input_area(super::win(null), [area.x, area.y, area.width, area.height], 0));
    assert_eq!(super::text(None), c"");
}
