//! Surface properties (`g_pPhysicsSystem.GetSurfacePropertyController`, `CPhysSurfaceProperties`):
//! the process-wide list managed's `Surface` resources register into. A surface's index is its
//! place in the list ("default" is 0); shapes take friction and elasticity from it unless their
//! settings override them, and physics data parts refer to surfaces by index.

use abi::slots::{c_phys_surface_properties as props, g_p_physics_system as sys, i_phys_surface_property_controller as ctl};
use abi::{NativeRef, Registry, StringToken, bind, class};
use std::ffi::{CStr, CString, c_void};
use std::sync::{LazyLock, OnceLock, RwLock};

pub type Props = NativeRef<class::CPhysSurfaceProperties>;

/// The physical part of a surface (`UpdatePhysics`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Physical {
    pub friction: f32,
    pub elasticity: f32,
    pub density: f32,
    pub rolling_resistance: f32,
    pub bounce_threshold: f32,
}

impl Default for Physical {
    fn default() -> Self {
        let d = physics::Surface::default();
        Self { friction: d.friction, elasticity: d.elasticity, density: 2000.0, rolling_resistance: 0.0, bounce_threshold: 0.0 }
    }
}

struct Entry {
    name: String,
    physical: Physical,
    object: Props,
}

/// What a surface object points at: its index.
struct Index(usize);

static SURFACES: LazyLock<RwLock<Vec<Entry>>> = LazyLock::new(|| {
    let mut list = Vec::new();
    add_to(&mut list, "default", None, "");
    RwLock::new(list)
});

macro_rules! field {
    ($ty:ty, $getter:path, $obj:expr, $value:expr) => {
        abi::rt::fields::set::<$ty, { $getter.index }>($obj.as_raw(), $value)
    };
}

fn add_to(list: &mut Vec<Entry>, name: &str, base: Option<usize>, description: &str) -> usize {
    let index = list.len();
    let base_index = base.unwrap_or(index);
    let physical = list.get(base_index).map_or_else(Physical::default, |b| b.physical);
    let object: Props = objects::create(Index(index));
    let c = |s: &str| CString::new(s.replace('\0', "")).unwrap_or_default();
    abi::rt::fields::set_str::<{ props::GET_M_NAME.index }>(object.as_raw(), Some(&c(name)));
    abi::rt::fields::set_str::<{ props::GET_M_DESCRIPTION.index }>(object.as_raw(), Some(&c(description)));
    field!(u32, props::GET_M_NAME_HASH, object, StringToken::of(name).0);
    let base_name = list.get(base_index).map_or(name, |b| b.name.as_str());
    field!(u32, props::GET_M_BASE_NAME_HASH, object, StringToken::of(base_name).0);
    field!(i32, props::GET_M_N_INDEX, object, index as i32);
    field!(i32, props::GET_M_N_BASE_INDEX, object, base_index as i32);
    list.push(Entry { name: name.to_owned(), physical, object });
    index
}

/// Register `name` (inheriting `base`'s physics), or return it if it's already there.
pub fn add(name: &str, base: &str, description: &str) -> Props {
    let mut list = SURFACES.write().unwrap();
    let find = |list: &[Entry], n: &str| list.iter().position(|e| e.name.eq_ignore_ascii_case(n));
    let i = match find(&list, name) {
        Some(i) => i,
        None => {
            let base = find(&list, base);
            add_to(&mut list, name, base, description)
        }
    };
    list[i].object
}

/// Surface `index`'s physics; `None` for an index no surface has.
pub fn physical(index: i32) -> Option<Physical> {
    SURFACES.read().unwrap().get(usize::try_from(index).ok()?).map(|e| e.physical)
}

/// The index of the surface whose name hashes (StringToken) to `hash`, as compiled physics names them.
pub fn by_hash(hash: u32) -> Option<i32> {
    SURFACES.read().unwrap().iter().position(|e| StringToken::of(&e.name).0 == hash).map(|i| i as i32)
}

/// The object for surface `index`, null for none.
pub fn object(index: i32) -> Props {
    usize::try_from(index).ok().and_then(|i| SURFACES.read().unwrap().get(i).map(|e| e.object)).unwrap_or(NativeRef::NULL)
}

