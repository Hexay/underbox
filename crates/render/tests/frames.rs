//! Frame-hash tests: draw known scenes offscreen, read them back, compare with a CPU reference
//! and with the recorded per-driver hash (tests/golden/frames.tsv).

use glam::{Mat4, Vec3};
use render::{Buffer, BufferKind, Camera, DrawItem, ForwardPass, Frame, Gpu, Mesh, Texture, TextureDesc, wgpu};
use std::path::PathBuf;
use std::sync::OnceLock;

const SIZE: u32 = 64;
const RGBA: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn gpu() -> &'static Gpu {
    static GPU: OnceLock<Gpu> = OnceLock::new();
    GPU.get_or_init(|| {
        let gpu = Gpu::headless().expect("a wgpu adapter (install mesa-vulkan-drivers for lavapipe)");
        eprintln!("render tests on {}", gpu.describe());
        gpu
    })
}

fn targets(gpu: &Gpu) -> (Texture, Texture) {
    (Texture::new(gpu, TextureDesc::render_target("color", SIZE, SIZE, RGBA)), Texture::new(gpu, TextureDesc::depth("depth", SIZE, SIZE)))
}

/// Compare with the reference exactly (edges sit on pixel boundaries), then with the golden hash.
fn assert_frame(name: &str, frame: &Frame, reference: &Frame) {
    let cmp = frame.compare(reference, 1);
    if cmp.differing != 0 {
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.ppm"));
        frame.write_ppm(&out).unwrap();
        panic!("{name}: {cmp:?} against the reference; frame written to {}", out.display());
    }
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/frames.tsv");
    let checked = frame.check_golden(&golden, name, &gpu().describe());
    eprintln!("{name}: hash {} ({})", frame.hash(), if checked { "golden checked" } else { "no golden for this adapter" });
}

const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

#[test]
fn clear_reads_back() {
    let gpu = gpu();
    let (color, _) = targets(gpu);
    color.clear(gpu, wgpu::Color { r: 1.0, g: 0.5, b: 0.0, a: 1.0 }, 1.0);
    let frame = color.read_back(gpu);
    assert_eq!(frame.compare(&Frame::filled(SIZE, SIZE, &[255, 128, 0, 255]), 1).differing, 0);
}

#[test]
fn upload_round_trips() {
    let gpu = gpu();
    let tex = Texture::new(gpu, TextureDesc { label: "upload".into(), width: 3, height: 2, format: RGBA, mips: 1, render_target: false });
    let data: Vec<u8> = (0..24).collect();
    tex.upload(gpu, &data);
    assert_eq!(tex.read_back(gpu).pixels, data);
}

#[test]
fn buffer_round_trips() {
    let gpu = gpu();
    let buf = Buffer::with_data(gpu, "roundtrip", BufferKind::Storage, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(buf.size(), 12);
    assert_eq!(buf.read(gpu, 2, 5), vec![3, 4, 5, 6, 7]);
}

/// A red quad over the left half and a green quad over the top-right quarter, drawn green first
/// but nearer the camera, so the depth test (not draw order) decides the overlap.
#[test]
fn forward_quads_depth_tested() {
    let gpu = gpu();
    let (color, depth) = targets(gpu);
    let pass = ForwardPass::new(gpu);
    let red = Mesh::quad(gpu, [-1.0, -1.0], [0.0, 1.0], -2.0, [1.0, 0.0, 0.0, 1.0]);
    let green = Mesh::quad(gpu, [-0.5, 0.0], [1.0, 1.0], -1.0, [0.0, 1.0, 0.0, 1.0]);
    let items = [
        DrawItem { mesh: &green, transform: Mat4::IDENTITY, tint: [1.0; 4] },
        DrawItem { mesh: &red, transform: Mat4::IDENTITY, tint: [1.0; 4] },
    ];
    pass.render(gpu, &color, &depth, Some(wgpu::Color::BLACK), &Camera::ortho(1.0, 1.0, 0.1, 10.0), &items);
    let frame = color.read_back(gpu);

    let mut want = Frame::filled(SIZE, SIZE, &BLACK);
    want.fill_rect(0.0, 0.0, 32.0, 64.0, &RED);
    want.fill_rect(16.0, 0.0, 64.0, 32.0, &GREEN);
    assert_frame("forward_quads_depth_tested", &frame, &want);
}

/// Per-draw transforms and tints: one quad mesh drawn twice, scaled, moved and tinted.
#[test]
fn forward_transforms_and_tints() {
    let gpu = gpu();
    let (color, depth) = targets(gpu);
    let pass = ForwardPass::new(gpu);
    let unit = Mesh::quad(gpu, [-0.5, -0.5], [0.5, 0.5], 0.0, [1.0; 4]);
    let at = |x, y| Mat4::from_translation(Vec3::new(x, y, -1.0)) * Mat4::from_scale(Vec3::splat(0.5));
    let items = [
        DrawItem { mesh: &unit, transform: at(-0.5, -0.5), tint: [0.0, 0.0, 1.0, 1.0] },
        DrawItem { mesh: &unit, transform: at(0.5, 0.5), tint: [1.0, 1.0, 0.0, 1.0] },
    ];
    pass.render(gpu, &color, &depth, Some(wgpu::Color::BLACK), &Camera::ortho(1.0, 1.0, 0.1, 10.0), &items);
    let frame = color.read_back(gpu);

    let mut want = Frame::filled(SIZE, SIZE, &BLACK);
    want.fill_rect(8.0, 40.0, 24.0, 56.0, &[0, 0, 255, 255]);
    want.fill_rect(40.0, 8.0, 56.0, 24.0, &[255, 255, 0, 255]);
    assert_frame("forward_transforms_and_tints", &frame, &want);
}

/// A perspective view has no exact CPU reference, so this checks determinism and coarse shape:
/// the same scene twice hashes the same, and the centre is covered while the corners are not.
#[test]
fn perspective_is_deterministic() {
    let gpu = gpu();
    let pass = ForwardPass::new(gpu);
    let quad = Mesh::quad(gpu, [-1.0, -1.0], [1.0, 1.0], 0.0, [0.2, 0.6, 1.0, 1.0]);
    let camera = Camera::perspective(60f32.to_radians(), 1.0, 0.1, Vec3::new(1.0, 1.5, 4.0), Vec3::ZERO);
    let draw = || {
        let (color, depth) = targets(gpu);
        let items = [DrawItem { mesh: &quad, transform: Mat4::from_rotation_z(0.3), tint: [1.0; 4] }];
        pass.render(gpu, &color, &depth, Some(wgpu::Color::BLACK), &camera, &items);
        color.read_back(gpu)
    };
    let (a, b) = (draw(), draw());
    assert_eq!(a.hash(), b.hash());
    assert_ne!(a.texel(SIZE / 2, SIZE / 2), BLACK);
    assert_eq!(a.texel(0, 0), BLACK);
    let golden = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/frames.tsv");
    a.check_golden(&golden, "perspective_is_deterministic", &gpu.describe());
}
