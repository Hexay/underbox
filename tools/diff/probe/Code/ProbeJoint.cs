using Sandbox;
using System;
using System.Globalization;
using HingeJoint = Sandbox.Physics.HingeJoint;
using PhysicsJoint = Sandbox.Physics.PhysicsJoint;
using PhysicsPoint = Sandbox.Physics.PhysicsPoint;

/// <summary>
/// Hinge motors, springs and joint collision in private worlds stepped like a scene (0.02 s, 2
/// sub-steps), plus when a resting stack falls asleep.
/// </summary>
public static class ProbeJoint
{
	const float Dt = 0.02f;
	const int SubSteps = 2;
	static readonly int[] Steps = { 1, 2, 3, 5, 10, 20, 50, 100 };
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	static PhysicsBody Dynamic( PhysicsWorld world, Vector3 at )
	{
		var b = world.CreateBody();
		b.BodyType = PhysicsBodyType.Dynamic;
		b.Position = at;
		return b;
	}

	/// A sphere hinged about Z to a static anchor, gravity off, driven by `setup`.
	static (PhysicsWorld, PhysicsBody, HingeJoint) Wheel( float spin, Action<HingeJoint> setup )
	{
		var world = new PhysicsWorld { Gravity = Vector3.Zero };
		var anchor = world.CreateBody();
		anchor.BodyType = PhysicsBodyType.Static;
		var wheel = Dynamic( world, Vector3.Zero );
		wheel.AddSphereShape( Vector3.Zero, 8 );
		var joint = PhysicsJoint.CreateHinge( new PhysicsPoint( anchor ), new PhysicsPoint( wheel ) );
		setup( joint );
		wheel.AngularVelocity = new Vector3( 0, 0, spin );
		return (world, wheel, joint);
	}

	public static void Run()
	{
		{
			var (world, wheel, _) = Wheel( 0, _ => { } );
			Mark( $"HINGEBODY mass={F4( wheel.Mass )} inertia={V4( wheel.Inertia )}" );
			world.Delete();
		}
		foreach ( var torque in new[] { 0f, 100f, 1000f, 10000f, 100000f } )
			Track( $"brake w0=10 T={torque}", Wheel( 10, j => j.SetAngularMotor( 0, torque ) ) );
		foreach ( var torque in new[] { 1000f, 100000f } )
			Track( $"drive v=5 T={torque}", Wheel( 0, j => j.SetAngularMotor( 5, torque ) ) );
		Track( "drive deg v=90 T=100000", Wheel( 0, j => j.SetAngularMotor( 90, 100000 ) ) );
		foreach ( var (hz, zeta) in new[] { (1f, 0f), (1f, 1f), (5f, 0.3f) } )
			Track( $"spring 1rad hz={hz} z={zeta}", Wheel( 0, j => j.SetAngularSpring( new Vector3( 1, hz, zeta ) ) ) );
		foreach ( var f in new[] { 1000f, 100000f, 10000000f } )
			Track( $"friction w0=10 f={f}", Wheel( 10, j => j.Friction = f ) );
		Track( "friction after motor w0=10 f=100000", Wheel( 10, j => { j.SetAngularMotor( 0, 1 ); j.Friction = 100000; } ) );
		foreach ( var collide in new[] { false, true } )
			Overlap( collide );
		Car();
		StackSleep();
	}

	static void Track( string name, (PhysicsWorld World, PhysicsBody Wheel, HingeJoint Joint) c )
	{
		for ( int step = 1; step <= Steps[^1]; step++ )
		{
			c.World.Step( Dt, SubSteps );
			if ( Array.IndexOf( Steps, step ) >= 0 )
				Mark( $"HINGE {name} t={step} w={F4( c.Wheel.AngularVelocity.z )} angle={F4( c.Joint.Angle )} sleep={c.Wheel.Sleeping}" );
		}
		c.World.Delete();
	}

