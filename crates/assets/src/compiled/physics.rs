//! Physics aggregates (`VPhysXAggregateData_t`): a `.vphys_c` DATA block, or a model's PHYS block.
//! Parts with their spheres, capsules, hulls, triangle meshes and compounds, bind poses, surfaces,
//! collision attributes and joints ([`super::joint`]). Hull and mesh BVH/topology fields are skipped: consumers rebuild them.

use super::kv::{self, array, float, float_or, floats, int, int_value, list, obj, objects, text, vec3};
use super::{Error, format};
use ::kv::{Object, Value};

/// Three rows of (rotation-scale row, translation), as world nodes store transforms.
pub type Matrix = [[f32; 4]; 3];
pub const IDENTITY: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// A shape's surface (index into [`Aggregate::surfaces`]) and collision attribute.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ids {
    pub surface: u32,
    pub attribute: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shapes {
    pub spheres: Vec<([f32; 3], f32, Ids)>,
    /// (end a, end b, radius).
    pub capsules: Vec<([f32; 3], [f32; 3], f32, Ids)>,
    /// Hull points.
    pub hulls: Vec<(Vec<[f32; 3]>, Ids)>,
    pub meshes: Vec<TriMesh>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriMesh {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// Per-triangle surface index; empty when all use `ids.surface`.
    pub materials: Vec<u32>,
    pub ids: Ids,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The bone the part follows (`m_boneNames[m_boneParents[i]]`), if any.
    pub bone: Option<String>,
    pub bind: Matrix,
    pub flags: i64,
    pub mass: f32,
    pub inertia_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub shapes: Shapes,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Aggregate {
    pub parts: Vec<Part>,
    /// Surface property name hashes (StringToken of the surface name).
    pub surfaces: Vec<u32>,
    /// Each collision attribute's interaction tags.
    pub attributes: Vec<Vec<String>>,
    pub joints: Vec<super::joint::Joint>,
}

fn ids(o: &Object) -> Ids {
    Ids { surface: int(o, "m_nSurfacePropertyIndex") as u32, attribute: int(o, "m_nCollisionAttributeIndex") as u32 }
}

fn vec3s(bytes: &[u8]) -> Vec<[f32; 3]> {
    words(bytes).as_chunks::<3>().0.iter().map(|c| c.map(f32::from_bits)).collect()
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect()
}

/// `m_Vertices` as vec3s, or (newer hulls) blob positions in `m_VertexPositions`.
fn vertices(o: &Object) -> Vec<[f32; 3]> {
    match o.get("m_VertexPositions") {
        Some(Value::Binary(b)) => vec3s(b),
        _ => match o.get("m_Vertices") {
            Some(Value::Binary(b)) => vec3s(b),
            _ => array(o, "m_Vertices").iter().map(|v| kv::floats_of(Some(v))).collect(),
        },
    }
}

fn triangles(o: &Object) -> Vec<[u32; 3]> {
    match o.get("m_Triangles") {
        Some(Value::Binary(b)) => words(b).as_chunks::<3>().0.to_vec(),
        _ => objects(o, "m_Triangles").map(|t| kv::floats_of::<3>(t.get("m_nIndex")).map(|v| v as u32)).collect(),
    }
}

fn materials(o: &Object, triangles: usize) -> Vec<u32> {
    match o.get("m_Materials") {
        Some(Value::Binary(b)) if b.len() == triangles * 4 => words(b),
        Some(Value::Binary(b)) => b.iter().map(|&m| u32::from(m)).collect(),
        _ => kv::ints(o, "m_Materials").into_iter().map(|m| m as u32).collect(),
    }
}

impl Shapes {
    fn add_sphere(&mut self, sphere: &Object, ids: Ids) {
        self.spheres.push((vec3(sphere, "m_vCenter"), float(sphere, "m_flRadius"), ids));
    }

    fn add_capsule(&mut self, c: &Object, ids: Ids) {
        let ends = array(c, "m_vCenter");
        self.capsules.push((kv::floats_of(ends.first()), kv::floats_of(ends.get(1)), float(c, "m_flRadius"), ids));
    }

    fn add_mesh(&mut self, m: &Object, ids: Ids) {
        let triangles = triangles(m);
        self.meshes.push(TriMesh { vertices: vertices(m), materials: materials(m, triangles.len()), triangles, ids });
    }
}

/// A part's `m_rnShape`. Compounds (`m_compounds[].m_Compound`) are flattened into the other
/// lists, their children taking the compound's surface and attribute.
fn shapes(o: &Object) -> Shapes {
    let mut s = Shapes::default();
    objects(o, "m_spheres").for_each(|d| s.add_sphere(kv::object(d, "m_Sphere"), ids(d)));
    objects(o, "m_capsules").for_each(|d| s.add_capsule(kv::object(d, "m_Capsule"), ids(d)));
    s.hulls = objects(o, "m_hulls").map(|d| (vertices(kv::object(d, "m_Hull")), ids(d))).collect();
    objects(o, "m_meshes").for_each(|d| s.add_mesh(kv::object(d, "m_Mesh"), ids(d)));
    for d in objects(o, "m_compounds") {
        let (c, i) = (kv::object(d, "m_Compound"), ids(d));
        let materials = compound_materials(c);
        // Child ids run over spheres, capsules, hulls, then meshes.
        let mut child = 0usize;
        let mut next = || {
            child += 1;
            materials.get(child - 1).map_or(i, |&m| Ids { surface: m, ..i })
        };
        objects(c, "m_Spheres").for_each(|x| s.add_sphere(x, next()));
        objects(c, "m_Capsules").for_each(|x| s.add_capsule(x, next()));
        for h in objects(c, "m_Hulls") {
            s.hulls.push((vertices(h), next()));
        }
        objects(c, "m_Meshes").for_each(|x| s.add_mesh(x, next()));
    }
    s
}

/// A compound's `m_CompoundMaterialIndices`: each child shape's surface index (a byte blob or
/// an int list).
fn compound_materials(c: &Object) -> Vec<u32> {
    match c.get("m_CompoundMaterialIndices") {
        Some(Value::Binary(b)) => b.iter().map(|&m| u32::from(m)).collect(),
        _ => kv::ints(c, "m_CompoundMaterialIndices").into_iter().map(|m| m as u32).collect(),
    }
}

/// A bind pose: 3 rows of 4, or 12 floats in a row.
pub fn matrix(v: Option<&Value>) -> Matrix {
    let rows = v.and_then(Value::as_array).unwrap_or_default();
    if rows.first().and_then(Value::as_array).is_some() {
        return std::array::from_fn(|r| kv::floats_of(rows.get(r)));
    }
    let flat: [f32; 12] = kv::floats_of(v);
    if flat.iter().all(|&f| f == 0.0) {
        return IDENTITY;
    }
    std::array::from_fn(|r| std::array::from_fn(|c| flat[r * 4 + c]))
}

/// The aggregate in a decoded DATA/PHYS object.
pub fn from_kv3(o: &Object) -> Aggregate {
    let bones = kv::strings(o, "m_boneNames");
    let parents = kv::ints(o, "m_boneParents");
    let poses = array(o, "m_bindPose");
    let parts = objects(o, "m_parts")
        .enumerate()
        .map(|(i, p)| Part {
            bone: parents.get(i).and_then(|&b| bones.get(usize::try_from(b).ok()?)).cloned(),
            bind: poses.get(i).map_or(IDENTITY, |m| matrix(Some(m))),
            flags: int(p, "m_nFlags"),
            mass: float(p, "m_flMass"),
            inertia_scale: float_or(p, "m_flInertiaScale", 1.0),
            linear_damping: float(p, "m_flLinearDamping"),
            angular_damping: float(p, "m_flAngularDamping"),
            shapes: shapes(kv::object(p, "m_rnShape")),
        })
        .collect();
    let attributes = objects(o, "m_collisionAttributes")
        .map(|a| if a.get("m_InteractAsStrings").is_some() { kv::strings(a, "m_InteractAsStrings") } else { kv::strings(a, "m_PhysicsTagStrings") })
        .collect();
    Aggregate { parts, surfaces: kv::ints(o, "m_surfacePropertyHashes").into_iter().map(|h| h as u32).collect(), attributes, joints: super::joint::read(o) }
}

/// A `.vphys_c` file.
pub fn read(file: &[u8]) -> Result<Aggregate, Error> {
    let data = super::kv3_block(file, b"DATA", "DATA")?;
    Ok(from_kv3(data.as_object().ok_or_else(|| format("physics DATA isn't an object"))?))
}

fn blob_of<const N: usize, T: Copy>(items: &[[T; N]], bytes: fn(T) -> [u8; 4]) -> Value {
    Value::Binary(items.iter().flatten().flat_map(|&v| bytes(v)).collect())
}

fn descriptor(name: &str, inner: Value, ids: Ids) -> Value {
    obj(vec![("m_nCollisionAttributeIndex", int_value(ids.attribute)), ("m_nSurfacePropertyIndex", int_value(ids.surface)), ("m_UserFriendlyName", text("")), (name, inner)])
}

fn write_shapes(s: &Shapes) -> Value {
    let spheres = s.spheres.iter().map(|(c, r, i)| descriptor("m_Sphere", obj(vec![("m_vCenter", floats(c)), ("m_flRadius", Value::Double((*r).into()))]), *i));
    let capsules = s.capsules.iter().map(|(a, b, r, i)| descriptor("m_Capsule", obj(vec![("m_vCenter", list([floats(a), floats(b)])), ("m_flRadius", Value::Double((*r).into()))]), *i));
    let hulls = s.hulls.iter().map(|(points, i)| descriptor("m_Hull", super::hull::write(points), *i));
    let meshes = s.meshes.iter().map(|m| {
        let materials = list(m.materials.iter().map(|&x| int_value(x)));
        descriptor("m_Mesh", obj(vec![("m_Vertices", blob_of(&m.vertices, f32::to_le_bytes)), ("m_Triangles", blob_of(&m.triangles, u32::to_le_bytes)), ("m_Materials", materials)]), m.ids)
    });
    obj(vec![("m_spheres", list(spheres)), ("m_capsules", list(capsules)), ("m_hulls", list(hulls)), ("m_meshes", list(meshes)), ("m_CollisionAttributeIndices", list([]))])
}

/// The DATA/PHYS object for `a`, with the members real's compiler writes (its schema conversion
/// rejects a block without them). Bone-less aggregates get no bind poses or bone parents, as there.
pub fn write_kv3(a: &Aggregate) -> Value {
    let mut bones: Vec<String> = Vec::new();
    let parents = a.parts.iter().map(|p| match &p.bone {
        Some(b) => int_value(bones.iter().position(|x| x == b).unwrap_or_else(|| {
            bones.push(b.clone());
            bones.len() - 1
        }) as i64),
        None => int_value(-1),
    });
    let parents: Vec<Value> = parents.collect();
    let boned = !bones.is_empty();
    let part = |p: &Part| {
        obj(vec![
            ("m_nFlags", int_value(p.flags)),
            ("m_flMass", Value::Double(p.mass.into())),
            ("m_rnShape", write_shapes(&p.shapes)),
            ("m_nCollisionAttributeIndex", int_value(0)),
            ("m_nReserved", int_value(0)),
            ("m_flInertiaScale", Value::Double(p.inertia_scale.into())),
            ("m_flLinearDamping", Value::Double(p.linear_damping.into())),
            ("m_flAngularDamping", Value::Double(p.angular_damping.into())),
            ("m_bOverrideMassCenter", Value::Bool(false)),
            ("m_vMassCenterOverride", floats(&[0.0; 3])),
            ("m_flGravityScale", Value::Double(1.0)),
        ])
    };
    let attributes = a.attributes.iter().map(|t| obj(vec![("m_PhysicsTagStrings", list(t.iter().map(|s| text(s))))]));
    let empty = || list([]);
    obj(vec![
        ("m_nFlags", int_value(0)),
        ("m_nRefCounter", int_value(0)),
        ("m_bonesHash", empty()),
        ("m_boneNames", list(bones.iter().map(|b| text(b)))),
        ("m_indexNames", empty()),
        ("m_indexHash", empty()),
        ("m_bindPose", if boned { list(a.parts.iter().map(|p| list(p.bind.iter().map(|r| floats(r))))) } else { empty() }),
        ("m_parts", list(a.parts.iter().map(part))),
        ("m_constraints2", empty()),
        ("m_joints", super::joint::write(&a.joints)),
        ("m_boneParents", if boned { list(parents) } else { empty() }),
        ("m_surfacePropertyHashes", list(a.surfaces.iter().map(|&h| int_value(h)))),
        ("m_collisionAttributes", list(attributes)),
        ("m_debugPartNames", empty()),
        ("m_embeddedKeyvalues", text("")),
    ])
}

/// A standalone `.vphys_c` (resource version 1, as VPhysWriter.cs sets).
pub fn write(a: &Aggregate) -> Vec<u8> {
    super::write(1, &[], vec![(*b"DATA", super::kv3(&write_kv3(a)))])
}
