//! `MeshGlue.Serialize*`: a model's blocks for managed's `Model.Save` (MBUF, MDAT, DATA, PHYS),
//! returned as owned `CUtlBuffer`s; null when there's nothing to write (managed skips the block).
// TODO: Source 2's binary block layouts. These are KV3 text with the same content, which our
// own loader can read back but real engine2 can't.

use super::render_mesh::RenderMesh;
use crate::model::physics::data::{JointDesc, Part, PhysicsData};
use crate::model::physics::data_slots;
use crate::model::{self, Model, Ref};
use crate::utl_buffer;
use abi::slots::mesh_glue as g;
use abi::structs::{BBox, Rotation, Transform, Vector3};
use abi::{Registry, bind};
use kv::{Object, Value};

fn obj(members: Vec<(&str, Value)>) -> Value {
    Value::Object(Object(members.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}

fn floats(v: impl IntoIterator<Item = f32>) -> Value {
    Value::Array(v.into_iter().map(|x| Value::Double(f64::from(x))).collect())
}

fn v3(v: Vector3) -> Value {
    floats([v.vec.x, v.vec.y, v.vec.z])
}

fn rot(r: Rotation) -> Value {
    floats([r.quat.x, r.quat.y, r.quat.z, r.quat.w])
}

fn xf(t: Transform) -> Value {
    obj(vec![("position", v3(t.position)), ("rotation", rot(t.rotation)), ("scale", v3(t.scale))])
}

fn bbox(b: BBox) -> Value {
    obj(vec![("mins", v3(b.mins)), ("maxs", v3(b.maxs))])
}

fn int(i: impl Into<i64>) -> Value {
    Value::Int(i.into())
}

fn text(s: &str) -> Value {
    Value::String(s.to_owned())
}

fn buffer(v: Option<Value>) -> utl_buffer::Ref {
    v.map_or(utl_buffer::Ref::NULL, |v| utl_buffer::create(kv::write::kv3(&v).into_bytes()))
}

pub fn mesh_buffers(m: &RenderMesh) -> Value {
    let s = m.state();
    let vertices = s.vertices.iter().map(|b| {
        let attributes = b.meta.fields.iter().map(|f| obj(vec![("semantic", text(&f.semantic)), ("index", int(f.index)), ("format", int(f.format)), ("offset", int(f.offset as i64))]));
        obj(vec![("element_count", int(b.count() as i64)), ("element_size", int(b.stride() as i64)), ("attributes", Value::Array(attributes.collect())), ("data", Value::Binary(b.read().clone()))])
    });
    let indices = s.indices.iter().map(|b| obj(vec![("element_size", int(if b.meta { 4 } else { 2 })), ("data", Value::Binary(b.read().clone()))]));
    obj(vec![("vertex_buffers", Value::Array(vertices.collect())), ("index_buffers", Value::Array(indices.collect()))])
}

pub fn mesh_data(m: &RenderMesh) -> Value {
    let calls = m.draw_calls().into_iter().map(|d| {
        obj(vec![("primitive_type", int(d.prim)), ("start_index", int(d.start_index)), ("index_count", int(d.index_count)), ("base_vertex", int(d.start_vertex)), ("vertex_count", int(d.vertex_count))])
    });
    let morphs = m.state().morphs.iter().map(|x| obj(vec![("name", text(&x.name)), ("delta_count", int(x.deltas.len() as i64))])).collect();
    obj(vec![("name", text(&m.name)), ("bounds", bbox(m.bounds())), ("draw_calls", Value::Array(calls.collect())), ("morphs", Value::Array(morphs))])
}

pub fn model_data(m: &Model) -> Value {
    let c = &m.content;
    let meshes = c.meshes.iter().map(|x| obj(vec![("name", text(&x.mesh.name)), ("mesh_group_mask", int(x.group_mask as i64)), ("lod_mask", int(x.lod_mask))]));
    let skins = c.material_groups.iter().map(|g| obj(vec![("name", text(&g.name)), ("material_count", int(g.materials.len() as i64))]));
    let skeleton = crate::anim::data::by_name(&m.name);
    let bones = skeleton.anim.skeleton.bones().iter().map(|b| obj(vec![("name", text(&b.name)), ("parent", int(b.parent.map_or(-1, |p| p as i64)))]));
    obj(vec![
        ("name", text(&m.name)),
        ("meshes", Value::Array(meshes.collect())),
        ("mesh_groups", Value::Array(c.groups.iter().map(|g| text(g)).collect())),
        ("default_mesh_group_mask", int(c.default_group_mask as i64)),
        ("material_groups", Value::Array(skins.collect())),
        ("lod_switch_distances", floats(c.lod_distances.iter().copied())),
        ("bones", Value::Array(bones.collect())),
        ("mass", Value::Double(f64::from(c.mass))),
        ("render_bounds", bbox(c.render_bounds())),
        ("game_data", c.game_data.as_ref().map_or(Value::Null, |n| crate::kv3::to_value(n))),
    ])
}

fn part(p: &Part) -> Value {
    let s = &p.shapes;
    let p3 = |v: glam::Vec3| floats([v.x, v.y, v.z]);
    let spheres = s.spheres.iter().map(|x| obj(vec![("center", p3(x.center)), ("radius", Value::Double(f64::from(x.radius)))]));
    let capsules = s.capsules.iter().map(|x| obj(vec![("a", p3(x.a)), ("b", p3(x.b)), ("radius", Value::Double(f64::from(x.radius)))]));
    let hulls = s.hulls.iter().map(|x| obj(vec![("vertices", Value::Array(x.hull.vertices.iter().copied().map(p3).collect()))]));
    let meshes = s.meshes.iter().map(|x| {
        obj(vec![("vertices", Value::Array(x.vertices.iter().copied().map(p3).collect())), ("triangles", Value::Array(x.triangles.iter().map(|t| Value::Array(t.iter().map(|&i| int(i)).collect())).collect())), ("materials", Value::Binary(x.materials.clone()))])
    });
    obj(vec![
        ("bone", text(&p.bone)),
        ("bind_pose", xf(p.bind)),
        ("mass", Value::Double(f64::from(p.mass))),
        ("inertia_scale", Value::Double(f64::from(p.inertia_scale))),
        ("linear_damping", Value::Double(f64::from(p.linear_damping))),
        ("angular_damping", Value::Double(f64::from(p.angular_damping))),
        ("spheres", Value::Array(spheres.collect())),
        ("capsules", Value::Array(capsules.collect())),
        ("hulls", Value::Array(hulls.collect())),
        ("meshes", Value::Array(meshes.collect())),
    ])
}

fn joint(j: &JointDesc) -> Value {
    obj(vec![
        ("type", int(j.kind)),
        ("body1", int(j.body1)),
        ("body2", int(j.body2)),
        ("flags", int(j.flags)),
        ("enable_collision", Value::Bool(j.enable_collision)),
        ("frame1", xf(j.frame1)),
        ("frame2", xf(j.frame2)),
        ("linear_limit", floats(j.linear_limit)),
        ("swing_limit", floats(j.swing_limit)),
        ("twist_limit", floats(j.twist_limit)),
        ("friction", Value::Double(f64::from(j.friction))),
    ])
}

pub fn physics_data(d: &PhysicsData) -> Value {
    let joints = (0..d.joints.len()).filter_map(|i| d.joint(i)).map(|j| joint(&j));
    obj(vec![
        ("parts", Value::Array(d.parts.iter().map(|p| part(p)).collect())),
        ("joints", Value::Array(joints.collect())),
        ("surfaces", Value::Array(d.surfaces.iter().map(|&s| int(s)).collect())),
        ("collision_attributes", Value::Array(d.attributes.iter().map(|a| Value::Array(a.iter().map(|t| text(t)).collect())).collect())),
    ])
}

fn mesh_of(r: Ref, i: i32) -> Option<std::sync::Arc<RenderMesh>> {
    model::get(r).and_then(|m| m.content.mesh(i).map(|x| x.mesh.clone()))
}

pub fn register(r: &mut Registry) {
    bind!(r, g::SERIALIZE_MESH_BUFFERS, |r, i| buffer(mesh_of(r, i).map(|m| mesh_buffers(&m))));
    bind!(r, g::SERIALIZE_MESH_DATA, |r, i| buffer(mesh_of(r, i).map(|m| mesh_data(&m))));
    bind!(r, g::SERIALIZE_MODEL_DATA, |r| buffer(model::get(r).map(|m| model_data(&m))));
    bind!(r, g::SERIALIZE_PHYSICS_DATA, |r| buffer(model::get(r).and_then(|m| m.content.physics.as_ref().map(|d| physics_data(d)))));
    bind!(r, g::SERIALIZE_PHYSICS_DATA_FROM_AGGREGATE, |d| buffer(data_slots::get(d).map(|d| physics_data(&d))));
}
