use super::rewrite;

const SRC: &str = "struct Args { uint Count; int Base; float Scale; };
RWStructuredBuffer<Args> A : register( u0 );
RWStructuredBuffer<uint> Stats;
void F( uint i )
{
	uint slot;
	InterlockedAdd( A[i + 1].Count, 1, slot ); // a comment naming A[0]
	A[i].Base = -2;
	float s = A[i].Scale * 2;
	Args a = A[slot];
	a.Count = 1;
	A[i * 2] = a;
	InterlockedAdd( Stats[0], 1 );
}
";

#[test]
fn struct_buffer_atomics_become_word_buffers() {
    let out = rewrite(SRC);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), SRC.lines().count(), "line numbers hold");
    assert!(lines[1].starts_with("RWStructuredBuffer<uint> A : register( u0 ); Args A_UbLoad( uint i ) { Args s; s.Count = ( A[i * 3 + 0] ); s.Base = asint( A[i * 3 + 1] );"), "{}", lines[1]);
    assert!(lines[1].contains("void A_UbStore( uint i, Args s ) { A[i * 3 + 0] = ( s.Count ); A[i * 3 + 1] = asuint( s.Base ); A[i * 3 + 2] = asuint( s.Scale ); }"), "{}", lines[1]);
    assert_eq!(lines[2], "RWStructuredBuffer<uint> Stats;");
    assert!(lines[6].contains("InterlockedAdd( A[(i + 1) * 3 + 0], 1, slot );"), "{}", lines[6]);
    assert_eq!(lines[7].trim(), "A[(i) * 3 + 1] = asuint( -2 );");
    assert_eq!(lines[8].trim(), "float s = asfloat( A[(i) * 3 + 2] ) * 2;");
    assert_eq!(lines[9].trim(), "Args a = A_UbLoad( slot );");
    assert_eq!(lines[11].trim(), "A_UbStore( i * 2, a );");
    assert_eq!(lines[12].trim(), "InterlockedAdd( Stats[0], 1 );");
}

#[test]
fn buffers_without_field_atomics_or_with_unsupported_uses_stay() {
    let plain = "struct S { uint a; };\nRWStructuredBuffer<S> B;\nvoid F() { B[0].a = 1; }\n";
    assert_eq!(rewrite(plain), plain);
    let compound = "struct S { float f; uint a; };\nRWStructuredBuffer<S> B;\nvoid F() { InterlockedAdd( B[0].a, 1 ); B[0].f += 1; }\n";
    assert_eq!(rewrite(compound), compound);
}
