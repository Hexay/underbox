//! Animation state of each `CSceneAnimatableObject`: the pose source (graph or direct sequence),
//! parent-space bones, and world-space render bones. Kept beside the scene object, keyed by its
//! address and checked against it so a reused address never inherits a dead object's state.

use super::data::{self, ModelData};
use super::graph;
use anim::graph::{Instance, Recording};
use anim::params::Params;
use anim::sequence::Player;
use anim::{Pose, Skeleton, Xform};
use abi::{NativeRef, class};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

pub type Ref = NativeRef<class::CSceneAnimatableObject>;
pub type Shared = Arc<Mutex<AnimState>>;

pub struct Graph2 {
    pub resource: graph::Ref2,
    pub instance: Instance,
    pub controller: NativeRef<class::CAnimGraph2Controller>,
    /// Belongs to the controller: replaced with it.
    pub recording: Recording,
}

pub struct Graph1 {
    pub graph: Arc<graph::Graph1>,
    pub params: Params,
    pub instances: Vec<NativeRef<class::IAnimParameterInstance>>,
}

pub struct AnimState {
    pub model: NativeRef<class::CModel>,
    pub data: Arc<ModelData>,
    pub local: Vec<Xform>,
    pub world: Vec<Xform>,
    pub prev_world: Vec<Xform>,
    /// World-space bones managed set since the last `CalculateWorldSpaceBones`.
    pub world_set: HashMap<usize, Xform>,
    pub physics: HashMap<usize, Xform>,
    pub sequence: Player,
    pub use_graph: bool,
    /// `SetAnimGraph2` chose a graph other than the model's.
    pub graph_override: bool,
    /// The model's own graphs have been attached (see [`attach_defaults`]).
    pub defaulted: bool,
    pub graph2: Option<Graph2>,
    pub graph1: Option<Graph1>,
    pub rate: f32,
    pub dt: f32,
    pub root_motion: Xform,
    pub low_detail: bool,
    pub flex: HashMap<i32, f32>,
    /// Managed callbacks: AnimGraph2 controller created; legacy graph changed.
    pub on_controller: usize,
    pub on_graph1: usize,
}

struct Entry {
    scene: Weak<crate::scene::object::SceneObject>,
    state: Shared,
}

static TABLE: LazyLock<Mutex<HashMap<usize, Entry>>> = LazyLock::new(Default::default);

/// The state of a live animatable object, made on first use.
pub fn state(r: Ref) -> Option<Shared> {
    let scene = crate::scene::object::get(r.cast())?;
    let mut table = TABLE.lock().unwrap();
    if let Some(e) = table.get(&r.addr()).filter(|e| e.scene.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &scene))) {
        return Some(e.state.clone());
    }
    table.retain(|_, e| e.scene.strong_count() > 0);
    let model = scene.state.lock().unwrap().model;
    let state = Arc::new(Mutex::new(AnimState::new(model)));
    table.insert(r.addr(), Entry { scene: Arc::downgrade(&scene), state: state.clone() });
    Some(state)
}

/// Run `f` on a live object's state, first catching up with a model change.
pub fn with<R: Default>(r: Ref, f: impl FnOnce(&mut AnimState) -> R) -> R {
    let Some(s) = state(r) else { return R::default() };
    let model = crate::scene::object::with(r.cast(), |s| s.model);
    let mut s = s.lock().unwrap();
    if s.model != model {
        let keep = (s.on_controller, s.on_graph1);
        *s = AnimState::new(model);
        (s.on_controller, s.on_graph1) = keep;
    }
    f(&mut s)
}

/// The object's transform as a bone-space `Xform`.
pub fn object_xform(r: Ref) -> Xform {
    super::conv::xform(crate::scene::object::with(r.cast(), |s| s.transform))
}

impl AnimState {
    pub(super) fn new(model: NativeRef<class::CModel>) -> Self {
        let data = data::of(model);
        let local = data.anim.skeleton.bind_pose();
        let world = data.anim.skeleton.model_space(&local);
        Self {
            model,
            data,
            prev_world: world.clone(),
            local,
            world,
            world_set: HashMap::new(),
            physics: HashMap::new(),
            sequence: Player::default(),
            use_graph: true,
            graph_override: false,
            defaulted: false,
            graph2: None,
            graph1: None,
            rate: 1.0,
            dt: 0.0,
            root_motion: Xform::IDENTITY,
            low_detail: false,
            flex: HashMap::new(),
            on_controller: 0,
            on_graph1: 0,
        }
    }

    pub fn skeleton(&self) -> &Skeleton {
        &self.data.anim.skeleton
    }

    /// Run `resource` as this object's AnimGraph2 (NULL clears it). True if a controller was made.
    /// `me` is this state's own handle, for the controller to point back at.
    pub(super) fn set_graph2(&mut self, resource: graph::Ref2, me: Weak<Mutex<AnimState>>) -> bool {
        if self.graph2.as_ref().is_some_and(|g| g.resource == resource) {
            return false;
        }
        if let Some(old) = self.graph2.take() {
            objects::destroy(old.controller);
        }
        let Some(def) = graph::definition(resource) else { return false };
        let data = self.data.clone();
        let mut instance = Instance::new(def, |slot| data.anim.clip(slot).cloned());
        instance.rig = Some(anim::graph::Rig { skeleton: Arc::new(data.anim.skeleton.clone()), world: Xform::IDENTITY });
        let controller = objects::create(super::controller::Handle(me));
        self.graph2 = Some(Graph2 { resource, instance, controller, recording: Recording::default() });
        true
    }

