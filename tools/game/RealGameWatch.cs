using Sandbox;
using System;
using System.Collections.Generic;
using System.Linq;

/// <summary>
/// Injected into a scratch copy of a real game by tools/game.sh. Logs `GAME:` markers once a second:
/// what the scene holds, when its bodies settle, and whether a PlayerController dropped onto the
/// floor comes to rest there. On the host, runs StartCommand once things settle. Any player pawn a
/// connection owns that comes to rest logs `GAME:PLAYER_STANDING`, on the host and on the client.
/// Logs `GAME:END` after idling IdleSeconds past the settle.
/// </summary>
public sealed class RealGameWatch : Component
{
	[Property] public int IdleSeconds { get; set; } = 15;
	[Property] public string StartCommand { get; set; } = "";

	const int SettledSeconds = 3;
	const int StandingSeconds = 5;
	const int ReportAfterStartSeconds = 10;
	const float DropHeight = 64f;

	TimeSince lastTick;
	PlayerController dropped;
	int ticks;
	int settled;
	int idle;
	int standing;
	int sinceStart = -1;
	bool reportedSettled;
	bool reportedStanding;
	bool ended;
	readonly Dictionary<PlayerController, int> ownedStanding = new();
	readonly HashSet<PlayerController> ownedReported = new();

	static void Mark( string what ) => Log.Info( $"GAME:{what}" );

	protected override void OnStart()
	{
		Mark( $"START server={Application.IsDedicatedServer} host={Networking.IsHost} idle={IdleSeconds}" );
	}

	void ReportBodies( List<Rigidbody> bodies )
	{
		foreach ( var b in bodies.Where( b => !b.PhysicsBody.Sleeping ) )
		{
			var model = b.Components.Get<ModelRenderer>( FindMode.EverythingInSelfAndDescendants )?.Model;
			var colliders = b.Components.GetAll<Collider>( FindMode.EverythingInSelfAndDescendants ).Select( c => c.GetType().Name );
			Mark( $"AWAKE {b.GameObject.Name} z={b.WorldPosition.z:0} v={b.Velocity.Length:0} model={model?.ResourcePath ?? "none"} error={model?.IsError}"
				+ $" shapes={b.PhysicsBody.ShapeCount} colliders=[{string.Join( " ", colliders )}]" );
		}
	}

	void DropController()
	{
		var down = Scene.Trace.Ray( new Vector3( 0, 0, 4000 ), new Vector3( 0, 0, -4000 ) ).Run();
		// Not onto the head of a player the scene already has at the origin.
		if ( down.GameObject?.Components.Get<PlayerController>().IsValid() == true )
			down = Scene.Trace.Ray( new Vector3( 128, 128, 4000 ), new Vector3( 128, 128, -4000 ) ).Run();
		Mark( $"TRACE origin hit={down.Hit} z={down.EndPosition.z:0} on={down.GameObject?.Name}" );
		if ( !down.Hit ) return;
		var go = Scene.CreateObject();
		go.Name = "RealGameWatchController";
		go.WorldPosition = down.EndPosition + Vector3.Up * DropHeight;
		dropped = go.Components.Create<PlayerController>();
		// A probe only: on a host client it must not take over the scene camera or the mouse look.
		dropped.UseInputControls = false;
		dropped.UseCameraControls = false;
		dropped.UseLookControls = false;
		Mark( $"DROP at z={go.WorldPosition.z:0}" );
	}

	// What a round built: resources by extension and the scene's components by type.
	void ReportContents()
	{
		var resources = ResourceLibrary.GetAll<GameResource>()
			.GroupBy( r => System.IO.Path.GetExtension( r.ResourcePath ?? "" ).TrimStart( '.' ) )
			.OrderBy( g => g.Key ).Select( g => $"{g.Key}={g.Count()}" );
		Mark( $"RESOURCES {string.Join( " ", resources )}" );
		// Scene.GetAllComponents<Component>() is empty: its type index has no entry for the base type.
		var components = Scene.GetAllObjects( false ).SelectMany( o => o.Components.GetAll() )
			.GroupBy( c => c.GetType().Name ).OrderByDescending( g => g.Count() ).Select( g => $"{g.Key}={g.Count()}" );
		Mark( $"COMPONENTS {string.Join( " ", components )}" );
		// Bodies made straight from code (e.g. Sdf2DChunk's terrain) have no Collider component.
		var physics = Scene.PhysicsWorld.Bodies.ToList();
		Mark( $"PHYSICS bodies={physics.Count} shapes={physics.Sum( b => b.ShapeCount )}" );
	}

	void TrackOwnedPlayers( List<PlayerController> players )
	{
		foreach ( var p in players.Where( p => p.Network.Owner is not null && !ownedReported.Contains( p ) ) )
		{
			var still = p.IsOnGround && p.Velocity.Length < 5;
			ownedStanding[p] = still ? ownedStanding.GetValueOrDefault( p ) + 1 : 0;
			if ( ownedStanding[p] < StandingSeconds ) continue;
			ownedReported.Add( p );
			Mark( $"PLAYER_STANDING owner={p.Network.Owner.DisplayName} local={!p.IsProxy} {Describe( p )}" );
			poses[p.GameObject] = (p.WorldPosition, p.EyeAngles.yaw);
		}
		foreach ( var p in players.Where( p => ownedReported.Contains( p ) ) )
			ReportMotion( "PLAYER", $"owner={p.Network.Owner?.DisplayName} local={!p.IsProxy}", p.GameObject, p.WorldPosition, p.EyeAngles.yaw );
	}

