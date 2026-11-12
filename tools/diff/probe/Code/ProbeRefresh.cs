using Sandbox;
using System;
using System.Globalization;

/// <summary>
/// When a persistent contact is refreshed (`REFRESH`): ball B (radius r) is pushed into ball A
/// every step at `vn` along +x and slid along +y at `vt`, optionally spun; A is stopped before each
/// step, so A's velocity after the step points along the normal the solver used. Gravity off,
/// 0.02 s, 2 sub-steps (docs/differential-joints.md, 16).
/// </summary>
public static class ProbeRefresh
{
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";
	static float Deg( Vector3 v ) => MathF.Atan2( v.y, v.x ) * 180f / MathF.PI;

	public static void Run()
	{
		foreach ( var vt in new[] { 25f, 50f, 100f, 200f } )
			Slide( $"r=16 vt={vt}", 16, 100, vt, Vector3.Zero, 0 );
		foreach ( var r in new[] { 8f, 32f } )
			Slide( $"r={r} vt=50", r, 100, 50, Vector3.Zero, 0 );
		foreach ( var vn in new[] { 50f, 200f } )
			Slide( $"r=16 vt=50 vn={vn}", 16, vn, 50, Vector3.Zero, 0 );
		foreach ( var (axis, w) in new[] { ("z", 2f), ("z", 5f), ("z", 10f), ("x", 10f), ("y", 5f) } )
		{
			var spin = axis == "x" ? new Vector3( w, 0, 0 ) : axis == "y" ? new Vector3( 0, w, 0 ) : new Vector3( 0, 0, w );
			Slide( $"r=16 vt=25 spin{axis}={w}", 16, 100, 25, spin, 0 );
			Slide( $"r=8 vt=25 spin{axis}={w}", 8, 100, 25, spin, 0 );
		}
		Slide( "r=16 vt=25 friction", 16, 100, 25, Vector3.Zero, -1 );
	}

	/// `friction` >= 0 overrides the shapes'; with friction A's spin is left alone.
	static void Slide( string name, float r, float vn, float vt, Vector3 spin, float friction )
	{
		var world = new PhysicsWorld { Gravity = Vector3.Zero };
		PhysicsBody Ball( Vector3 at )
		{
			var b = world.CreateBody();
			b.BodyType = PhysicsBodyType.Dynamic;
			b.Position = at;
			var s = b.AddSphereShape( Vector3.Zero, r );
			if ( friction >= 0 ) s.Friction = friction;
			return b;
		}
		var a = Ball( Vector3.Zero );
		var b = Ball( new Vector3( -2 * r - 1, 0, 0 ) );
		for ( int step = 1; step <= 30; step++ )
		{
			a.Velocity = Vector3.Zero;
			if ( friction >= 0 ) a.AngularVelocity = Vector3.Zero;
			b.Velocity = new Vector3( vn, vt, 0 );
			if ( spin != Vector3.Zero ) b.AngularVelocity = spin;
			world.Step( 0.02f, 2 );
			var q = b.Rotation;
			Log.Info( $"DIFF:REFRESH {name} t={step} n={F4( Deg( a.Velocity ) )} geo={F4( Deg( a.Position - b.Position ) )} va={F4( a.Velocity.Length )} a={V4( a.Position )} b={V4( b.Position )} qa={V4( new Vector3( a.Rotation.x, a.Rotation.y, a.Rotation.z ) )},{F4( a.Rotation.w )} qb={V4( new Vector3( q.x, q.y, q.z ) )},{F4( q.w )}" );
		}
		world.Delete();
	}
}
