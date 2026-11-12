	// Spliced into standin.shader's PS (standin.rs). The sun's cascades are the maps managed's shadow
	// mapper names by bindless index in DirectionalLightCB (shadow::managed_cascade, reverse Z), one
	// binding per cascade: naga can't compare-sample a binding array. Matrix n maps world to uv and depth.
	// Filtering follows core's Shadows/DirectionalLightShadow.hlsl and ShadowFiltering.hlsl at
	// ShadowFilterQuality 3 (the High profile frame runs pin): 16-tap tent, cascade blend.
	Texture2D g_tUnderboxSunShadow < Attribute( "g_tUnderboxSunShadow" ); >;
	Texture2D g_tUnderboxSunShadow1 < Attribute( "g_tUnderboxSunShadow1" ); >;
	Texture2D g_tUnderboxSunShadow2 < Attribute( "g_tUnderboxSunShadow2" ); >;
	Texture2D g_tUnderboxSunShadow3 < Attribute( "g_tUnderboxSunShadow3" ); >;
	#include "math_general.fxc"
	#include "common/classes/Bindless.hlsl"
	#include "common/utils/MSAAUtils.hlsl"
	SamplerComparisonState g_sUnderboxReverseShadow < Filter( COMPARISON_MIN_MAG_MIP_LINEAR ); AddressU( CLAMP ); AddressV( CLAMP ); ComparisonFunc( GREATER_EQUAL ); >;

	#define UNDERBOX_SUN_TAPS 4

	// Tent over 2N x 2N texels from N x N weighted bilinear compare taps (Castano 2013).
	float CascadeVisibility( Texture2D tMap, float3 vShadow, float flBias, float flHardness )
	{
		float flGrid = g_DirectionalLightInverseShadowMapSize * max( rcp( flHardness ), 1.0 );
		float flCompare = saturate( vShadow.z + flBias );
		float flTaps = UNDERBOX_SUN_TAPS;
		float2 vGrid = vShadow.xy / flGrid - 0.5;
		float2 vBase = floor( vGrid ) - ( flTaps - 1.0 );
		float2 vPhase = frac( vGrid );
		float flSum = 0.0;
		for ( int s = 0; s < UNDERBOX_SUN_TAPS * UNDERBOX_SUN_TAPS; s++ )
		{
			float2 k = 2.0 * float2( s % UNDERBOX_SUN_TAPS, s / UNDERBOX_SUN_TAPS );
			float2 w0 = flTaps - abs( k - ( flTaps - 1.0 ) - vPhase );
			float2 w1 = flTaps - abs( k - ( flTaps - 2.0 ) - vPhase );
			float2 uv = ( vBase + k + w1 / ( w0 + w1 ) + 0.5 ) * flGrid;
			flSum += ( w0.x + w1.x ) * ( w0.y + w1.y ) * tMap.SampleCmpLevelZero( g_sUnderboxReverseShadow, uv, flCompare );
		}
		flSum /= flTaps * flTaps * flTaps * flTaps;
		return saturate( ( flSum - 0.5 ) * max( flHardness, 1.0 ) + 0.5 );
	}

	// The first cascade whose sphere holds the point (FindCascade).
	int SunCascade( float3 vPositionWs )
	{
		for ( int n = 0; n < min( 4, (int)g_DirectionalLightCascadeCount ); n++ )
		{
			float3 vToCenter = vPositionWs - g_DirectionalLightCascadeSpheres[n].xyz;
			if ( dot( vToCenter, vToCenter ) < g_DirectionalLightCascadeSpheres[n].w )
				return n;
		}
		return -1;
	}

	float SunCascadeVisibility( int n, float3 vPositionWs, float3 vNormalWs )
	{
		float4x4 m = g_DirectionalLightWorldToShadowViewMatrices[n];
		float flHardness = g_DirectionalLightCascadeHardness[n];
		float flTexelWs = g_DirectionalLightInverseShadowMapSize / length( m[0].xyz );
		float flOffset = flTexelWs * ( UNDERBOX_SUN_TAPS * rcp( max( flHardness, 1.0 ) ) + 1.0 );
		float3 vShadow = mul( m, float4( vPositionWs + vNormalWs * flOffset, 1.0 ) ).xyz;
		float flBias = g_DirectionalLightShadowBias[n];
		if ( n == 1 )
			return CascadeVisibility( g_tUnderboxSunShadow1, vShadow, flBias, flHardness );
		if ( n == 2 )
			return CascadeVisibility( g_tUnderboxSunShadow2, vShadow, flBias, flHardness );
		if ( n == 3 )
			return CascadeVisibility( g_tUnderboxSunShadow3, vShadow, flBias, flHardness );
		return CascadeVisibility( g_tUnderboxSunShadow, vShadow, flBias, flHardness );
	}

	bool DisableScreenSpaceShadows < Attribute( "DisableScreenSpaceShadows" ); Default( 0 ); >;

	// Managed's contact-shadow mask by bindless index (DirectionalLightShadow.hlsl SampleScreenSpaceShadow); 1 = lit.
	float ScreenSpaceShadow( float4 vPositionSs )
	{
		if ( DisableScreenSpaceShadows || g_DirectionalLightScreenSpaceShadowIndex == 0 )
			return 1.0;
		Texture2D tMask = g_bindless_Texture2D[ NonUniformResourceIndex( g_DirectionalLightScreenSpaceShadowIndex ) ];
		vPositionSs.xy -= g_vViewportOffset.xy;
		return MSAAUtils::SampleRed( tMask, vPositionSs );
	}

	float SunCascadesVisibility( float3 vPositionWs, float3 vNormalWs )
	{
		int n = SunCascade( vPositionWs );
		if ( n < 0 )
			return 1.0;
		float flVisibility = SunCascadeVisibility( n, vPositionWs, vNormalWs );
		float3 vToCenter = vPositionWs - g_DirectionalLightCascadeSpheres[n].xyz;
		float flWeight = 1.0 - smoothstep( 0.81 * g_DirectionalLightCascadeSpheres[n].w, g_DirectionalLightCascadeSpheres[n].w, dot( vToCenter, vToCenter ) );
		if ( flWeight < 1.0 )
		{
			float flNext = n + 1 < (int)g_DirectionalLightCascadeCount ? SunCascadeVisibility( n + 1, vPositionWs, vNormalWs ) : 1.0;
			flVisibility = lerp( flNext, flVisibility, flWeight );
		}
		return flVisibility;
	}

	float SunVisibility( float3 vPositionWs, float3 vNormalWs, float4 vPositionSs )
	{
		return SunCascadesVisibility( vPositionWs, vNormalWs ) * ScreenSpaceShadow( vPositionSs );
	}
