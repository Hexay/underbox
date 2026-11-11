use crate::filter::ShapeMeta;
use crate::heightfield::Grid;
use crate::world::World;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ActiveCollisionTypes, ActiveHooks, Collider, ColliderBuilder, ColliderHandle, RigidBodyHandle, SharedShape};

/// A collision shape in its body's space, as managed's `IPhysicsBody.Add*Shape` describes it.
#[derive(Debug, Clone)]
pub enum Shape {
    Sphere { center: Vector, radius: f32 },
    Capsule { a: Vector, b: Vector, radius: f32 },
    Box { center: Vector, rotation: Rotation, half_extents: Vector },
    /// Along Z, centred on the origin.
    Cylinder { half_height: f32, radius: f32 },
    /// Along Z, centred on the origin, `radius` at the base (-Z), apex at +Z.
    Cone { half_height: f32, radius: f32 },
    Hull { pose: Pose, points: Vec<Vector> },
    Mesh { vertices: Vec<Vector>, triangles: Vec<[u32; 3]> },
    /// Terrain, in Rubikon's heightfield frame (see [`Grid`]).
    HeightField(Grid),
    /// Several shapes as one, e.g. a whole body swept by a trace. Meshes and heightfields are
    /// left out (parry compounds can't nest them).
    Compound(Vec<Shape>),
}

/// Per-shape contact response.
#[derive(Debug, Clone, Copy)]
pub struct Surface {
    pub friction: f32,
    pub elasticity: f32,
    pub trigger: bool,
}

impl Default for Surface {
    /// s&box's `default.surface`.
    fn default() -> Self {
        Self { friction: 0.8, elasticity: 0.05, trigger: false }
    }
}

fn y_to_z() -> Pose {
    Pose::from_parts(Vector::ZERO, Rotation::from_rotation_x(std::f32::consts::FRAC_PI_2))
}


impl Shape {
    /// The collider for this shape; `None` for a degenerate hull or mesh.
    pub(crate) fn collider(&self, surface: Surface) -> Option<ColliderBuilder> {
        let builder = match self {
            Shape::Sphere { center, radius } => ColliderBuilder::ball(*radius).translation(*center),
            Shape::Capsule { a, b, radius } => ColliderBuilder::capsule_from_endpoints(*a, *b, *radius),
            Shape::Box { center, rotation, half_extents: h } => {
                ColliderBuilder::cuboid(h.x, h.y, h.z).position(Pose::from_parts(*center, *rotation))
            }
            // Rapier's cylinders and cones run along Y; s&box's along Z.
            Shape::Cylinder { half_height, radius } => ColliderBuilder::cylinder(*half_height, *radius).position(y_to_z()),
            Shape::Cone { half_height, radius } => ColliderBuilder::cone(*half_height, *radius).position(y_to_z()),
            Shape::Hull { pose, points } => ColliderBuilder::convex_hull(points)?.position(*pose),
            Shape::Mesh { vertices, triangles } => {
                ColliderBuilder::new(SharedShape::trimesh(vertices.clone(), triangles.clone()).ok()?)
            }
            Shape::HeightField(grid) => {
                let (field, pose) = grid.shape();
                ColliderBuilder::new(SharedShape::new(field)).position(pose)
            }
            Shape::Compound(parts) => {
                let parts: Vec<_> = parts
                    .iter()
                    .filter(|p| !matches!(p, Shape::Mesh { .. } | Shape::HeightField(_) | Shape::Compound(_)))
                    .filter_map(|p| p.collider(surface).map(|b| b.build()))
                    .map(|c| (*c.position(), c.shared_shape().clone()))
                    .collect();
                if parts.is_empty() {
                    return None;
                }
                ColliderBuilder::new(SharedShape::compound(parts))
            }
        };
        Some(builder.friction(surface.friction).restitution(surface.elasticity).sensor(surface.trigger))
    }
}

