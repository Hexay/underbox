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
		float4 g_DirectionalLightCascadeHardness;
		float4 g_DirectionalLightCascadeSpheres[4];
		float4 g_DirectionalLightShadowBias;
	};

	// Native's GPULight is scalar layout, which WGSL can't declare: read it by word (lighting/blocks.rs).
	StructuredBuffer<uint> BinnedLightBufferV2 < Attribute( "BinnedLightBufferV2" ); >;
	#define GPU_LIGHT_WORDS 39u

	struct BinnedLight
	{
		uint Type;
		float3 Position;
		float3 Forward;
		float3 Color;
		float LinearFalloff;
		float QuadraticFalloff;
		float FalloffBias;
		float RadiusSquared;
		float4 SpotLightInnerOuterConeCosines;
		uint ShadowMapIndex;
	};

	float LightWord( uint nBase, uint i ) { return asfloat( BinnedLightBufferV2[nBase + i] ); }

	BinnedLight LoadLight( uint n )
	{
		uint b = n * GPU_LIGHT_WORDS;
		BinnedLight l;
		l.Type = BinnedLightBufferV2[b];
		l.Forward = float3( LightWord( b, 3 ), LightWord( b, 7 ), LightWord( b, 11 ) );
		l.Position = float3( LightWord( b, 6 ), LightWord( b, 10 ), LightWord( b, 14 ) );
		l.Color = float3( LightWord( b, 19 ), LightWord( b, 20 ), LightWord( b, 21 ) );
		l.LinearFalloff = LightWord( b, 22 );
		l.QuadraticFalloff = LightWord( b, 23 );
		l.FalloffBias = LightWord( b, 24 );
		l.RadiusSquared = LightWord( b, 26 );
		l.SpotLightInnerOuterConeCosines = float4( LightWord( b, 29 ), LightWord( b, 30 ), LightWord( b, 31 ), LightWord( b, 32 ) );
		l.ShadowMapIndex = BinnedLightBufferV2[b + 34];
		return l;
	}

	Texture2D g_tColor < Attribute( "g_tColor" ); SrgbRead( true ); >;
	SamplerState g_sStandin < Filter( ANISO ); AddressU( WRAP ); AddressV( WRAP ); >;

	float4 g_vColorTint < Attribute( "g_vColorTint" ); Default4( 1.0, 1.0, 1.0, 1.0 ); >;
	float g_flModelTintAmount < Attribute( "g_flModelTintAmount" ); Default( 1.0 ); >;
	// 1 for shaders that map here from an unlit original (standin.rs).
	float g_flStandinUnlit < Attribute( "StandinUnlit" ); Default( 0.0 ); >;
	// UNDERBOX_STANDIN_DEBUG (standin.rs): 1 paints sun visibility, 2 cascade 0's shadow-space position.
	float g_flStandinDebug < Attribute( "StandinDebug" ); Default( 0.0 ); >;

	struct PS_OUTPUT
	{
		float4 vColor : SV_Target0;
	};

	// Diffuse only; falloff and cone as vr_lighting.fxc computes them.
	float3 BinnedLightDiffuse( BinnedLight l, float3 vPositionWs, float3 vNormalWs )
	{
		float3 vRay = l.Position - vPositionWs;
		float flDistSq = dot( vRay, vRay );
		if ( flDistSq > l.RadiusSquared )
			return 0.0;
		float3 vDir = vRay * rsqrt( max( flDistSq, 1e-6 ) );
		float4 vCone = l.SpotLightInnerOuterConeCosines;
		float flCone = saturate( ( dot( vDir, -l.Forward ) - vCone.y ) * vCone.z );
		float flDot = dot( float2( sqrt( flDistSq ), flDistSq ), float2( l.LinearFalloff, l.QuadraticFalloff ) );
		float flFalloff = saturate( 1.0 / max( flDot, 1e-6 ) - l.FalloffBias );
		return l.Color * saturate( dot( vNormalWs, vDir ) ) * flCone * flFalloff;
	}

	//@sun_shadows

	// Point and spot shadow tiles (render/lighting/local_shadow.rs).
	#define UNDERBOX_LOCAL_SHADOW_INDEX 0x7FFF0000u
	#define UNDERBOX_LOCAL_SHADOW_TILES 16
	cbuffer UnderboxLocalShadows
	{
		float4x4 g_matUnderboxLocalShadow[UNDERBOX_LOCAL_SHADOW_TILES];
	};
	Texture2D g_tUnderboxLocalShadow < Attribute( "g_tUnderboxLocalShadow" ); >;

	//@managed_shadows

	float LocalVisibility( BinnedLight l, float3 vPositionWs, float3 vNormalWs )
	{
		if ( l.ShadowMapIndex == 0xFFFFFFFFu )
			return 1.0;
		if ( l.ShadowMapIndex < UNDERBOX_LOCAL_SHADOW_INDEX )
			return ManagedLocalVisibility( l, vPositionWs, vNormalWs );
		uint nTile = l.ShadowMapIndex - UNDERBOX_LOCAL_SHADOW_INDEX;
		if ( nTile >= UNDERBOX_LOCAL_SHADOW_TILES )
			return 1.0;
		float3 vRay = vPositionWs - l.Position;
		if ( l.Type == 1 )
		{
			// Cube face +X, -X, +Y, -Y, +Z, -Z by the ray's dominant axis.
			float3 a = abs( vRay );
			uint nFace = ( a.x >= a.y && a.x >= a.z ) ? ( vRay.x < 0.0 ? 1u : 0u ) : ( a.y >= a.z ? ( vRay.y < 0.0 ? 3u : 2u ) : ( vRay.z < 0.0 ? 5u : 4u ) );
			nTile += nFace;
		}
		if ( nTile >= UNDERBOX_LOCAL_SHADOW_TILES )
			return 1.0;
		// Normal offset grows with distance: a texel covers more world further out.
		float3 vOffset = vNormalWs * ( 1.0 + length( vRay ) * 0.01 );
		float4 vClip = mul( g_matUnderboxLocalShadow[nTile], float4( vPositionWs + vOffset, 1.0 ) );
		float3 vShadow = vClip.xyz / vClip.w;
		const float flTile = 1.0 / 4.0;
		float2 vMin = float2( nTile % 4, nTile / 4 ) * flTile;
		const float flTexel = 1.0 / 2048.0;
		float flSum = 0.0;
		for ( int y = -1; y <= 1; y++ )
			for ( int x = -1; x <= 1; x++ )
			{
				float2 uv = clamp( vShadow.xy + float2( x, y ) * flTexel, vMin + flTexel, vMin + flTile - flTexel );
				flSum += g_tUnderboxLocalShadow.SampleCmpLevelZero( g_sUnderboxSunShadow, uv, vShadow.z - 0.0002 );
			}
		return flSum / 9.0;
	}

	float3 Irradiance( float3 vPositionWs, float3 vNormalWs )
	{
		float3 vLight = AmbientLightColor.rgb;
		if ( g_DirectionalLightEnabled )
			vLight += g_DirectionalLightColor.rgb * saturate( dot( vNormalWs, -g_DirectionalLightDirection.xyz ) ) * SunVisibility( vPositionWs, vNormalWs );
		for ( int n = 0; n < NumLights.x; n++ )
		{
			BinnedLight l = LoadLight( n );
			float3 vDiffuse = BinnedLightDiffuse( l, vPositionWs, vNormalWs );
			if ( any( vDiffuse > 0.0 ) )
				vDiffuse *= LocalVisibility( l, vPositionWs, vNormalWs );
			vLight += vDiffuse;
		}
		return vLight;
	}

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		PS_OUTPUT o;
		float4 vAlbedo = g_tColor.Sample( g_sStandin, i.vTexCoord );
		float3 vTint = lerp( float3( 1.0, 1.0, 1.0 ), i.vTint.rgb, g_flModelTintAmount );
		float3 vColor = vAlbedo.rgb * g_vColorTint.rgb * vTint;

		float3 vNormalWs = normalize( i.vNormalWs );
		float3 vLight = Irradiance( i.vPositionWs, vNormalWs );
		o.vColor = float4( vColor * lerp( vLight, 1.0, g_flStandinUnlit ), vAlbedo.a * i.vTint.a );
		if ( g_flStandinDebug == 1.0 )
			o.vColor = float4( SunVisibility( i.vPositionWs, vNormalWs ).xxx, 1.0 );
		else if ( g_flStandinDebug == 2.0 )
			o.vColor = float4( mul( g_DirectionalLightWorldToShadowViewMatrices[0], float4( i.vPositionWs, 1.0 ) ).xyz, 1.0 );
		return o;
	}
}
