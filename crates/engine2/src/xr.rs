//! Facepunch's OpenXR wrapper (`fpxr`): no XR runtime here, by design. `HasHeadset` is false, which
//! is managed's "no VR" path (`VRSystem.Init` returns before creating anything); every other call
//! only sees null handles and answers with the `XRResult` that ends its loop or check.
//! Gotcha: `XRResult` 0 is `Error`, so a zero return isn't "nothing to do".

use abi::slots::{fpxr_application_config as cfg, fpxr_compositor as comp, fpxr_event_manager as ev, fpxr_input as input, fpxr_instance as inst};
use abi::structs::InstanceProperties;
use abi::{NativeRef, Registry, StrRet, bind};

/// `XRResult.NoEventsPending`: ends managed's `PumpEvent` loop.
const NO_EVENTS_PENDING: i64 = 2;
/// `XRResult.NoSessionRunning`.
const NO_SESSION: i64 = 3;
/// `XRResult.CompositorNotAttached`.
const NO_COMPOSITOR: i64 = -5;

pub fn register(r: &mut Registry) {
    bind!(r, cfg::SET_DEBUG_CALLBACK, |_| {});
    bind!(r, cfg::SET_ERROR_CALLBACK, |_| {});

    bind!(r, inst::HAS_HEADSET, || false);
    bind!(r, inst::CREATE, |_| NativeRef::null());
    bind!(r, inst::COMPOSITOR, |_, _| NativeRef::null());
    bind!(r, inst::INPUT, |_| NativeRef::null());
    bind!(r, inst::GET_REQUIRED_DEVICE_EXTENSIONS, |_| StrRet::from(c""));
    bind!(r, inst::GET_REQUIRED_INSTANCE_EXTENSIONS, |_| StrRet::from(c""));
    bind!(r, inst::GET_PROPERTIES, |_| InstanceProperties::default());

    bind!(r, comp::EVENT_MANAGER, |_| NativeRef::null());
    bind!(r, comp::SUBMIT, |_, _| NO_SESSION);
    bind!(r, comp::BEGIN_FRAME, |_| NO_SESSION);
    bind!(r, comp::END_FRAME, |_| NO_SESSION);
    bind!(r, comp::GET_EYE_WIDTH, |_| 0);
    bind!(r, comp::GET_EYE_HEIGHT, |_| 0);
    bind!(r, comp::GET_RENDER_TARGET_WIDTH, |_| 0);
    bind!(r, comp::GET_RENDER_TARGET_HEIGHT, |_| 0);
    bind!(r, comp::GET_DISPLAY_REFRESH_RATE, |_| 0.0);
    bind!(r, comp::GET_VIEW_INFO, |_, _, _| NO_COMPOSITOR);
    bind!(r, comp::GET_PROJECTION_MATRIX, |_, _, _, _, _| NO_COMPOSITOR);
    bind!(r, comp::SHUTDOWN, |_| {});

    bind!(r, ev::PUMP_EVENT, |_, _| NO_EVENTS_PENDING);
    // SessionState.Unknown.
    bind!(r, ev::GET_SESSION_STATE, |_| 0);

    bind!(r, input::GET_BOOLEAN_ACTION_STATE, |_, _, _, _| NO_SESSION);
    bind!(r, input::GET_FLOAT_ACTION_STATE, |_, _, _, _| NO_SESSION);
    bind!(r, input::GET_VECTOR2_ACTION_STATE, |_, _, _, _| NO_SESSION);
    bind!(r, input::GET_POSE_ACTION_STATE, |_, _, _, _| NO_SESSION);
    bind!(r, input::TRIGGER_HAPTIC_VIBRATION, |_, _, _, _, _| NO_SESSION);
    bind!(r, input::GET_HAND_POSE_STATE, |_, _, _, _| NO_SESSION);
}
