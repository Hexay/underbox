//! `CSceneAnimatableObject` graph assignment, direct sequence playback, playback rate, morph
//! overrides and animation events.

use super::bone_slots::settle;
use super::callback;
use super::graph;
use super::object::{self, AnimState, Ref, with};
use abi::slots::c_scene_animatable_object as o;
use abi::{RawPtr, Registry, StrRet, bind};
use anim::Clip;
use std::ffi::{CStr, c_void};

fn on_sequence<R: Default>(r: Ref, f: impl FnOnce(&mut AnimState, &Clip) -> R) -> R {
    with(r, |s| {
        let data = s.data.clone();
        data.anim.clip(&s.sequence.name).map(|c| f(s, c)).unwrap_or_default()
    })
}

fn play(r: Ref, name: Option<&CStr>) {
    let name = name.map(CStr::to_string_lossy).unwrap_or_default();
    with(r, |s| s.sequence.play(&name));
}

fn set_graph2(r: Ref, g: graph::Ref2) {
    if object::attach_graph2(r, (!g.is_null()).then_some(g)) {
        callback::controller_created(with(r, |s| s.on_controller));
    }
}

fn set_graph1(r: Ref, graph: Option<std::sync::Arc<graph::Graph1>>) {
    if with(r, |s| s.set_graph1(graph)) {
        callback::graph1_changed(with(r, |s| s.on_graph1), r.as_raw());
    }
}

fn store_callback(f: RawPtr<c_void>) -> usize {
    f.as_ptr() as usize
}

pub fn register(r: &mut Registry) {
    bind!(r, o::INIT_ANIMATION, settle);
    bind!(r, o::SET_ANIM_GRAPH2_CONTROLLER_CREATED_CALLBACK, |r, f| with(r, |s| s.on_controller = store_callback(f)));
    bind!(r, o::SET_ANIM_GRAPH_CHANGED_CALLBACK, |r, f| with(r, |s| s.on_graph1 = store_callback(f)));
    bind!(r, o::SET_ANIM_GRAPH2, set_graph2);
    bind!(r, o::GET_ANIM_GRAPH2_CONTROLLER, |r| with(r, |s| s.graph2.as_ref().map(|g| g.controller)).unwrap_or_default());
    bind!(r, o::GET_ANIM_GRAPH2_UPDATE_TIME_MS, |_| 0.0);
    bind!(r, o::SET_ANIM_GRAPH, |r, name| {
        let graph = name.map(|n| graph::load1(&n.to_string_lossy(), abi::Guid::default()));
        set_graph1(r, graph);
    });
    bind!(r, o::SET_ANIM_GRAPH_1, |r, g| set_graph1(r, graph::graph1(g)));
    // A fresh handle each call: managed destroys the one it gets if it already wraps the graph.
    bind!(r, o::GET_ANIM_GRAPH, |r| with(r, |s| s.graph1.as_ref().map(|g| graph::handle1(g.graph.clone()))).unwrap_or_default());
    bind!(r, o::RESET_GRAPH_PARAMETERS, |r| with(r, |s| {
        if let Some(g) = &mut s.graph1 {
            g.params.reset_all();
        }
    }));
    bind!(r, o::SET_SHOULD_USE_ANIM_GRAPH, |r, v| with(r, |s| s.use_graph = v));
    bind!(r, o::GET_SHOULD_USE_ANIM_GRAPH, |r| with(r, |s| s.use_graph));
    bind!(r, o::SET_LOW_DETAIL_ANIMATION, |r, v| with(r, |s| s.low_detail = v));
    bind!(r, o::IS_LOW_DETAIL_ANIMATION, |r| with(r, |s| s.low_detail));
    bind!(r, o::GET_PLAYBACK_RATE, |r| with(r, |s| s.rate));
    bind!(r, o::SET_PLAYBACK_RATE, |r, v| with(r, |s| s.rate = v));

    bind!(r, o::SET_SEQUENCE, play);
    bind!(r, o::GET_SEQUENCE, |r| StrRet::from(with(r, |s| Some(s.sequence.name.clone()).filter(|n| !n.is_empty()))));
    bind!(r, o::GET_SEQUENCE_DURATION, |r| on_sequence(r, |_, c| c.duration()));
    bind!(r, o::GET_SEQUENCE_CYCLE, |r| on_sequence(r, |s, c| s.sequence.cycle(c)));
    bind!(r, o::SET_SEQUENCE_CYCLE, |r, v| on_sequence(r, |s, c| s.sequence.set_cycle(c, v)));
    bind!(r, o::SET_SEQUENCE_LOOPING, |r, v| with(r, |s| s.sequence.looping = Some(v)));
    bind!(r, o::IS_SEQUENCE_FINISHED, |r| on_sequence(r, |s, c| s.sequence.finished(c)));
    bind!(r, o::SET_SEQUENCE_BLENDING, |r, v| with(r, |s| s.sequence.blending = v));
    bind!(r, o::DIRECT_PLAYBACK_PLAY_SEQUENCE, play);
    bind!(r, o::DIRECT_PLAYBACK_PLAY_SEQUENCE_1, |r, name, _target, _heading, _time| play(r, name));
    bind!(r, o::DIRECT_PLAYBACK_CANCEL_SEQUENCE, |r| with(r, |s| s.sequence.play("")));
    bind!(r, o::DIRECT_PLAYBACK_GET_SEQUENCE, |r| StrRet::from(with(r, |s| Some(s.sequence.name.clone()).filter(|n| !n.is_empty()))));
    bind!(r, o::DIRECT_PLAYBACK_GET_SEQUENCE_CYCLE, |r| on_sequence(r, |s, c| s.sequence.cycle(c)));
    bind!(r, o::DIRECT_PLAYBACK_GET_SEQUENCE_DURATION, |r| on_sequence(r, |_, c| c.duration()));
    bind!(r, o::DIRECT_PLAYBACK_SET_SEQUENCE_START_TIME, |r, t| with(r, |s| s.sequence.time = t));

    bind!(r, o::PENDING_ANIMATION_EVENTS, |r| with(r, |s| s.pending.len() as i32));
    bind!(r, o::RUN_ANIMATION_EVENTS, |r, cb: RawPtr<c_void>| {
        let events = with(r, |s| std::mem::take(&mut s.pending));
        callback::animation_events(cb.as_ptr() as usize, &events);
    });
    // TODO: AnimGraph1 tag spans aren't read; only AnimGraph2 state tags dispatch.
    bind!(r, o::DISPATCH_TAG_EVENTS, |r, cb: RawPtr<c_void>| {
        let tags = with(r, |s| s.graph2.as_mut().map(|g| g.tags.drain()).unwrap_or_default());
        callback::tag_events(cb.as_ptr() as usize, &tags);
    });

    // Morph overrides are in `super::flex`.
    // Deformation volumes only change rendered vertices.
    bind!(r, o::SET_DEFORMATION_VOLUMES, |_, _, _| {});
    bind!(r, o::SET_DEFORMATION_ANCHORS, |_, _, _| {});
}
