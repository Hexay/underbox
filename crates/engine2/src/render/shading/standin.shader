//-------------------------------------------------------------------------------------------------
// underbox stand-in for the core material shaders that ship without source (complex, generic,
// simple, unlit, ...): colour texture times colour tint and model tint, lit by the view's ambient
// colour, sun and binned lights. See render/shading/standin.rs for which shaders map here, and
// render/lighting for what fills the lighting blocks.
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
	float3 vPositionWs : TEXCOORD2;
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
		o.vPositionWs = vPositionWs;
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
	// The leading members of common/lightbinner.hlsl's and DirectionalLightShadow.hlsl's blocks,
	// copied because those headers pull in bindless arrays and wave ops the binder can't serve.
	cbuffer ViewLightingConfigV2
	{
		int4 NumLights;
		int4 BakedLightIndexMapping[256];
		float4 AmbientLightColor;
	};

	cbuffer DirectionalLightCB
	{
		float4 g_DirectionalLightColor;
		float4 g_DirectionalLightDirection;
		float4x4 g_DirectionalLightWorldToShadowViewMatrices[4];
		uint4 g_DirectionalLightShadowMapTextureIndex;
		uint g_DirectionalLightCascadeCount;
		float g_DirectionalLightInverseShadowMapSize;
		uint g_DirectionalLightScreenSpaceShadowIndex;
		bool g_DirectionalLightEnabled;
	};

	struct BinnedLight
	{
		uint Type;
		uint Shape;
		uint Flags;
		float4x4 LightToWorld;
		float3 Color;
		float LinearFalloff;
		float QuadraticFalloff;
		float FalloffBias;
		float Radius;
		float RadiusSquared;
		float2 ShapeSize;
		float4 SpotLightInnerOuterConeCosines;
		float FogIntensity;
		uint ShadowMapIndex;
		uint LightCookieTextureIndex;
		uint ShadowMaskTextureIndex;
		uint LightModulatorOffset;
		uint LightModulatorCount;
	};

	StructuredBuffer<BinnedLight> BinnedLightBufferV2 < Attribute( "BinnedLightBufferV2" ); >;

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

	// Diffuse only; falloff and cone as vr_lighting.fxc computes them.
	float3 BinnedLightDiffuse( BinnedLight l, float3 vPositionWs, float3 vNormalWs )
	{
		float3 vRay = l.LightToWorld[3].xyz - vPositionWs;
		float flDistSq = dot( vRay, vRay );
		if ( flDistSq > l.RadiusSquared )
			return 0.0;
		float3 vDir = vRay * rsqrt( max( flDistSq, 1e-6 ) );
		float4 vCone = l.SpotLightInnerOuterConeCosines;
		float flCone = saturate( ( dot( vDir, -l.LightToWorld[0].xyz ) - vCone.y ) * vCone.z );
		float flDot = dot( float2( sqrt( flDistSq ), flDistSq ), float2( l.LinearFalloff, l.QuadraticFalloff ) );
		float flFalloff = saturate( 1.0 / max( flDot, 1e-6 ) - l.FalloffBias );
		return l.Color * saturate( dot( vNormalWs, vDir ) ) * flCone * flFalloff;
	}

	// The sun's one cascade (render/lighting/shadow.rs): matrix 0 maps world to uv and depth.
	Texture2D g_tUnderboxSunShadow < Attribute( "g_tUnderboxSunShadow" ); >;
	SamplerComparisonState g_sUnderboxSunShadow < Filter( COMPARISON_MIN_MAG_MIP_LINEAR ); AddressU( CLAMP ); AddressV( CLAMP ); ComparisonFunc( LESS_EQUAL ); >;

	float SunVisibility( float3 vPositionWs, float3 vNormalWs )
	{
		if ( g_DirectionalLightCascadeCount == 0 )
			return 1.0;
		float4x4 m = g_DirectionalLightWorldToShadowViewMatrices[0];
		float flTexelWs = g_DirectionalLightInverseShadowMapSize / length( m[0].xyz );
		float3 vShadow = mul( m, float4( vPositionWs + vNormalWs * flTexelWs * 1.5, 1.0 ) ).xyz;
		if ( any( vShadow.xy < 0.0 ) || any( vShadow.xy > 1.0 ) || vShadow.z > 1.0 )
			return 1.0;
		float flSum = 0.0;
		for ( int y = -1; y <= 1; y++ )
			for ( int x = -1; x <= 1; x++ )
				flSum += g_tUnderboxSunShadow.SampleCmpLevelZero( g_sUnderboxSunShadow, vShadow.xy + float2( x, y ) * g_DirectionalLightInverseShadowMapSize, vShadow.z - 0.0005 );
		return flSum / 9.0;
	}

	float3 Irradiance( float3 vPositionWs, float3 vNormalWs )
	{
		float3 vLight = AmbientLightColor.rgb;
		if ( g_DirectionalLightEnabled )
			vLight += g_DirectionalLightColor.rgb * saturate( dot( vNormalWs, -g_DirectionalLightDirection.xyz ) ) * SunVisibility( vPositionWs, vNormalWs );
		for ( int n = 0; n < NumLights.x; n++ )
			vLight += BinnedLightDiffuse( BinnedLightBufferV2[n], vPositionWs, vNormalWs );
		return vLight;
	}

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		PS_OUTPUT o;
		float4 vAlbedo = g_tColor.Sample( g_sStandin, i.vTexCoord );
		float3 vTint = lerp( float3( 1.0, 1.0, 1.0 ), i.vTint.rgb, g_flModelTintAmount );
		float3 vColor = vAlbedo.rgb * g_vColorTint.rgb * vTint;

		float3 vLight = Irradiance( i.vPositionWs, normalize( i.vNormalWs ) );
		o.vColor = float4( vColor * lerp( vLight, 1.0, g_flStandinUnlit ), vAlbedo.a * i.vTint.a );
		return o;
	}
}
