//! External graph slots: the fallback while empty, a connected graph's pose and timing after.

use super::tests::{clip, clips, x};
use super::*;
use crate::pose::Pose;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn external_slot_plays_its_connected_graph_else_the_fallback() {
    let mut def = Definition { graph_slots: vec!["Extra".into()], ..Definition::default() };
    let fallback = clip(&mut def, "walk", true);
    let filled = def.add_value(ValueNode::ExternalFilled("Extra".into()));
    def.root = Some(def.add_pose(PoseNode::External { slot: "Extra".into(), fallback: Some(fallback) }));
    let mut g = Instance::new(def.into_shared(), clips);
    let bind = Pose::zero(1);
    assert!(close(x(&g.update(0.1, &bind)), 10.0), "empty: the fallback");
    assert!(!g.value(filled).as_bool() && g.external_timing("Extra").is_none());

    let mut child = Definition::default();
    child.root = Some(clip(&mut child, "run", true));
    let child = || Instance::new(child.clone().into_shared(), clips);
    assert!(!g.connect("Nope", child()), "no such slot");
    assert!(g.connect("Extra", child()));
    assert!(!g.connect("Extra", child()), "occupied");
    assert!(g.has_graph_slot("Extra") && g.is_connected("Extra") && g.value(filled).as_bool());
    assert!(close(x(&g.update(0.1, &bind)), 30.0), "the connected graph's pose");
    let t = g.external_timing("Extra").unwrap();
    assert!(t.active && close(t.duration, 1.0) && close(t.normalized, 0.0), "{t:?}");
    g.update(0.25, &bind);
    assert!(close(g.external_timing("Extra").unwrap().normalized, 0.25));
    g.update(0.8, &bind);
    let t = g.external_timing("Extra").unwrap();
    assert!(close(t.normalized, 0.05) && close(t.duration, 1.0), "looped: {t:?}");

    assert!(g.disconnect("Extra") && !g.disconnect("Extra"));
    assert!(close(x(&g.update(0.1, &bind)), 10.0), "the fallback again");
}
