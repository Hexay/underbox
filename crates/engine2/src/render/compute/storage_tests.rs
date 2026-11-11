//! A `RWTexture2D<float4>` (DXC infers rgba32f) bound to an Rg32Float texture, as downsample_spd
//! writes managed's depth pyramid: the program is rebuilt for the bound format.

use crate::render::shading::program;
use render::{Texture, TextureDesc, wgpu};

const SHADER: &str = "HEADER { DevShader = true; }
MODES { Default(); }
FEATURES { }
COMMON { }
CS
{
	RWTexture2D<float4> Out < Attribute( \"Out\" ); >;

	[numthreads( 4, 4, 1 )]
	void MainCs( uint3 id : SV_DispatchThreadID )
	{
		Out[id.xy] = float4( id.x, id.y + 0.5, 9, 9 );
	}
}
";

#[test]
fn storage_images_take_the_bound_format() {
    let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
    let compiler = match shaders::Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("skipped: DXC unavailable ({e})"),
    };
    let shader = shaders::VfxShader::parse(SHADER, std::path::Path::new("storage_test.shader"), &shaders::IncludePaths::new(Vec::new())).unwrap();
    let declared = program::build_compute(gpu, &compiler, &shader, "storage_test", &shaders::Variant::default()).unwrap();
    let out = &declared.bindings[0];
    assert!(matches!(out.ty, wgpu::BindingType::StorageTexture { format: wgpu::TextureFormat::Rgba32Float, .. }), "{:?}", out.ty);

    let format = wgpu::TextureFormat::Rg32Float;
    let built = program::build_compute_for(gpu, &compiler, &shader, "storage_test", &shaders::Variant::default(), &[(out.group, out.binding, format)]).unwrap();
    let target = Texture::new(gpu, TextureDesc { render_target: false, ..TextureDesc::render_target("pyramid", 4, 4, format) });
    let entry = wgpu::BindGroupEntry { binding: out.binding, resource: wgpu::BindingResource::TextureView(&target.view) };
    let group = gpu.checked(|| gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &built.program.bind_group_layouts[out.group as usize], entries: &[entry] }));
    let group = group.expect("an Rg32Float view binds");
    gpu.checked(|| built.program.dispatch(gpu, &[group], [4, 4, 1])).unwrap();

    let texels: Vec<f32> = target.read_back(gpu).pixels.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    assert_eq!(&texels[..4], [0.0, 0.5, 1.0, 0.5], "row 0: (x, y + 0.5) per texel");
    assert_eq!(&texels[30..], [3.0, 3.5]);
}
