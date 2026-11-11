//-------------------------------------------------------------------------------------------------
// underbox stand-in for gizmo/gizmo_line: its VS and GS folded into one vertex shader over the
// corners render/draw/gizmo.rs expands each segment into; the PS is the original's.
//-------------------------------------------------------------------------------------------------
HEADER
{
	DevShader = true;
	Description = "underbox gizmo line";
}

MODES
{
	Default();
	Forward();
}

FEATURES
{
	Feature( F_NO_ZTEST, 0..1 );
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
	// xyz: the segment's other endpoint; w: the side to extrude to.
	float4 vOtherOs : TEXCOORD1 < Semantic( LowPrecisionUv1 ); >;
	// xyz: the segment's first endpoint; w: 1 on it.
	float4 vFirstOs : TANGENT0 < Semantic( TangentU_SignV ); >;
	uint nInstanceTransformID : TEXCOORD13 < Semantic( InstanceTransformUv ); >;
};

struct PS_INPUT
{
	float4 vColor : COLOR0;
	float4 vFirstLineVertPosPs : TANGENT0;
	float4 vThisVertPosPs : TEXCOORD3;

	#if ( PROGRAM == VFX_PROGRAM_VS )
		float4 vPositionPs : SV_Position;
	#endif
};

VS
{
	#include "instancing.fxc"

	float g_LineThickness < Attribute( "LineThickness" ); Default( 1.0 ); >;
	float g_DepthBiasAmount < Default( 0.0 ); >;

	PS_INPUT MainVs( const VS_INPUT i )
	{
		float3x4 matObjectToWorld = GetTransformMatrix( i.nInstanceTransformID );
		float4 vThisPs = Position3WsToPs( mul( matObjectToWorld, float4( i.vPositionOs.xyz, 1.0 ) ) );
		float4 vOtherPs = Position3WsToPs( mul( matObjectToWorld, float4( i.vOtherOs.xyz, 1.0 ) ) );

		// The GS's near-plane fix: an endpoint behind the camera moves onto w = 0.01.
		const float flNearPlaneW = 0.01;
		if ( vThisPs.w < flNearPlaneW && vOtherPs.w >= flNearPlaneW )
			vThisPs = lerp( vThisPs, vOtherPs, ( flNearPlaneW - vThisPs.w ) / ( vOtherPs.w - vThisPs.w ) );
		else if ( vOtherPs.w < flNearPlaneW && vThisPs.w >= flNearPlaneW )
			vOtherPs = lerp( vOtherPs, vThisPs, ( flNearPlaneW - vOtherPs.w ) / ( vThisPs.w - vOtherPs.w ) );

		// The GS's normal runs from endpoint 0 to 1 whichever corner this is.
		float flFirst = i.vFirstOs.w;
		float4 vPs0 = flFirst > 0.5 ? vThisPs : vOtherPs;
		float4 vPs1 = flFirst > 0.5 ? vOtherPs : vThisPs;
		float2 vDir = ( ( vPs1.xy / vPs1.w ) - ( vPs0.xy / vPs0.w ) ) / ( 2.0 * g_vInvViewportSize.xy );
		float2 vNormal = float2( vDir.y, -vDir.x ) / max( length( vDir ), 1e-6 );
		vNormal *= 2.0 * g_vInvViewportSize.xy;

		PS_INPUT o;
		o.vPositionPs = vThisPs;
		o.vPositionPs.xy += i.vOtherOs.w * 0.5 * g_LineThickness * vNormal * o.vPositionPs.w;
		o.vFirstLineVertPosPs = Position3WsToPs( mul( matObjectToWorld, float4( i.vFirstOs.xyz, 1.0 ) ) );
		o.vThisVertPosPs = o.vPositionPs;

		float flProjDepth = saturate( o.vPositionPs.z / o.vPositionPs.w );
		o.vPositionPs.z += lerp( g_DepthBiasAmount * 0.00001, 0.00001, flProjDepth ) * o.vPositionPs.w;

		o.vColor = float4( SrgbGammaToLinear( i.vColor.rgb ), i.vColor.a );
		return o;
	}
}

PS
{
	StaticCombo( S_NO_ZTEST, F_NO_ZTEST, Sys( ALL ) );
	DynamicCombo( D_NO_ZTEST, 0..1, Sys( ALL ) );

	RenderState( FillMode, SOLID );
	RenderState( CullMode, NONE );
	RenderState( DepthWriteEnable, false );
	RenderState( BlendEnable, true );
	RenderState( SrcBlend, SRC_ALPHA );
	RenderState( DstBlend, INV_SRC_ALPHA );

	#if ( D_NO_ZTEST || S_NO_ZTEST )
		RenderState( DepthEnable, false );
	#else
		RenderState( DepthEnable, true );
	#endif

	// 0 solid, 1 dashed, 2 dotted.
	float g_flPatternType < Attribute( "PatternType" ); Default( 0.0 ); >;
	float4 g_tint < Attribute( "g_tint" ); Default4( 1.0, 1.0, 1.0, 1.0 ); >;

	struct PS_OUTPUT
	{
		float4 vColor : SV_Target0;
	};

	bool RejectDottedLinePixel( PS_INPUT i )
	{
		float flGapDist = g_flPatternType == 1 ? 16.0 : ( g_flPatternType == 2 ? 3.0 : 0.0 );
		float2 vScreenPos1 = i.vFirstLineVertPosPs.xy / i.vFirstLineVertPosPs.w / g_vInvViewportSize.xy;
		float2 vScreenPos2 = i.vThisVertPosPs.xy / i.vThisVertPosPs.w / g_vInvViewportSize.xy;
		return flGapDist != 0.0 && fmod( length( vScreenPos2 - vScreenPos1 ), flGapDist * 2.0 ) > flGapDist;
	}

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		if ( RejectDottedLinePixel( i ) )
		{
			clip( -1.0 );
		}

		PS_OUTPUT o;
		o.vColor = i.vColor * g_tint;
		return o;
	}
}
