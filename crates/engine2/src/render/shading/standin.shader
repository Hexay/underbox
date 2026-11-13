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
	// vr_common_vs_input.fxc's order, which real's capture shows as its input locations.
	float3 vPositionOs : POSITION < Semantic( PosXyz ); >;
	float2 vTexCoord : TEXCOORD0 < Semantic( LowPrecisionUv ); >;
	float4 vNormalOs : NORMAL < Semantic( OptionallyCompressedTangentFrame ); >;
	float4 vTangentUOs_flTangentVSign : TANGENT < Semantic( TangentU_SignV ); >;
	#if ( D_CS_VERTEX_ANIMATION )
		float4 vBlendWeight : BLENDWEIGHT < Semantic( BlendWeight ); >;
		float nVertexIndex : TEXCOORD14 < Semantic( MorphIndex ); >;
		float nVertexCacheIndex : TEXCOORD15 < Semantic( MorphIndex ); >;
	#endif
	uint nInstanceTransformID : TEXCOORD13 < Semantic( InstanceTransformUv ); >;
	uint4 vBlendIndices : BLENDINDICES < Semantic( BlendIndices ); >;
};

struct PS_INPUT
{
	float3 vNormalWs : TEXCOORD0;
	float2 vTexCoord : TEXCOORD1;
	float3 vPositionWs : TEXCOORD2;
	float4 vTangentUWs_flTangentVSign : TEXCOORD3;
	float4 vTint : COLOR0;

	#if ( PROGRAM == VFX_PROGRAM_VS )
		float4 vPositionPs : SV_Position;
	#else
		float4 vPositionSs : SV_Position;
	#endif
};

