//-------------------------------------------------------------------------------------------------
// underbox stand-in for the closed depth_only.shader, which real engine2 draws shadow casters
// with: positions placed through the instance's transform-buffer entry (or
// the vertex cache) and the view's per-view constants, as the material stand-in's vertex stage
// does (render/draw/caster.rs).
//-------------------------------------------------------------------------------------------------
HEADER
{
	DevShader = true;
	Description = "underbox stand-in depth-only shader";
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
	#if ( D_CS_VERTEX_ANIMATION )
		float nVertexIndex : TEXCOORD14 < Semantic( MorphIndex ); >;
		float nVertexCacheIndex : TEXCOORD15 < Semantic( MorphIndex ); >;
	#endif
	uint nInstanceTransformID : TEXCOORD13 < Semantic( InstanceTransformUv ); >;
	uint4 vBlendIndices : BLENDINDICES < Semantic( BlendIndices ); >;
};

struct PS_INPUT
{
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

	// A vertex-animation draw's vertices were placed by skinning_cs in the vertex cache (standin.shader).
	PS_INPUT MainVs( const VS_INPUT i )
	{
		PS_INPUT o;
		#if ( D_CS_VERTEX_ANIMATION )
			ExtraShaderData_t extra = GetExtraPerInstanceShaderData( i.nInstanceTransformID );
			float3 vPositionWs = FetchCachedVertex( extra.nVertexCacheOffset + uint( i.nVertexCacheIndex ) ).vPosWs;
		#else
			float3 vPositionWs = mul( GetTransformMatrix( i.nInstanceTransformID, i.vBlendIndices.x ), float4( i.vPositionOs.xyz, 1.0 ) );
		#endif
		o.vPositionPs = Position3WsToPs( vPositionWs );
		return o;
	}
}

// Writes nothing: lets the main view's depth prepass, which keeps a colour target with writes masked, draw it.
PS
{
	void MainPs( PS_INPUT i )
	{
	}
}
