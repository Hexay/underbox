//! CMAA2's `InterlockedExchange` on a `RWTexture2D<uint>` (its blend-list heads) runs after the
//! SPIR-V pass lowers it to an image read and write (shaders' image_atomics.rs). One thread, so
//! the non-atomic lowering is exact here.

use crate::render::shading::program;
use render::{Buffer, BufferKind, Texture, TextureDesc, wgpu};

const SHADER: &str = "HEADER { DevShader = true; }
MODES { Default(); }
FEATURES { }
COMMON { }
CS
{
	RWTexture2D<uint> Heads < Attribute( \"Heads\" ); >;
	RWStructuredBuffer<uint> Out < Attribute( \"Out\" ); >;

	[numthreads( 1, 1, 1 )]
	void MainCs( uint3 id : SV_DispatchThreadID )
	{
		Heads[uint2( 0, 0 )] = 0xFFFFFFFF;
		Heads[uint2( 1, 0 )] = 10;
		for ( uint k = 0; k < 3; k++ )
		{
			uint previous;
			InterlockedExchange( Heads[uint2( 0, 0 )], k, previous );
			Out[k] = previous;
		}
		uint before;
		InterlockedAdd( Heads[uint2( 1, 0 )], 5, before );
		Out[3] = before;
	}
}
";

#[test]
fn texel_exchanges_return_the_previous_head() {
    let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
    let compiler = match shaders::Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("skipped: DXC unavailable ({e})"),
    };
    let shader = shaders::VfxShader::parse(SHADER, std::path::Path::new("image_atomics_test.shader"), &shaders::IncludePaths::new(Vec::new())).unwrap();
    let built = program::build_compute(gpu, &compiler, &shader, "image_atomics_test", &shaders::Variant::default()).expect("the texel atomics translate");
    let heads = Texture::new(gpu, TextureDesc { render_target: false, ..TextureDesc::render_target("heads", 2, 1, wgpu::TextureFormat::R32Uint) });
    let out = Buffer::new(gpu, "Out", BufferKind::Storage, 16);
    let entries: Vec<_> = built
        .bindings
        .iter()
        .map(|b| {
            let resource = if b.name == "Heads" { wgpu::BindingResource::TextureView(&heads.view) } else { out.buffer.as_entire_binding() };
            wgpu::BindGroupEntry { binding: b.binding, resource }
        })
        .collect();
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &built.program.bind_group_layouts[0], entries: &entries });
    gpu.checked(|| built.program.dispatch(gpu, &[group], [1, 1, 1])).unwrap();
    let words = |b: &[u8]| -> Vec<u32> { b.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect() };
    assert_eq!(words(&out.read(gpu, 0, 16)), [0xFFFF_FFFF, 0, 1, 10], "each exchange returns the head it replaced; the add its old value");
    assert_eq!(words(&heads.read_back(gpu).pixels), [2, 15]);
}
