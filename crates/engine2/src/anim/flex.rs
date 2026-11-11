//! Flex controllers (morphs): the model's controller names, managed's overrides by index or
//! name, and each object's controller values (override, else the playing sequence's morph track).

use super::data;
use super::object::{AnimState, Ref, with};
use abi::slots::{c_model as m, c_scene_animatable_object as o};
use abi::{Registry, StrRet, StringToken, bind};

impl AnimState {
    fn flex_index(&self, name: StringToken) -> Option<i32> {
        self.data.anim.flex.iter().position(|f| StringToken::of(f) == name).map(|i| i as i32)
    }

    /// Each flex controller's value now; what a renderer feeds the model's flex rules.
    pub fn flex_values(&self) -> Vec<f32> {
        let clip = self.data.anim.clip(&self.sequence.name).and_then(|c| c.morphs_at(self.sequence.time));
        (0..self.data.anim.flex.len()).map(|i| self.flex.get(&(i as i32)).copied().or_else(|| clip.as_ref()?.get(i).copied()).unwrap_or(0.0)).collect()
    }
}

/// [`AnimState::flex_values`] of a live object (empty if it isn't one).
pub fn values(r: Ref) -> Vec<f32> {
    with(r, |s| s.flex_values())
}

pub fn register(r: &mut Registry) {
    bind!(r, m::NUM_FLEX_CONTROLLERS, |model| data::of(model).anim.flex.len() as i32);
    bind!(r, m::GET_FLEX_CONTROLLER_NAME, |model, i| StrRet::from(usize::try_from(i).ok().and_then(|i| data::of(model).anim.flex.get(i).cloned())));
    bind!(r, m::HAS_ANIMATION_DRIVEN_FLEXES, |model| data::of(model).anim.clips.iter().any(|c| !c.morphs.is_empty()));

    bind!(r, o::S_BOX_SET_FLEX_OVERRIDE_1, |r, i, w, _fade| with(r, |s| {
        s.flex.insert(i, w);
    }));
    bind!(r, o::S_BOX_GET_FLEX_OVERRIDE, |r, i| with(r, |s| s.flex.get(&i).copied().unwrap_or(0.0)));
    bind!(r, o::S_BOX_CLEAR_FLEX_OVERRIDE, |r, _fade| with(r, |s| s.flex.clear()));
    bind!(r, o::S_BOX_CLEAR_FLEX_OVERRIDE_2, |r, i, _fade| with(r, |s| {
        s.flex.remove(&i);
    }));
    bind!(r, o::S_BOX_SET_FLEX_OVERRIDE, |r, name, w, _fade| with(r, |s| {
        if let Some(i) = s.flex_index(name) {
            s.flex.insert(i, w);
        }
    }));
    bind!(r, o::S_BOX_GET_FLEX_OVERRIDE_1, |r, name| with(r, |s| s.flex_index(name).and_then(|i| s.flex.get(&i).copied()).unwrap_or(0.0)));
    bind!(r, o::S_BOX_CLEAR_FLEX_OVERRIDE_1, |r, name, _fade| with(r, |s| {
        if let Some(i) = s.flex_index(name) {
            s.flex.remove(&i);
        }
    }));
}
