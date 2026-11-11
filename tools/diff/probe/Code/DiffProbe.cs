using Sandbox;
using System.Collections.Generic;
using System.Globalization;

/// <summary>
/// Differential probe (tools/diff/run.sh probe): a few rigid bodies and traces whose state is
/// logged as `DIFF:` markers at fixed ticks, for comparing engines tick by tick.
/// </summary>
public sealed class DiffProbe : Component
{
	static readonly int[] Ticks = { 1, 2, 5, 10, 15, 20, 25, 30, 40, 50, 75, 100, 150, 250 };
	static readonly Angles[] Tilts = { new( 0, 0, 0 ), new( 45, 0, 0 ), new( 0, 0, 45 ), new( 30, 45, 15 ), new( 60, 10, 0 ), new( 10, 10, 10 ), new( 89, 0, 0 ), new( 35, 45, 0 ) };
	readonly List<Rigidbody> bodies = new();
	GameObject errorProp;
	int tick;

	static string F( float v ) => v.ToString( "F3", CultureInfo.InvariantCulture );
	static string V( Vector3 v ) => $"{F( v.x )},{F( v.y )},{F( v.z )}";
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	protected override void OnStart()
	{
		if ( IsProxy ) return;
		var ground = new GameObject( true, "Ground" );
		ground.WorldPosition = new Vector3( 0, 0, -8 );
		ground.Components.Create<BoxCollider>().Scale = new Vector3( 4000, 4000, 16 );

		bodies.Add( Body( "DropSphere", new Vector3( 0, 0, 200 ), Rotation.Identity, sphere: true, gravity: true ) );
		bodies.Add( Body( "DropBox", new Vector3( 200, 0, 200 ), Rotation.From( 20, 30, 10 ), sphere: false, gravity: true ) );
		bodies.Add( Body( "HeadOnA", new Vector3( -200, 400, 64 ), Rotation.Identity, sphere: true, gravity: false, v: new Vector3( 200, 0, 0 ) ) );
		bodies.Add( Body( "HeadOnB", new Vector3( 200, 400, 64 ), Rotation.Identity, sphere: true, gravity: false, v: new Vector3( -200, 0, 0 ) ) );
		bodies.Add( Body( "Slide", new Vector3( 0, -400, 16 ), Rotation.Identity, sphere: false, gravity: true, v: new Vector3( 300, 0, 0 ) ) );
		for ( int i = 0; i < Tilts.Length; i++ )
			bodies.Add( Body( $"Tilt{i}", new Vector3( -600 + 150 * i, -800, 200 ), Rotation.From( Tilts[i] ), sphere: false, gravity: true ) );
		errorProp = ProbeExtras.ErrorProp( new Vector3( 0, -1400, 300 ) );
		bodies.Add( ProbeExtras.ErrorChair( new Vector3( 600, -1400, 0 ) ) );
		Mark( $"START bodies={bodies.Count}" );
	}

	Rigidbody Body( string name, Vector3 pos, Rotation rot, bool sphere, bool gravity, Vector3 v = default )
	{
		var go = new GameObject( true, name );
		go.WorldPosition = pos;
		go.WorldRotation = rot;
		if ( sphere ) go.Components.Create<SphereCollider>().Radius = 16f;
		else go.Components.Create<BoxCollider>().Scale = new Vector3( 32, 32, 32 );
		var body = go.Components.Create<Rigidbody>();
		body.Gravity = gravity;
		body.Velocity = v;
		return body;
	}

	protected override void OnFixedUpdate()
	{
		if ( IsProxy || bodies.Count == 0 ) return;
		tick++;
		if ( System.Array.IndexOf( Ticks, tick ) >= 0 )
		{
			foreach ( var b in bodies )
				Mark( $"POS t={tick} {b.GameObject.Name} pos={V( b.WorldPosition )} vel={V( b.Velocity )} ang={V( b.WorldRotation.Angles().AsVector3() )} sleep={b.PhysicsBody.Sleeping}" );
		}
		if ( tick == 5 )
		{
			Traces();
			ProbeExtras.Skin( Scene );
			ProbeExtras.ErrorShapes( errorProp, Scene );
			ProbeExtras.Substeps();
			ProbeRest.Solid( Scene );
			ProbeRest.Depth();
			ProbeRest.Sleep();
			ProbeJoint.Run();
			ProbePush.Run();
		}
		if ( tick == Ticks[^1] ) Mark( "END" );
	}

	void Traces()
	{
		Trace( "ray-ground", Scene.Trace.Ray( new Vector3( 0, 800, 300 ), new Vector3( 0, 800, -300 ) ).Run() );
		Trace( "ray-miss", Scene.Trace.Ray( new Vector3( 0, 800, 300 ), new Vector3( 0, 800, 100 ) ).Run() );
		Trace( "ray-slant", Scene.Trace.Ray( new Vector3( -300, 800, 300 ), new Vector3( 300, 800, -300 ) ).Run() );
		Trace( "sphere-ground", Scene.Trace.Sphere( 10f, new Vector3( 0, 800, 300 ), new Vector3( 0, 800, -300 ) ).Run() );
		Trace( "box-ground", Scene.Trace.Box( new Vector3( 20, 20, 20 ), new Vector3( 0, 800, 300 ), new Vector3( 0, 800, -300 ) ).Run() );
		Trace( "ray-headon", Scene.Trace.Ray( new Vector3( -600, 400, 64 ), new Vector3( 0, 400, 64 ) ).Run() );
		Trace( "ray-inside", Scene.Trace.Ray( new Vector3( 0, 800, -4 ), new Vector3( 0, 800, -300 ) ).Run() );
	}

	static void Trace( string name, SceneTraceResult r ) =>
		Mark( $"TRACE {name} hit={r.Hit} solid={r.StartedSolid} frac={F( r.Fraction )} end={V( r.EndPosition )} normal={V( r.Normal )} obj={r.GameObject?.Name ?? "-"}" );
}
