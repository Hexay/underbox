//-------------------------------------------------------------------------------------------------
// underbox stand-in for gizmo/gizmo_sprite: its VS and GS folded into one vertex shader over the
// four corners render/draw/gizmo.rs expands each point into; the PS is the original's.
//-------------------------------------------------------------------------------------------------
HEADER
{
	DevShader = true;
	Description = "underbox gizmo sprite";
}

MODES
{
	Default();
	Forward();
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
	float3 vPositionWs : POSITION < Semantic( PosXyz ); >;
	float4 vColor : COLOR0 < Semantic( Color ); >;
	// xy: size; z: 0 screen space, 1 world space; w: rotation.
	float4 vTexCoord : TEXCOORD0 < Semantic( LowPrecisionUv ); >;
	// xy: the corner, ±1.
	float4 vCorner : TEXCOORD1 < Semantic( LowPrecisionUv1 ); >;
};

struct PS_INPUT
{
	float4 vColor : COLOR0;
	float2 vTexCoord : TEXCOORD0;

	#if ( PROGRAM == VFX_PROGRAM_VS )
		float4 vPositionPs : SV_Position;
	#endif
};

VS
{
	float2 RotateDelta( float2 delta, float angle )
	{
		float c = cos( angle );
		float s = sin( angle );
		return float2( delta.x * c + delta.y * s, -delta.x * s + delta.y * c );
	}

	PS_INPUT MainVs( const VS_INPUT i )
	{
		float2 vSize = i.vTexCoord.xy;
		float fWorldSpace = i.vTexCoord.z;
		float2 vDelta = RotateDelta( i.vCorner.xy, i.vTexCoord.w );

		float3 vWs = i.vPositionWs.xyz;
		float3 vRight = cross( g_vCameraDirWs.xyz, g_vCameraUpDirWs.xyz );
		vWs += 0.5 * vSize.x * vDelta.x * vRight * fWorldSpace;
		vWs += 0.5 * vSize.y * vDelta.y * g_vCameraUpDirWs.xyz * fWorldSpace;

		PS_INPUT o;
		o.vPositionPs = Position3WsToPs( vWs );
		o.vPositionPs.xy += ( vSize * g_vInvViewportSize.xy * vDelta * o.vPositionPs.w ) * ( 1.0 - fWorldSpace );
		o.vColor = float4( SrgbGammaToLinear( i.vColor.rgb ), i.vColor.a );
		o.vTexCoord = float2( i.vCorner.x * 0.5 + 0.5, 0.5 - i.vCorner.y * 0.5 );
		return o;
	}
}

PS
{
	DynamicCombo( D_NO_ZTEST, 0..1, Sys( ALL ) );

	CreateInputTexture2D( TextureColor, Srgb, 8, "", "_sprite", "TextureColor", Default4( 1.0, 1.0, 1.0, 1.0 ) );
	Texture2D g_color < Attribute( "TextureColor" ); Channel( RGBA, Box( TextureColor ), Srgb ); OutputFormat( DXT5 ); SrgbRead( true ); >;

	RenderState( FillMode, SOLID );
	RenderState( CullMode, NONE );

	#if ( D_NO_ZTEST )
		RenderState( DepthEnable, false );
		RenderState( DepthWriteEnable, false );
	#else
		RenderState( DepthEnable, true );
		RenderState( DepthWriteEnable, true );
	#endif

	RenderState( BlendEnable, true );
	RenderState( SrcBlend, SRC_ALPHA );
	RenderState( DstBlend, INV_SRC_ALPHA );
	RenderState( BlendOp, ADD );
	RenderState( SrcBlendAlpha, ONE );
	RenderState( DstBlendAlpha, INV_SRC_ALPHA );
	RenderState( BlendOpAlpha, ADD );

	struct PS_OUTPUT
	{
		float4 vColor : SV_Target0;
	};

	PS_OUTPUT MainPs( PS_INPUT i )
	{
		PS_OUTPUT o;
		o.vColor = i.vColor * g_color.Sample( g_sDefault, i.vTexCoord ).rgba;
		clip( o.vColor.a - 0.1 );
		return o;
	}
}