fn update(p: Props, physical: Physical) {
    if let Some(i) = objects::get::<_, Index>(p)
        && let Some(e) = SURFACES.write().unwrap().get_mut(i.0)
    {
        e.physical = physical;
    }
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn register(r: &mut Registry) {
    static CONTROLLER: OnceLock<usize> = OnceLock::new();
    bind!(r, sys::GET_SURFACE_PROPERTY_CONTROLLER, || -> NativeRef<class::IPhysSurfacePropertyController> {
        let addr = *CONTROLLER.get_or_init(|| objects::create::<class::IPhysSurfacePropertyController, _>(()).addr());
        NativeRef::from_raw(addr as *mut c_void)
    });
    bind!(r, ctl::GET_SURFACE_PROP_COUNT, |_| SURFACES.read().unwrap().len() as i32);
    bind!(r, ctl::GET_SURFACE_PROPERTY, |_, i| object(i));
    bind!(r, ctl::ADD_PROPERTY, |_, name, base, description| add(&text(name), &text(base), &text(description)));
    bind!(r, props::UPDATE_PHYSICS, |p, friction, elasticity, density, rolling_resistance, bounce_threshold| {
        update(p, Physical { friction, elasticity, density, rolling_resistance, bounce_threshold })
    });
    // `UpdatePhysics` only records; this pushes the surface into live shapes that inherit from it.
    bind!(r, sys::UPDATE_SURFACE_PROPERTIES, reapply);
}

fn reapply(p: Props) {
    if let Some(i) = objects::get::<_, Index>(p) {
        super::shape::reapply_surface(i.0 as i32);
    }
}

/// Surface `index`'s name.
pub fn name(index: i32) -> Option<String> {
    SURFACES.read().unwrap().get(usize::try_from(index).ok()?).map(|e| e.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_of(p: Props) -> i32 {
        abi::rt::fields::get::<i32, { props::GET_M_N_INDEX.index }>(p.as_raw())
    }

    #[test]
    fn surfaces_register_once_and_inherit_their_base() {
        let rock = add("test_rock", "default", "stone");
        let i = index_of(rock);
        assert!(i > 0, "default is 0");
        assert_eq!(add("TEST_ROCK", "default", ""), rock, "names are case-blind");
        update(rock, Physical { friction: 0.3, ..Physical::default() });
        let gravel = add("test_gravel", "test_rock", "");
        assert_eq!(abi::rt::fields::get::<i32, { props::GET_M_N_BASE_INDEX.index }>(gravel.as_raw()), i);
        assert_eq!(physical(index_of(gravel)).map(|p| p.friction), Some(0.3));
        assert_eq!(object(i), rock);
        assert_eq!(physical(-1), None);
        assert_eq!(physical(0), Some(Physical::default()));
    }

    #[test]
    fn surface_reloads_reach_shapes_that_inherit_them() {
        use super::super::{body, shape, world};
        crate::handles::fake_managed();
        let w: world::Ref = objects::create_shared(std::sync::Arc::new(world::PhysicsWorld::default()));
        let b = body::spawn(w, physics::BodyType::Static, physics::Pose::IDENTITY, &Default::default()).unwrap();
        let ice = add("test_ice", "default", "");
        update(ice, Physical { friction: 0.5, elasticity: 0.2, ..Physical::default() });
        let ball = || physics::Shape::Sphere { center: physics::Vector::ZERO, radius: 1.0 };
        let settings = |friction| abi::structs::PhysicsShapeSettings { friction, elasticity: -1.0, surface_index: -1, ..Default::default() };
        let add_ref = |friction| {
            shape::add(b, shape::Type::Sphere, ball(), settings(friction));
            body::get(b).unwrap().shapes.lock().unwrap().last().copied().unwrap()
        };
        let (inherits, fixed) = (add_ref(-1.0), add_ref(0.9));
        let friction = |s| shape::with_collider(s, |c| c.friction());
        for s in [inherits, fixed] {
            shape::set_surface(s, by_hash(StringToken::of("test_ice").0).unwrap());
        }
        assert_eq!((friction(inherits), friction(fixed)), (0.5, 0.9));
        assert_eq!(shape::surface_name(inherits), "test_ice");
        update(ice, Physical { friction: 0.1, elasticity: 0.2, ..Physical::default() });
        assert_eq!(friction(inherits), 0.5, "UpdatePhysics alone only records");
        reapply(ice);
        assert_eq!((friction(inherits), friction(fixed)), (0.1, 0.9));
        assert_eq!(shape::with_collider(fixed, |c| c.restitution()), 0.2, "elasticity still inherited");
        shape::set_physical(fixed, false, -1.0);
        assert_eq!(friction(fixed), 0.1, "-1 reverts to the surface");
    }
}