VS
{
	#include "instancing.fxc"
	#include "vs_decompress.fxc"

	float4 g_vTexCoordScale < Attribute( "g_vTexCoordScale" ); Default4( 1.0, 1.0, 0.0, 0.0 ); >;
	float4 g_vTexCoordOffset < Attribute( "g_vTexCoordOffset" ); Default4( 0.0, 0.0, 0.0, 0.0 ); >;

	// A vertex-animation draw (render/skin) reads what skinning_cs placed in the vertex cache, as
	// vr_common_vs_code.fxc does; a rigid entry's bone is ignored.
	PS_INPUT MainVs( const VS_INPUT i )
	{
		PS_INPUT o;
		ExtraShaderData_t extra = GetExtraPerInstanceShaderData( i.nInstanceTransformID );
		float3 vPositionWs;
		#if ( D_CS_VERTEX_ANIMATION )
		{
			CachedAnimatedVertex_t v = FetchCachedVertex( extra.nVertexCacheOffset + uint( i.nVertexCacheIndex ) );
			float3 vTangent;
			float flSign;
			DecodeCachedVertexTangentSpace( v, o.vNormalWs, vTangent, flSign );
			o.vTangentUWs_flTangentVSign = float4( vTangent, flSign );
			vPositionWs = v.vPosWs;
		}
		#else
		{
			float3x4 matObjectToWorld = GetTransformMatrix( i.nInstanceTransformID, i.vBlendIndices.x );
			vPositionWs = mul( matObjectToWorld, float4( i.vPositionOs.xyz, 1.0 ) );
			float3 vNormalOs;
			float4 vTangentOs;
			DecompressNormalTangent( i.vNormalOs, i.vTangentUOs_flTangentVSign, vNormalOs, vTangentOs );
			o.vNormalWs = mul( (float3x3)matObjectToWorld, vNormalOs );
			o.vTangentUWs_flTangentVSign = float4( mul( (float3x3)matObjectToWorld, vTangentOs.xyz ), vTangentOs.w );
		}
		#endif
		o.vPositionPs = Position3WsToPs( vPositionWs );
		o.vPositionWs = vPositionWs;
		o.vTexCoord = i.vTexCoord * g_vTexCoordScale.xy + g_vTexCoordOffset.xy;
		o.vTint = extra.vTint;
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
	float g_flOpacityScale < Attribute( "g_flOpacityScale" ); Default( 1.0 ); >;
	// complex's normal map: hemi-octahedral normal in rg, roughness in b (unused normal kept flat).
	Texture2D g_tNormal < Attribute( "g_tNormal" ); >;
	float g_flRoughnessScaleFactor < Attribute( "g_flRoughnessScaleFactor" ); Default( 1.0 ); >;
	float g_flMetalness < Attribute( "g_flMetalness" ); Default( 0.0 ); >;
	// 1 for shaders that map here from an unlit original (standin.rs).
	float g_flStandinUnlit < Attribute( "StandinUnlit" ); Default( 0.0 ); >;
	// A managed draw's compiled draw-call tint (draw/draw_call.rs); native draws fold it into the instance tint.
	float4 g_vStandinDrawTint < Attribute( "StandinDrawTint" ); Default4( 1.0, 1.0, 1.0, 1.0 ); >;
	// UNDERBOX_STANDIN_DEBUG (standin.rs): 1 paints sun visibility, 2 cascade 0's shadow-space position.
	float g_flStandinDebug < Attribute( "StandinDebug" ); Default( 0.0 ); >;
	// 1 for error.shader (standin.rs): albedo is a world-space red checker, fitted to real's frames.
	float g_flStandinError < Attribute( "StandinError" ); Default( 0.0 ); >;
	// 1 for generic.shader (standin.rs), whose tint is linear.
	float g_flStandinLinearTint < Attribute( "StandinLinearTint" ); Default( 0.0 ); >;

	float3 ErrorChecker( float3 vPositionWs )
	{
		float3 c = floor( vPositionWs / 2.0 );
		return fmod( abs( c.x + c.y + c.z ), 2.0 ) < 1.0 ? float3( 0.42, 0.0, 0.0 ) : float3( 0.17, 0.0, 0.0 );
	}

	struct PS_OUTPUT
	{
		float4 vColor : SV_Target0;
	};

	//@sun_shadows

	//@managed_shadows
	#include "common/classes/Fog.hlsl"
	// Local lights and decals come from managed's cluster lists (ClusteredCullingLayer), as in vr_lighting.fxc.
	#include "common/classes/ClusterCulling.hlsl"

	float LocalVisibility( BinnedLight l, float3 vPositionWs, float3 vNormalWs )
	{
		if ( l.ShadowMapIndex == 0xFFFFFFFFu )
			return 1.0;
		return ManagedLocalVisibility( l, vPositionWs, vNormalWs );
	}

	//@lighting

	//@indirect

	//@decals

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		PS_OUTPUT o;
		float4 vAlbedo = g_tColor.Sample( g_sStandin, i.vTexCoord );
		if ( g_flStandinError != 0.0 )
			vAlbedo = float4( ErrorChecker( i.vPositionWs ), 1.0 );
		float4 vInstanceTint = i.vTint * g_vStandinDrawTint;
		float3 vTint = lerp( float3( 1.0, 1.0, 1.0 ), vInstanceTint.rgb, g_flModelTintAmount );
		// complex's colour tint is gamma space (vr_shared_standard_vs_code.fxc).
		float3 vColorTint = g_flStandinLinearTint != 0.0 ? g_vColorTint.rgb : SrgbGammaToLinear( g_vColorTint.rgb );
		float3 vColor = vAlbedo.rgb * vColorTint * vTint;

		float3 vNormalWs = normalize( i.vNormalWs );
		Surface s;
		s.vPositionWs = i.vPositionWs;
		s.vToCameraWs = CalculatePositionToCameraDirWs( i.vPositionWs );
		// Before any branch: derivatives (BRDF.hlsl's AdjustRoughnessByGeometricNormal).
		float3 vDx = ddx( vNormalWs ), vDy = ddy( vNormalWs );
		float flGeometric = pow( saturate( max( dot( vDx, vDx ), dot( vDy, vDy ) ) ), 0.333 );
		// A uniform branch, so its derivatives stay valid.
		float3 vBentNormalWs = vNormalWs;
		if ( g_flStandinBentNormals != 0.0 )
			vBentNormalWs = BentNormalWs( vNormalWs, i.vPositionWs, i.vTexCoord );
		uint nCluster = Cluster::Index( i.vPositionSs );
		DecalSurface m;
		m.vAlbedo = vColor;
		m.vNormalWs = vNormalWs;
		m.flRoughness = pow( g_tNormal.Sample( g_sStandin, i.vTexCoord ).b, g_flRoughnessScaleFactor );
		m.flMetalness = g_flMetalness;
		m.vEmission = 0.0;
		if ( g_flStandinUnlit == 0.0 )
			ApplyDecals( i.vPositionWs, m, nCluster );
		vColor = m.vAlbedo;
		s.vNormalWs = m.vNormalWs;
		s.flRoughness = max( m.flRoughness, flGeometric );
		s.vSpecularColor = lerp( float3( 0.04, 0.04, 0.04 ), vColor, m.flMetalness );
		s.vPositionSs = i.vPositionSs;
		DirectLight d = Direct( s, nCluster );
		Surface sIndirect = s;
		if ( g_flStandinBentNormals != 0.0 )
			sIndirect.vNormalWs = vBentNormalWs;
		float3 vAmbient = AmbientLightColor.rgb * AmbientOcclusion( i.vTexCoord );
		float3 vLit = vColor * ( 1.0 - m.flMetalness ) * ( vAmbient + d.vDiffuse ) + d.vSpecular + EnvMapSpecular( sIndirect, nCluster ) + m.vEmission;
		o.vColor = float4( Fog::Apply( i.vPositionWs, float2( 0.0, 0.0 ), lerp( vLit, vColor, g_flStandinUnlit ) ), vAlbedo.a * g_flOpacityScale * vInstanceTint.a );
		if ( g_flStandinDebug == 1.0 )
			o.vColor = float4( SunVisibility( i.vPositionWs, vNormalWs, i.vPositionSs ).xxx, 1.0 );
		else if ( g_flStandinDebug == 2.0 )
			o.vColor = float4( mul( g_DirectionalLightWorldToShadowViewMatrices[0], float4( i.vPositionWs, 1.0 ) ).xyz, 1.0 );
		return o;
	}
}
