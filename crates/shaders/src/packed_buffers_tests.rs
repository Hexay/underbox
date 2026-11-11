use super::{audit, rewrite};

const SRC: &str = "struct Inner { float2 Uv; uint Id; };
struct Sprite { float3 Pos; float3 Rot; int Count; Inner In; float W[2]; bool On; };
StructuredBuffer<Sprite> In : register( t0 );
RWStructuredBuffer<Sprite> Out;
RWStructuredBuffer<float3> Points;
StructuredBuffer<float4> Plain;
void F( uint i )
{
	Sprite s = In[i]; // a comment naming In[0]
	Out[i + 1] = s;
	Out[i].Count = In.Load( i ).Count + 1;
	float3 p = Points[i] * In[i].Pos;
	Points[i] = p;
	uint n, stride; Out.GetDimensions( n, stride );
}
";

#[test]
fn padded_elements_become_word_buffers() {
    let out = rewrite(SRC);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), SRC.lines().count(), "line numbers hold");
    // Words: Pos 0-2, Rot 3-5, Count 6, In.Uv 7-8, In.Id 9, W 10-11, On 12.
    assert!(lines[2].starts_with("StructuredBuffer<uint> In : register( t0 ); Sprite In_UbLoad( uint ub_i ) { uint ub_w = ub_i * 13; Sprite ub_s;"), "{}", lines[2]);
    assert!(lines[2].contains(" ub_s.Rot = asfloat( uint3( In[ub_w + 3], In[ub_w + 4], In[ub_w + 5] ) );"), "{}", lines[2]);
    assert!(lines[2].contains(" ub_s.In.Uv = asfloat( uint2( In[ub_w + 7], In[ub_w + 8] ) ); ub_s.In.Id = In[ub_w + 9];"), "{}", lines[2]);
    assert!(lines[2].contains(" ub_s.W[1] = asfloat( In[ub_w + 11] ); ub_s.On = ( In[ub_w + 12] != 0 ); return ub_s; }"), "{}", lines[2]);
    assert!(!lines[2].contains("UbStore"), "a read-only buffer gets no store helper");
    assert!(lines[3].contains("void Out_UbStore( uint ub_i, Sprite ub_s ) { uint ub_w = ub_i * 13; Out[ub_w + 0] = asuint( ub_s.Pos.x );"), "{}", lines[3]);
    assert!(lines[3].contains("Out[ub_w + 12] = (uint)( ub_s.On ); }"), "{}", lines[3]);
    assert!(lines[3].contains("void Out_UbStore_Count( uint ub_i, int ub_v ) { uint ub_w = ub_i * 13; Out[ub_w + 6] = asuint( ub_v ); }"), "{}", lines[3]);
    assert!(lines[3].contains("void Out_UbDims( out uint ub_n, out uint ub_stride ) { Out.GetDimensions( ub_n, ub_stride ); ub_n /= 13; ub_stride = 52; }"), "{}", lines[3]);
    assert!(lines[4].starts_with("RWStructuredBuffer<uint> Points; float3 Points_UbLoad( uint ub_i ) { uint ub_w = ub_i * 3; float3 ub_s; ub_s = asfloat("), "{}", lines[4]);
    assert_eq!(lines[5], "StructuredBuffer<float4> Plain;", "std430 doesn't pad float4");
    assert_eq!(lines[8].trim(), "Sprite s = In_UbLoad( i ); // a comment naming In[0]");
    assert_eq!(lines[9].trim(), "Out_UbStore( i + 1, s );");
    assert_eq!(lines[10].trim(), "Out_UbStore_Count( i, In_UbLoad( i ).Count + 1 );");
    assert_eq!(lines[11].trim(), "float3 p = Points_UbLoad( i ) * In_UbLoad( i ).Pos;");
    assert_eq!(lines[12].trim(), "Points_UbStore( i, p );");
    assert_eq!(lines[13].trim(), "uint n, stride; Out_UbDims( n, stride );");
}

