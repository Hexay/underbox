using Sandbox;
using System.Globalization;
using System.Linq;

/// <summary>
/// Measurements beyond the scene's bodies: integration substeps in a private world, sweep skin,
/// and the error model's collision.
/// </summary>
public static class ProbeExtras
{
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	/// A free fall in a private world, stepped 10 × 0.02 s with each managed sub-step count:
	/// the drop fits g·dt²·nk(nk+1)/(2k²) for the native integration count k.
	public static void Substeps()
	{
		foreach ( var s in new[] { 1, 2, 3, 4, 6 } )
		{
			var world = new PhysicsWorld { Gravity = new Vector3( 0, 0, -850 ) };
			var body = world.CreateBody();
			body.BodyType = PhysicsBodyType.Dynamic;
			body.AddSphereShape( Vector3.Zero, 8 );
			body.GravityEnabled = true;
			for ( int i = 0; i < 10; i++ ) world.Step( 0.02f, s );
			Mark( $"SUBSTEP s={s} z={F4( body.Position.z )} vz={F4( body.Velocity.z )}" );
			world.Delete();
		}
	}

	/// Sweeps onto the ground slab (top at z = 0) of several sizes and starting heights.
	public static void Skin( Scene scene )
	{
		foreach ( var r in new[] { 1f, 10f, 50f } )
			foreach ( var h in new[] { 300f, 20f } )
				Sweep( $"sphere r={r} h={h}", scene.Trace.Sphere( r, new Vector3( 0, 900, h + r ), new Vector3( 0, 900, -300 ) ).Run(), r );
		foreach ( var e in new[] { 2f, 20f, 100f } )
			Sweep( $"box e={e}", scene.Trace.Box( new Vector3( e, e, e ), new Vector3( 0, 900, 300 ), new Vector3( 0, 900, -300 ) ).Run(), e / 2 );
		Sweep( "sphere slant", scene.Trace.Sphere( 10f, new Vector3( -300, 900, 310 ), new Vector3( 300, 900, -290 ) ).Run(), 10 );
		Sweep( "sphere sideways", scene.Trace.Sphere( 10f, new Vector3( -300, 900, 5 ), new Vector3( 300, 900, 5 ) ).Run(), 10 );
	}

	static void Sweep( string name, SceneTraceResult r, float half ) =>
		Mark( $"SKIN {name} hit={r.Hit} solid={r.StartedSolid} frac={r.Fraction.ToString( "F6", CultureInfo.InvariantCulture )} gap={F4( r.EndPosition.z - half )} end={V4( r.EndPosition )} normal={V4( r.Normal )}" );

	/// A static prop whose model doesn't exist, so it gets the error model.
	public static GameObject ErrorProp( Vector3 at )
	{
		var go = new GameObject( true, "ErrorProp" );
		go.WorldPosition = at;
		var collider = go.Components.Create<ModelCollider>();
		collider.Model = Model.Load( "models/does_not_exist.vmdl" );
		var m = collider.Model;
		Mark( $"ERRMODEL error={m.IsError} bounds={V4( m.Bounds.Mins )}..{V4( m.Bounds.Maxs )} phys={V4( m.PhysicsBounds.Mins )}..{V4( m.PhysicsBounds.Maxs )} parts={m.Physics?.Parts.Count ?? -1}" );
		return go;
	}

	/// Sandbox's car seat with a missing model: the error model's hull plus a sphere high up on a
	/// child, resting on the slab. Real puts it to sleep.
	public static Rigidbody ErrorChair( Vector3 at )
	{
		var go = new GameObject( true, "ErrorChair" );
		go.WorldPosition = at;
		go.Components.Create<ModelCollider>().Model = Model.Load( "models/does_not_exist.vmdl" );
		var body = go.Components.Create<Rigidbody>();
		var head = new GameObject( go, true, "head" );
		head.LocalPosition = new Vector3( 1.74f, 0, 52.7f );
		head.Components.Create<SphereCollider>().Radius = 8;
		return body;
	}

	/// The error prop's shapes, then rays at it from each side.
	public static void ErrorShapes( GameObject go, Scene scene )
	{
		var body = go.Components.Get<ModelCollider>()?.KeyframeBody;
		var shapes = body?.Shapes.ToList() ?? new();
		foreach ( var s in shapes )
		{
			var verts = 0;
			if ( s.IsHullShape || s.IsMeshShape ) { s.Triangulate( out var p, out _ ); verts = p.Length; }
			Mark( $"ERRSHAPE hull={s.IsHullShape} mesh={s.IsMeshShape} sphere={s.IsSphereShape} capsule={s.IsCapsuleShape} verts={verts}" );
		}
		Mark( $"ERRBODY shapes={shapes.Count}" );
		var c = go.WorldPosition;
		for ( var y = -55f; y <= 55f; y += 5f )
		{
			Ray( "down", c, new Vector3( 0, y, 100 ), new Vector3( 0, y, -100 ) );
			Ray( "up", c, new Vector3( 0, y, -100 ), new Vector3( 0, y, 100 ) );
		}
		for ( var z = 1f; z <= 25f; z += 3f )
		{
			Ray( "left", c, new Vector3( 0, 100, z ), new Vector3( 0, -100, z ) );
			Ray( "right", c, new Vector3( 0, -100, z ), new Vector3( 0, 100, z ) );
			foreach ( var y in new[] { -45f, 0f, 45f } ) Ray( "fwd", c, new Vector3( -100, y, z ), new Vector3( 100, y, z ) );
		}
	}

	static void Ray( string name, Vector3 c, Vector3 from, Vector3 to )
	{
		var tr = Game.ActiveScene.Trace.Ray( c + from, c + to ).Run();
		if ( tr.GameObject?.Name != "ErrorProp" ) return;
		Mark( $"ERRRAY {name} from={V4( from )} at={V4( tr.EndPosition - c )} normal={V4( tr.Normal )}" );
	}
}
