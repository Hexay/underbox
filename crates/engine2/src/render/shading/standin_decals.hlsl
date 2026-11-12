	// Clustered decals as common/classes/Decals.hlsl resolves them, without its cluster walk (which
	// needs wave ops): every decal is tested against every pixel. render/lighting/decals.rs packs
	// GPUDecal (16 words) and GPUDecalExtraData; managed's SceneRenderer packs the same.
	// TODO: decal normal maps and parallax height maps.
	StructuredBuffer<uint> DecalsBuffer < Attribute( "DecalsBuffer" ); >;
	StructuredBuffer<uint> DecalsExtraDataBuffer < Attribute( "DecalsExtraDataBuffer" ); >;
	SamplerState g_sStandinDecal < Filter( MIN_MAG_MIP_LINEAR ); AddressU( CLAMP ); AddressV( CLAMP ); >;

	struct DecalSurface
	{
		float3 vAlbedo;
		float3 vNormalWs;
		float flRoughness;
		float flMetalness;
		float3 vEmission;
	};

	float DecalWord( uint nBase, uint i ) { return asfloat( DecalsBuffer[nBase + i] ); }
	float DecalExtra( int nOffset, uint i ) { return asfloat( DecalsExtraDataBuffer[nOffset / 4 + i] ); }
	float3 DecalRotate( float3 v, float4 q ) { return v + 2.0 * cross( q.xyz, cross( q.xyz, v ) + q.w * v ); }

	float4 DecalSample( uint nTexture, uint nSampler, bool bSampler, float2 uv, float4 vGrad )
	{
		Texture2D t = g_bindless_Texture2D[ NonUniformResourceIndex( nTexture ) ];
		if ( bSampler )
			return t.SampleGrad( g_bindless_Sampler[ NonUniformResourceIndex( nSampler ) ], uv, vGrad.xy, vGrad.zw );
		return t.SampleGrad( g_sStandinDecal, uv, vGrad.xy, vGrad.zw );
	}

	void ApplyDecals( float3 vPositionWs, inout DecalSurface m )
	{
		// Before the loop: the whole quad must take derivatives.
		float3 vDx = ddx( vPositionWs ), vDy = ddy( vPositionWs );
		for ( uint n = 0; n < (uint)NumLights.w; n++ )
		{
			uint b = n * 16u;
			float4 q = float4( DecalWord( b, 3 ), DecalWord( b, 4 ), DecalWord( b, 5 ), DecalWord( b, 6 ) );
			float3 vScale = float3( DecalWord( b, 7 ), DecalWord( b, 8 ), DecalWord( b, 9 ) );
			float3 p = DecalRotate( vPositionWs + float3( DecalWord( b, 0 ), DecalWord( b, 1 ), DecalWord( b, 2 ) ), q ) * vScale;
			if ( any( abs( p ) > 0.5 ) )
				continue;
			uint nPacked0 = DecalsBuffer[b + 10], nPacked1 = DecalsBuffer[b + 11];
			uint nColor = nPacked0 & 0xFFFF, nRma = nPacked1 & 0xFFFF;
			if ( nColor == 0 )
				continue;
			float2 uv = 0.5 - p.yz;
			float3 vGx = DecalRotate( vDx, q ) * vScale, vGy = DecalRotate( vDy, q ) * vScale;
			float4 vGrad = float4( -vGx.y, -vGx.z, -vGy.y, -vGy.z );

			float3 vAxis = DecalRotate( float3( 1.0, 0.0, 0.0 ), float4( -q.xyz, q.w ) );
			float flAngle = smoothstep( 0.55, 1.0, clamp( dot( m.vNormalWs, -vAxis ), 0.55, 1.0 ) );
			float flAttenuation = lerp( 1.0, flAngle, float( nPacked1 >> 24 ) / 255.0 );

			uint nTint = DecalsBuffer[b + 14];
			uint nExp = ( nPacked1 >> 16 ) & 0xFF;
			float4 vTint = float4( ( nTint & 0xFF ), ( nTint >> 8 ) & 0xFF, ( nTint >> 16 ) & 0xFF, ( nTint >> 24 ) & 0xFF ) / 255.0;
			vTint.rgb = nExp == 0 ? 0.0 : ( vTint.rgb * 255.0 - 128.0 ) * exp2( float( nExp ) - 135.0 );

			int nExtra = asint( DecalsBuffer[b + 15] );
			float flMix = 1.0;
			uint nBlend = 0, nSampler = 0, nEmission = 0;
			if ( nExtra >= 0 )
			{
				flMix = DecalExtra( nExtra, 5 );
				nBlend = DecalsExtraDataBuffer[nExtra / 4 + 6] & 3;
				nSampler = DecalsExtraDataBuffer[nExtra / 4 + 9];
				nEmission = DecalsExtraDataBuffer[nExtra / 4 + 10];
			}
			float4 vDecal = DecalSample( nColor, nSampler, nExtra >= 0, uv, vGrad );
			if ( nEmission > 0 )
				m.vEmission = lerp( m.vEmission, DecalSample( nEmission, nSampler, true, uv, vGrad ).rgb * DecalExtra( nExtra, 11 ), vDecal.a );
			vDecal *= vTint;
			if ( nBlend == 0 )
				vDecal.rgb *= flAttenuation;
			vDecal.a *= flAttenuation;
			if ( nBlend == 1 )
				m.vAlbedo *= lerp( 1.0, vDecal.rgb, flMix * vDecal.a );
			else if ( nBlend == 2 )
				m.vEmission += vDecal.rgb * flMix * vDecal.a;
			else
				m.vAlbedo = lerp( m.vAlbedo, vDecal.rgb, flMix * vDecal.a );
			if ( nRma > 0 )
			{
				float4 vRma = DecalSample( nRma, nSampler, nExtra >= 0, uv, vGrad );
				m.flRoughness = lerp( m.flRoughness, vRma.r, vDecal.a );
				m.flMetalness = lerp( m.flMetalness, vRma.g, vDecal.a );
			}
		}
	}
