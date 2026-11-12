	// Spliced into standin.shader's PS (standin.rs). Cascade maps: ours (render/lighting/shadow.rs,
	// one cascade) or, for managed's scene renderer, the maps its bindless indices name
	// (shadow::managed_cascade), one binding per cascade; all reverse Z. Matrix n maps world to uv and depth.
	Texture2D g_tUnderboxSunShadow < Attribute( "g_tUnderboxSunShadow" ); >;
	Texture2D g_tUnderboxSunShadow1 < Attribute( "g_tUnderboxSunShadow1" ); >;
	Texture2D g_tUnderboxSunShadow2 < Attribute( "g_tUnderboxSunShadow2" ); >;
	Texture2D g_tUnderboxSunShadow3 < Attribute( "g_tUnderboxSunShadow3" ); >;
	SamplerComparisonState g_sUnderboxReverseShadow < Filter( COMPARISON_MIN_MAG_MIP_LINEAR ); AddressU( CLAMP ); AddressV( CLAMP ); ComparisonFunc( GREATER_EQUAL ); >;

	#define UNDERBOX_SUN_SHADOW_INDEX 0x7FFFFF00u

	// 3x3 PCF of one cascade; ours has no bias of its own.
	float CascadeVisibility( Texture2D tMap, bool bOurs, float3 vShadow, float flBias )
	{
		float flSum = 0.0;
		float flRef = saturate( vShadow.z + ( bOurs ? 0.0005 : flBias ) );
		for ( int y = -1; y <= 1; y++ )
			for ( int x = -1; x <= 1; x++ )
			{
				float2 uv = vShadow.xy + float2( x, y ) * g_DirectionalLightInverseShadowMapSize;
				flSum += tMap.SampleCmpLevelZero( g_sUnderboxReverseShadow, uv, flRef );
			}
		return flSum / 9.0;
	}

	// The first cascade whose sphere holds the point (DirectionalLightShadow.hlsl's FindCascade); ours has
	// no spheres and one cascade.
	int SunCascade( float3 vPositionWs, bool bOurs )
	{
		for ( int n = 0; !bOurs && n < min( 4, (int)g_DirectionalLightCascadeCount ); n++ )
		{
			float3 vToCenter = vPositionWs - g_DirectionalLightCascadeSpheres[n].xyz;
			if ( dot( vToCenter, vToCenter ) < g_DirectionalLightCascadeSpheres[n].w )
				return n;
		}
		return bOurs ? 0 : -1;
	}

	float SunVisibility( float3 vPositionWs, float3 vNormalWs )
	{
		if ( g_DirectionalLightCascadeCount == 0 )
			return 1.0;
		bool bOurs = g_DirectionalLightShadowMapTextureIndex.x == UNDERBOX_SUN_SHADOW_INDEX;
		int n = SunCascade( vPositionWs, bOurs );
		if ( n < 0 )
			return 1.0;
		float4x4 m = g_DirectionalLightWorldToShadowViewMatrices[n];
		float flTexelWs = g_DirectionalLightInverseShadowMapSize / length( m[0].xyz );
		float3 vShadow = mul( m, float4( vPositionWs + vNormalWs * flTexelWs * 1.5, 1.0 ) ).xyz;
		if ( any( vShadow.xy < 0.0 ) || any( vShadow.xy > 1.0 ) || vShadow.z > 1.0 )
			return 1.0;
		float flBias = g_DirectionalLightShadowBias[n];
		if ( n == 1 )
			return CascadeVisibility( g_tUnderboxSunShadow1, false, vShadow, flBias );
		if ( n == 2 )
			return CascadeVisibility( g_tUnderboxSunShadow2, false, vShadow, flBias );
		if ( n == 3 )
			return CascadeVisibility( g_tUnderboxSunShadow3, false, vShadow, flBias );
		return CascadeVisibility( g_tUnderboxSunShadow, bOurs, vShadow, flBias );
	}
