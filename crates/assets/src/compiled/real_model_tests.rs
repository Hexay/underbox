//! With `UNDERBOX_REAL_CORE=<real engine2's game/core>`, real's compiled core models through our
//! reader: their packed tangent frames (`vertex::unpack_frame`) against the geometry.

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { walk(&p, out) } else if p.extension().is_some_and(|x| x == "vmdl_c") { out.push(p) }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| a[i] * b[i]).sum()
}

fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    a.map(|x| x / l)
}

/// Per vertex: the summed face normal and UV-gradient tangent of its triangles.
fn geometry(mesh: &super::mesh::Mesh) -> Vec<([f32; 3], [f32; 3])> {
    let v = &mesh.vertices;
    let mut out = vec![([0.0f32; 3], [0.0f32; 3]); v.len()];
    for t in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| i as usize);
        let (e1, e2) = (sub(v[b].position, v[a].position), sub(v[c].position, v[a].position));
        let uv = |i: usize, k: usize| v[i].uv[0][k] - v[a].uv[0][k];
        let r = uv(b, 0) * uv(c, 1) - uv(c, 0) * uv(b, 1);
        let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        let t: [f32; 3] = std::array::from_fn(|k| if r.abs() > 1e-12 { (e1[k] * uv(c, 1) - e2[k] * uv(b, 1)) / r } else { 0.0 });
        for i in [a, b, c] {
            out[i] = (std::array::from_fn(|k| out[i].0[k] + n[k]), std::array::from_fn(|k| out[i].1[k] + t[k]));
        }
    }
    out
}

#[test]
fn real_core_tangent_frames_match_their_geometry() {
    let Some(core) = std::env::var_os("UNDERBOX_REAL_CORE") else { return };
    let core = std::path::PathBuf::from(core);
    let mut files = Vec::new();
    walk(&core.join("models"), &mut files);
    let (mut normals, mut tangents, mut count) = (0.0, 0.0, 0);
    for f in &files {
        let load = |p: &str| std::fs::read(core.join(p)).ok();
        let m = super::model::read(&std::fs::read(f).unwrap(), &load).unwrap();
        for mesh in m.meshes.iter().map(|m| &m.mesh) {
            for (v, (n, t)) in mesh.vertices.iter().zip(geometry(mesh)) {
                // Mirrored and double-sided helper geometry says nothing about the decode.
                let n = unit(n);
                if dot(v.normal, n) < 0.95 || dot(t, t) < 1e-6 {
                    continue;
                }
                let t = unit(sub(t, v.normal.map(|x| x * dot(t, v.normal))));
                normals += dot(v.normal, n);
                tangents += dot([v.tangent[0], v.tangent[1], v.tangent[2]], t);
                count += 1;
            }
        }
    }
    assert!(count > 20_000, "only {count} vertices face out of {} models", files.len());
    let (normals, tangents) = (normals / count as f32, tangents / count as f32);
    // Real's compiler stores a tangent exactly opposite the basis axis as the axis itself (0.6% of vertices).
    assert!(normals > 0.999 && tangents > 0.98, "normals {normals}, tangents {tangents}");
}
