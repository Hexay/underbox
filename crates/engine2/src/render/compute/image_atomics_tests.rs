//! CMAA2's `InterlockedExchange` on a `RWTexture2D<uint>` (its blend-list heads) after shaders'
//! image_atomics.rs moves the texels to a word buffer (shading/texel_words.rs).

use crate::render::shading::{program, texel_words};
use render::{Buffer, BufferKind, Texture, TextureDesc, wgpu};

const SHADER: &str = "HEADER { DevShader = true; }
MODES { Default(); }
FEATURES { }
COMMON { }
CS
{
	RWTexture2D<uint> Heads < Attribute( \"Heads\" ); >;
	RWStructuredBuffer<uint> Out < Attribute( \"Out\" ); >;

	[numthreads( 64, 1, 1 )]
	void MainCs( uint3 id : SV_DispatchThreadID )
	{
		if ( id.x == 0 && id.y == 0 )
		{
			Heads[uint2( 1, 0 )] = 10;
			uint before;
			InterlockedAdd( Heads[uint2( 1, 0 )], 5, before );
			Out[64] = before;
		}
		if ( id.y == 1 )
		{
			// Every invocation pushes itself on one list: each must see a different previous head.
			uint previous;
			InterlockedExchange( Heads[uint2( 0, 0 )], id.x, previous );
			Out[id.x] = previous;
		}
	}
}
";

fn words(b: &[u8]) -> Vec<u32> {
    b.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect()
}

#[test]
fn texel_atomics_are_atomic_on_the_texture_words() {
    let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
    let compiler = match shaders::Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("skipped: DXC unavailable ({e})"),
    };
    let shader = shaders::VfxShader::parse(SHADER, std::path::Path::new("image_atomics_test.shader"), &shaders::IncludePaths::new(Vec::new())).unwrap();
    let built = program::build_compute(gpu, &compiler, &shader, "image_atomics_test", &shaders::Variant::default()).expect("the texel atomics translate");
    let heads = Texture::new(gpu, TextureDesc { render_target: false, ..TextureDesc::render_target("heads", 2, 1, wgpu::TextureFormat::R32Uint) });
    let heads_words = Buffer::with_data(gpu, "heads words", BufferKind::Storage, &[0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0]);
    let out = Buffer::new(gpu, "Out", BufferKind::Storage, 65 * 4);
    let view = &heads.view;
    let entries: Vec<_> = built
        .bindings
        .iter()
        .map(|b| {
            let resource = match b.name.as_str() {
                "Heads" => wgpu::BindingResource::TextureView(view),
                n if texel_words::image_of(n) == Some("Heads") => heads_words.buffer.as_entire_binding(),
                _ => out.buffer.as_entire_binding(),
            };
            wgpu::BindGroupEntry { binding: b.binding, resource }
        })
        .collect();
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &built.program.bind_group_layouts[0], entries: &entries });
    gpu.checked(|| built.program.dispatch(gpu, &[group], [1, 2, 1])).unwrap();
    let out = words(&out.read(gpu, 0, 65 * 4));
    assert_eq!(out[64], 10, "the add returns the value it replaced");
    let mut seen: Vec<u32> = out[..64].to_vec();
    let heads = words(&heads_words.read(gpu, 0, 8));
    assert_eq!(heads[1], 15);
    let head = heads[0];
    seen.push(head);
    seen.sort_unstable();
    let mut want: Vec<u32> = (0..64).collect();
    want.push(0xFFFF_FFFF);
    assert_eq!(seen, want, "the previous heads and the last head are each item once: no exchange lost");
}
