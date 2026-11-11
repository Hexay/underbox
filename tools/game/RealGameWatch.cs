using Sandbox;
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
		Mark( $"TRACE origin hit={down.Hit} z={down.EndPosition.z:0} on={down.GameObject?.Name}" );
		if ( !down.Hit ) return;
		var go = Scene.CreateObject();
		go.Name = "RealGameWatchController";
		go.WorldPosition = down.EndPosition + Vector3.Up * DropHeight;
		dropped = go.Components.Create<PlayerController>();
		dropped.UseInputControls = false;
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
		}
	}

	protected override void OnFixedUpdate()
	{
		if ( ended || lastTick < 1 ) return;
		lastTick = 0;

		var bodies = Scene.GetAllComponents<Rigidbody>().Where( b => b.PhysicsBody.IsValid() ).ToList();
		var awake = bodies.Count( b => !b.PhysicsBody.Sleeping );
		var below = bodies.Count( b => b.WorldPosition.z < -2000 );
		var colliders = Scene.GetAllComponents<Collider>().ToList();
		var meshes = Scene.GetAllComponents<MeshComponent>().Count();
		var renderers = Scene.GetAllComponents<ModelRenderer>().ToList();
		var noModel = renderers.Count( r => r.Model is null || r.Model.IsError );
		var players = Scene.GetAllComponents<PlayerController>().ToList();

		Mark( $"TICK bodies={bodies.Count} awake={awake} fallen={below} colliders={colliders.Count} meshes={meshes}"
			+ $" renderers={renderers.Count} error_models={noModel} objects={Scene.Directory.GameObjectCount} players={players.Count}"
			+ $" connections={Connection.All.Count} " + string.Join( "; ", players.Select( Describe ) ) );

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

	static string Describe( PlayerController p )
	{
		var ground = p.GroundObject?.Name ?? "nothing";
		return $"{p.GameObject.Name}@{p.WorldPosition.x:0},{p.WorldPosition.y:0},{p.WorldPosition.z:0} on={ground} v={p.Velocity.Length:0}";
	}
}