	/// Two boxes overlapping by 4 units, hinged at their contact, gravity off.
	static void Overlap( bool collide )
	{
		var world = new PhysicsWorld { Gravity = Vector3.Zero };
		var a = Dynamic( world, Vector3.Zero );
		a.AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, 16 ), Rotation.Identity );
		var b = Dynamic( world, new Vector3( 12, 0, 0 ) );
		b.AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, 16 ), Rotation.Identity );
		var joint = PhysicsJoint.CreateHinge( new PhysicsPoint( a, new Vector3( 6, 0, 0 ) ), new PhysicsPoint( b, new Vector3( -6, 0, 0 ) ) );
		joint.Collisions = collide;
		joint.SetAngularMotor( 0, 9.2f );
		for ( int step = 1; step <= Steps[^1]; step++ )
		{
			world.Step( Dt, SubSteps );
			if ( Array.IndexOf( Steps, step ) >= 0 )
				Mark( $"OVERLAP collide={collide} t={step} a={V4( a.Position )} b={V4( b.Position )} va={V4( a.Velocity )} wa={V4( a.AngularVelocity )} sleep={a.Sleeping}" );
		}
		world.Delete();
	}

	/// A box chassis on a slab with three sphere wheels hinged about Y inside its corners, motors at
	/// zero speed, joint collision on: the zoo hoover's shape.
	static void Car()
	{
		var world = new PhysicsWorld { Gravity = new Vector3( 0, 0, -850 ) };
		var slab = world.CreateBody();
		slab.BodyType = PhysicsBodyType.Static;
		slab.AddBoxShape( new BBox( new Vector3( -500, -500, -16 ), new Vector3( 500, 500, 0 ) ), Rotation.Identity );
		var chassis = Dynamic( world, new Vector3( 0, 0, 8 ) );
		chassis.AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, new Vector3( 24, 16, 8 ) ), Rotation.Identity );
		chassis.Mass = 6;
		var axle = Rotation.FromRoll( 90 );
		var wheels = new[] { new Vector3( 8, 6, -4 ), new Vector3( 8, -6, -4 ), new Vector3( -8, 0, -4 ) };
		foreach ( var (at, i) in new[] { (wheels[0], 0), (wheels[1], 1), (wheels[2], 2) } )
		{
			var w = Dynamic( world, chassis.Position + at );
			w.AddSphereShape( Vector3.Zero, 4 );
			w.Mass = 1;
			var j = PhysicsJoint.CreateHinge( new PhysicsPoint( chassis, at, axle ), new PhysicsPoint( w, Vector3.Zero, axle ) );
			j.Collisions = true;
			j.SetAngularMotor( 0, i == 2 ? 506.5f : 9.23f );
		}
		for ( int step = 1; step <= 500; step++ )
		{
			world.Step( Dt, SubSteps );
			if ( Array.IndexOf( Steps, step ) >= 0 || step % 100 == 0 )
				Mark( $"CAR t={step} pos={V4( chassis.Position )} v={F4( chassis.Velocity.Length )} w={F4( chassis.AngularVelocity.Length )} sleep={chassis.Sleeping}" );
		}
		world.Delete();
	}

	/// ProbeRest.Depth's three-box stack and a lone box: the step each falls asleep at, and each
	/// body's largest speed before then.
	static void StackSleep()
	{
		var world = new PhysicsWorld { Gravity = new Vector3( 0, 0, -850 ) };
		var slab = world.CreateBody();
		slab.BodyType = PhysicsBodyType.Static;
		slab.AddBoxShape( new BBox( new Vector3( -5000, -500, -16 ), new Vector3( 5000, 500, 0 ) ), Rotation.Identity );
		var bodies = new PhysicsBody[4];
		for ( int i = 0; i < 3; i++ )
		{
			bodies[i] = Dynamic( world, new Vector3( 0, 0, 16 + 32 * i ) );
			bodies[i].AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, 32 ), Rotation.Identity );
		}
		bodies[3] = Dynamic( world, new Vector3( 400, 0, 16 ) );
		bodies[3].AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, 32 ), Rotation.Identity );
		var asleep = new int[4];
		var peak = new float[4];
		for ( int step = 1; step <= 200; step++ )
		{
			world.Step( Dt, SubSteps );
			for ( int i = 0; i < 4; i++ )
			{
				if ( asleep[i] == 0 && bodies[i].Sleeping ) asleep[i] = step;
				if ( asleep[i] == 0 ) peak[i] = MathF.Max( peak[i], bodies[i].Velocity.Length );
			}
			if ( step <= 30 && step % 5 == 0 || step == 1 || step == 2 )
				Mark( $"STACKV t={step} vz={F4( bodies[0].Velocity.z )},{F4( bodies[1].Velocity.z )},{F4( bodies[2].Velocity.z )} lone={F4( bodies[3].Velocity.z )}" );
		}
		for ( int i = 0; i < 4; i++ )
			Mark( $"STACKSLEEP body={i} at={asleep[i]} peak={F4( peak[i] )}" );
		world.Delete();
	}
}
