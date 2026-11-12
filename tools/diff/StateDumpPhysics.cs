using Sandbox;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json.Nodes;

/// <summary>
/// The physics half of tools/diff/StateDump.cs: every body in the scene's PhysicsWorld, keyed by its
/// GameObject's path ("$world" for bodies without one) and `$body[i]` in world order, with its shapes.
/// </summary>
public static class StateDumpPhysics
{
	const int MaxShapes = 64;

	public static int DumpBodies( PhysicsWorld world, Action<string, string, JsonNode> emit )
	{
		if ( world is null ) return 0;
		var bodies = world.Bodies.ToList();
		if ( world.Body is not null && !bodies.Contains( world.Body ) ) bodies.Insert( 0, world.Body );
		var seen = new Dictionary<string, int>();
		foreach ( var body in bodies )
		{
			string path;
			try { path = StateDump.PathOf( body.GameObject ) ?? "$world"; }
			catch ( Exception ) { path = "$world"; }
			seen.TryGetValue( path, out var i );
			seen[path] = i + 1;
			emit( path, $"$body[{i}]", Body( body ) );
		}
		return bodies.Count;
	}

	static JsonObject Body( PhysicsBody b )
	{
		var o = new JsonObject();
		void Set( string key, Func<object> get ) => StateDumpValues.Set( o, key, get );
		Set( "component", () => b.Component is null ? null : b.Component.GetType().Name );
		Set( "BodyType", () => b.BodyType );
		Set( "Enabled", () => b.Enabled );
		Set( "MotionEnabled", () => b.MotionEnabled );
		Set( "Position", () => b.Position );
		Set( "Rotation", () => b.Rotation );
		Set( "Velocity", () => b.Velocity );
		Set( "AngularVelocity", () => b.AngularVelocity );
		Set( "Mass", () => b.Mass );
		Set( "MassCenter", () => b.MassCenter );
		Set( "Inertia", () => b.Inertia );
		Set( "Sleeping", () => b.Sleeping );
		Set( "SleepThreshold", () => b.SleepThreshold );
		Set( "GravityEnabled", () => b.GravityEnabled );
		Set( "GravityScale", () => b.GravityScale );
		Set( "LinearDamping", () => b.LinearDamping );
		Set( "AngularDamping", () => b.AngularDamping );
		Set( "EnableTouch", () => b.EnableTouch );
		Set( "EnableSolidCollisions", () => b.EnableSolidCollisions );
		Set( "Locking", () => b.Locking.ToString() );
		Set( "Surface", () => b.Surface );
		Set( "Bounds", () => b.GetBounds() );
		Set( "ShapeCount", () => b.ShapeCount );
		try
		{
			o["shapes"] = new JsonArray( b.Shapes.Take( MaxShapes ).Select( s => (JsonNode)Shape( s ) ).ToArray() );
		}
		catch ( Exception e ) { o["shapes"] = StateDumpValues.Error( e ); }
		return o;
	}

	static JsonObject Shape( PhysicsShape s )
	{
		var o = new JsonObject();
		void Set( string key, Func<object> get ) => StateDumpValues.Set( o, key, get );
		Set( "kind", () => s.IsSphereShape ? "sphere" : s.IsCapsuleShape ? "capsule" : s.IsHullShape ? "hull"
			: s.IsMeshShape ? "mesh" : s.IsHeightfieldShape ? "heightfield" : "other" );
		Set( "collider", () => s.Collider );
		Set( "IsTrigger", () => s.IsTrigger );
		Set( "EnableSolidCollisions", () => s.EnableSolidCollisions );
		Set( "EnableTouch", () => s.EnableTouch );
		Set( "Friction", () => s.Friction );
		Set( "Surface", () => s.Surface );
		Set( "Scale", () => s.Scale );
		Set( "tags", () => new JsonArray( s.Tags.TryGetAll().OrderBy( t => t ).Select( t => (JsonNode)t ).ToArray() ) );
		if ( s.IsSphereShape ) Set( "Sphere", () => s.Sphere );
		if ( s.IsCapsuleShape ) Set( "Capsule", () => s.Capsule );
		try
		{
			s.Triangulate( out var positions, out var indices );
			o["verts"] = positions.Length;
			o["tris"] = indices.Length / 3;
			if ( positions.Length > 0 )
				o["hull"] = StateDumpValues.Encode( BBox.FromPoints( positions ), 0 );
		}
		catch ( Exception e ) { o["verts"] = StateDumpValues.Error( e ); }
		return o;
	}
}
