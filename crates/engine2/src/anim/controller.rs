//! `CAnimGraph2Controller` (managed's `AnimationGraph2Controller`): an object's AnimGraph2
//! parameters, addressed by `StringToken`, and the legacy `IAnimParameterInstance`s.

use super::conv::{to_transform, to_v3, v3, xform};
use super::object::{self, AnimState};
use super::values::{from_variant, graph_kind, legacy_type, to_variant};
use abi::slots::{c_anim_graph2_controller as c, c_scene_animatable_object as o, i_anim_parameter_instance as pi};
use abi::{NativeRef, Registry, StrRet, StringToken, bind, class};
use anim::graph::{Definition, ValueNode};
use anim::params::{Params, Target, Value};
use std::sync::{Arc, Mutex, Weak};

/// A controller points back at its object's state.
pub struct Handle(pub Weak<Mutex<AnimState>>);

/// A legacy parameter instance: an object's state and the parameter's index.
struct Instance(Weak<Mutex<AnimState>>, usize);

type Ctl = NativeRef<class::CAnimGraph2Controller>;

fn on<R: Default>(ctl: Ctl, f: impl FnOnce(&mut Params, &Definition, &AnimState) -> R) -> R {
    let Some(state) = objects::get::<_, Handle>(ctl).and_then(|h| h.0.upgrade()) else { return R::default() };
    let mut s = state.lock().unwrap();
    let s = &mut *s;
    let Some(g) = s.graph2.as_mut() else { return R::default() };
    let def = g.instance.definition().clone();
    // The graph is moved out for the call so `f` can read the rest of the state.
    let mut params = std::mem::replace(&mut g.instance.params, Params::new(Arc::from([])));
    let out = f(&mut params, &def, s);
    if let Some(g) = s.graph2.as_mut() {
        g.instance.params = params;
    }
    out
}

fn index(p: &Params, name: StringToken) -> Option<usize> {
    p.defs().iter().position(|d| StringToken::of(&d.name) == name)
}

/// The text of an ID token: one the graph names (parameter options, ID comparisons), else hex.
fn id_name(def: &Definition, token: StringToken) -> String {
    let options = def.params.iter().flat_map(|p| p.options.iter().chain(match &p.default {
        Value::Id(s) => Some(s),
        _ => None,
    }));
    let compared = def.values.iter().flat_map(|v| match v {
        ValueNode::IdMatches { ids, .. } => ids.as_slice(),
        _ => &[],
    });
    match options.chain(compared).find(|s| StringToken::of(s) == token) {
        Some(s) => s.clone(),
        None if token.0 == 0 => String::new(),
        None => format!("#{:08x}", token.0),
    }
}

pub(super) fn set(ctl: Ctl, name: StringToken, value: Value) -> bool {
    on(ctl, |p, _, _| index(p, name).is_some_and(|i| p.set(i, value)))
}

pub(super) fn get(ctl: Ctl, name: StringToken) -> Option<Value> {
    on(ctl, |p, _, _| index(p, name).and_then(|i| p.get(i).cloned()))
}

fn set_target(ctl: Ctl, name: StringToken, t: abi::structs::Transform, is_set: bool, is_bone: bool, bone: StringToken) -> bool {
    on(ctl, |p, _, s| {
        let bone = is_bone.then(|| s.skeleton().bones().iter().find(|b| StringToken::of(&b.name) == bone).map(|b| b.name.clone())).flatten();
        index(p, name).is_some_and(|i| p.set(i, Value::Target(Target { set: is_set, bone, xform: xform(t) })))
    })
}

fn get_target(ctl: Ctl, name: StringToken, out: &mut abi::structs::Transform) -> bool {
    let Some(Value::Target(t)) = get(ctl, name) else { return false };
    *out = to_transform(&t.xform);
    t.set
}

fn instance<R: Default>(p: NativeRef<class::IAnimParameterInstance>, f: impl FnOnce(&mut Params, usize) -> R) -> R {
    let Some(i) = objects::get::<_, Instance>(p) else { return R::default() };
    let Some(state) = i.0.upgrade() else { return R::default() };
    let mut s = state.lock().unwrap();
    s.graph1.as_mut().map(|g| f(&mut g.params, i.1)).unwrap_or_default()
}

