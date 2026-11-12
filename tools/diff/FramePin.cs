using Sandbox;
using System.Linq;

/// <summary>
/// Injected into a scratch copy of a real game by tools/diff/frame.sh so both engines render the same frame:
/// pins the scene camera to Position/Angles/FieldOfView (when Position is set), stops scene time at
/// FreezeAt scene seconds, holds every Rigidbody still, removes particle effects (their emitters
/// draw from an unseeded random) and, unless KeepHud, hides screen panels (real draws no HUD under
/// -nosteam, docs/differential-render.md). Logs `FRAME:PINNED` once time stops.
/// </summary>
public sealed class FramePin : Component
{
	[Property] public string Position { get; set; } = "";
	[Property] public string Angles { get; set; } = "";
	[Property] public float FieldOfView { get; set; } = 0;
	[Property] public float FreezeAt { get; set; } = 0;
	[Property] public bool KeepParticles { get; set; } = false;
	[Property] public bool KeepHud { get; set; } = false;

	/// <summary>Stands in for the game's Networking.CreateLobby (tools/game/prepare.py --stub-lobby).</summary>
	public static void SkipLobby( Sandbox.Network.LobbyConfig config ) => Log.Info( "FRAME:NO_LOBBY" );

	TimeSince started;
	bool frozen;

	protected override void OnStart()
	{
		started = 0;
		// Bodies would move for a frame-timed while before time stops.
		foreach ( var body in Scene.GetAllComponents<Rigidbody>() )
			body.MotionEnabled = false;
		if ( KeepParticles ) return;
		foreach ( var effect in Scene.GetAllComponents<ParticleEffect>().ToList() )
		{
			Log.Info( $"FRAME:DROPPED {effect.GameObject.Name}" );
			effect.GameObject.Destroy();
		}
	}

	protected override void OnUpdate()
	{
		Pin();
		// Every frame: additively loaded scenes (walker's engine.scene) bring their panels later.
		if ( !KeepHud )
		{
			foreach ( var panel in Scene.GetAllComponents<ScreenPanel>().Where( p => p.Enabled ).ToList() )
			{
				Log.Info( $"FRAME:HID {panel.GameObject.Name}" );
				panel.Enabled = false;
			}
		}
		if ( frozen || started < FreezeAt ) return;
		frozen = true;
		Scene.TimeScale = 0;
		var cam = Scene.Camera;
		Log.Info( $"FRAME:PINNED camera={cam?.WorldPosition} angles={cam?.WorldRotation.Angles()} fov={cam?.FieldOfView}" );
	}

	// Again here: a player's camera controller may move the camera after our OnUpdate.
	protected override void OnPreRender() => Pin();

	void Pin()
	{
		var cam = Scene.Camera;
		if ( cam is null || string.IsNullOrEmpty( Position ) ) return;
		cam.WorldPosition = Vector3.Parse( Position );
		cam.WorldRotation = global::Angles.Parse( Angles );
		if ( FieldOfView > 0 ) cam.FieldOfView = FieldOfView;
	}
}
