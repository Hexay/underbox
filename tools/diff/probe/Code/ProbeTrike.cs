using Sandbox;
using System;
using System.Globalization;
using HingeJoint = Sandbox.Physics.HingeJoint;
using PhysicsJoint = Sandbox.Physics.PhysicsJoint;
using PhysicsPoint = Sandbox.Physics.PhysicsPoint;

/// <summary>
/// The zoo hoover in a private world (`TRIKE`): a jointed trike (base, rear axle, two casters,
/// motorised hinges with joint collision, approximated by boxes, cylinders and spheres) spawned
/// through a 10 × 100 × 25 sign on a slab, gravity on, 0.02 s and 2 sub-steps
/// (docs/differential-joints.md, the zoo hoover).
/// </summary>
public static class ProbeTrike
{
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";

	public static void Run()
	{
		Trike( "dynamic", sign: PhysicsBodyType.Dynamic, joints: true, collide: true );
		Trike( "static", sign: PhysicsBodyType.Static, joints: true, collide: true );
		Trike( "nojoint", sign: PhysicsBodyType.Dynamic, joints: false, collide: true );
		Trike( "nocollide", sign: PhysicsBodyType.Dynamic, joints: true, collide: false );
		Trike( "nosign", sign: null, joints: true, collide: true );
	}

	static PhysicsBody Body( PhysicsWorld world, Vector3 at, float mass, float friction, Action<PhysicsBody> shapes )
	{
		var b = world.CreateBody();
		b.BodyType = PhysicsBodyType.Dynamic;
		b.Position = at;
		shapes( b );
		foreach ( var s in b.Shapes ) s.Friction = friction;
		if ( mass > 0 ) b.Mass = mass;
		return b;
	}

	static void Box( PhysicsBody b, Vector3 min, Vector3 max ) => b.AddBoxShape( new BBox( min, max ), Rotation.Identity );

	static void Hinge( PhysicsBody a, Vector3 at1, PhysicsBody b, Vector3 at2, Rotation frame, float torque, bool collide )
	{
		var j = PhysicsJoint.CreateHinge( new PhysicsPoint( a, at1, frame ), new PhysicsPoint( b, at2, frame ) );
		j.Collisions = collide;
		j.SetAngularMotor( 0, torque );
	}

	static void Trike( string name, PhysicsBodyType? sign, bool joints, bool collide )
	{
		var world = new PhysicsWorld { Gravity = new Vector3( 0, 0, -850 ) };
		var slab = world.CreateBody();
		slab.BodyType = PhysicsBodyType.Static;
		slab.AddBoxShape( new BBox( new Vector3( -1000, -1000, -16 ), new Vector3( 1000, 1000, 0 ) ), Rotation.Identity );
		PhysicsBody board = null;
		if ( sign is { } type )
		{
			board = world.CreateBody();
			board.BodyType = type;
			board.Position = Vector3.Zero;
			Box( board, new Vector3( -5, -50, 0 ), new Vector3( 5, 50, 25 ) );
		}
		var bas = Body( world, Vector3.Zero, 6, 0.8f, b =>
		{
			Box( b, new Vector3( -9.1f, -9.03f, 10.17f ), new Vector3( 9.27f, 9.05f, 19.17f ) );
			Box( b, new Vector3( -8.6f, -8.81f, 1.72f ), new Vector3( 8.7f, 8.84f, 11.02f ) );
			Box( b, new Vector3( -9.88f, -5.73f, 2.57f ), new Vector3( -4.81f, 5.73f, 6.02f ) );
			Box( b, new Vector3( 7.81f, -1.87f, 6.06f ), new Vector3( 10.55f, 1.87f, 9.65f ) );
		} );
		var axleY = Rotation.FromRoll( 90 );
		var rear = Body( world, new Vector3( -7.276f, 0, 2.945f ), 1, 0.01f, b =>
		{
			b.AddCylinderShape( new Vector3( 0, 7.83f, 0 ), axleY, 1.55f, 3.1f );
			b.AddCylinderShape( new Vector3( 0, -7.93f, 0 ), axleY, 1.55f, 3.1f );
		} );
		var right = Body( world, new Vector3( 5.581f, -4.286f, 3.385f ), 1, 0.01f, b => b.AddSphereShape( new Vector3( -0.033f, 0, -1.975f ), 1.4f ) );
		var left = Body( world, new Vector3( 5.531f, 4.289f, 3.385f ), 1, 0.01f, b => b.AddSphereShape( new Vector3( 0.021f, 0, -1.975f ), 1.4f ) );
		if ( joints )
		{
			var roll = new Rotation( 0.7071068f, 0, 0, 0.7071068f );
			Hinge( bas, new Vector3( -7.3226f, 0, 2.9703f ), rear, new Vector3( -0.0466f, 0, 0.0257f ), roll, 506.5f, collide );
			Hinge( bas, new Vector3( 5.5664f, -4.2821f, 1.6592f ), right, new Vector3( -0.0149f, 0.0037f, -1.7258f ), Rotation.Identity, 9.235f, collide );
			Hinge( bas, new Vector3( 5.5664f, 4.3116f, 1.6592f ), left, new Vector3( 0.0354f, 0.0225f, -1.7258f ), Rotation.Identity, 9.235f, collide );
		}
		Log.Info( $"DIFF:TRIKE {name} mass={F4( bas.Mass )},{F4( rear.Mass )},{F4( board?.Mass ?? 0 )}" );
		for ( int step = 1; step <= 1000; step++ )
		{
			world.Step( 0.02f, 2 );
			if ( step <= 10 || step <= 100 && step % 5 == 0 || step % 50 == 0 )
				Log.Info( $"DIFF:TRIKE {name} t={step} base={V4( bas.Position )} up={F4( bas.Rotation.Up.z )} v={V4( bas.Velocity )} rear={V4( rear.Position )} sign={V4( board?.Position ?? Vector3.Zero )} sup={F4( board?.Rotation.Up.z ?? 1 )} sleep={bas.Sleeping}" );
		}
		world.Delete();
	}
}
