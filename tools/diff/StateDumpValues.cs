using Sandbox;
using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json.Nodes;

/// <summary>
/// Value encoding for tools/diff/StateDump.cs; tools/diff/state.py reads it back. Vectors and colours
/// are arrays, a Rotation is {"q":[x,y,z,w]}, Angles {"ang":[p,y,r]}, a BBox {"min","max"}, a Transform
/// {"pos","rot","scale"}, resources their path, objects "@path" and components "@path#Type". A Model
/// or SceneObject one level down from a component gets its own properties; anything deeper is a type
/// name or ToString. A getter that throws gives {"$err": type: message}.
/// </summary>
public static class StateDumpValues
{
	// On every component; the object's own line covers them, or they differ run to run by design.
	static readonly HashSet<string> ComponentSkip = new()
	{
		"GameObject", "Scene", "Components", "Transform", "WorldTransform", "WorldPosition", "WorldRotation", "WorldScale",
		"LocalTransform", "LocalPosition", "LocalRotation", "LocalScale", "Id", "Network", "Tags", "IsProxy", "Enabled",
		"Active", "DebugOverlay", "Gizmo",
	};

	const int MaxText = 200;

	public static JsonObject Error( Exception e ) => new() { ["$err"] = Clip( $"{e.GetType().Name}: {e.Message}" ) };

	public static void Set( JsonObject o, string key, Func<object> get )
	{
		try { o[key] = Encode( get(), 0 ); }
		catch ( Exception e ) { o[key] = Error( e ); }
	}

	/// <summary>Every public instance property TypeLibrary describes, by name.</summary>
	public static JsonObject Props( object obj, bool component, int depth = 0 )
	{
		var o = new JsonObject();
		var type = Game.TypeLibrary.GetType( obj.GetType() );
		if ( type is null ) { o["$err"] = $"no TypeDescription for {obj.GetType().Name}"; return o; }
		foreach ( var p in type.Properties.OrderBy( p => p.Name, StringComparer.Ordinal ) )
		{
			if ( p.IsStatic || !p.CanRead || !p.IsGetMethodPublic || p.IsIndexer ) continue;
			if ( component && ComponentSkip.Contains( p.Name ) ) continue;
			try
			{
				var value = p.GetValue( obj );
				if ( value is Delegate ) continue;
				o[p.Name] = Encode( value, depth );
			}
			catch ( Exception e ) { o[p.Name] = Error( e.InnerException ?? e ); }
		}
		return o;
	}

	public static JsonNode Encode( object v, int depth )
	{
		switch ( v )
		{
			case null: return null;
			case JsonNode n: return n;
			case string s: return Clip( s );
			case bool b: return b;
			case float f: return Num( f );
			case double d: return Num( d );
			case int i: return i;
			case long l: return l;
			case uint u: return u;
			case ulong ul: return ul;
			case short sh: return sh;
			case ushort us: return us;
			case byte by: return by;
			case Enum e: return e.ToString();
			case Vector2 a: return Arr( a.x, a.y );
			case Vector3 a: return Arr( a.x, a.y, a.z );
			case Vector4 a: return Arr( a.x, a.y, a.z, a.w );
			case Color a: return Arr( a.r, a.g, a.b, a.a );
			case Color32 a: return Arr( a.r, a.g, a.b, a.a );
			case Rotation r: return new JsonObject { ["q"] = Arr( r.x, r.y, r.z, r.w ) };
			case Angles a: return new JsonObject { ["ang"] = Arr( a.pitch, a.yaw, a.roll ) };
			case BBox b: return new JsonObject { ["min"] = Encode( b.Mins, depth ), ["max"] = Encode( b.Maxs, depth ) };
			case Sphere s: return new JsonObject { ["center"] = Encode( s.Center, depth ), ["radius"] = Num( s.Radius ) };
			case Capsule c: return new JsonObject { ["a"] = Encode( c.CenterA, depth ), ["b"] = Encode( c.CenterB, depth ), ["radius"] = Num( c.Radius ) };
			case Transform t:
				return new JsonObject { ["pos"] = Encode( t.Position, depth ), ["rot"] = Encode( t.Rotation, depth ), ["scale"] = Encode( t.Scale, depth ) };
			case Model m when depth == 0:
				var model = Props( m, component: false, depth + 1 );
				model["$path"] = m.ResourcePath;
				return model;
			case Resource r: return r.ResourcePath ?? $"<{r.GetType().Name}>";
			case GameObject g: return "@" + StateDump.PathOf( g );
			case Component c: return $"@{StateDump.PathOf( c.GameObject )}#{c.GetType().Name}";
			case PhysicsBody body: return "$body@" + (StateDump.PathOf( body.GameObject ) ?? "");
			case SceneObject so when depth == 0: return Props( so, component: false, depth + 1 );
			case ICollection col: return new JsonObject { ["count"] = col.Count };
		}
		var type = v.GetType();
		return type.IsValueType ? Clip( v.ToString() ) : $"<{type.Name}>";
	}

	public static JsonNode Num( double d ) => double.IsFinite( d ) ? JsonValue.Create( d ) : JsonValue.Create( d.ToString() );

	static JsonArray Arr( params double[] xs ) => new( xs.Select( Num ).ToArray() );

	static string Clip( string s ) => s is null || s.Length <= MaxText ? s : s[..MaxText] + "…";
}
