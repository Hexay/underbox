	// Direct lighting as vr_lighting.fxc's ComputeDiffuseAndSpecularTerms does it for an isotropic,
	// non-metal-textured surface: normalized pow( N.L ) diffuse and GGX specular with Schlick fresnel.
	struct Surface
	{
		float3 vPositionWs;
		float3 vNormalWs;
		float3 vToCameraWs;
		float flRoughness;
		float3 vSpecularColor;
	};

	struct DirectLight
	{
		float3 vDiffuse;
		float3 vSpecular;
	};

	// common/BRDF.hlsl's ComputeGGXBRDF (D·π over Schlick-Smith G) for one isotropic lobe.
	float Ggx( float flRoughness, float flNDotL, float flNDotV, float flNDotH )
	{
		float flAlpha2 = flRoughness * flRoughness * flRoughness * flRoughness;
		float flDenom = flNDotH * flNDotH * ( flAlpha2 - 1.0 ) + 1.0;
		float flK = ( flRoughness + 1.0 ) * ( flRoughness + 1.0 ) / 8.0;
		float flOoG = 4.0 * ( flNDotL * ( 1.0 - flK ) + flK ) * ( flNDotV * ( 1.0 - flK ) + flK );
		return flAlpha2 / ( max( flDenom * flDenom, 1e-30 ) * flOoG );
	}

	void AddLight( inout DirectLight o, Surface s, float3 vToLightWs, float3 vColor )
	{
		float flNDotL = saturate( dot( s.vNormalWs, vToLightWs ) );
		if ( flNDotL <= 0.0 )
			return;
		float flExponent = ( 1.0 - s.flRoughness ) * 0.8 + 0.6;
		o.vDiffuse += vColor * pow( flNDotL, flExponent ) * ( ( flExponent + 1.0 ) * 0.5 );

		float3 vHalf = normalize( vToLightWs + s.vToCameraWs );
		float flNDotV = saturate( dot( s.vNormalWs, s.vToCameraWs ) );
		float flSpecular = Ggx( s.flRoughness, flNDotL, flNDotV, dot( vHalf, s.vNormalWs ) ) * flNDotL;
		float3 vFresnel = s.vSpecularColor + ( 1.0 - s.vSpecularColor ) * pow( 1.0 - saturate( dot( vToLightWs, vHalf ) ), 5.0 );
		o.vSpecular += vColor * vFresnel * flSpecular;
	}

	// Falloff and cone as vr_lighting.fxc computes them: the cone factor isn't clamped, so a spot
	// is brighter than its colour inside the inner cone. A point light's cone is (0, -1, 1, 0)
	// with a zero direction (lighting/blocks.rs), so it factors 1.
	float BinnedLightMask( BinnedLight l, float3 vPositionWs, out float3 vDir )
	{
		float3 vRay = l.Position - vPositionWs;
		float flDistSq = dot( vRay, vRay );
		vDir = vRay * rsqrt( max( flDistSq, 1e-6 ) );
		if ( flDistSq > l.RadiusSquared )
			return 0.0;
		float4 vCone = l.SpotLightInnerOuterConeCosines;
		float flCone = dot( vDir, -l.Forward ) - vCone.y;
		if ( flCone <= 0.0 )
			return 0.0;
		float flDot = dot( float2( sqrt( max( flDistSq, 1.0 ) ), max( flDistSq, 1.0 ) ), float2( l.LinearFalloff, l.QuadraticFalloff ) );
		return flCone * vCone.z * saturate( 1.0 / flDot - l.FalloffBias );
	}

	// Local lights are the pixel's cluster's (Cluster::Query); a per-lane walk, since ClusterForEach needs wave ops.
	DirectLight Direct( Surface s, uint nCluster )
	{
		DirectLight o;
		o.vDiffuse = 0.0;
		o.vSpecular = 0.0;
		if ( g_DirectionalLightEnabled )
		{
			float flVisibility = SunVisibility( s.vPositionWs, s.vNormalWs );
			if ( flVisibility > 0.0 )
				AddLight( o, s, -g_DirectionalLightDirection.xyz, g_DirectionalLightColor.rgb * flVisibility );
		}
		ClusterRange r = Cluster::Query( ClusterItemType_Light, nCluster );
		for ( uint k = 0; k < r.Count; k++ )
		{
			BinnedLight l = LoadLight( Cluster::LoadItem( r, k ) );
			float3 vDir;
			float flMask = BinnedLightMask( l, s.vPositionWs, vDir );
			if ( flMask > 0.0 && dot( s.vNormalWs, vDir ) > 0.0 )
				AddLight( o, s, vDir, l.Color * flMask * LocalVisibility( l, s.vPositionWs, s.vNormalWs ) );
		}
		return o;
	}
