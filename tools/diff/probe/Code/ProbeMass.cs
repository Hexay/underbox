using Sandbox;
using System;
using System.Globalization;

/// <summary>
/// Mass queries on static, keyframed and dynamic bodies (`MASS`): mass, centre, inertia and
/// density as created, after a type change, and after overrides set while not dynamic
/// (docs/differential-joints.md, 20).
/// </summary>
public static class ProbeMass
{
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";
	static string R4( Rotation r ) => $"{F4( r.x )},{F4( r.y )},{F4( r.z )},{F4( r.w )}";
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	static readonly Vector3 At = new( 100, 200, 300 );

	static void Log4( string name, PhysicsBody b ) =>
		Mark( $"MASS {name} type={b.BodyType} mass={F4( b.Mass )} center={V4( b.MassCenter )} local={V4( b.LocalMassCenter )} inertia={V4( b.Inertia )} irot={R4( b.InertiaRotation )} density={F4( b.Density )} override={b.OverrideMassCenter}" );

	/// A body at `At`, yawed 30°, with an off-centre box (the zoo sign's size) added after its type is set.
	static PhysicsBody Make( PhysicsWorld world, PhysicsBodyType type )
	{
		var b = world.CreateBody();
		b.BodyType = type;
		b.Position = At;
		b.Rotation = Rotation.FromYaw( 30 );
		b.AddBoxShape( new BBox( new Vector3( -5, -40, 0 ), new Vector3( 5, 60, 25 ) ), Rotation.Identity );
		return b;
	}

	static void Case( string name, PhysicsBodyType type, Action<PhysicsBody> then = null, PhysicsBodyType? after = null )
	{
		var world = new PhysicsWorld();
		var b = Make( world, type );
		then?.Invoke( b );
		Log4( name, b );
		if ( after is { } t )
		{
			b.BodyType = t;
			Log4( $"{name} >{t}", b );
			b.BodyType = type;
			Log4( $"{name} >{t} >{type}", b );
		}
		world.Delete();
	}

	public static void Run()
	{
		Case( "static", PhysicsBodyType.Static, after: PhysicsBodyType.Dynamic );
		Case( "keyframed", PhysicsBodyType.Keyframed, after: PhysicsBodyType.Dynamic );
		Case( "dynamic", PhysicsBodyType.Dynamic, after: PhysicsBodyType.Static );
		Case( "dynkey", PhysicsBodyType.Dynamic, after: PhysicsBodyType.Keyframed );
		Case( "static mass=5", PhysicsBodyType.Static, b => b.Mass = 5, PhysicsBodyType.Dynamic );
		Case( "dynamic mass=5", PhysicsBodyType.Dynamic, b => b.Mass = 5, PhysicsBodyType.Static );
		Case( "keyframed mass=5", PhysicsBodyType.Keyframed, b => b.Mass = 5, PhysicsBodyType.Dynamic );
		Case( "static inertia", PhysicsBodyType.Static, b => b.SetInertiaTensor( new Vector3( 1, 2, 3 ), Rotation.Identity ), PhysicsBodyType.Dynamic );
		Case( "keyframed center", PhysicsBodyType.Keyframed, b => { b.OverrideMassCenter = true; b.LocalMassCenter = new Vector3( 1, 2, 3 ); }, PhysicsBodyType.Dynamic );
		Case( "static motion", PhysicsBodyType.Static, b => b.MotionEnabled = true );
		Case( "dynamic nomotion", PhysicsBodyType.Dynamic, b => b.MotionEnabled = false );
		var world = new PhysicsWorld();
		var empty = world.CreateBody();
		empty.BodyType = PhysicsBodyType.Static;
		Log4( "static empty", empty );
		empty.BodyType = PhysicsBodyType.Dynamic;
		Log4( "static empty >Dynamic", empty );
		world.Delete();
	}

	/// Scene bodies: a collider alone (its keyframe body) and a rigidbody with motion off.
	public static void Scene( GameObject collider, Rigidbody frozen )
	{
		var key = collider.Components.Get<Collider>().KeyframeBody;
		if ( key.IsValid() ) Log4( "scene collider", key );
		Log4( "scene world", collider.Scene.PhysicsWorld.Body );
		Mark( $"MASS scene frozen rb={F4( frozen.Mass )} center={V4( frozen.MassCenter )} inertia={V4( frozen.PhysicsBody.Inertia )} type={frozen.PhysicsBody.BodyType}" );
		frozen.MotionEnabled = true;
		Mark( $"MASS scene unfrozen rb={F4( frozen.Mass )} center={V4( frozen.MassCenter )} type={frozen.PhysicsBody.BodyType}" );
	}

	public static (GameObject, Rigidbody) Spawn()
	{
		var wall = new GameObject( true, "MassWall" );
		wall.WorldPosition = new Vector3( 1200, 1200, 50 );
		wall.Components.Create<BoxCollider>().Scale = new Vector3( 10, 100, 25 );
		var crate = new GameObject( true, "MassFrozen" );
		crate.WorldPosition = new Vector3( 1400, 1200, 50 );
		crate.Components.Create<BoxCollider>().Scale = new Vector3( 10, 100, 25 );
		var rb = crate.Components.Create<Rigidbody>();
		rb.MassOverride = 50;
		rb.MotionEnabled = false;
		return (wall, rb);
	}
}
