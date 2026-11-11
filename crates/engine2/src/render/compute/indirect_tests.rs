//! `RenderTools.ComputeIndirect`: the workgroup counts come from a GPU buffer at a byte offset
//! (managed passes `element * 12`), as CMAA2's blend passes size themselves.

use crate::render::shading::program;
use render::{Buffer, BufferKind, wgpu};

const SHADER: &str = "HEADER { DevShader = true; }
MODES { Default(); }
FEATURES { }
COMMON { }
CS
{
	RWStructuredBuffer<uint> Out < Attribute( \"Out\" ); >;

	[numthreads( 2, 1, 1 )]
	void MainCs( uint3 id : SV_DispatchThreadID )
	{
		Out[id.x] = id.x + 1;
	}
}
";

#[test]
fn indirect_dispatch_reads_group_counts_at_the_offset() {
    let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
    let compiler = match shaders::Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("skipped: DXC unavailable ({e})"),
    };
    let shader = shaders::VfxShader::parse(SHADER, std::path::Path::new("indirect_test.shader"), &shaders::IncludePaths::new(Vec::new())).unwrap();
    let built = program::build_compute(gpu, &compiler, &shader, "indirect_test", &shaders::Variant::default()).unwrap();
    let out = Buffer::new(gpu, "Out", BufferKind::Storage, 32);
    // Element 0 asks for 4 groups, element 1 (offset 12) for 3.
    let args: Vec<u8> = [4u32, 1, 1, 3, 1, 1].iter().flat_map(|w| w.to_le_bytes()).collect();
    let args = Buffer::with_data(gpu, "args", BufferKind::Storage, &args);
    let layout = &built.program.bind_group_layouts[0];
    let entries: Vec<_> = built.bindings.iter().map(|b| wgpu::BindGroupEntry { binding: b.binding, resource: out.buffer.as_entire_binding() }).collect();
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries });
    built.program.dispatch_indirect(gpu, &[group], &args.buffer, 12);
    let words: Vec<u32> = out.read(gpu, 0, 32).as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
    assert_eq!(words, [1, 2, 3, 4, 5, 6, 0, 0], "3 groups of 2 threads");
}
