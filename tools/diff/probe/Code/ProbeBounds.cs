using Sandbox;
using System;
using System.Globalization;
using System.Linq;

/// <summary>
/// Bounds, friction, sleep and gravity as managed reads them back (`BOUNDS`, `FRICTION`, `STATIC`,
/// `GRAVITY`): renderer and scene-object bounds, body and collider bounds, shape friction with and
/// without a collider value (docs/differential-tap.md, "Fixed").
/// </summary>
public static class ProbeBounds
{
	static string F4( float v ) => v.ToString( "F4", CultureInfo.InvariantCulture );
	static string V4( Vector3 v ) => $"{F4( v.x )},{F4( v.y )},{F4( v.z )}";
	static string B4( BBox b ) => $"{V4( b.Mins )}..{V4( b.Maxs )}";
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	public record Objects( ModelRenderer Box, SkinnedModelRenderer Citizen, Collider Wall, Rigidbody Ball, Rigidbody Error );

	public static Objects Spawn()
	{
		var box = new GameObject( true, "BoundsBox" );
		box.WorldPosition = new Vector3( -1200, 1200, 50 );
		box.WorldRotation = Rotation.From( 10, 30, 0 );
		box.WorldScale = new Vector3( 2, 1, 1 );
		var renderer = box.Components.Create<ModelRenderer>();
		renderer.Model = Model.Load( "models/dev/box.vmdl" );

		var citizen = new GameObject( true, "BoundsCitizen" );
		citizen.WorldPosition = new Vector3( -1400, 1200, 0 );
		citizen.WorldRotation = Rotation.FromYaw( 45 );
		var skinned = citizen.Components.Create<SkinnedModelRenderer>();
		skinned.Model = Model.Load( "models/citizen/citizen.vmdl" );

		var wall = new GameObject( true, "BoundsWall" );
		wall.WorldPosition = new Vector3( -1600, 1200, 50 );
		var collider = wall.Components.Create<BoxCollider>();
		collider.Scale = new Vector3( 10, 100, 25 );

		var ball = new GameObject( true, "BoundsBall" );
		ball.WorldPosition = new Vector3( -1800, 1200, 100 );
		var sphere = ball.Components.Create<SphereCollider>();
		sphere.Radius = 8;
		sphere.Center = new Vector3( 4, 0, 10 );
		sphere.Friction = 0.3f;
		var rb = ball.Components.Create<Rigidbody>();
		rb.Gravity = false;

		var err = new GameObject( true, "BoundsError" );
		err.WorldPosition = new Vector3( -2000, 1200, 100 );
		err.Components.Create<ModelCollider>().Model = Model.Load( "models/does_not_exist.vmdl" );
		var errBody = err.Components.Create<Rigidbody>();
		errBody.Gravity = false;
		return new Objects( renderer, skinned, collider, rb, errBody );
	}

	static string R4( Rotation r ) => $"{F4( r.x )},{F4( r.y )},{F4( r.z )},{F4( r.w )}";

	/// The error model's principal inertia, and a sphere's triangulation: its vertex count and rings.
	static void Shapes( Rigidbody error )
	{
		var b = error.PhysicsBody;
		Mark( $"INERTIA error inertia={V4( b.Inertia )} irot={R4( b.InertiaRotation )} mass={F4( b.Mass )}" );
		var world = new PhysicsWorld();
		var body = world.CreateBody();
		body.AddSphereShape( Vector3.Zero, 8 );
		body.Shapes.First().Triangulate( out var p, out var idx );
		var rings = p.GroupBy( v => MathF.Round( v.z, 3 ) ).OrderBy( g => g.Key ).Select( g => g.Count() );
		Mark( $"TRI sphere verts={p.Length} tris={idx.Length / 3} rings={string.Join( "/", rings )} first={string.Join( " ", p.Take( 4 ).Select( V4 ) )}" );
		world.Delete();
	}

	static string Body( PhysicsBody b ) =>
		b.IsValid() ? $"type={b.BodyType} sleep={b.Sleeping} bounds={B4( b.GetBounds() )}" : "none";

	public static void Report( int tick, Objects o )
	{
		Mark( $"BOUNDS t={tick} box so={B4( o.Box.SceneObject.Bounds )} renderer={B4( o.Box.Bounds )}" );
		Mark( $"BOUNDS t={tick} citizen so={B4( o.Citizen.SceneModel.Bounds )} renderer={B4( o.Citizen.Bounds )} local={B4( o.Citizen.LocalBounds )}" );
		Mark( $"BOUNDS t={tick} wall {Body( o.Wall.KeyframeBody )} collider={B4( o.Wall.GetWorldBounds() )}" );
		Mark( $"BOUNDS t={tick} ball {Body( o.Ball.PhysicsBody )} collider={B4( o.Ball.Components.Get<Collider>().GetWorldBounds() )}" );
		Mark( $"STATIC t={tick} world {Body( o.Wall.Scene.PhysicsWorld.Body )}" );
		if ( tick != 1 ) return;
		Shapes( o.Error );
		var model = o.Citizen.Model;
		Mark( $"BOUNDS citizen model render={B4( model.RenderBounds )} phys={B4( model.PhysicsBounds )} bones={model.BoneCount}" );
		var so = o.Citizen.SceneModel;
		var bones = Enumerable.Range( 0, model.BoneCount );
		Mark( $"BOUNDS citizen bones world={B4( BBox.FromPoints( bones.Select( i => so.GetBoneWorldTransform( i ).Position ) ) )} local={B4( BBox.FromPoints( bones.Select( i => so.GetBoneLocalTransform( i ).Position ) ) )}" );
		Mark( $"FRICTION wall={F4( o.Wall.KeyframeBody.Shapes.First().Friction )} ball={F4( o.Ball.PhysicsBody.Shapes.First().Friction )}" );
		var world = new PhysicsWorld();
		var empty = world.CreateBody();
		Mark( $"GRAVITY new={V4( world.Gravity )}" );
		Mark( $"BOUNDS empty {Body( empty )}" );
		empty.BodyType = PhysicsBodyType.Static;
		var shaped = world.CreateBody();
		shaped.BodyType = PhysicsBodyType.Static;
		shaped.AddSphereShape( new Vector3( 1, 2, 3 ), 8 );
		Mark( $"STATIC t=0 private empty {Body( empty )} shaped {Body( shaped )}" );
		world.Step( 0.02f, 2 );
		Mark( $"STATIC t=1 private empty {Body( empty )} shaped {Body( shaped )}" );
		world.Delete();
	}
}
