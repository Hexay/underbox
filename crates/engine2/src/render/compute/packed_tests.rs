//! A structured buffer of a struct with `float3`s, filled tightly the way managed packs
//! `SpriteData` (Vector3 = 12 bytes): the shader must read each field from its packed offset and
//! write elements back packed. Under std430 `Count` read `Tint.x`'s bits and the loop ran to its
//! cap (sprite_cs had none: the GPU hang); see shaders/src/packed_buffers.rs.

use crate::render::shading::program;
use render::{Buffer, BufferKind, wgpu};

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect()
}

const SHADER: &str = "HEADER { DevShader = true; }
MODES { Default(); }
FEATURES { }
COMMON { }
CS
{
	struct Item { float3 Pos; float3 Rot; int Count; float4 Tint; float3 Vel; };
	StructuredBuffer<Item> Items < Attribute( \"Items\" ); >;
	RWStructuredBuffer<Item> Moved < Attribute( \"Moved\" ); >;
	RWStructuredBuffer<int> Steps < Attribute( \"Steps\" ); >;

	[numthreads( 1, 1, 1 )]
	void MainCs( uint3 id : SV_DispatchThreadID )
	{
		Item it = Items[id.x];
		int n = 0;
		for ( int k = 0; k < it.Count && k < 64; k++ )
			n++;
		Steps[id.x] = n;
		it.Pos += it.Vel;
		Moved[id.x] = it;
		Moved[id.x].Count = n * 2;
	}
}
";

/// Managed's packing: 14 words, no padding (std430: 64 bytes).
fn item(pos: [f32; 3], count: i32, vel: [f32; 3]) -> Vec<u32> {
    let f = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    [f(&pos), f(&[0.1, 0.2, 0.3]), vec![count as u32], f(&[1.0, 0.5, 0.25, 1.0]), f(&vel)].concat()
}

#[test]
fn tightly_packed_float3_structs_read_and_write_in_place() {
    let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
    let compiler = match shaders::Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("skipped: DXC unavailable ({e})"),
    };
    let includes = shaders::IncludePaths::new(Vec::new());
    let shader = shaders::VfxShader::parse(SHADER, std::path::Path::new("packed_test.shader"), &includes).unwrap();
    let built = program::build_compute(gpu, &compiler, &shader, "packed_test", &shaders::Variant::default()).unwrap();

    let input = [item([1.0, 2.0, 3.0], 3, [10.0, 20.0, 30.0]), item([4.0, 5.0, 6.0], 5, [1.0, 1e6, 1.0])].concat();
    let items = Buffer::with_data(gpu, "Items", BufferKind::Storage, &input.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>());
    let moved = Buffer::new(gpu, "Moved", BufferKind::Storage, (input.len() * 4) as u64);
    let steps = Buffer::new(gpu, "Steps", BufferKind::Storage, 8);
    let by_name = |name: &str| match name {
        "Items" => &items,
        "Moved" => &moved,
        "Steps" => &steps,
        other => panic!("unexpected binding {other}"),
    };
    let groups: Vec<wgpu::BindGroup> = built
        .program
        .bind_group_layouts
        .iter()
        .enumerate()
        .map(|(g, layout)| {
            let entries: Vec<_> = built
                .bindings
                .iter()
                .filter(|b| b.group == g as u32)
                .map(|b| wgpu::BindGroupEntry { binding: b.binding, resource: by_name(&b.name).buffer.as_entire_binding() })
                .collect();
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
        })
        .collect();
    built.program.dispatch(gpu, &groups, [2, 1, 1]);

    assert_eq!(words(&steps.read(gpu, 0, 8)), [3, 5], "Count read from word 6 of each 14-word element");
    let out = words(&moved.read(gpu, 0, (input.len() * 4) as u64));
    let mut want = input.clone();
    for (e, (pos, vel, n)) in [([1.0f32, 2.0, 3.0], [10.0f32, 20.0, 30.0], 3), ([4.0, 5.0, 6.0], [1.0, 1e6, 1.0], 5)].into_iter().enumerate() {
        for k in 0..3 {
            want[e * 14 + k] = (pos[k] + vel[k]).to_bits();
        }
        want[e * 14 + 6] = (n * 2) as u32;
    }
    assert_eq!(out, want, "written back packed, field store included");
}
