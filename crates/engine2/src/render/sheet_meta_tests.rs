use super::*;
use crate::render::shading::program;
use assets::compiled::fixture;
use render::{Buffer, BufferKind, Texture, TextureDesc, wgpu};

#[test]
fn rows_hold_header_bounds_and_frame_times() {
    let [cycle, still] = <[Sequence; 2]>::try_from(fixture::sheet_sequences()).unwrap();
    let r = row(&cycle);
    assert_eq!(r[0], [3.0, 0.3, 0.0, 0.0]);
    assert_eq!(&r[1..4], &[[0.0, 0.0, 0.5, 0.5], [0.5, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0]]);
    assert_eq!(r[4][..2], [0.0, 0.1]);
    assert!((r[6][0] - 0.2).abs() < 1e-6 && r[7] == [0.0; 4]);
    assert_eq!(row(&still)[0], [1.0, 1.0, 1.0, 0.0], "clamped");
}

/// A sheet keeps its rows until dropped; `GetSheetInfo` addresses them.
#[test]
fn sheets_take_and_give_back_rows() {
    let sequences = fixture::sheet_sequences();
    let a = allocate(&sequences).expect("rows");
    let b = allocate(&sequences).expect("more rows");
    let (ia, ib) = (a.info().vec, b.info().vec);
    assert_eq!((ia.y, ia.z, ia.w), (WIDTH as f32, 1.0 / HEIGHT as f32, 2.0));
    let row_of = |v: f32| (v * HEIGHT as f32) as usize;
    assert!(row_of(ib.x).abs_diff(row_of(ia.x)) >= 2, "{ia:?} {ib:?}");
    let texels = texture::resolve(texture()).unwrap().cpu_mip0().expect("RGBA32F on the CPU").pixels;
    let at = |r: usize, x: usize| f32::from_le_bytes(texels[(r * WIDTH + x) * 16..][..4].try_into().unwrap());
    assert_eq!((at(row_of(ia.x), 0), at(row_of(ia.x) + 1, 0)), (3.0, 1.0), "frame counts of both sequences");
    assert!(allocate(&[]).is_none());
    let first = row_of(ia.x);
    drop(a);
    let c = allocate(&sequences[..1]).unwrap();
    assert!(row_of(c.info().vec.x) <= first, "freed rows are reused");
}

const SHADER: &str = "HEADER { DevShader = true; }
MODES { Default(); }
FEATURES { }
COMMON { #include \"sheet_sampling.fxc\" }
CS
{
	Texture2D Sheet < Attribute( \"Sheet\" ); >;
	SamplerState Point < Filter( POINT ); >;
	RWStructuredBuffer<float4> Out < Attribute( \"Out\" ); >;

	[numthreads( 6, 1, 1 )]
	void MainCs( uint3 id : SV_DispatchThreadID )
	{
		const float seq[6] = { 0, 0, 0, 1, 7, 0 };
		const float time[6] = { 0.05, 0.25, 0.35, 5.0, 0.0, 0.05 };
		SheetDataSamplerParams_t p;
		p.m_flSheetTextureBaseV = 3.5 / 8.0;
		p.m_flOOSheetTextureWidth = 1.0 / 256.0;
		p.m_flOOSheetTextureHeight = 1.0 / 8.0;
		p.m_flSheetTextureWidth = 256.0;
		p.m_flSheetSequenceCount = id.x == 5 ? 0.0 : 2.0;
		p.m_flSequenceAnimationTimescale = 1.0;
		p.m_flSequenceIndex = seq[id.x];
		p.m_flSequenceAnimationTime = time[id.x];
		SheetDataSamplerOutput_t o = SampleSheetData( Point, Sheet, p, false );
		Out[id.x * 3 + 0] = o.m_vFrame0Bounds;
		Out[id.x * 3 + 1] = o.m_vFrame1Bounds;
		Out[id.x * 3 + 2] = float4( o.m_flAnimationBlendValue, 0, 0, 0 );
	}
}
";

/// The fixture's sheet at rows 3 and 4 of an 8-row texture, sampled by `SampleSheetData`.
#[test]
fn sample_sheet_data_walks_frames_and_blends() {
    let Some(gpu) = crate::render::device::create() else { return eprintln!("skipped: no GPU") };
    let compiler = match shaders::Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("skipped: DXC unavailable ({e})"),
    };
    let shader = shaders::VfxShader::parse(SHADER, std::path::Path::new("sheet_test.shader"), &shaders::IncludePaths::new(Vec::new())).unwrap();
    let built = program::build_compute(gpu, &compiler, &shader, "sheet_test", &shaders::Variant::default()).expect("SampleSheetData translates");
    let sheet = Texture::new(gpu, TextureDesc { render_target: false, ..TextureDesc::render_target("sheet", WIDTH as u32, 8, wgpu::TextureFormat::Rgba32Float) });
    let mut texels = vec![[0.0f32; 4]; WIDTH * 3];
    fixture::sheet_sequences().iter().for_each(|s| texels.extend(row(s)));
    texels.resize(WIDTH * 8, [0.0; 4]);
    sheet.upload(gpu, &texels.iter().flatten().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>());
    let out = Buffer::new(gpu, "Out", BufferKind::Storage, 18 * 16);
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor::default());
    let entries: Vec<_> = built
        .bindings
        .iter()
        .map(|b| {
            let resource = match b.name.as_str() {
                "Sheet" => wgpu::BindingResource::TextureView(&sheet.view),
                "Out" => out.buffer.as_entire_binding(),
                _ => wgpu::BindingResource::Sampler(&sampler),
            };
            wgpu::BindGroupEntry { binding: b.binding, resource }
        })
        .collect();
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &built.program.bind_group_layouts[0], entries: &entries });
    gpu.checked(|| built.program.dispatch(gpu, &[group], [1, 1, 1])).unwrap();
    let v: Vec<f32> = out.read(gpu, 0, 18 * 16).as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    let case = |i: usize| (&v[i * 12..i * 12 + 4], &v[i * 12 + 4..i * 12 + 8], v[i * 12 + 8]);
    let (cell0, cell1, cell2, cell3) = ([0.0, 0.0, 0.5, 0.5], [0.5, 0.0, 1.0, 0.5], [0.0, 0.5, 0.5, 1.0], [0.5, 0.5, 1.0, 1.0]);
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    let (f0, f1, blend) = case(0);
    assert!(f0 == cell0 && f1 == cell1 && near(blend, 0.5), "{:?}", case(0));
    let (f0, f1, blend) = case(1);
    assert!(f0 == cell2 && f1 == cell0 && near(blend, 0.5), "the loop wraps to its first frame: {:?}", case(1));
    let (f0, f1, blend) = case(2);
    assert!(f0 == cell0 && f1 == cell1 && near(blend, 0.5), "time wraps: {:?}", case(2));
    let (f0, f1, blend) = case(3);
    assert!(f0 == cell3 && f1 == cell3 && near(blend, 1.0), "the clamped sequence holds: {:?}", case(3));
    assert_eq!(case(4).0, cell3, "a sequence index past the end clamps");
    assert_eq!((case(5).0, case(5).1), (&[0.0, 0.0, 1.0, 1.0][..], &[0.0, 0.0, 1.0, 1.0][..]), "no sheet: the whole texture");
}
