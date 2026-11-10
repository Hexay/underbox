//! Animation graph resources: AnimGraph2 (`.ag`, `CAnimGraph2Resource`) compiled from source, and
//! legacy AnimGraph1 (`.vanmgrph`, `IAnimationGraph`) of which we keep the parameter list.

use super::data;
use super::values::{legacy_type, to_variant};
use crate::resource::{self, ByName, Resource};
use abi::slots::{c_anim_graph2_resource as g2, c_model as m, glue_resources as glue, i_anim_parameter as ip, i_anim_parameter_list as list, i_animation_graph as g1};
use abi::{Guid, NativeRef, Registry, StrRet, bind, class};
use anim::graph::Definition;
use anim::params::{ParamDef, Value};
use std::ffi::CStr;
use std::sync::{Arc, OnceLock};

pub type Ref2 = NativeRef<class::CAnimGraph2Resource>;
pub type Ref1 = NativeRef<class::IAnimationGraph>;

pub struct Graph2 {
    pub def: Option<Arc<Definition>>,
}

impl Resource for Graph2 {
    fn is_error(&self) -> bool {
        self.def.is_none()
    }
}

pub struct Graph1 {
    name: String,
    pub params: Arc<[ParamDef]>,
    error: bool,
    guid: Guid,
    list: OnceLock<NativeRef<class::IAnimParameterList>>,
    entries: OnceLock<Vec<NativeRef<class::IAnimParameter>>>,
}

impl Resource for Graph1 {
    fn is_error(&self) -> bool {
        self.error
    }
    fn guid(&self) -> Guid {
        self.guid
    }
}

/// What an `IAnimParameterList`/`IAnimParameter` object points at.
struct Entry(Arc<Graph1>, usize);

static GRAPHS2: ByName<Graph2> = ByName::new();
static GRAPHS1: ByName<Graph1> = ByName::new();

fn source(name: &str) -> Option<String> {
    std::fs::read_to_string(crate::fs::resolve(name.strip_suffix("_c").unwrap_or(name), "GAME")?).ok()
}

fn compile2(name: &str) -> Option<Arc<Definition>> {
    let compiled = anim::source::ag::compile(&source(name)?).inspect_err(|e| abi::rt::log::warn(&format!("{name}: {e}"))).ok()?;
    if !compiled.unsupported.is_empty() {
        abi::rt::log::log(abi::rt::log::Level::Info, &format!("{name}: stand-ins for {}", compiled.unsupported.join(", ")));
    }
    Some(compiled.def.into_shared())
}

pub fn load2(name: &str) -> Ref2 {
    resource::new_handle(GRAPHS2.find_or_insert(name, || Graph2 { def: compile2(name) }))
}

pub fn definition(r: Ref2) -> Option<Arc<Definition>> {
    resource::resolve::<_, Graph2>(r)?.def.clone()
}

pub fn load1(name: &str, guid: Guid) -> Arc<Graph1> {
    GRAPHS1.find_or_insert(name, || {
        let params = source(name).and_then(|s| anim::source::vanmgrph::parameters(&s).inspect_err(|e| abi::rt::log::warn(&format!("{name}: {e:?}"))).ok());
        let error = params.is_none();
        Graph1 { name: name.to_owned(), params: params.unwrap_or_default().into(), error, guid, list: OnceLock::new(), entries: OnceLock::new() }
    })
}

pub fn handle1(g: Arc<Graph1>) -> Ref1 {
    resource::new_handle(g)
}

pub fn graph1(r: Ref1) -> Option<Arc<Graph1>> {
    resource::resolve::<_, Graph1>(r)
}

impl Graph1 {
    fn list(self: &Arc<Self>) -> NativeRef<class::IAnimParameterList> {
        *self.list.get_or_init(|| objects::create(Entry(self.clone(), 0)))
    }

    fn entry(self: &Arc<Self>, i: usize) -> NativeRef<class::IAnimParameter> {
        let entries = self.entries.get_or_init(|| (0..self.params.len()).map(|i| objects::create(Entry(self.clone(), i))).collect());
        entries.get(i).copied().unwrap_or_default()
    }
}

fn param2<R: Default>(r: Ref2, i: i32, f: impl FnOnce(&ParamDef) -> R) -> R {
    definition(r).and_then(|d| usize::try_from(i).ok().and_then(|i| d.params.get(i).map(f))).unwrap_or_default()
}

fn param1<R: Default>(p: NativeRef<class::IAnimParameter>, f: impl FnOnce(&ParamDef) -> R) -> R {
    objects::get::<_, Entry>(p).and_then(|e| e.0.params.get(e.1).map(f)).unwrap_or_default()
}

fn bound(p: &ParamDef, v: f32) -> abi::structs::AnimVariant {
    to_variant(&match p.default {
        Value::Int(_) => Value::Int(v as i32),
        _ => Value::Float(v),
    })
}

