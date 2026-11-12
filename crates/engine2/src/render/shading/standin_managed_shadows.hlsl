	// Spliced into standin.shader's PS (standin.rs). Managed's shadow mapper: a light's
	// ShadowMapIndex is an element of ProjectedShadows (spot) or ProjectedCubeShadows (point), whose
	// maps are bindless (reverse Z). Both are read as words: ProjectedShadow.hlsl's and
	// ProjectedShadowCube.hlsl's structs, matrices transposed by managed. The maps come in slots
	// (render/shading/shadow_slots.rs): naga can't compare-sample a binding array.
	#define PROJECTED_SHADOW_WORDS 19u
	#define PROJECTED_CUBE_SHADOW_WORDS 102u
	StructuredBuffer<uint> ProjectedShadows < Attribute( "ProjectedShadows" ); >;
	StructuredBuffer<uint> ProjectedCubeShadows < Attribute( "ProjectedCubeShadows" ); >;
	Texture2D g_tUnderboxShadow2D0;
	Texture2D g_tUnderboxShadow2D1;
	Texture2D g_tUnderboxShadow2D2;
	Texture2D g_tUnderboxShadow2D3;
	TextureCube g_tUnderboxShadowCube0;
	TextureCube g_tUnderboxShadowCube1;
	TextureCube g_tUnderboxShadowCube2;
	TextureCube g_tUnderboxShadowCube3;
	float4 g_vUnderboxShadow2DIndices;
	float4 g_vUnderboxShadowCubeIndices;

	float ShadowCmp2D( uint nMap, float2 uv, float z )
	{
		float4 i = g_vUnderboxShadow2DIndices;
		if ( nMap == 0 ) return 1.0;
		if ( nMap == (uint)i.x ) return g_tUnderboxShadow2D0.SampleCmpLevelZero( g_sUnderboxReverseShadow, uv, z );
		if ( nMap == (uint)i.y ) return g_tUnderboxShadow2D1.SampleCmpLevelZero( g_sUnderboxReverseShadow, uv, z );
		if ( nMap == (uint)i.z ) return g_tUnderboxShadow2D2.SampleCmpLevelZero( g_sUnderboxReverseShadow, uv, z );
		if ( nMap == (uint)i.w ) return g_tUnderboxShadow2D3.SampleCmpLevelZero( g_sUnderboxReverseShadow, uv, z );
		return 1.0;
	}

	float ShadowCmpCube( uint nMap, float3 dir, float z )
	{
		float4 i = g_vUnderboxShadowCubeIndices;
		if ( nMap == 0 ) return 1.0;
		if ( nMap == (uint)i.x ) return g_tUnderboxShadowCube0.SampleCmpLevelZero( g_sUnderboxReverseShadow, dir, z );
		if ( nMap == (uint)i.y ) return g_tUnderboxShadowCube1.SampleCmpLevelZero( g_sUnderboxReverseShadow, dir, z );
		if ( nMap == (uint)i.z ) return g_tUnderboxShadowCube2.SampleCmpLevelZero( g_sUnderboxReverseShadow, dir, z );
		if ( nMap == (uint)i.w ) return g_tUnderboxShadowCube3.SampleCmpLevelZero( g_sUnderboxReverseShadow, dir, z );
		return 1.0;
	}

	float4 ProjectWords( StructuredBuffer<uint> b, uint nBase, float4 v )
	{
		float4 r;
		[unroll] for ( uint c = 0; c < 4; c++ )
			r[c] = dot( v, asfloat( uint4( b[nBase + c * 4], b[nBase + c * 4 + 1], b[nBase + c * 4 + 2], b[nBase + c * 4 + 3] ) ) );
		return r;
	}

	float ManagedLocalVisibility( BinnedLight l, float3 vPositionWs, float3 vNormalWs )
	{
		if ( l.Type == 1 )
		{
			uint b = l.ShadowMapIndex * PROJECTED_CUBE_SHADOW_WORDS;
			float3 vLightPos = asfloat( uint3( ProjectedCubeShadows[b + 96], ProjectedCubeShadows[b + 97], ProjectedCubeShadows[b + 98] ) );
			uint nMap = ProjectedCubeShadows[b + 99];
			float flTexel = asfloat( ProjectedCubeShadows[b + 100] );
			float3 vPos = vPositionWs + vNormalWs * 2.0 * length( vLightPos - vPositionWs ) * flTexel;
			float3 vToLight = normalize( vLightPos - vPos );
			float3 a = abs( vToLight );
			uint nFace = ( a.x >= a.y && a.x >= a.z ) ? ( vToLight.x < 0.0 ? 1u : 0u ) : ( a.y >= a.z ? ( vToLight.y < 0.0 ? 3u : 2u ) : ( vToLight.z < 0.0 ? 5u : 4u ) );
			float4 vClip = ProjectWords( ProjectedCubeShadows, b + nFace * 16, float4( vPos, 1.0 ) );
			return ShadowCmpCube( nMap, vToLight, vClip.z / vClip.w );
		}
		uint b = l.ShadowMapIndex * PROJECTED_SHADOW_WORDS;
		uint nMap = ProjectedShadows[b + 16];
		float flTexel = asfloat( ProjectedShadows[b + 17] );
		float4 vDepth = ProjectWords( ProjectedShadows, b, float4( vPositionWs, 1.0 ) );
		float4 vClip = ProjectWords( ProjectedShadows, b, float4( vPositionWs + vNormalWs * ( 1.0 + abs( vDepth.w ) * flTexel ), 1.0 ) );
		float3 vShadow = vClip.xyz / vClip.w;
		if ( vClip.w <= 0.0 || any( vShadow.xy < 0.0 ) || any( vShadow.xy > 1.0 ) )
			return 1.0;
		float flSum = 0.0;
		for ( int y = -1; y <= 1; y++ )
			for ( int x = -1; x <= 1; x++ )
				flSum += ShadowCmp2D( nMap, vShadow.xy + float2( x, y ) * flTexel, vShadow.z );
		return flSum / 9.0;
	}
