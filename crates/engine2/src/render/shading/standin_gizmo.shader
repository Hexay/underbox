//-------------------------------------------------------------------------------------------------
// underbox stand-in for gizmo/gizmo_line and gizmo/gizmo_sprite, whose geometry shaders naga can't
// translate: the primitives as submitted (1-pixel lines, points), unlit in vertex colour times
// g_tint. See render/shading/standin.rs.
//-------------------------------------------------------------------------------------------------
HEADER
{
	DevShader = true;
	Description = "underbox gizmo stand-in";
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
	#include "math_general.fxc"
}

struct VS_INPUT
{
	float3 vPositionOs : POSITION < Semantic( PosXyz ); >;
	float4 vColor : COLOR0 < Semantic( Color ); >;
	uint nInstanceTransformID : TEXCOORD13 < Semantic( InstanceTransformUv ); >;
};

struct PS_INPUT
{
	float4 vColor : COLOR0;

	#if ( PROGRAM == VFX_PROGRAM_VS )
		float4 vPositionPs : SV_Position;
	#endif
};

VS
{
	#include "instancing.fxc"

	PS_INPUT MainVs( const VS_INPUT i )
	{
		PS_INPUT o;
		float3x4 matObjectToWorld = GetTransformMatrix( i.nInstanceTransformID );
		o.vPositionPs = Position3WsToPs( mul( matObjectToWorld, float4( i.vPositionOs.xyz, 1.0 ) ) );
		o.vColor = float4( SrgbGammaToLinear( i.vColor.rgb ), i.vColor.a );
		return o;
	}
}

PS
{
	float4 g_tint < Attribute( "g_tint" ); Default4( 1.0, 1.0, 1.0, 1.0 ); >;

	struct PS_OUTPUT
	{
		float4 vColor : SV_Target0;
	};

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		PS_OUTPUT o;
		o.vColor = i.vColor * g_tint;
		return o;
	}
}
