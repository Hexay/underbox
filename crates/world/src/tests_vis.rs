//! The fixture's world visibility: written, read back from the pak, and queried.

use crate::fixture;
use crate::vis::{self, Visibility};
use crate::vpk::Vpk;

fn tiny_vis() -> Visibility {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("underbox_world_vis_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tiny.vpk");
    std::fs::write(&path, fixture::tiny_map("tiny", |_| 0).single()).unwrap();
    let file = Vpk::open(&path).unwrap().read("maps/tiny/world_visibility.vvis_c").unwrap().unwrap();
    std::fs::remove_dir_all(&dir).ok();
    Visibility::parse(&file).unwrap().expect("three clusters cull")
}

#[test]
fn round_trips_through_the_pak() {
    assert_eq!(tiny_vis(), fixture::visibility());
}

#[test]
fn points_find_their_cluster_and_pvs() {
    let v = tiny_vis();
    assert_eq!([-400.0, -100.0, 200.0, 511.0].map(|x| v.cluster_at([x, 0.0, 0.0])), [Some(0), Some(1), Some(1), Some(2)]);
    assert_eq!(v.cluster_at([0.0, 0.0, 600.0]), None, "outside the octree");
    assert_eq!((v.pvs_at([-400.0, 0.0, 0.0]), v.pvs_at([0.0, 0.0, 900.0])), (vec![0b011], vec![0xFF]), "outside sees everything");
    let (west, east) = ([-400.0, 0.0, 0.0], [400.0, 0.0, 0.0]);
    assert!(!v.point_visible(&[west], east) && !v.point_visible(&[east], west));
    assert!(v.point_visible(&[west], [-100.0, 50.0, 0.0]) && v.point_visible(&[west, east], east), "any source will do");
    assert!(v.point_visible(&[west], [0.0, 0.0, 700.0]), "a point in no cluster isn't culled");
}

#[test]
fn boxes_take_every_cluster_they_reach() {
    let v = tiny_vis();
    assert_eq!(v.clusters_in_box([300.0, 0.0, 0.0], [320.0, 10.0, 10.0]), [2]);
    assert_eq!(v.clusters_in_box([-300.0, 0.0, 0.0], [-200.0, 10.0, 10.0]), [0, 1], "straddles a cell edge");
    assert_eq!(v.clusters_in_box([-600.0; 3], [600.0; 3]), [0], "the whole world is cluster 0");
    let west = [-400.0, 0.0, 0.0];
    assert!(!v.box_visible(&[west], [300.0, 0.0, 0.0], [320.0, 10.0, 10.0]));
    assert!(v.box_visible(&[west], [200.0, 0.0, 0.0], [300.0, 10.0, 10.0]), "its west end is in view");
}

#[test]
fn sky_and_sun_rows() {
    let v = tiny_vis();
    let at = |x: f32| [x, 0.0, 0.0];
    assert_eq!([-400.0, 0.0, 400.0].map(|x| v.sky_visible(at(x), false)), [false, false, true]);
    assert_eq!([-400.0, 0.0, 400.0].map(|x| v.sky_visible(at(x), true)), [false, true, true]);
}

#[test]
fn small_octrees_and_other_layouts_cull_nothing() {
    let two = Visibility { clusters: 2, rows: vec![0b11, 0b11], ..fixture::visibility() };
    assert_eq!(Visibility::parse(&vis::write(&two)).unwrap(), None);
    let bsp = crate::resource::write(&kv::Value::Object(kv::Object(vec![("m_visDataDwords".into(), kv::Value::Array(vec![]))])), assets::kv3bin::Compression::None);
    assert_eq!(Visibility::parse(&bsp).unwrap(), None);
}
