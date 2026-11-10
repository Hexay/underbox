//! `CAnimGraph2Controller`'s recording, external-graph and debugger slots.
//!
//! Recording is real (see `anim::graph::recording`). Our compiled graphs carry no float channels,
//! external graph slots or editor node numbering, so channel reads, external-graph connections and
//! per-node debug queries answer "no such channel/slot/node" (false, outputs zeroed).

use super::controller::Handle;
use super::conv::to_transform;
use super::object::Graph2;
use abi::slots::c_anim_graph2_controller as c;
use abi::structs::{Transform, Vector2, Vector3};
use abi::{NativeRef, Registry, bind, class};
use anim::graph::recording::{Seek, Update};
use anim::{Pose, Xform};

type Ctl = NativeRef<class::CAnimGraph2Controller>;

/// `f` on the controller's graph, with the skeleton's bind pose; `None` once the object is gone.
fn graph<R>(ctl: Ctl, f: impl FnOnce(&mut Graph2, &Pose) -> R) -> Option<R> {
    let state = objects::get::<_, Handle>(ctl)?.0.upgrade()?;
    let mut s = state.lock().unwrap();
    let bind = s.bind_pose();
    s.graph2.as_mut().map(|g| f(g, &bind))
}

fn seek(ctl: Ctl, i: i32) -> i64 {
    let Some(state) = objects::get::<_, Handle>(ctl).and_then(|h| h.0.upgrade()) else { return Seek::Failure as i64 };
    let mut s = state.lock().unwrap();
    let bind = s.bind_pose();
    let Some(g) = s.graph2.as_mut() else { return Seek::Failure as i64 };
    let pose = usize::try_from(i).map_err(|_| Seek::Failure).and_then(|i| g.recording.seek(i, &mut g.instance, &bind));
    match pose {
        Ok(p) => {
            s.local = p.0;
            Seek::Success as i64
        }
        Err(e) => e as i64,
    }
}

/// Write an update's info; managed's sync events aren't tracked, so they read as none (-1, 0%).
#[allow(clippy::too_many_arguments)]
fn info(u: Option<Update>, kind: &mut i32, dt: &mut f32, start: &mut i32, start_pct: &mut f32, end: &mut i32, end_pct: &mut f32) -> bool {
    (*start, *start_pct, *end, *end_pct) = (-1, 0.0, -1, 0.0);
    let Some(u) = u else {
        (*kind, *dt) = (0, 0.0);
        return false;
    };
    (*kind, *dt) = (u.kind as i32, u.dt);
    true
}

fn recording(r: &mut Registry) {
    bind!(r, c::START_RECORDING, |ctl| graph(ctl, |g, _| g.recording.start()).unwrap_or_default());
    bind!(r, c::STOP_RECORDING, |ctl| graph(ctl, |g, _| g.recording.stop()).unwrap_or_default());
    bind!(r, c::CLEAR_RECORDING, |ctl| graph(ctl, |g, _| g.recording.clear()).unwrap_or_default());
    bind!(r, c::IS_RECORDING, |ctl| graph(ctl, |g, _| g.recording.is_recording()).unwrap_or_default());
    bind!(r, c::HAS_RECORDED_DATA, |ctl| graph(ctl, |g, _| g.recording.has_data()).unwrap_or_default());
    bind!(r, c::START_RECORDING_PLAYBACK, |ctl| graph(ctl, |g, _| g.recording.start_playback()).unwrap_or_default());
    bind!(r, c::STOP_RECORDING_PLAYBACK, |ctl| graph(ctl, |g, _| g.recording.stop_playback()).unwrap_or_default());
    bind!(r, c::IS_RECORDING_PLAYBACK_ACTIVE, |ctl| graph(ctl, |g, _| g.recording.is_playing()).unwrap_or_default());
    bind!(r, c::GO_TO_RECORDED_UPDATE, seek);
    bind!(r, c::GET_NUM_RECORDED_UPDATES, |ctl| graph(ctl, |g, _| g.recording.len() as i32).unwrap_or_default());
    bind!(r, c::GET_CURRENT_RECORDED_UPDATE_INDEX, |ctl| graph(ctl, |g, _| g.recording.current_index().map(|i| i as i32)).flatten().unwrap_or(-1));
    bind!(r, c::GET_CURRENT_RECORDED_UPDATE_INFO, |ctl, kind, dt, s, sp, e, ep| {
        info(graph(ctl, |g, _| g.recording.current().cloned()).flatten(), kind, dt, s, sp, e, ep)
    });
    bind!(r, c::GET_RECORDED_UPDATE_INFO, |ctl, i, kind, dt, s, sp, e, ep| {
        let u = usize::try_from(i).ok().and_then(|i| graph(ctl, |g, _| g.recording.get(i).cloned()).flatten());
        info(u, kind, dt, s, sp, e, ep)
    });
    bind!(r, c::GET_RECORDED_CHARACTER_TRANSFORM_S2, |ctl| {
        let x = graph(ctl, |g, _| g.recording.current().map(|u| u.character)).flatten();
        to_transform(&x.unwrap_or(Xform::IDENTITY))
    });
}