/// `GetAnimParameter(index)`: the object's legacy parameter `i`, made once per object.
fn legacy_parameter(r: object::Ref, i: i32) -> NativeRef<class::IAnimParameterInstance> {
    let (Some(shared), Ok(i)) = (object::state(r), usize::try_from(i)) else { return NativeRef::NULL };
    let me = Arc::downgrade(&shared);
    object::with(r, |s| {
        let Some(g) = s.graph1.as_mut() else { return NativeRef::NULL };
        if i >= g.params.defs().len() {
            return NativeRef::NULL;
        }
        if g.instances.is_empty() {
            g.instances = (0..g.params.defs().len()).map(|i| objects::create(Instance(me.clone(), i))).collect();
        }
        g.instances[i]
    })
}

pub fn register(r: &mut Registry) {
    bind!(r, c::GET_CONTROL_PARAMETER_INDEX, |ctl, name| on(ctl, |p, _, _| index(p, name).map_or(-1, |i| i as i32)));
    bind!(r, c::GET_CONTROL_PARAMETER_TYPE, |ctl, i| on(ctl, |p, _, _| usize::try_from(i).ok().and_then(|i| p.get(i)).map_or(0, graph_kind)));
    bind!(r, c::GET_CONTROL_PARAMETER_ID_HASH, |ctl, i| on(ctl, |p, _, _| match usize::try_from(i).ok().and_then(|i| p.get(i)) {
        Some(Value::Id(s)) => StringToken::of(s).0,
        _ => 0,
    }));
    bind!(r, c::SET_PARAM_FLOAT, |ctl, name, v| set(ctl, name, Value::Float(v)));
    bind!(r, c::SET_PARAM_BOOL, |ctl, name, v| set(ctl, name, Value::Bool(v)));
    bind!(r, c::SET_PARAM_VECTOR, |ctl, name, v| set(ctl, name, Value::Vector(v3(v))));
    bind!(r, c::SET_PARAM_ID, |ctl, name, id| on(ctl, |p, def, _| index(p, name).is_some_and(|i| p.set(i, Value::Id(id_name(def, id))))));
    bind!(r, c::SET_PARAM_TARGET, set_target);
    bind!(r, c::GET_PARAM_FLOAT, |ctl, name| get(ctl, name).map_or(0.0, |v| v.as_float()));
    bind!(r, c::GET_PARAM_BOOL, |ctl, name| get(ctl, name).is_some_and(|v| v.as_bool()));
    bind!(r, c::GET_PARAM_VECTOR, |ctl, name| to_v3(match get(ctl, name) {
        Some(Value::Vector(v)) => v,
        _ => glam::Vec3::ZERO,
    }));
    bind!(r, c::GET_PARAM_TARGET, get_target);
    bind!(r, c::RESET_CONTROL_PARAMETERS, |ctl| on(ctl, |p, _, _| p.reset_all()));
    bind!(r, c::RESET_CONTROL_PARAMETER, |ctl, name| on(ctl, |p, _, _| index(p, name).is_some_and(|i| p.reset(i))));
    bind!(r, c::GET_DEBUG_ID_NAME, |ctl, id| StrRet::from(on(ctl, |_, def, _| Some(id_name(def, id)))));
    // TODO: tags, float channels and external graph slots need the graph's event and slot data.
    bind!(r, c::IS_TAG_ACTIVE, |_, _| false);
    bind!(r, c::HAS_EXTERNAL_GRAPH_SLOT, |_, _| false);
    bind!(r, c::IS_EXTERNAL_GRAPH_SLOT_CONNECTED, |_, _| false);

    bind!(r, o::GET_ANIM_PARAMETER_1, legacy_parameter);
    bind!(r, o::GET_ANIM_PARAMETER, |obj, name| {
        let name = name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let i = object::with(obj, |s| s.graph1.as_ref().and_then(|g| g.params.index(&name)));
        i.map_or(NativeRef::NULL, |i| legacy_parameter(obj, i as i32))
    });
    bind!(r, pi::GET_NAME, |p| StrRet::from(instance(p, |ps, i| ps.defs().get(i).map(|d| d.name.clone()))));
    bind!(r, pi::GET_VALUE, |p| instance(p, |ps, i| ps.get(i).map(to_variant).unwrap_or_default()));
    bind!(r, pi::GET_PARAMETER_TYPE, |p| instance(p, |ps, i| ps.defs().get(i).map_or(0, |d| legacy_type(&d.default))));
    bind!(r, pi::IS_AUTO_RESET, |p| instance(p, |ps, i| ps.defs().get(i).is_some_and(|d| d.auto_reset)));
    bind!(r, pi::SET_VALUE, |p, v| instance(p, |ps, i| {
        if let Some(v) = from_variant(v) {
            ps.set(i, v);
        }
    }));
}
