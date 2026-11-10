//-------------------------------------------------------------------------------------------------
// underbox stand-in for the core material shaders that ship without source (complex, generic,
// simple, unlit, ...): colour texture times colour tint and model tint, one fixed light.
// See render/shading/standin.rs for which shaders map here.
//-------------------------------------------------------------------------------------------------
HEADER
{
	DevShader = true;
	Description = "underbox stand-in material shader";
}

MODES
{
	Default();
}

FEATURES
{
}

COMMON
{
	#include "system.fxc"
	#include "common.fxc"
}

struct VS_INPUT
{
	float3 vPositionOs : POSITION < Semantic( PosXyz ); >;
	float4 vNormalOs : NORMAL < Semantic( OptionallyCompressedTangentFrame ); >;
	float2 vTexCoord : TEXCOORD0 < Semantic( LowPrecisionUv ); >;
	uint nInstanceTransformID : TEXCOORD13 < Semantic( InstanceTransformUv ); >;
};

struct PS_INPUT
{
	float3 vNormalWs : TEXCOORD0;
	float2 vTexCoord : TEXCOORD1;
	float4 vTint : COLOR0;

	#if ( PROGRAM == VFX_PROGRAM_VS )
		float4 vPositionPs : SV_Position;
	#endif
};

VS
{
	#include "instancing.fxc"
	#include "vs_decompress.fxc"

	float4 g_vTexCoordScale < Attribute( "g_vTexCoordScale" ); Default4( 1.0, 1.0, 0.0, 0.0 ); >;
	float4 g_vTexCoordOffset < Attribute( "g_vTexCoordOffset" ); Default4( 0.0, 0.0, 0.0, 0.0 ); >;

	PS_INPUT MainVs( const VS_INPUT i )
	{
		PS_INPUT o;
		float3x4 matObjectToWorld = GetTransformMatrix( i.nInstanceTransformID );
		float3 vPositionWs = mul( matObjectToWorld, float4( i.vPositionOs.xyz, 1.0 ) );
		o.vPositionPs = Position3WsToPs( vPositionWs );
		float3 vNormalOs;
		DecompressNormal( i.vNormalOs, vNormalOs );
		o.vNormalWs = mul( (float3x3)matObjectToWorld, vNormalOs );
		o.vTexCoord = i.vTexCoord * g_vTexCoordScale.xy + g_vTexCoordOffset.xy;
		o.vTint = GetExtraPerInstanceShaderData( i.nInstanceTransformID ).vTint;
		return o;
	}
}

PS
{
	Texture2D g_tColor < Attribute( "g_tColor" ); >;
	SamplerState g_sStandin < Filter( ANISO ); AddressU( WRAP ); AddressV( WRAP ); >;

	float4 g_vColorTint < Attribute( "g_vColorTint" ); Default4( 1.0, 1.0, 1.0, 1.0 ); >;
	float g_flModelTintAmount < Attribute( "g_flModelTintAmount" ); Default( 1.0 ); >;
	// 1 for shaders that map here from an unlit original (standin.rs).
	float g_flStandinUnlit < Attribute( "StandinUnlit" ); Default( 0.0 ); >;

	struct PS_OUTPUT
	{
		float4 vColor : SV_Target0;
	};

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		PS_OUTPUT o;
		float4 vAlbedo = g_tColor.Sample( g_sStandin, i.vTexCoord );
		float3 vTint = lerp( float3( 1.0, 1.0, 1.0 ), i.vTint.rgb, g_flModelTintAmount );
		float3 vColor = vAlbedo.rgb * g_vColorTint.rgb * vTint;

		float3 vNormalWs = normalize( i.vNormalWs );
		float flLight = saturate( dot( vNormalWs, normalize( float3( 0.4, 0.3, 0.85 ) ) ) ) * 0.7 + 0.3;
		o.vColor = float4( vColor * lerp( flLight, 1.0, g_flStandinUnlit ), vAlbedo.a * i.vTint.a );
		return o;
	}
}
