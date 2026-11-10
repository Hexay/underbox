use super::*;

fn core(path: &str) -> Option<String> {
    let root = std::env::var("SBOX_PUBLIC").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../../sbox-public").into());
    std::fs::read_to_string(format!("{root}/game/core/{path}")).ok()
}

#[test]
fn dev_box_is_a_closed_50_unit_cube() {
    let Some(src) = core("models/dev/box.vmdl") else { return eprintln!("skipped: no sbox-public") };
    let model = from_vmdl(&src).unwrap();
    assert_eq!(model.meshes.len(), 1);
    let mesh = &model.meshes[0];
    assert_eq!(mesh.material, "materials/dev/primary_white.vmat");
    assert_eq!((mesh.vertices.len(), mesh.indices.len()), (24, 36), "6 faces × (4 verts, 2 tris)");
    assert_eq!(model.bounds(), Some(([-25.0; 3], [25.0; 3])));
    // Every triangle winds counter-clockwise about its vertices' outward normal.
    for t in mesh.indices.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[t[i] as usize]);
        let (u, v) = ([0, 1, 2].map(|i| b.position[i] - a.position[i]), [0, 1, 2].map(|i| c.position[i] - a.position[i]));
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        assert!(n.iter().zip(a.normal).map(|(x, y)| x * y).sum::<f32>() > 0.0, "inward triangle {t:?}");
        assert!(a.position.iter().zip(a.normal).map(|(p, n)| p * n).sum::<f32>() > 0.0, "inward normal");
    }
}

#[test]
fn dev_plane_and_sphere() {
    let Some(plane) = core("models/dev/plane.vmdl") else { return };
    assert_eq!(from_vmdl(&plane).unwrap().bounds(), Some(([-50.0, -50.0, 0.0], [50.0, 50.0, 0.0])));
    let sphere = from_vmdl(&core("models/dev/sphere.vmdl").unwrap()).unwrap();
    let (lo, hi) = sphere.bounds().unwrap();
    assert!((hi[2] - 32.0).abs() < 1e-3 && (lo[2] + 32.0).abs() < 1e-3);
}

#[test]
fn mesh_files_are_reported_missing() {
    let Some(src) = core("models/dev/error.vmdl") else { return };
    let model = from_vmdl(&src).unwrap();
    assert!(model.meshes.is_empty());
    assert!(model.missing.iter().any(|f| f.ends_with("error_mesh.dmx")));
}

#[test]
fn angles_rotate_yaw_about_z() {
    let m = primitive::rotation([0.0, 90.0, 0.0]);
    let x = [0, 1, 2].map(|i| m[i][0]);
    assert!((x[0]).abs() < 1e-6 && (x[1] - 1.0).abs() < 1e-6, "yaw 90 takes +X to +Y: {x:?}");
}
