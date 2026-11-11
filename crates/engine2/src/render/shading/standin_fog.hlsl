	// Managed's analytic fog (Sandbox.Fog → FogSetup.ToConstants), as core common/classes/Fog.hlsl
	// applies it: distance (ramp or exponential) and height opacity in their constant colours.
	// TODO: the sky-cubemap tint, the sun lobe, and the gradient/cubemap fallback below weight 1.
	cbuffer FogConstants
	{
		float4 FogControl;
		float4 FogDistance;
		float4 FogHeight;
		float3 FogColor;
		float FogOpacityScale;
		float4 FogHeightColor;
		float2 FogLayerParams;
		float2 FogSky;
		float4 FogSkyBlurRamp;
		float2 FogSun;
		float2 FogHeightSky;
		float4 FogSunSettings;
		float2 FogSkyFormat;
		float2 FogHeightSkyFormat;
		int FogSkyTextureIndex;
		int FogHeightSkyTextureIndex;
		float FogHeightSkyShared;
		float FogDistanceMetric;
		float4x4 FogSkyTransform;
		float4x4 FogHeightSkyTransform;
	};

	float FogDistanceOpacity( float3 vRay )
	{
		if ( FogControl.y <= 0.0 || FogLayerParams.x <= 0.0 )
			return 0.0;
		float flDistance = FogDistanceMetric > 0.5 ? max( 0.0, dot( vRay, g_vCameraDirWs.xyz ) ) : length( vRay );
		float flOpacity;
		if ( FogControl.z > 0.5 )
		{
			flOpacity = 1.0 - exp( -min( max( 0.0, flDistance - FogDistance.x ) / FogDistance.w, 80.0 ) );
		}
		else
		{
			float flRange = FogDistance.y - FogDistance.x;
			float flInverse = flRange != 0.0 ? 1.0 / flRange : 1000.0;
			flOpacity = pow( saturate( ( flDistance - FogDistance.x ) * flInverse ), FogDistance.z );
		}
		return min( flOpacity, FogControl.y ) * FogLayerParams.x;
	}

	float FogHeightOpacity( float flHeight )
	{
		if ( FogControl.w <= 0.0 || FogHeightColor.a <= 0.0 )
			return 0.0;
		float flGradient = FogHeight.y == 0.0 ? ( flHeight <= FogHeight.x ? 1.0 : 0.0 ) : saturate( 1.0 - ( flHeight - FogHeight.x ) / FogHeight.y );
		return pow( flGradient, FogLayerParams.y ) * FogHeightColor.a * FogControl.w;
	}

	float3 ApplyFog( float3 vPositionWs, float3 vColor )
	{
		float flWeight = saturate( FogControl.x );
		if ( flWeight <= 0.0 )
			return vColor;
		float3 vRay = vPositionWs - g_vCameraPositionWs.xyz;
		float flDistance = FogDistanceOpacity( vRay );
		float flHeight = FogHeightOpacity( vPositionWs.z );
		float flOpacity = flDistance + flHeight * ( 1.0 - flDistance );
		if ( flOpacity <= 0.0 )
			return vColor;
		float3 vFog = ( FogColor * flDistance + FogHeightColor.rgb * flHeight * ( 1.0 - flDistance ) ) / flOpacity;
		return lerp( vColor, lerp( vColor, vFog, flOpacity * FogOpacityScale ), flWeight );
	}
