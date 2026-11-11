use super::trace::trace;
use super::world::PhysicsWorld;
use super::{aggregate, shape, world};
use crate::math::{identity, vec3};
use abi::structs::Request;
use assets::compiled::physics::{Aggregate, Ids, Part, Shapes, TriMesh};
use std::sync::Arc;

/// A ground quad at z = 0 whose second triangle (x < y) uses surface slot 1.
fn two_surface_ground() -> Aggregate {
    let g = 50.0;
    let quad = TriMesh { vertices: vec![[-g, -g, 0.0], [g, -g, 0.0], [g, g, 0.0], [-g, g, 0.0]], triangles: vec![[0, 1, 2], [0, 2, 3]], materials: vec![0, 1], ids: Ids::default() };
    let part = Part { bone: None, bind: assets::compiled::physics::IDENTITY, flags: 0, mass: 0.0, inertia_scale: 1.0, linear_damping: 0.0, angular_damping: 0.0, shapes: Shapes { meshes: vec![quad], ..Shapes::default() } };
    Aggregate { parts: vec![part], surfaces: vec![0, 0], attributes: vec![vec!["solid".into()]], ..Aggregate::default() }
}

#[test]
fn mesh_traces_report_triangle_and_its_surface() {
    crate::handles::fake_managed();
    let w: world::Ref = objects::create_shared(Arc::new(PhysicsWorld::default()));
    let data = crate::model::physics::compiled::data("ground", &two_surface_ground());
    let ground = aggregate::spawn(w, data, identity(), 2).unwrap();
    let body = aggregate::get(ground).unwrap().bodies[0].1;
    let mesh = crate::physics::body::get(body).unwrap().shapes.lock().unwrap()[0];
    shape::with_state(mesh, |s| s.set_slot_surface(1, 7));
    world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    let down = |x: f32, y: f32| {
        let mut req = Request::default();
        (req.start_pos, req.end_pos, req.world.self_) = (vec3(x, y, 10.0), vec3(x, y, -10.0), w.addr());
        let r = trace(req);
        (r.fraction, r.triangle_index, r.surface_property)
    };
    assert_eq!(down(20.0, -20.0), (0.5, 0, 0));
    assert_eq!(down(-20.0, 20.0), (0.5, 1, 7));
    aggregate::destroy(ground);
}

#[test]
fn mesh_shapes_take_slot_surfaces_from_their_settings() {
    crate::handles::fake_managed();
    let w: world::Ref = objects::create_shared(Arc::new(PhysicsWorld::default()));
    let body = super::body::spawn(w, physics::BodyType::Static, physics::Pose::IDENTITY, &Default::default()).unwrap();
    let surfaces = [-1i32, 9];
    let settings = abi::structs::PhysicsShapeSettings { surface_indices: surfaces.as_ptr() as usize, surface_count: 2, surface_index: 4, friction: -1.0, elasticity: -1.0, ..Default::default() };
    assert_eq!(shape::slot_surfaces(&settings), [-1, 9]);
    let g = 50.0;
    let quad = physics::Shape::Mesh {
        vertices: [[-g, -g], [g, -g], [g, g], [-g, g]].map(|[x, y]| physics::Vector::new(x, y, 0.0)).to_vec(),
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    };
    shape::add_with_slots(body, shape::Type::Mesh, quad, settings, vec![0, 1], shape::slot_surfaces(&settings));
    world::with_sim(w, |s| s.step(1.0 / 60.0, 1));
    let down = |x: f32, y: f32| {
        let mut req = Request::default();
        (req.start_pos, req.end_pos, req.world.self_) = (vec3(x, y, 10.0), vec3(x, y, -10.0), w.addr());
        let r = trace(req);
        (r.triangle_index, r.surface_property)
    };
    assert_eq!(down(20.0, -20.0), (0, 4), "slot 0 is unset (-1): the shape's own surface");
    assert_eq!(down(-20.0, 20.0), (1, 9));
}
