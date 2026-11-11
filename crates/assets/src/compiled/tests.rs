use super::fixture::{self, CHECKER, HALF, MATERIAL, SKIN_MATERIAL, TEXTURE};
use super::texture::{self, Format, Pixels, Texture};
use super::vbib::Packing;
use super::{material, model, physics, references};

fn no_files(_: &str) -> Option<Vec<u8>> {
    None
}

#[test]
fn model_round_trips_in_every_packing() {
    let want = fixture::model();
    for packing in [Packing::Raw, Packing::Meshopt, Packing::MeshoptZstd] {
        let got = model::read(&model::write(&want, packing), &no_files).unwrap();
        for (g, w) in got.meshes.iter().zip(&want.meshes) {
            assert_eq!((&g.name, g.group_mask, g.lod_mask), (&w.name, w.group_mask, w.lod_mask));
            assert_eq!(g.mesh.vertices, w.mesh.vertices, "{packing:?}");
            assert_eq!((&g.mesh.indices, &g.mesh.draws, g.mesh.bounds), (&w.mesh.indices, &w.mesh.draws, w.mesh.bounds), "{packing:?}");
        }
        assert_eq!(got.physics, want.physics);
        assert_eq!(model::Model { meshes: vec![], physics: None, ..got }, model::Model { meshes: vec![], physics: None, ..want.clone() });
    }
}

#[test]
fn model_lists_its_materials_as_references() {
    let file = model::write(&fixture::model(), Packing::Raw);
    let refs: Vec<String> = references(&file).unwrap().into_iter().map(|(_, p)| p).collect();
    assert!(refs.contains(&MATERIAL.to_owned()) && refs.contains(&SKIN_MATERIAL.to_owned()), "{refs:?}");
}

#[test]
fn crate_mesh_is_a_closed_box_with_two_draws() {
    let m = &fixture::model().meshes[0].mesh;
    assert_eq!((m.vertices.len(), m.indices.len(), m.draws.len()), (24, 36, 2));
    assert!(m.vertices.iter().all(|v| v.position[2] >= 0.0 && v.position[2] <= 2.0 * HALF));
    assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
}

#[test]
fn external_meshes_load_through_the_callback_or_are_reported_missing() {
    let mut m = fixture::model();
    m.physics = None;
    let file = model::write(&m, Packing::Raw);
    let got = model::read(&file, &no_files).unwrap();
    assert!(got.missing.is_empty());
    assert_eq!(got.meshes.len(), 2);
}

#[test]
fn material_round_trips_and_references_its_texture() {
    let want = fixture::material(MATERIAL, [1.0, 0.5, 0.25, 0.0]);
    let file = material::write(&want);
    assert_eq!(material::read(&file).unwrap(), want);
    assert_eq!(references(&file).unwrap().into_iter().map(|(_, p)| p).collect::<Vec<_>>(), [TEXTURE]);
}

#[test]
fn physics_round_trips() {
    let want = fixture::world_physics(512.0, &[([-8.0, -8.0, 0.0], [8.0, 8.0, 16.0])]);
    assert_eq!(physics::read(&physics::write(&want)).unwrap(), want);
}

#[test]
fn texture_round_trips_compressed_and_raw() {
    let want = fixture::texture();
    assert_eq!(want.levels.len(), 5);
    for compress in [false, true] {
        assert_eq!(texture::read(&texture::write(&want, compress)).unwrap(), want, "compress {compress}");
    }
}

/// `t` laid out as real engine2's compiler writes it: zstd mip 0, raw smaller mips, and a
/// compressed-mip entry that declares only its 12-byte head with the size array after it.
fn real_zstd_vtex(t: &Texture) -> Vec<u8> {
    let zstd = |l: &[u8]| ruzstd::encoding::compress_to_vec(l, ruzstd::encoding::CompressionLevel::Fastest);
    let stored: Vec<Vec<u8>> = t.levels.iter().enumerate().map(|(i, l)| if i == 0 { zstd(l) } else { l.clone() }).collect();
    assert!(stored[0].len() < t.levels[0].len());
    let mut data = vec![1, 0];
    data.extend(t.flags.to_le_bytes());
    t.reflectivity.iter().for_each(|f| data.extend(f.to_le_bytes()));
    [t.width, t.height, t.depth].iter().for_each(|v| data.extend(v.to_le_bytes()));
    data.extend([t.format.0, t.levels.len() as u8]);
    [0, 8, 1, 4, 8, 12, 2, 8, stored.len() as u32].iter().for_each(|v| data.extend(v.to_le_bytes()));
    stored.iter().for_each(|s| data.extend((s.len() as u32).to_le_bytes()));
    let mut file = super::write(1, &[], vec![(*b"DATA", data)]);
    stored.iter().rev().for_each(|s| file.extend(s));
    file
}