    /// Run `graph` as the legacy graph (`None` clears it). True if it changed.
    pub fn set_graph1(&mut self, graph: Option<Arc<graph::Graph1>>) -> bool {
        if self.graph1.as_ref().map(|g| &g.graph).zip(graph.as_ref()).is_some_and(|(a, b)| Arc::ptr_eq(a, b)) {
            return false;
        }
        for old in self.graph1.take().into_iter().flat_map(|g| g.instances) {
            objects::destroy(old);
        }
        self.graph1 = graph.map(|graph| Graph1 { params: Params::new(graph.params.clone()), graph, instances: Vec::new() });
        true
    }

    pub fn bind_pose(&self) -> Pose {
        Pose::bind(self.skeleton())
    }

    /// Advance the pose by `dt` seconds (times the playback rate); `character` is the object's
    /// transform, kept with a graph recording. During recording playback the graph holds still.
    pub fn update(&mut self, dt: f32, character: Xform) {
        let dt = dt * self.rate;
        self.dt = dt;
        self.root_motion = Xform::IDENTITY;
        let bind = self.bind_pose();
        let pose = match (&mut self.graph2, self.data.anim.clip(&self.sequence.name)) {
            (Some(g), _) if self.use_graph && g.recording.is_playing() => return,
            (Some(g), _) if self.use_graph => {
                if let Some(rig) = &mut g.instance.rig {
                    rig.world = character;
                }
                let pose = g.instance.update(dt, &bind);
                g.recording.record(dt, &g.instance.params, character);
                pose
            }
            (_, Some(clip)) => {
                let t0 = self.sequence.sample_time(clip);
                self.sequence.advance(dt);
                let t1 = self.sequence.sample_time(clip);
                self.root_motion = clip.root_motion(t0, if t1 < t0 { t1 + clip.duration() } else { t1 });
                clip.sample(t1, &bind)
            }
            _ => bind,
        };
        self.local = pose.0;
        if let Some(g) = &mut self.graph1 {
            g.params.reset_auto();
        }
    }

    pub fn calculate_world(&mut self, object: &Xform) {
        self.prev_world = std::mem::take(&mut self.world);
        let model = self.skeleton().model_space(&self.local);
        self.world = model.iter().map(|m| object.mul(m)).collect();
        for (&i, x) in self.physics.iter().chain(self.world_set.iter()) {
            if let Some(w) = self.world.get_mut(i) {
                *w = *x;
            }
        }
        self.world_set.clear();
    }

    /// Bones named like one of `parent`'s take its world transform; the rest follow their parents.
    pub fn merge_from(&mut self, parent: &[(String, Xform)], object: &Xform) {
        let s = &self.data.anim.skeleton;
        let mut world: Vec<Xform> = Vec::with_capacity(s.len());
        for (i, bone) in s.bones().iter().enumerate() {
            let merged = parent.iter().find(|(n, _)| n.eq_ignore_ascii_case(&bone.name)).map(|(_, x)| *x);
            let local = self.local.get(i).copied().unwrap_or(bone.local);
            world.push(merged.unwrap_or_else(|| bone.parent.map_or(*object, |p| world[p]).mul(&local)));
        }
        self.world = world;
    }

    pub fn named_world(&self) -> Vec<(String, Xform)> {
        self.skeleton().bones().iter().zip(&self.world).map(|(b, x)| (b.name.clone(), *x)).collect()
    }
}

/// Run `resource` as `r`'s AnimGraph2; `None` restores the model's own. True if a controller
/// was made (managed's created-callback is then due).
pub fn attach_graph2(r: Ref, resource: Option<graph::Ref2>) -> bool {
    let Some(shared) = state(r) else { return false };
    let me = Arc::downgrade(&shared);
    with(r, |s| {
        s.graph_override = resource.is_some();
        let resource = resource.or_else(|| s.data.graph2_name().map(|n| graph::load2(&n))).unwrap_or_default();
        s.set_graph2(resource, me)
    })
}

/// Give a freshly (re)initialised object its model's graphs. Returns (controller made, legacy
/// graph changed): the callbacks managed is owed.
pub fn attach_defaults(r: Ref) -> (bool, bool) {
    if with(r, |s| std::mem::replace(&mut s.defaulted, true)) {
        return (false, false);
    }
    let needs2 = with(r, |s| s.graph2.is_none() && !s.graph_override);
    let made = needs2 && attach_graph2(r, None);
    let changed = with(r, |s| match (s.graph1.is_none(), s.data.graph1_name().map(str::to_owned)) {
        (true, Some(name)) => s.set_graph1(Some(graph::load1(&name, abi::Guid::default()))),
        _ => false,
    });
    (made, changed)
}
