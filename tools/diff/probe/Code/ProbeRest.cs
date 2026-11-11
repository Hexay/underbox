using Sandbox;
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

/// <summary>
/// Sleep thresholds and resting depth, measured in private worlds stepped like a scene (0.02 s, 2
/// sub-steps), plus start-solid sweeps and rays grazing the slab.
/// </summary>
public static class ProbeRest
{
	const float Dt = 0.02f;
	const int SubSteps = 2;
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	record Case( string Name, PhysicsBody Body );

	/// Free bodies with gravity off: the step each falls asleep at, for a range of speeds and sizes.
	public static void Sleep()
	{
		var world = new PhysicsWorld { Gravity = Vector3.Zero };
		var cases = new List<Case>();
		var x = 0f;
		PhysicsBody Make( float radius, bool box )
		{
			var b = world.CreateBody();
			b.BodyType = PhysicsBodyType.Dynamic;
			b.Position = new Vector3( x += 1000, 0, 0 );
			if ( box ) b.AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, radius * 2 ), Rotation.Identity );
			else b.AddSphereShape( Vector3.Zero, radius );
			return b;
		}
		foreach ( var v in new[] { 0f, 0.5f, 1f, 1.5f, 1.9f, 2.1f, 3f, 5f, 10f, 20f } )
		{
			var b = Make( 8, false );
			b.Velocity = new Vector3( v, 0, 0 );
			cases.Add( new( $"lin r=8 v={v}", b ) );
		}
		foreach ( var r in new[] { 4f, 8f, 32f } )
			foreach ( var w in new[] { 0.02f, 0.05f, 0.1f, 0.2f, 0.3f, 0.5f, 1f, 2f } )
			{
				var b = Make( r, false );
				b.AngularVelocity = new Vector3( 0, 0, w );
				cases.Add( new( $"ang r={r} w={w}", b ) );
			}
		foreach ( var w in new[] { 0.05f, 0.1f, 0.2f, 0.5f } )
		{
			var b = Make( 16, true );
			b.AngularVelocity = new Vector3( 0, 0, w );
			cases.Add( new( $"angbox e=16 w={w}", b ) );
		}
		foreach ( var (r, ws) in new[] { (8f, new[] { 0.12f, 0.14f, 0.16f, 0.18f }), (4f, new[] { 0.32f, 0.36f, 0.4f, 0.45f }), (32f, new[] { 0.03f, 0.04f }) } )
			foreach ( var w in ws )
			{
				var b = Make( r, false );
				b.AngularVelocity = new Vector3( 0, 0, w );
				cases.Add( new( $"fine r={r} w={w}", b ) );
			}
		foreach ( var w in new[] { 0.055f, 0.065f, 0.075f, 0.085f, 0.095f } )
		{
			var b = Make( 16, true );
			b.AngularVelocity = new Vector3( 0, 0, w );
			cases.Add( new( $"finebox e=16 w={w}", b ) );
		}
		foreach ( var (axis, w) in new[] { ("long", 0.1f), ("long", 0.2f), ("long", 0.4f), ("short", 0.05f), ("short", 0.07f), ("short", 0.1f) } )
		{
			var b = world.CreateBody();
			b.BodyType = PhysicsBodyType.Dynamic;
			b.Position = new Vector3( x += 1000, 0, 0 );
			b.AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, new Vector3( 8, 8, 64 ) ), Rotation.Identity );
			b.AngularVelocity = axis == "long" ? new Vector3( 0, 0, w ) : new Vector3( w, 0, 0 );
			cases.Add( new( $"bar 8x8x64 {axis} w={w}", b ) );
		}
		foreach ( var (v, w) in new[] { (1f, 0.1f), (1.5f, 0.1f), (1f, 0.15f) } )
		{
			var b = Make( 8, false );
			b.Velocity = new Vector3( v, 0, 0 );
			b.AngularVelocity = new Vector3( 0, 0, w );
			cases.Add( new( $"both r=8 v={v} w={w}", b ) );
		}
		var heavy = Make( 8, false );
		heavy.Mass = 1000;
		heavy.Velocity = new Vector3( 1, 0, 0 );
		cases.Add( new( "lin heavy v=1", heavy ) );
		var custom = Make( 8, false );
		custom.SleepThreshold = 10;
		custom.Velocity = new Vector3( 5, 0, 0 );
		cases.Add( new( "lin thr=10 v=5", custom ) );
		Mark( $"SLEEPDEF threshold={F4( cases[0].Body.SleepThreshold )} world={world.SleepingEnabled}" );

		var asleep = new int[cases.Count];
		for ( int step = 1; step <= 1000; step++ )
		{
			world.Step( Dt, SubSteps );
			for ( int i = 0; i < cases.Count; i++ )
				if ( asleep[i] == 0 && cases[i].Body.Sleeping ) asleep[i] = step;
			if ( step == 50 ) Mark( $"ANGUNIT turned={F4( 2 * MathF.Acos( MathF.Min( 1, MathF.Abs( cases.Find( c => c.Name == "ang r=8 w=1" ).Body.Rotation.w ) ) ) * 180 / MathF.PI )}" );
		}
		foreach ( var (c, i) in cases.Select( ( c, i ) => (c, i) ) )
			Mark( $"SLEEP {c.Name} at={asleep[i]} v={F4( c.Body.Velocity.Length )} w={F4( c.Body.AngularVelocity.Length )}" );
		world.Delete();
	}

	/// Bodies set down on a slab (top z = 0) with gravity on: how far each sinks.
	public static void Depth()
	{
		var world = new PhysicsWorld { Gravity = new Vector3( 0, 0, -850 ) };
		var slab = world.CreateBody();
		slab.BodyType = PhysicsBodyType.Static;
		slab.AddBoxShape( new BBox( new Vector3( -5000, -500, -16 ), new Vector3( 5000, 500, 0 ) ), Rotation.Identity );
		var cases = new List<(string Name, PhysicsBody Body, float Height)>();
		var x = -4000f;
		PhysicsBody Put( string name, float half, bool box, float rest, float lift = 0 )
		{
			var b = world.CreateBody();
			b.BodyType = PhysicsBodyType.Dynamic;
			b.Position = new Vector3( x, 0, rest + lift );
			if ( box ) b.AddBoxShape( BBox.FromPositionAndSize( Vector3.Zero, half * 2 ), Rotation.Identity );
			else b.AddSphereShape( Vector3.Zero, half );
			cases.Add( (name, b, rest) );
			return b;
		}
		foreach ( var e in new[] { 2f, 16f, 64f } ) { Put( $"box e={e}", e, true, e ); x += 400; }
		Put( "box e=16 mass=1000", 16, true, 16 ).Mass = 1000; x += 400;
		Put( "sphere r=16", 16, false, 16 ); x += 400;
		for ( int i = 0; i < 3; i++ ) Put( $"stack{i} e=16", 16, true, 16 + 32 * i );
		x += 400;
		Put( "box e=16 drop", 16, true, 16, 50 );
		for ( int step = 1; step <= 150; step++ )
		{
			world.Step( Dt, SubSteps );
			if ( step != 25 && step != 150 ) continue;
			foreach ( var c in cases )
				Mark( $"DEPTH t={step} {c.Name} sink={F4( c.Height - c.Body.Position.z )} vz={F4( c.Body.Velocity.z )} sleep={c.Body.Sleeping}" );
		}
		world.Delete();
	}

	/// Sweeps that start overlapping the slab (top z = 0), and rays that graze its faces.
	public static void Solid( Scene scene )
	{
		var c = new Vector3( 0, 1200, 0 );
		void S( string name, SceneTrace t ) => Hit( $"SOLID {name}", t.Run() );
		foreach ( var z in new[] { 9f, 5f, 0f, -5f } )
			S( $"sphere r=10 z={z} +x", scene.Trace.Sphere( 10, c + new Vector3( 0, 0, z ), c + new Vector3( 100, 0, z ) ) );
		var at = c + new Vector3( 0, 0, 5 );
		foreach ( var (n, d) in new[] { ("-x", new Vector3( -100, 0, 0 )), ("+y", new Vector3( 0, 100, 0 )), ("+xy", new Vector3( 100, 100, 0 )), ("down", new Vector3( 0, 0, -100 )), ("up", new Vector3( 0, 0, 100 )), ("+x+z", new Vector3( 100, 0, 100 )), ("still", Vector3.Zero) } )
			S( $"sphere r=10 z=5 {n}", scene.Trace.Sphere( 10, at, at + d ) );
		S( "box e=20 z=5 +x", scene.Trace.Box( new Vector3( 20, 20, 20 ), at, at + new Vector3( 100, 0, 0 ) ) );
		S( "box e=20 z=5 down", scene.Trace.Box( new Vector3( 20, 20, 20 ), at, at + new Vector3( 0, 0, -100 ) ) );
		S( "ray z=-5 +x", scene.Trace.Ray( c + new Vector3( 0, 0, -5 ), c + new Vector3( 100, 0, -5 ) ) );
		S( "ray z=-5 up", scene.Trace.Ray( c + new Vector3( 0, 0, -5 ), c + new Vector3( 0, 0, 100 ) ) );

		foreach ( var z in new[] { 0.01f, 0f, -0.01f } )
			Hit( $"GRAZE top z={z}", scene.Trace.Ray( c + new Vector3( -300, 0, z ), c + new Vector3( 300, 0, z ) ).Run() );
		foreach ( var dx in new[] { 0.01f, 0f, -0.01f } )
			Hit( $"GRAZE side x=2000+{dx}", scene.Trace.Ray( new Vector3( 2000 + dx, 1200, 100 ), new Vector3( 2000 + dx, 1200, -100 ) ).Run() );
		Hit( "GRAZE corner", scene.Trace.Ray( new Vector3( 2100, 1200, 100 ), new Vector3( 1900, 1200, -100 ) ).Run() );
	}

	static void Hit( string name, SceneTraceResult r ) =>
		Mark( $"{name} hit={r.Hit} solid={r.StartedSolid} frac={r.Fraction.ToString( "F6", CultureInfo.InvariantCulture )} end={V4( r.EndPosition )} normal={V4( r.Normal )} obj={r.GameObject?.Name ?? "-"}" );
}