fn external_and_channels(r: &mut Registry) {
    bind!(r, c::GET_FLOAT_CHANNEL_VALUE, |_, _, _, v| {
        *v = 0.0;
        false
    });
    bind!(r, c::CONNECT_EXTERNAL_GRAPH, |_, _, _| false);
    bind!(r, c::DISCONNECT_EXTERNAL_GRAPH, |_, _| false);
    bind!(r, c::GET_EXTERNAL_GRAPH_TIMING, |_, _, duration, time, active| {
        (*duration, *time, *active) = (0.0, 0.0, 0);
        false
    });
}

fn debugger(r: &mut Registry) {
    bind!(r, c::IS_GRAPH_NODE_ACTIVE, |_, _| false);
    bind!(r, c::GET_NODE_DEBUG_INFO, |_, _, flags, kind, duration, time, prev, events, event, pct| {
        (*flags, *kind, *events, *event) = (0, 0, 0, -1);
        (*duration, *time, *prev, *pct) = (0.0, 0.0, 0.0, 0.0);
        false
    });
    bind!(r, c::GET_NODE_DEBUG_VALUE, |_, _, b, f, id, v, t, tflags, bone| {
        (*b, *f, *id, *tflags, *bone) = (0, 0.0, 0, 0, 0);
        *v = Vector3::default();
        *t = Transform::default();
        false
    });
    bind!(r, c::GET_SYNC_TRACK_EVENT, |_, _, _, id, start, duration| {
        (*id, *start, *duration) = (0, 0.0, 0.0);
        false
    });
    bind!(r, c::GET_STATE_MACHINE_TRANSITION_STATE, |_, _, state, source, transition, progress| {
        (*state, *source, *transition, *progress) = (-1, -1, -1, 0.0);
        false
    });
    bind!(r, c::GET_SELECTOR_STATE, |_, _, option| {
        *option = -1;
        false
    });
    bind!(r, c::GET_TRANSITION_PROGRESS_PERCENTAGE, |_, _, p| {
        *p = 0.0;
        false
    });
    bind!(r, c::GET_PARAMETERIZED_BLEND_DEBUG_INFO, |_, _, a, b, w| {
        (*a, *b, *w) = (-1, -1, 0.0);
        false
    });
    bind!(r, c::GET_BLEND2_D_PARAMETER, |_, _, p| {
        *p = Vector2::default();
        false
    });
    bind!(r, c::GET_LAYER_WEIGHT, |_, _, _, w| {
        *w = 0.0;
        false
    });
    bind!(r, c::GET_IK_WEIGHT, |_, _, w| {
        *w = 0.0;
        false
    });
    bind!(r, c::HAS_DEBUGGABLE_GRAPH_INSTANCE, |_, _, has| {
        *has = 0;
        false
    });
}

pub fn register(r: &mut Registry) {
    recording(r);
    external_and_channels(r);
    debugger(r);
}
