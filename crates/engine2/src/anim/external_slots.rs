//! `CAnimGraph2Controller`'s external graph slots (managed `GraphExternalSlots`): the slots the
//! running graph declares, and graphs connected to them (see `anim::graph::external`).

use super::controller::Handle;
use super::object::AnimState;
use abi::slots::c_anim_graph2_controller as c;
use abi::{NativeRef, Registry, StringToken, bind, class};
use anim::graph::Instance;

type Ctl = NativeRef<class::CAnimGraph2Controller>;

fn on<R: Default>(ctl: Ctl, f: impl FnOnce(&mut AnimState) -> Option<R>) -> R {
    let Some(state) = objects::get::<_, Handle>(ctl).and_then(|h| h.0.upgrade()) else { return R::default() };
    let mut s = state.lock().unwrap();
    f(&mut s).unwrap_or_default()
}

/// The running graph's slot named by `token`, and its graph.
fn slot(s: &mut AnimState, token: StringToken) -> Option<(String, &mut Instance)> {
    let g = s.graph2.as_mut()?;
    let name = g.instance.definition().graph_slots.iter().find(|n| StringToken::of(n) == token)?.clone();
    Some((name, &mut g.instance))
}

/// A new instance of `graph` on the object's skeleton and clips; refused during recording
/// playback, as managed documents.
fn connect(ctl: Ctl, token: StringToken, graph: super::graph::Ref2) -> bool {
    let Some(def) = super::graph::definition(graph) else { return false };
    on(ctl, |s| {
        if s.graph2.as_ref()?.recording.is_playing() {
            return None;
        }
        let (data, rig) = (s.data.clone(), s.graph2.as_ref()?.instance.rig.clone());
        let mut child = Instance::new(def, |clip| data.anim.clip(clip).cloned());
        child.rig = rig;
        let (name, parent) = slot(s, token)?;
        Some(parent.connect(&name, child))
    })
}

pub fn register(r: &mut Registry) {
    bind!(r, c::HAS_EXTERNAL_GRAPH_SLOT, |ctl, token| on(ctl, |s| Some(slot(s, token).is_some())));
    bind!(r, c::IS_EXTERNAL_GRAPH_SLOT_CONNECTED, |ctl, token| on(ctl, |s| slot(s, token).map(|(n, g)| g.is_connected(&n))));
    bind!(r, c::CONNECT_EXTERNAL_GRAPH, connect);
    bind!(r, c::DISCONNECT_EXTERNAL_GRAPH, |ctl, token| on(ctl, |s| {
        if s.graph2.as_ref()?.recording.is_playing() {
            return None;
        }
        let (name, g) = slot(s, token)?;
        g.disconnect(&name);
        Some(true)
    }));
    bind!(r, c::GET_EXTERNAL_GRAPH_TIMING, |ctl, token, duration, time, active| {
        let t = on(ctl, |s| slot(s, token).and_then(|(n, g)| g.external_timing(&n)).map(Some));
        (*duration, *time, *active) = t.map_or((0.0, 0.0, 0), |t| (t.duration, t.normalized, i32::from(t.active)));
        t.is_some()
    });
}
