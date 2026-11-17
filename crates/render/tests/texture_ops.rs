//! Texture operations beyond draws: copies between mips and layers, mip generation, integer fills,
//! partial clears and volume textures.

use render::{Gpu, MipGenerator, PipelineKey, Program, RasterState, RectClearer, Subresource, Texture, TextureDesc, wgpu};
use std::sync::OnceLock;

const RGBA: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn gpu() -> &'static Gpu {
    static GPU: OnceLock<Gpu> = OnceLock::new();
    GPU.get_or_init(|| Gpu::headless().expect("a wgpu adapter (install mesa-vulkan-drivers for lavapipe)"))
}

fn desc(label: &str, size: u32, format: wgpu::TextureFormat, mips: u32, layers: u32) -> TextureDesc {
    TextureDesc { label: label.into(), width: size, height: size, format, mips, render_target: false, layers, cube: false, volume: false }
}

#[test]
fn copies_any_mip_and_layer() {
    let gpu = gpu();
    let src = Texture::new(gpu, desc("src", 4, RGBA, 2, 2));
    src.upload(gpu, &[0u8; 4 * 16 * 2]);
    let mip1: Vec<u8> = (0..2 * 16).map(|i| i as u8 + 1).collect();
    assert_eq!(src.upload_smaller_mips(gpu, &mip1), 1);
    let dst = Texture::new(gpu, desc("dst", 2, RGBA, 1, 3));
    let at = |mip, layer| Subresource { mip, layer };
    assert!(src.copy_region(gpu, &dst, (at(1, 1), at(0, 2)), 1, [0, 0, 2, 2], (0, 0)));
    let read_layer = |t: &Texture, layer: u32| {
        let out = Texture::new(gpu, desc("out", 2, RGBA, 1, 1));
        assert!(t.copy_region(gpu, &out, (at(0, layer), at(0, 0)), 1, [0, 0, 2, 2], (0, 0)));
        out.read_back(gpu).pixels
    };
    assert_eq!(read_layer(&dst, 2), mip1[16..], "mip 1 layer 1 into layer 2");
    assert_eq!(read_layer(&dst, 0), [0; 16], "other layers untouched");
    assert!(!src.copy_region(gpu, &dst, (at(2, 0), at(0, 0)), 1, [0, 0, 1, 1], (0, 0)), "no mip 2");
    assert!(!src.copy_region(gpu, &dst, (at(0, 0), at(0, 0)), 1, [0, 0, 4, 4], (0, 0)), "too big for dst");
}