#[test]
fn real_zstd_mips_read_like_ours() {
    let want = fixture::texture();
    assert_eq!(texture::read(&real_zstd_vtex(&want)).unwrap(), want);
}

/// With `UNDERBOX_REAL_CORE=<real engine2's game/core>`, every compiled core texture reads.
#[test]
fn real_core_textures_read() {
    let Some(core) = std::env::var_os("UNDERBOX_REAL_CORE") else { return };
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out) } else if p.extension().is_some_and(|x| x == "vtex_c") { out.push(p) }
        }
    }
    let mut files = Vec::new();
    walk(std::path::Path::new(&core), &mut files);
    let failed: Vec<_> = files.iter().filter(|f| texture::read(&std::fs::read(f).unwrap()).is_err()).collect();
    assert!(failed.is_empty(), "{} of {} unreadable, e.g. {:?}", failed.len(), files.len(), &failed[..failed.len().min(5)]);
}

#[test]
fn bc1_checker_decodes_to_its_colours() {
    let t = texture::read(&texture::write(&fixture::texture(), true)).unwrap();
    let image = texture::decode(&t, 0, 0, 0).unwrap();
    let Pixels::Rgba8(px) = image.pixels else { panic!("BC1 decodes to RGBA8") };
    let near = |a: &[u8], b: &[u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 8);
    assert!(near(&px[0..4], &CHECKER[0]) && near(&px[4 * 4..4 * 4 + 4], &CHECKER[1]));
    let smallest = texture::decode(&t, 4, 0, 0).unwrap();
    assert_eq!((smallest.width, smallest.height), (1, 1));
}

#[test]
fn cube_faces_are_separate_surfaces() {
    let face = |i: u8| vec![i * 40, 0, 0, 255];
    let t = Texture { flags: texture::flags::CUBE, width: 1, height: 1, depth: 1, format: Format::RGBA8888, levels: vec![(0..6).flat_map(face).collect()], ..Texture::default() };
    let t = texture::read(&texture::write(&t, false)).unwrap();
    for f in 0..6 {
        let Pixels::Rgba8(px) = texture::decode(&t, 0, 0, f).unwrap().pixels else { panic!() };
        assert_eq!(px, face(f as u8));
    }
    assert!(texture::decode(&t, 0, 0, 6).is_none());
}

#[test]
fn hdr_formats_decode_to_half_floats() {
    let texel: Vec<u8> = [2.5f32, 0.5, 0.0, 1.0].iter().flat_map(|f| f.to_le_bytes()).collect();
    let t = Texture { width: 1, height: 1, depth: 1, format: Format::RGBA32323232F, levels: vec![texel], ..Texture::default() };
    let Pixels::Rgba16F(px) = texture::decode(&t, 0, 0, 0).unwrap().pixels else { panic!() };
    assert_eq!(half::f16::from_bits(px[0]).to_f32(), 2.5);
}

#[test]
fn bc6h_block_decodes_without_clamping() {
    // Mode 3 (11-bit endpoints), all endpoint bits set: both endpoints are the brightest value.
    let mut block = [0xffu8; 16];
    block[0] = 0b0_0011;
    let t = Texture { width: 4, height: 4, depth: 1, format: Format::BC6H, levels: vec![block.to_vec()], ..Texture::default() };
    let Pixels::Rgba16F(px) = texture::decode(&t, 0, 0, 0).unwrap().pixels else { panic!() };
    assert!(half::f16::from_bits(px[0]).to_f32() > 1.0, "{}", half::f16::from_bits(px[0]));
}
