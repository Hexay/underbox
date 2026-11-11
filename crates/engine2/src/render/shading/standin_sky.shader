//-------------------------------------------------------------------------------------------------
// underbox stand-in for the closed sky.shader: the material's SkyTexture cube sampled along the
// view ray, times the sky's tint. Drawn as one full-screen triangle at the far plane
// (render/scene/execute.rs `sky`); the vertices are already in clip space.
//-------------------------------------------------------------------------------------------------
HEADER
{
	DevShader = true;
	Description = "underbox stand-in sky shader";
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
	float3 vPositionPs : POSITION < Semantic( PosXyz ); >;
};

struct PS_INPUT
{
	// The clip-space point halfway in depth, unprojected per pixel (w kept, so it interpolates linearly).
	float4 vRayPs : TEXCOORD0;

	#if ( PROGRAM == VFX_PROGRAM_VS )
		float4 vPositionPs : SV_Position;
	#endif
};

VS
{
	PS_INPUT MainVs( const VS_INPUT i )
	{
		PS_INPUT o;
		o.vPositionPs = float4( i.vPositionPs, 1.0 );
		o.vRayPs = float4( i.vPositionPs.xy, 0.5, 1.0 );
		return o;
	}
}

PS
{
	TextureCube g_tSkyTexture < Attribute( "SkyTexture" ); >;
	SamplerState g_sSky < Filter( BILINEAR ); AddressU( CLAMP ); AddressV( CLAMP ); AddressW( CLAMP ); >;
	float4 g_vSkyTint < Attribute( "g_vSkyTint" ); Default4( 1.0, 1.0, 1.0, 1.0 ); >;
	float g_flBrightnessExposureBias < Attribute( "g_flBrightnessExposureBias" ); Default( 0.0 ); >;

	#include "common/classes/Fog.hlsl"

	struct PS_OUTPUT
	{
		float4 vColor : SV_Target0;
	};

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		PS_OUTPUT o;
		// Halfway in depth is finite for forward, reverse and infinite-far projections alike.
		float4 vWorld = mul( g_matProjectionToWorld, i.vRayPs );
		float3 vRay = vWorld.xyz / vWorld.w - g_vCameraPositionWs.xyz;
		float3 vDirection = normalize( vRay );
		float3 vSky = g_tSkyTexture.Sample( g_sSky, vDirection ).rgb * g_vSkyTint.rgb * exp2( g_flBrightnessExposureBias );
		// Far past the froxel grid and any fog range: Fog.hlsl only reads the sky ray's direction.
		o.vColor = float4( Fog::ApplySky( g_vCameraPositionWs.xyz + vDirection * 1000000.0, float2( 0.0, 0.0 ), vSky ), 1.0 );
		return o;
	}
}