#[test]
fn mips_average_and_srgb_averages_decoded() {
    let gpu = gpu();
    let generator = MipGenerator::new(gpu);
    for (format, half) in [(RGBA, 128u8), (wgpu::TextureFormat::Rgba8UnormSrgb, 188)] {
        let t = Texture::new(gpu, desc("mips", 4, format, 3, 2));
        let checker: Vec<u8> = (0..16 * 2).flat_map(|i| if (i % 4 + i / 4) % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        t.upload(gpu, &checker);
        assert!(generator.generate(gpu, &t), "{format:?}");
        for mip in [1, 2] {
            let (w, h) = t.mip_size(mip);
            let px = t.read_region(gpu, mip, [0, 0, w, h]).unwrap().pixels;
            assert!(px.chunks(4).all(|p| p[..3].iter().all(|c| c.abs_diff(half) <= 1) && p[3] == 255), "{format:?} mip {mip}: {px:?}");
        }
    }
    assert!(!generator.generate(gpu, &Texture::new(gpu, desc("one", 4, RGBA, 1, 1))), "a single mip has nothing to generate");
}

#[test]
fn fill_sets_every_mip_to_the_integer() {
    let gpu = gpu();
    let t = Texture::new(gpu, desc("uint", 3, wgpu::TextureFormat::R32Uint, 2, 1));
    assert!(t.fill(gpu, &0xdead_beefu32.to_le_bytes()));
    for mip in [0, 1] {
        let (w, h) = t.mip_size(mip);
        let px = t.read_region(gpu, mip, [0, 0, w, h]).unwrap().pixels;
        assert!(px.chunks(4).all(|p| u32::from_le_bytes(p.try_into().unwrap()) == 0xdead_beef), "mip {mip}");
    }
    assert!(!t.fill(gpu, &[0; 2]), "wrong texel size");
}

#[test]
fn rect_clears_leave_the_rest() {
    let gpu = gpu();
    let clearer = RectClearer::new(gpu);
    let color = Texture::new(gpu, TextureDesc::render_target("color", 8, 8, RGBA));
    color.clear(gpu, wgpu::Color::BLACK, 1.0);
    assert!(clearer.clear(gpu, &color, 0, [2, 3, 4, 2], wgpu::Color::RED, 0.0));
    let frame = color.read_back(gpu);
    for (x, y) in [(2, 3), (5, 4), (1, 3), (6, 3), (2, 2), (2, 5)] {
        let inside = (2..6).contains(&x) && (3..5).contains(&y);
        assert_eq!(frame.texel(x, y), if inside { [255, 0, 0, 255] } else { [0, 0, 0, 255] }, "({x}, {y})");
    }
    let depth = Texture::new(gpu, TextureDesc::depth("depth", 8, 8));
    depth.clear(gpu, wgpu::Color::BLACK, 1.0);
    assert!(clearer.clear(gpu, &depth, 0, [0, 0, 4, 8], wgpu::Color::BLACK, 0.25));
    let d = depth.read_back(gpu);
    let at = |x: u32| f32::from_le_bytes(d.pixels[(x * 4) as usize..(x * 4 + 4) as usize].try_into().unwrap());
    assert_eq!((at(3), at(4)), (0.25, 1.0));
    assert!(!clearer.clear(gpu, &color, 0, [6, 6, 4, 4], wgpu::Color::RED, 0.0), "outside the target");
}

#[test]
fn volumes_upload_copy_and_sample_as_3d() {
    let gpu = gpu();
    let volume = Texture::new(gpu, TextureDesc { volume: true, ..desc("volume", 2, RGBA, 2, 2) });
    assert_eq!(volume.dimension(), wgpu::TextureViewDimension::D3);
    assert_eq!((volume.mip_layers(0), volume.mip_layers(1)), (2, 1));
    let data: Vec<u8> = (0..32).collect();
    volume.upload(gpu, &data);
    assert_eq!(volume.upload_smaller_mips(gpu, &[9; 4]), 1, "mip 1 is one 1x1 slice");
    let slice = Texture::new(gpu, desc("slice", 2, RGBA, 1, 1));
    assert!(volume.copy_region(gpu, &slice, (Subresource { mip: 0, layer: 1 }, Subresource::default()), 1, [0, 0, 2, 2], (0, 0)));
    assert_eq!(slice.read_back(gpu).pixels, data[16..]);
    assert_eq!(sample_3d(gpu, &volume, [1, 0, 1]), data[20..24]);
}

/// The texel at `p` of `volume`, loaded through a `texture_3d` binding into a 1x1 target.
fn sample_3d(gpu: &Gpu, volume: &Texture, [x, y, z]: [u32; 3]) -> Vec<u8> {
    let wgsl = format!(
        "@group(0) @binding(0) var v: texture_3d<f32>;
         @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
             let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
             return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
         }}
         @fragment fn fs() -> @location(0) vec4<f32> {{ return textureLoad(v, vec3<i32>({x}, {y}, {z}), 0); }}"
    );
    let ty = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: volume.dimension(), multisampled: false };
    let entry = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
    let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[entry] });
    let program = Program::new(gpu, "sample 3d", &wgsl, vec![layout], ("vs", "fs"));
    let target = Texture::new(gpu, TextureDesc::render_target("out", 1, 1, RGBA));
    let key = PipelineKey { color: Some(RGBA), depth: None, vertex: Vec::new(), state: RasterState { depth_write: false, ..Default::default() } };
    let pipeline = program.pipeline_for(gpu, &key);
    let resource = wgpu::BindingResource::TextureView(&volume.view);
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &program.bind_group_layouts[0], entries: &[wgpu::BindGroupEntry { binding: 0, resource }] });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let ops = wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store };
        let attachment = wgpu::RenderPassColorAttachment { view: &target.target_view, depth_slice: None, resolve_target: None, ops };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { color_attachments: &[Some(attachment)], ..Default::default() });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    gpu.submit(encoder.finish());
    target.read_back(gpu).pixels
}