	// Input evidence for tools/game.sh MODE=play: a pawn or camera that moved or turned since its rest pose.
	const float MovedUnits = 32f;
	const float TurnedDegrees = 10f;
	readonly Dictionary<GameObject, (Vector3 Pos, float Yaw)> poses = new();
	readonly HashSet<string> motionReported = new();

	void ReportMotion( string kind, string who, GameObject key, Vector3 pos, float yaw )
	{
		if ( !poses.TryGetValue( key, out var rest ) ) { poses[key] = (pos, yaw); return; }
		var moved = (pos - rest.Pos).WithZ( 0 ).Length;
		var turned = MathF.Abs( Angles.NormalizeAngle( yaw - rest.Yaw ) );
		if ( moved >= MovedUnits && motionReported.Add( $"{kind}_MOVED {who}" ) )
			Mark( $"{kind}_MOVED {who} dist={moved:0} from={rest.Pos.x:0},{rest.Pos.y:0} to={pos.x:0},{pos.y:0}" );
		if ( turned >= TurnedDegrees && motionReported.Add( $"{kind}_TURNED {who}" ) )
			Mark( $"{kind}_TURNED {who} yaw={rest.Yaw:0}->{yaw:0}" );
	}

	bool inputSeen;

	// The camera's rest pose is where it was when input first arrived, so spawning can't count as moving.
	protected override void OnUpdate()
	{
		if ( Application.IsDedicatedServer || ended || !Scene.Camera.IsValid() ) return;
		var cam = Scene.Camera;
		if ( !inputSeen && (!Input.AnalogMove.IsNearlyZero() || !Input.AnalogLook.IsNearlyZero()) )
		{
			inputSeen = true;
			Mark( $"INPUT move={Input.AnalogMove} look={Input.AnalogLook} mouse_visible={Mouse.Visible}" );
			poses[cam.GameObject] = (cam.WorldPosition, cam.WorldRotation.Yaw());
		}
		if ( inputSeen ) ReportMotion( "CAMERA", "view", cam.GameObject, cam.WorldPosition, cam.WorldRotation.Yaw() );
	}

	protected override void OnFixedUpdate()
	{
		if ( ended || lastTick < 1 ) return;
		lastTick = 0;

		// A PlayerController keeps its own body awake, so a scene with a player in it would never settle.
		var bodies = Scene.GetAllComponents<Rigidbody>()
			.Where( b => b.PhysicsBody.IsValid() && !b.Components.Get<PlayerController>().IsValid() ).ToList();
		var awake = bodies.Count( b => !b.PhysicsBody.Sleeping );
		var below = bodies.Count( b => b.WorldPosition.z < -2000 );
		var colliders = Scene.GetAllComponents<Collider>().ToList();
		var meshes = Scene.GetAllComponents<MeshComponent>().Count();
		var renderers = Scene.GetAllComponents<ModelRenderer>().ToList();
		var noModel = renderers.Count( r => r.Model is null || r.Model.IsError );
		var players = Scene.GetAllComponents<PlayerController>().ToList();

		Mark( $"TICK bodies={bodies.Count} awake={awake} fallen={below} colliders={colliders.Count} meshes={meshes}"
			+ $" renderers={renderers.Count} error_models={noModel} objects={Scene.Directory.GameObjectCount} players={players.Count}"
			+ $" connections={Connection.All.Count} {DescribeCamera()}" + string.Join( "; ", players.Select( Describe ) ) );

		if ( ++ticks == 4 ) ReportBodies( bodies );
		TrackOwnedPlayers( players );

		// A body falling forever never sleeps; it is counted in `fallen`, not waited on.
		settled = awake - below <= 0 ? settled + 1 : 0;
		if ( !reportedSettled && settled >= SettledSeconds )
		{
			reportedSettled = true;
			Mark( $"SETTLED bodies={bodies.Count} fallen={below}" );
			// A client's controller would be a local object nobody else sees: only the host drops one.
			if ( Networking.IsHost ) DropController();
			if ( Networking.IsHost && !string.IsNullOrWhiteSpace( StartCommand ) )
			{
				Mark( $"COMMAND {StartCommand}" );
				ConsoleSystem.Run( StartCommand );
				sinceStart = 0;
			}
		}

		if ( !reportedSettled ) return;

		if ( sinceStart >= 0 && ++sinceStart == ReportAfterStartSeconds ) ReportContents();

		if ( dropped.IsValid() && !reportedStanding )
		{
			standing = dropped.IsOnGround && dropped.Velocity.Length < 5 ? standing + 1 : 0;
			if ( standing >= StandingSeconds )
			{
				reportedStanding = true;
				Mark( $"STANDING {Describe( dropped )}" );
			}
		}

		if ( ++idle >= IdleSeconds )
		{
			ended = true;
			if ( sinceStart < ReportAfterStartSeconds ) ReportContents();
			Mark( "END" );
		}
	}

	string DescribeCamera()
	{
		var cam = Scene.Camera;
		if ( Application.IsDedicatedServer || !cam.IsValid() ) return "";
		return $"camera@{cam.WorldPosition.x:0},{cam.WorldPosition.y:0},{cam.WorldPosition.z:0} yaw={cam.WorldRotation.Yaw():0} ";
	}

	static string Describe( PlayerController p )
	{
		var ground = p.GroundObject?.Name ?? "nothing";
		return $"{p.GameObject.Name}@{p.WorldPosition.x:0},{p.WorldPosition.y:0},{p.WorldPosition.z:0} on={ground} v={p.Velocity.Length:0} yaw={p.EyeAngles.yaw:0}";
	}
}