impl World {
    /// Mass comes from the shape's [`ShapeMeta`] density (see `mass.rs`), so colliders are massless.
    /// Triggers keep rapier's default pair types too: only a pair with a dynamic body reports, as
    /// native. A keyframed or static collider resting in a static trigger stays silent; sbox-walker's
    /// map relies on it (`contacts_tests::static_trigger_*`).
    pub fn add_shape(&mut self, body: RigidBodyHandle, shape: &Shape, surface: Surface) -> Option<ColliderHandle> {
        let hooks = ActiveHooks::FILTER_CONTACT_PAIRS | ActiveHooks::FILTER_INTERSECTION_PAIR;
        let collider = shape.collider(surface)?.density(0.0).active_hooks(hooks).active_collision_types(ActiveCollisionTypes::default());
        self.body(body)?;
        let id = self.raw_mut().insert_collider(collider, Some(body));
        self.meta.insert(id, ShapeMeta::default());
        self.contacts.created(id);
        self.rebuild_mass(body);
        Some(id)
    }

    pub fn shape_meta(&self, id: ColliderHandle) -> Option<&ShapeMeta> {
        self.meta.get(&id)
    }

    /// Change a shape's tags, switches or density; mass follows.
    pub fn update_shape_meta(&mut self, id: ColliderHandle, f: impl FnOnce(&mut ShapeMeta)) {
        let Some(meta) = self.meta.get_mut(&id) else { return };
        f(meta);
        meta.keep_base_tag();
        if let Some(body) = self.shape(id).and_then(|c| c.parent()) {
            self.rebuild_mass(body);
        }
    }

    pub fn set_trigger(&mut self, id: ColliderHandle, trigger: bool) {
        if let Some(c) = self.shape_mut(id) {
            c.set_sensor(trigger);
        }
    }

    /// Swap a shape's geometry for `shape`'s, keeping its body and surface.
    pub fn update_shape(&mut self, id: ColliderHandle, shape: &Shape) {
        let Some(c) = self.shape_mut(id) else { return };
        let surface = Surface { friction: c.friction(), elasticity: c.restitution(), trigger: c.is_sensor() };
        let Some(new) = shape.collider(surface).map(|b| b.build()) else { return };
        c.set_shape(new.shared_shape().clone());
        if let Some(local) = new.position_wrt_parent() {
            c.set_position_wrt_parent(*local);
        }
        if let Some(body) = c.parent() {
            self.rebuild_mass(body);
        }
    }

    /// A shape's world-space bounds, (mins, maxs).
    pub fn shape_bounds(&self, id: ColliderHandle) -> Option<(Vector, Vector)> {
        let aabb = self.shape(id)?.compute_aabb();
        Some((aabb.mins, aabb.maxs))
    }

    /// The union of a body's shapes' world-space bounds.
    pub fn body_bounds(&self, id: RigidBodyHandle) -> Option<(Vector, Vector)> {
        let bounds = self.body(id)?.colliders().iter().filter_map(|&c| self.shape_bounds(c));
        bounds.reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
    }

    /// Shapes overlapping `probe` (in world space). Gotcha: the broad phase updates in `step`,
    /// so shapes added or moved since the last step aren't seen yet.
    pub fn overlapping(&self, probe: &Shape) -> Vec<ColliderHandle> {
        let Some(probe) = probe.collider(Surface::default()).map(|b| b.build()) else { return Vec::new() };
        let pipeline = self.raw().query_pipeline();
        pipeline.intersect_shape(*probe.position(), probe.shape()).map(|(h, _)| h).collect()
    }

    pub fn remove_shape(&mut self, id: ColliderHandle) {
        let body = self.shape(id).and_then(|c| c.parent());
        self.raw_mut().remove_collider(id);
        self.meta.remove(&id);
        self.contacts.forget(id);
        if let Some(body) = body {
            self.rebuild_mass(body);
        }
    }

    pub fn shape(&self, id: ColliderHandle) -> Option<&Collider> {
        self.raw().colliders.get(id)
    }

    pub fn shape_mut(&mut self, id: ColliderHandle) -> Option<&mut Collider> {
        self.raw_mut().colliders.get_mut(id)
    }
}
