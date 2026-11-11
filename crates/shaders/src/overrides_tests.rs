use super::rewrite;
use crate::{Compiler, IncludePaths, Stage, Variant, VfxShader};

#[test]
fn a_shaders_own_definition_turns_the_builtin_into_a_prototype() {
    let src = "#line 1 \"<underbox>/m.fxc\"
float F( float c ) { return c; }
float3 F( float3 c ) { return c * 2; }
float3 G( float3 c )
{
	return F( c );
}
#line 4 \"game/x.shader\"
// float F( float c ) { mentioned in a comment only }
float3 F( in float3 color )
{
	return color;
}
";
    let out = rewrite(src);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), src.lines().count(), "line numbers hold");
    assert_eq!(lines[1], "float F( float c ) { return c; }", "another overload stays");
    assert_eq!(lines[2].trim_end(), "float3 F( float3 c ) ;");
    assert_eq!(&lines[3..7], &src.lines().collect::<Vec<_>>()[3..7], "built-in callers keep calling it");
    assert_eq!(&lines[7..], &src.lines().collect::<Vec<_>>()[7..], "the shader's own code is untouched");
    let plain = "#line 1 \"game/x.shader\"\nfloat3 F( float3 c ) { return c; }\n";
    assert_eq!(rewrite(plain), plain);
}

/// The whole path: a built-in header's overload, core-style code calling it before the shader's
/// own definition, then DXC.
#[test]
fn redefining_a_builtin_compiles() {
    let compiler = match Compiler::new() {
        Ok(c) => c,
        Err(e) => return eprintln!("skipped: DXC unavailable ({e})"),
    };
    let src = "HEADER { DevShader = true; }
MODES { Default(); }
FEATURES { }
COMMON
{
	#include \"math_general.fxc\"
}
CS
{
	RWStructuredBuffer<float> Out < Attribute( \"Out\" ); >;
	float3 Early( float3 c ) { return SrgbGammaToLinear( c ); }
	float3 SrgbGammaToLinear( float3 color )
	{
		return lerp( color / 12.92, pow( ( color + 0.055 ) / 1.055, 2.4 ), step( 0.04045, color ) );
	}
	[numthreads( 1, 1, 1 )]
	void MainCs( uint3 id : SV_DispatchThreadID )
	{
		Out[id.x] = Early( float3( 0.5, 0.5, 0.5 ) ).x + SrgbGammaToLinear( 0.5 );
	}
}
";
    let shader = VfxShader::parse(src, std::path::Path::new("override_test.shader"), &IncludePaths::new(Vec::new())).unwrap();
    let cs = shader.programs.iter().find(|p| p.stage == Stage::Cs).unwrap();
    compiler.compile(&shader, cs, &Variant::default()).unwrap();
}
