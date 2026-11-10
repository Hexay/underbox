use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderBuilder, SharedShape};

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
}

/// Per-shape contact response.
#[derive(Debug, Clone, Copy)]
pub struct Surface {
    pub friction: f32,
    pub elasticity: f32,
    pub trigger: bool,
}

impl Default for Surface {
    fn default() -> Self {
        Self { friction: 0.5, elasticity: 0.0, trigger: false }
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
        };
        Some(builder.friction(surface.friction).restitution(surface.elasticity).sensor(surface.trigger))
    }
}
