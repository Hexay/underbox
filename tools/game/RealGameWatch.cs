using Sandbox;
using System.Collections.Generic;
using System.Linq;

/// <summary>
/// Injected into a scratch copy of a real game by tools/game.sh. Logs `GAME:` markers once a second:
/// what the scene holds, when its bodies settle, and whether a PlayerController dropped onto the
/// floor comes to rest there. Logs `GAME:END` after idling IdleSeconds past the settle.
/// </summary>
public sealed class RealGameWatch : Component
{
	[Property] public int IdleSeconds { get; set; } = 15;

	const int SettledSeconds = 3;
	const int StandingSeconds = 5;
	const float DropHeight = 64f;

	TimeSince lastTick;
	PlayerController dropped;
	int ticks;
	int settled;
	int idle;
	int standing;
	int probes;
	bool reportedSettled;
	bool reportedStanding;
	bool ended;

	static void Mark( string what ) => Log.Info( $"GAME:{what}" );

	protected override void OnStart()
	{
		Mark( $"START server={Application.IsDedicatedServer} idle={IdleSeconds}" );
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

		// A body falling forever never sleeps; it is counted in `fallen`, not waited on.
		settled = awake - below <= 0 ? settled + 1 : 0;
		if ( !reportedSettled && settled >= SettledSeconds )
		{
			reportedSettled = true;
			Mark( $"SETTLED bodies={bodies.Count} fallen={below}" );
			DropController();
		}

		if ( !reportedSettled ) return;

		if ( dropped.IsValid() && !reportedStanding )
		{
			if ( probes++ < 3 ) ProbeDropped();
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
			Mark( "END" );
		}
	}

	void ProbeDropped()
	{
		var p = dropped.WorldPosition;
		var feet = new BBox( new Vector3( -12, -12, -2 ), new Vector3( 12, 12, 2 ) );
		var box = Scene.Trace.Box( feet, p + Vector3.Up * 6, p - Vector3.Up * 6 ).IgnoreGameObjectHierarchy( dropped.GameObject ).WithCollisionRules( dropped.Tags ).UseHitPosition().Run();
		var ray = Scene.Trace.Ray( p + Vector3.Up * 2, p - Vector3.Up * 6 ).IgnoreGameObjectHierarchy( dropped.GameObject ).WithCollisionRules( dropped.Tags ).UseHitPosition().Run();
		Mark( $"PROBE z={p.z:0.00} box hit={box.Hit} solid={box.StartedSolid} n={box.Normal} f={box.Fraction:0.00} hitz={box.HitPosition.z:0.00} body={box.Body?.BodyType} on={box.GameObject?.Name}"
			+ $" | ray hit={ray.Hit} solid={ray.StartedSolid} n={ray.Normal} hitz={ray.HitPosition.z:0.00} on={ray.GameObject?.Name}"
			+ $" | mode={dropped.Mode?.GetType().Name} ground={dropped.IsOnGround} tags=[{string.Join( " ", dropped.Tags )}] bodyv={dropped.Body?.Velocity} bodyz={dropped.Body?.WorldPosition.z:0.00} gravity={dropped.Body?.Gravity}" );
	}

	static string Describe( PlayerController p )
	{
		var ground = p.GroundObject?.Name ?? "nothing";
		return $"{p.GameObject.Name}@{p.WorldPosition.x:0},{p.WorldPosition.y:0},{p.WorldPosition.z:0} on={ground} v={p.Velocity.Length:0}";
	}
}