/// lightbinner.hlsl's shape: an enum, a matrix after three words (std430 moves it to 16) and
/// member functions.
#[test]
fn matrices_enums_and_methods() {
    let src = "enum Shape { Sphere = 0, Box = 1 };
struct Light
{
	uint Type; Shape S; uint Flags;
	float4x4 ToWorld;
	row_major float2x3 R;
	float3 GetPosition() { return ToWorld[3].xyz; }
	float Scale( float k = 1.0f ) { float x = k; return x; }
};
StructuredBuffer<Light> Lights;
float3 F( uint i ) { return Lights[i].GetPosition(); }
";
    let out = rewrite(src);
    let decl = out.lines().nth(9).unwrap();
    assert!(decl.starts_with("StructuredBuffer<uint> Lights; Light Lights_UbLoad( uint ub_i ) { uint ub_w = ub_i * 25;"), "{decl}");
    assert!(decl.contains(" ub_s.S = (Shape)asint( Lights[ub_w + 1] );"), "{decl}");
    // Column-major: element [0][1] is word 3 + 4; the constructor takes rows.
    assert!(decl.contains(" ub_s.ToWorld = float4x4( asfloat( Lights[ub_w + 3] ), asfloat( Lights[ub_w + 7] ),"), "{decl}");
    assert!(decl.contains(" ub_s.R = float2x3( asfloat( Lights[ub_w + 19] ), asfloat( Lights[ub_w + 20] ), asfloat( Lights[ub_w + 21] ), asfloat( Lights[ub_w + 22] ),"), "{decl}");
    assert!(out.lines().nth(10).unwrap().contains("Lights_UbLoad( i ).GetPosition()"));
}

#[test]
fn unsupported_uses_and_types_stay() {
    let cases = [
        ("struct S { float3 a; float b; float c; };\nRWStructuredBuffer<S> B;\nvoid F() { B[0].c += 1; }\n", "compound assignment"),
        ("struct S { float3 a; float b; float c; };\nRWStructuredBuffer<S> B;\nvoid F() { B[0].a.x = 1; }\n", "part of a field"),
        ("struct S { float3 a; uint b; uint c; };\nRWStructuredBuffer<S> B;\nvoid F() { InterlockedAdd( B[0].c, 1 ); }\n", "atomic"),
        ("struct S { float3 a; float b; float c; };\nStructuredBuffer<S> B;\nfloat G( StructuredBuffer<S> b ) { return b[0].b; }\nvoid F() { G( B ); }\n", "used whole"),
        ("struct S { float3 a; float m[COUNT]; };\nStructuredBuffer<S> B;\nvoid F() { float x = B[0].a.x; }\n", "array size"),
    ];
    for (src, why) in cases {
        assert_eq!(rewrite(src), src, "{why}");
        let report = audit(src);
        assert!(report.len() == 1 && report[0].contains("kept") && report[0].contains(why), "{why}: {report:?}");
    }
}

/// ui_cssbox_batched's text instances: a struct included under a `#define` rename, then the name
/// reused. Unrewritten, every instance after the first read the wrong words (one glyph drew).
#[test]
fn structs_renamed_by_define_are_laid_out_under_their_real_name() {
    let src = "#define Box Text\nstruct Box { float4 r; int n; };\n#undef Box\nStructuredBuffer<Text> T;\nstruct Box { float4 r; };\nStructuredBuffer<Box> B;\nfloat F( uint i ) { return T[i].n + B[i].r.x; }\n";
    let out = rewrite(src);
    assert!(out.contains("StructuredBuffer<uint> T; Text T_UbLoad( uint ub_i ) { uint ub_w = ub_i * 5;"), "{out}");
    assert!(out.contains("StructuredBuffer<Box> B;\n"), "the later Box is unpadded: {out}");
}

#[test]
fn unpadded_elements_keep_their_type() {
    // Tightly packed and std430 agree: 4-byte scalars, a float4 at 16, a float2 at 8.
    let src = "struct A { float4 p; uint a; uint b; float2 c; };\nstruct B { uint x; float y; };\nStructuredBuffer<A> Sa;\nRWStructuredBuffer<B> Sb;\nStructuredBuffer<float2> Sc;\nvoid F() { Sb[0].x = Sa[0].a; }\n";
    assert_eq!(rewrite(src), src);
    assert!(audit(src).is_empty());
}
