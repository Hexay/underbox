//! Each model's animation data, keyed by model name: read from its `.vmdl` source on first use,
//! or registered by `CModelBuilder` for runtime models. Also the native objects managed reads
//! that data through (`CAttachment`, `CHitBoxSet`, `CHitBox`), made once per model.

use abi::{NativeRef, class};
use anim::ModelAnim;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

/// A hitbox set's object and its hitboxes' objects.
type SetRefs = (NativeRef<class::CHitBoxSet>, Vec<NativeRef<class::CHitBox>>);

pub struct ModelData {
    pub anim: ModelAnim,
    attachments: OnceLock<Vec<NativeRef<class::CAttachment>>>,
    sets: OnceLock<Vec<SetRefs>>,
}

/// What a `CAttachment`/`CHitBoxSet`/`CHitBox` object points at.
pub struct Part {
    pub model: Arc<ModelData>,
    pub set: usize,
    pub index: usize,
}

static MODELS: LazyLock<Mutex<HashMap<String, Arc<ModelData>>>> = LazyLock::new(Default::default);

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(crate::fs::resolve(path, "GAME")?).ok()
}

fn load(name: &str) -> ModelAnim {
    let Some(text) = read(&crate::model::source_path(name)) else { return ModelAnim::default() };
    anim::source::vmdl::read(&text, &|p| read(p))
        .inspect_err(|e| abi::rt::log::warn(&format!("{name}: animation data: {e:?}")))
        .unwrap_or_default()
}

pub fn by_name(name: &str) -> Arc<ModelData> {
    let key = name.to_ascii_lowercase();
    if let Some(d) = MODELS.lock().unwrap().get(&key) {
        return d.clone();
    }
    let data = ModelData::new(load(name));
    MODELS.lock().unwrap().entry(key).or_insert(data).clone()
}

pub fn of(model: NativeRef<class::CModel>) -> Arc<ModelData> {
    crate::model::name(model).map_or_else(|| ModelData::new(ModelAnim::default()), |n| by_name(&n))
}

/// The first loaded model with a clip named `clip`, and its name.
pub fn find_clip(clip: &str) -> Option<(String, Arc<ModelData>)> {
    let models = MODELS.lock().unwrap();
    models.iter().find(|(_, d)| d.anim.clip(clip).is_some()).map(|(n, d)| (n.clone(), d.clone()))
}

/// Register a runtime model's data (replacing any earlier model of that name).
pub fn insert(name: &str, anim: ModelAnim) {
    MODELS.lock().unwrap().insert(name.to_ascii_lowercase(), ModelData::new(anim));
}

impl ModelData {
    fn new(anim: ModelAnim) -> Arc<Self> {
        Arc::new(Self { anim, attachments: OnceLock::new(), sets: OnceLock::new() })
    }

    pub fn attachment_ref(self: &Arc<Self>, i: usize) -> NativeRef<class::CAttachment> {
        let refs = self.attachments.get_or_init(|| {
            (0..self.anim.attachments.len()).map(|index| objects::create(Part { model: self.clone(), set: 0, index })).collect()
        });
        refs.get(i).copied().unwrap_or_default()
    }

    fn sets(self: &Arc<Self>) -> &[SetRefs] {
        self.sets.get_or_init(|| {
            let part = |set, index| Part { model: self.clone(), set, index };
            let sets = self.anim.hitbox_sets.iter().enumerate();
            sets.map(|(s, set)| (objects::create(part(s, 0)), (0..set.hitboxes.len()).map(|h| objects::create(part(s, h))).collect())).collect()
        })
    }

    pub fn set_ref(self: &Arc<Self>, i: usize) -> NativeRef<class::CHitBoxSet> {
        self.sets().get(i).map(|s| s.0).unwrap_or_default()
    }

    pub fn hitbox_ref(self: &Arc<Self>, set: usize, i: usize) -> NativeRef<class::CHitBox> {
        self.sets().get(set).and_then(|s| s.1.get(i)).copied().unwrap_or_default()
    }

    /// The AnimGraph2 (`.ag`) this model uses: its own graph if that's one, else an `.ag` beside
    /// its `.vanmgrph` (how converted graphs ship, e.g. citizen_human).
    pub fn graph2_name(&self) -> Option<String> {
        let graph = self.anim.graph.as_deref()?;
        if graph.ends_with(".ag") {
            return Some(graph.to_owned());
        }
        let ag = format!("{}.ag", graph.rsplit_once('.').map_or(graph, |(stem, _)| stem));
        crate::fs::resolve(&ag, "GAME").map(|_| ag)
    }

    pub fn graph1_name(&self) -> Option<&str> {
        self.anim.graph.as_deref().filter(|g| g.ends_with(".vanmgrph"))
    }
}
