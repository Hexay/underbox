using Sandbox;
using System;
using System.Globalization;

/// <summary>
/// Contact resolution between dynamic bodies in private worlds stepped like a scene (0.02 s, 2
/// sub-steps), gravity off: unjointed overlaps pushed apart (`PUSHOUT`), and a pair re-aimed into
/// each other every step as orbmatch's bots are (`REAIM`).
/// </summary>
public static class ProbePush
{
	const float Dt = 0.02f;
	const int SubSteps = 2;
	static readonly int[] Steps = { 1, 2, 3, 4, 5, 10, 20, 50 };
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	static PhysicsBody Body( PhysicsWorld world, Vector3 at, Action<PhysicsBody> shape )
	{
		var b = world.CreateBody();
		b.BodyType = PhysicsBodyType.Dynamic;
		b.Position = at;
		shape( b );
		return b;
	}

	static void Box( PhysicsBody b ) => b.AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, 16 ), Rotation.Identity );
	static void Ball( PhysicsBody b ) => b.AddSphereShape( Vector3.Zero, 16 );

	public static void Run()
	{
		foreach ( var depth in new[] { 0.5f, 1f, 2f, 4f, 8f, 12f } )
			PushOut( $"box d={depth}", 16 - depth, Box, 0 );
		foreach ( var depth in new[] { 1f, 4f, 12f } )
			PushOut( $"ball d={depth}", 32 - depth, Ball, 0 );
		PushOut( "box d=4 heavy", 12, Box, 100 );
		foreach ( var y in new[] { 0f, 8f } )
			ReAim( $"y={y}", y, 300, -1 );
		ReAim( "y=8 nofriction", 8, 300, 0 );
		foreach ( var rear in new[] { 240f, 242f, 245f, 248f, 250f } )
			ReAim( $"y=0 rear={rear}", 0, rear, -1 );
	}

	/// Two bodies along x, `gap` apart between centres, at rest; `mass` > 0 sets the second's mass.
	static void PushOut( string name, float gap, Action<PhysicsBody> shape, float mass )
	{
		var world = new PhysicsWorld { Gravity = Vector3.Zero };
		var a = Body( world, Vector3.Zero, shape );
		var b = Body( world, new Vector3( gap, 0, 0 ), shape );
		if ( mass > 0 ) b.Mass = mass;
		Mark( $"PUSHOUT {name} mass={F4( a.Mass )},{F4( b.Mass )}" );
		for ( int step = 1; step <= Steps[^1]; step++ )
		{
			world.Step( Dt, SubSteps );
			if ( Array.IndexOf( Steps, step ) >= 0 )
				Mark( $"PUSHOUT {name} t={step} gap={F4( b.Position.x - a.Position.x )} va={V4( a.Velocity )} vb={V4( b.Velocity )} sleep={a.Sleeping}" );
		}
		world.Delete();
	}

	/// orbmatch's push: radius-16 spheres at (-400, 0) and (-440, y), velocities set toward the
	/// origin every step (200 and `rearSpeed` u/s); `friction` >= 0 overrides the shapes'.
	static void ReAim( string name, float y, float rearSpeed, float friction )
	{
		var world = new PhysicsWorld { Gravity = Vector3.Zero };
		var front = Body( world, new Vector3( -400, 0, 32 ), Ball );
		var rear = Body( world, new Vector3( -440, y, 32 ), Ball );
		if ( friction >= 0 )
			foreach ( var b in new[] { front, rear } )
				foreach ( var s in b.Shapes ) s.Friction = friction;
		for ( int step = 1; step <= 30; step++ )
		{
			foreach ( var (body, speed) in new[] { (front, 200f), (rear, rearSpeed) } )
				body.Velocity = (-body.Position).WithZ( 0 ).Normal * speed;
			world.Step( Dt, SubSteps );
			Mark( $"REAIM {name} t={step} f={V4( front.Position )} r={V4( rear.Position )} vf={V4( front.Velocity )} vr={V4( rear.Velocity )} wf={V4( front.AngularVelocity )}" );
		}
		world.Delete();
	}
}
