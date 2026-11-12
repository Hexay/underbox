	// Spliced into standin.shader's PS (standin.rs). Indirect terms as vr_lighting.fxc's
	// CalculateIndirectLighting takes them without lightmaps: ambient times the material's AO, and
	// specular from the pixel's cluster env maps times BRDF.hlsl's CalcBRDFReflectionFactor.
	// Bent normals (vr_shared_standard_ps_code.fxc) replace the normal for indirect lighting.
	Texture2D g_tAmbientOcclusion < Attribute( "g_tAmbientOcclusion" ); >;
	Texture2D g_tBentNormal < Attribute( "g_tBentNormal" ); >;
	float g_flStandinBentNormals < Attribute( "F_USE_BENT_NORMALS" ); Default( 0.0 ); >;
	Texture2D BRDFLookup < Attribute( "BRDFLookup" ); >;
	SamplerState g_sStandinClamp < Filter( MIN_MAG_MIP_LINEAR ); AddressU( CLAMP ); AddressV( CLAMP ); >;
	// lightbinner.hlsl's BinnedEnvMap, read by word: WorldToLocal rows, BoxMins, BoxMaxs, Color (w feather), SH, Attributes.
	StructuredBuffer<uint> BinnedEnvMapBuffer < Attribute( "BinnedEnvMapBuffer" ); >;
	#define ENV_MAP_WORDS 32u

	float4 EnvWord4( uint b, uint i )
	{
		return asfloat( uint4( BinnedEnvMapBuffer[b + i], BinnedEnvMapBuffer[b + i + 1], BinnedEnvMapBuffer[b + i + 2], BinnedEnvMapBuffer[b + i + 3] ) );
	}

	float3 EnvToLocal( uint b, float4 v )
	{
		return float3( dot( EnvWord4( b, 0 ), v ), dot( EnvWord4( b, 4 ), v ), dot( EnvWord4( b, 8 ), v ) );
	}

	float3 CalcBRDFReflectionFactor( float flNDotV, float flRoughness, float3 vSpecularColor )
	{
		float2 vTerms = BRDFLookup.SampleLevel( g_sStandinClamp, float2( flNDotV, flRoughness ), 0.0 ).rg;
		vTerms *= vTerms;
		return vSpecularColor * vTerms.x + vTerms.y;
	}

	// The cotangent frame from screen derivatives: the stand-in carries no vertex tangents.
	float3 BentNormalWs( float3 vNormalWs, float3 vPositionWs, float2 vTexCoord )
	{
		float3 dp1 = ddx( vPositionWs ), dp2 = ddy( vPositionWs );
		float2 duv1 = ddx( vTexCoord ), duv2 = ddy( vTexCoord );
		float3 dp2perp = cross( dp2, vNormalWs ), dp1perp = cross( vNormalWs, dp1 );
		float3 t = dp2perp * duv1.x + dp1perp * duv2.x;
		float3 b = dp2perp * duv1.y + dp1perp * duv2.y;
		float flScale = rsqrt( max( max( dot( t, t ), dot( b, b ) ), 1e-20 ) );
		float2 e = g_tBentNormal.Sample( g_sStandin, vTexCoord ).rg;
		float2 f = float2( e.x + e.y - 1.00392163, e.x - e.y );
		float3 n = normalize( float3( f, 1.0 - abs( f.x ) - abs( f.y ) ) );
		n.y = -n.y;
		return normalize( t * flScale * n.x + b * flScale * n.y + vNormalWs * n.z );
	}

	float AmbientOcclusion( float2 vTexCoord )
	{
		return g_tAmbientOcclusion.Sample( g_sStandin, vTexCoord ).r;
	}

	float3 EnvMapSpecular( Surface s, uint nCluster )
	{
		float3 vSpecular = 0.0;
		float flNDotV = saturate( dot( s.vToCameraWs, s.vNormalWs ) );
		float flLevel = sqrt( s.flRoughness );
		float flAccumulated = 0.0;
		ClusterRange r = Cluster::Query( ClusterItemType_EnvMap, nCluster );
		for ( uint k = 0; k < r.Count && flAccumulated < 1.0; k++ )
		{
			uint b = Cluster::LoadItem( r, k ) * ENV_MAP_WORDS;
			float3 vCubePos = EnvToLocal( b, float4( s.vPositionWs, 1.0 ) );
			float3 vMin = EnvWord4( b, 12 ).xyz, vMax = EnvWord4( b, 16 ).xyz;
			float4 vColor = EnvWord4( b, 20 );
			float3 a = min( vCubePos - vMin, vMax - vCubePos ), c = min( vCubePos - vMax, vMin - vCubePos );
			float flDistance = min( min( a.x, min( a.y, a.z ) ), min( -c.x, -min( c.y, c.z ) ) ) + 0.5;
			if ( flDistance + max( vColor.w, 0.0 ) < 0.0 )
				continue;
			float3 vNormalLocal = normalize( EnvToLocal( b, float4( s.vNormalWs, 0.0 ) ) );
			float3 vReflectLocal = normalize( EnvToLocal( b, float4( reflect( -s.vToCameraWs, s.vNormalWs ), 0.0 ) ) );
			// Box projection: the ray from the point to the probe box, seen from the probe's centre.
			float3 vFar = max( ( vMax - vCubePos ) / vReflectLocal, ( vMin - vCubePos ) / vReflectLocal );
			float3 vParallax = vCubePos + vReflectLocal * min( vFar.x, min( vFar.y, vFar.z ) ) - ( vMin + vMax ) * 0.5;
			float3 vDir = normalize( lerp( normalize( vParallax ), vNormalLocal, flLevel ) );
			TextureCube tCube = g_bindless_TextureCube[ NonUniformResourceIndex( BinnedEnvMapBuffer[b + 28] ) ];
			uint w, h, nLevels;
			tCube.GetDimensions( 0, w, h, nLevels );
			float3 vTexel = tCube.SampleLevel( g_sStandinClamp, vDir, flLevel * nLevels ).rgb * vColor.rgb;
			vSpecular = lerp( vSpecular, vTexel * CalcBRDFReflectionFactor( flNDotV, s.flRoughness, s.vSpecularColor ), 1.0 - flAccumulated );
			float flLo = min( -vColor.w, 0.0 ), flHi = max( -vColor.w, 0.0 );
			flAccumulated += flHi > flLo ? saturate( ( flDistance - flLo ) / ( flHi - flLo ) ) : step( flLo, flDistance );
		}
		return vSpecular;
	}
