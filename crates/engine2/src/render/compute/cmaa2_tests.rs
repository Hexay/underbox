//! Core's `cmaa2_cs` run headless the way managed's `Cmaa2Renderer.Render` dispatches it, over a
//! synthetic frame of hard diagonal edges. Pass 0 clears the blend-list heads with plain image
//! writes, pass 2 pushes onto them atomically (texel words) and pass 3 walks them by plain reads,
//! so the words must round-trip through the image (shading/texel_words.rs). Before they did, pass
//! 2 chained onto last frame's stale heads and pass 3 saw only empty lists.

use super::gpu::{self, Work};
use crate::render::attributes::{self, Value};
use crate::render::shading::tests::core_dir;
use crate::render::{buffers, texture};
use std::sync::Arc;

const SHADER: &str = "postprocess/cmaa2_cs";
const SIZE: u32 = 64;
const RGBA8888: i32 = 0;
const R32_UINT: i32 = 87;

/// A white disc and a slanted white bar on black: long, aliased edges at many slopes.
fn scene() -> Vec<u8> {
    let mut px = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let disc = (x - 22).pow(2) + (y - 24).pow(2) < 15 * 15;
            let bar = (3 * x - 7 * y + 120).abs() < 24 && x > 36;
            let v = if disc || bar { 255 } else { 0 };
            px.extend([v, v, v, 255]);
        }
    }
    px
}

struct Frame {
    color: texture::Ref,
    edges: texture::Ref,
    heads: texture::Ref,
    buffers: Vec<(&'static str, buffers::Handle, u32)>,
}

impl Frame {
    fn new(gpu: &render::Gpu) -> Frame {
        let pixels = SIZE * SIZE;
        let storage = |name: &str, elements: u32, stride: u32| {
            let b = render::Buffer::new(gpu, name, render::BufferKind::Storage, u64::from(elements * stride));
            buffers::insert(Arc::new(b))
        };
        let buffers = vec![
            ("Cmaa2Candidates", storage("candidates", pixels / 4, 4), pixels / 4),
            ("Cmaa2BlendLocations", storage("locations", pixels.div_ceil(6), 4), pixels.div_ceil(6)),
            ("Cmaa2BlendItems", storage("items", pixels / 2, 8), pixels / 2),
            ("Cmaa2Control", storage("control", 16, 4), 16),
            ("Cmaa2DispatchArgs", storage("args", 1, 12), 1),
        ];
        Frame {
            color: texture::create_target("cmaa2 color", SIZE, SIZE, RGBA8888),
            edges: texture::create_target("cmaa2 edges", SIZE.div_ceil(2), SIZE, R32_UINT),
            heads: texture::create_target("cmaa2 heads", SIZE.div_ceil(2), SIZE.div_ceil(2), R32_UINT),
            buffers,
        }
    }

    fn attributes(&self, pass: u8, color: bool, output: bool) -> attributes::Ref {
        let a = attributes::create();
        attributes::set(a, "D_PASS", Value::Combo(pass));
        attributes::set(a, "D_SRGB", Value::Combo(1));
        attributes::set(a, "Cmaa2Dimensions", Value::IntVector4([SIZE as i32, SIZE as i32, 0, 0]));
        for (name, handle, elements) in &self.buffers {
            attributes::set(a, name, Value::Buffer(handle.value));
            match *name {
                "Cmaa2Candidates" => attributes::set(a, "Cmaa2CandidateCapacity", Value::Int(*elements as i32)),
                "Cmaa2BlendLocations" => attributes::set(a, "Cmaa2BlendLocationCapacity", Value::Int(*elements as i32)),
                "Cmaa2BlendItems" => attributes::set(a, "Cmaa2BlendItemCapacity", Value::Int(*elements as i32)),
                _ => {}
            }
        }
        attributes::set(a, "Cmaa2Edges", Value::Texture(self.edges, -1));
        attributes::set(a, "Cmaa2BlendHeads", Value::Texture(self.heads, -1));
        if color {
            attributes::set(a, "Cmaa2Color", Value::Texture(self.color, -1));
        }
        if output {
            attributes::set(a, "Cmaa2Output", Value::Texture(self.color, -1));
        }
        a
    }

    fn buffer(&self, name: &str) -> Arc<render::Buffer> {
        let (_, h, _) = self.buffers.iter().find(|(n, ..)| *n == name).unwrap();
        buffers::get(*h).unwrap()
    }

    /// One `Cmaa2Renderer.Render` over `input`; the anti-aliased frame.
    fn render(&self, gpu: &render::Gpu, input: &[u8]) -> Vec<u8> {
        let color = texture::resolve(self.color).unwrap();
        let color = color.gpu.as_ref().unwrap();
        color.upload(gpu, input);
        self.buffer("Cmaa2Control").fill(gpu, 0);
        let args = self.buffer("Cmaa2DispatchArgs");
        let run = |pass: u8, c: bool, o: bool, work: Work| gpu::dispatch(gpu, SHADER, &[self.attributes(pass, c, o)], work).unwrap();
        let groups = |n: u32| n.div_ceil(28) * 16;
        run(0, true, false, Work::Threads([groups(SIZE), groups(SIZE), 1]));
        run(1, false, false, Work::Threads([2, 1, 1]));
        run(2, true, false, Work::Indirect(&args.buffer, 0));
        run(1, false, false, Work::Threads([1, 2, 1]));
        run(3, false, true, Work::Indirect(&args.buffer, 0));
        color.read_back(gpu).pixels
    }
}

#[test]
fn cmaa2_blends_edges_and_repeats_exactly() {
    let (Some(core), Some(gpu)) = (core_dir(), crate::render::device::create()) else {
        return eprintln!("skipped: no sbox-public checkout or GPU");
    };
    crate::fs::add_search_path("GAME", &core.to_string_lossy(), false);
    let input = scene();
    let frame = Frame::new(gpu);
    let first = frame.render(gpu, &input);
    let changed = first.chunks(4).zip(input.chunks(4)).filter(|(a, b)| a != b).count();
    assert!(changed > 20, "CMAA2 blends the aliased edges: {changed} pixels changed");
    let flat = first.chunks(4).zip(input.chunks(4)).filter(|(a, b)| a != b && (b[0] == 0 || b[0] == 255) && (a[0] == 0 || a[0] == 255));
    assert_eq!(flat.count(), 0, "a changed pixel takes a blend, never the other flat colour");
    for run in 1..8 {
        let again = frame.render(gpu, &input);
        let diff = again.iter().zip(&first).filter(|(a, b)| a != b).count();
        assert_eq!(diff, 0, "run {run} differs from the first in {diff} bytes");
    }
}