pub fn register(r: &mut Registry) {
    crate::bind_resource_handle!(r, abi::slots::c_anim_graph2_resource, Graph2);
    bind!(r, glue::GET_ANIM_GRAPH2, |name| load2(&name.map(CStr::to_string_lossy).unwrap_or_default()));
    // Facepunch's compiled graph definitions are their native's private format, and managed's
    // GraphBuilder needs CGraphBuilder; both make error graphs until we can read them.
    bind!(r, glue::CREATE_COMPILED_ANIM_GRAPH2, |_, _, _| Ref2::NULL);
    bind!(r, glue::CREATE_BUILT_ANIM_GRAPH2, |_, _, _| Ref2::NULL);
    bind!(r, m::GET_ANIMATION_GRAPH2, |model| data::of(model).graph2_name().map_or(Ref2::NULL, |n| load2(&n)));
    bind!(r, g2::GET_NUM_PARAMETERS, |g| definition(g).map_or(0, |d| d.params.len() as i32));
    bind!(r, g2::GET_PARAMETER_NAME, |g, i| StrRet::from(param2(g, i, |p| Some(p.name.clone()))));
    bind!(r, g2::GET_PARAMETER_TYPE, |g, i| param2(g, i, |p| super::values::graph_kind(&p.default) as i32));
    bind!(r, g2::GET_PARAMETER_MIN, |g, i| param2(g, i, |p| p.min));
    bind!(r, g2::GET_PARAMETER_MAX, |g, i| param2(g, i, |p| p.max));
    bind!(r, g2::GET_PARAMETER_OPTION_COUNT, |g, i| param2(g, i, |p| p.options.len() as i32));
    bind!(r, g2::GET_PARAMETER_OPTION, |g, i, o| StrRet::from(param2(g, i, |p| usize::try_from(o).ok().and_then(|o| p.options.get(o).cloned()))));
    bind!(r, g2::GET_NUM_ANIMATION_SLOTS, |g| definition(g).map_or(0, |d| d.slots.len() as i32));
    bind!(r, g2::GET_ANIMATION_SLOT_NAME, |g, i| StrRet::from(definition(g).and_then(|d| d.slots.get(usize::try_from(i).ok()?).cloned())));
    bind!(r, g2::GET_VARIATION_ID, |g| StrRet::from(definition(g).map(|d| d.variation.clone())));
    bind!(r, g2::GET_MODEL_NAME, |g| StrRet::from(definition(g).map(|d| d.model.clone())));

    crate::bind_resource_handle!(r, abi::slots::i_animation_graph, Graph1);
    bind!(r, glue::GET_ANIMATION_GRAPH, |name, guid| handle1(load1(&name.map(CStr::to_string_lossy).unwrap_or_default(), guid)));
    bind!(r, m::GET_ANIMATION_GRAPH, |model| data::of(model).graph1_name().map_or(Ref1::NULL, |n| handle1(load1(n, Guid::default()))));
    bind!(r, g1::GET_RESOURCE_NAME, |g| StrRet::from(graph1(g).map(|g| g.name.clone())));
    bind!(r, g1::GET_PARAMETER_LIST, |g| graph1(g).map(|g| g.list()).unwrap_or_default());
    bind!(r, list::COUNT, |l| objects::get::<_, Entry>(l).map_or(0, |e| e.0.params.len() as i32));
    bind!(r, list::GET_PARAMETER, |l, i| objects::get::<_, Entry>(l).zip(usize::try_from(i).ok()).map(|(e, i)| e.0.entry(i)).unwrap_or_default());
    bind!(r, list::GET_PARAMETER_1, |l, name| {
        let name = name.map(CStr::to_string_lossy).unwrap_or_default();
        objects::get::<_, Entry>(l).and_then(|e| e.0.params.iter().position(|p| p.name.eq_ignore_ascii_case(&name)).map(|i| e.0.entry(i))).unwrap_or_default()
    });
    bind!(r, ip::GET_NAME, |p| StrRet::from(param1(p, |d| Some(d.name.clone()))));
    bind!(r, ip::GET_PARAMETER_TYPE, |p| param1(p, |d| legacy_type(&d.default)));
    bind!(r, ip::GET_DEFAULT_VALUE, |p| param1(p, |d| to_variant(&d.default)));
    bind!(r, ip::GET_MIN_VALUE, |p| param1(p, |d| bound(d, d.min)));
    bind!(r, ip::GET_MAX_VALUE, |p| param1(p, |d| bound(d, d.max)));
    bind!(r, ip::GET_NUM_OPTION_NAMES, |p| param1(p, |d| d.options.len() as i32));
    bind!(r, ip::GET_OPTION_NAME, |p, i| StrRet::from(param1(p, |d| usize::try_from(i).ok().and_then(|i| d.options.get(i).cloned()))));
}
