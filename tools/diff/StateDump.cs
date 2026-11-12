using Sandbox;
using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text.Json.Nodes;

/// <summary>
/// Injected into a scratch copy of a corpus scene or game by tools/diff/state.sh (docs/differential-state.md).
/// At the chosen fixed-update ticks it logs the whole scene as `STATE:` JSON lines: one per GameObject
/// (`k` "#"), one per component (every public instance property through TypeLibrary) and one per
/// physics body in the scene's world (`k` "$body..."). Logs `STATE:END` after the last tick.
/// </summary>
public sealed class StateDump : Component
{
	/// <summary>Comma-separated ticks to dump at.</summary>
	[Property] public string Ticks { get; set; } = "1,5,25,50,100,200";
	/// <summary>Also dump every N ticks (0: only Ticks).</summary>
	[Property] public int Every { get; set; } = 0;
	/// <summary>The last tick; 0 means the largest of Ticks.</summary>
	[Property] public int Last { get; set; } = 0;

	/// <summary>Stands in for the game's Networking.CreateLobby (tools/game/prepare.py --stub-lobby).</summary>
	public static void SkipLobby( Sandbox.Network.LobbyConfig config ) => Log.Info( "STATE:NO_LOBBY" );

	HashSet<int> ticks;
	int last;
	int tick;
	bool ended;

	protected override void OnStart()
	{
		ticks = Ticks.Split( ',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries )
			.Select( s => int.Parse( s, CultureInfo.InvariantCulture ) ).ToHashSet();
		last = Last > 0 ? Last : (ticks.Count > 0 ? ticks.Max() : 1);
		Log.Info( $"STATE:START ticks={string.Join( ",", ticks.OrderBy( t => t ) )} every={Every} last={last} server={Application.IsDedicatedServer}" );
	}

	protected override void OnFixedUpdate()
	{
		if ( ended ) return;
		tick++;
		if ( ticks.Contains( tick ) || (Every > 0 && tick % Every == 0) || tick == last )
			DumpScene();
		if ( tick < last ) return;
		ended = true;
		Log.Info( "STATE:END" );
	}

	void DumpScene()
	{
		int lines = 0;
		foreach ( var (go, path) in Walk( Scene.Children, "" ) )
		{
			if ( go == GameObject ) continue;
			Emit( path, "#", ObjectState( go ) );
			lines++;
			foreach ( var (key, c) in Keyed( go.Components.GetAll<Component>( FindMode.EverythingInSelf ), c => c.GetType().Name ) )
			{
				Emit( path, key, ComponentState( c ) );
				lines++;
			}
		}
		lines += StateDumpPhysics.DumpBodies( Scene.PhysicsWorld, Emit );
		Log.Info( $"STATE:TICK {tick} lines={lines}" );
	}

	void Emit( string path, string key, JsonNode value )
	{
		var line = new JsonObject { ["t"] = tick, ["p"] = path, ["k"] = key, ["v"] = value };
		string text;
		try { text = line.ToJsonString(); }
		catch ( Exception e ) { text = new JsonObject { ["t"] = tick, ["p"] = path, ["k"] = key, ["v"] = StateDumpValues.Error( e ) }.ToJsonString(); }
		Log.Info( $"STATE:{text}" );
	}

	static JsonObject ObjectState( GameObject go )
	{
		var o = new JsonObject();
		StateDumpValues.Set( o, "name", () => go.Name );
		StateDumpValues.Set( o, "enabled", () => go.Enabled );
		StateDumpValues.Set( o, "active", () => go.Active );
		StateDumpValues.Set( o, "static", () => go.IsStatic );
		StateDumpValues.Set( o, "flags", () => go.Flags );
		StateDumpValues.Set( o, "tags", () => new JsonArray( go.Tags.TryGetAll().OrderBy( t => t ).Select( t => (JsonNode)t ).ToArray() ) );
		StateDumpValues.Set( o, "world", () => go.WorldTransform );
		StateDumpValues.Set( o, "local", () => go.LocalTransform );
		StateDumpValues.Set( o, "parent", () => go.Parent is Sandbox.Scene ? null : go.Parent?.Name );
		StateDumpValues.Set( o, "children", () => go.Children.Count );
		StateDumpValues.Set( o, "components", () => new JsonArray( go.Components.GetAll<Component>( FindMode.EverythingInSelf )
			.Select( c => (JsonNode)c.GetType().Name ).ToArray() ) );
		return o;
	}

	static JsonObject ComponentState( Component c )
	{
		var o = StateDumpValues.Props( c, component: true );
		StateDumpValues.Set( o, "$enabled", () => c.Enabled );
		StateDumpValues.Set( o, "$active", () => c.Active );
		return o;
	}

	/// <summary>
	/// Depth-first with paths `A/B/C`; a name shared by siblings gets `[i]` (its index among them, in
	/// child order) so objects match across engines by path.
	/// </summary>
	static IEnumerable<(GameObject, string)> Walk( IEnumerable<GameObject> children, string prefix )
	{
		foreach ( var (key, go) in Keyed( children.ToList(), g => g.Name ?? "" ) )
		{
			var path = prefix + key;
			yield return (go, path);
			foreach ( var inner in Walk( go.Children, path + "/" ) )
				yield return inner;
		}
	}

	static IEnumerable<(string, T)> Keyed<T>( IEnumerable<T> items, Func<T, string> name )
	{
		var list = items.ToList();
		var counts = list.GroupBy( name ).ToDictionary( g => g.Key, g => g.Count() );
		var seen = new Dictionary<string, int>();
		foreach ( var item in list )
		{
			var n = name( item );
			if ( counts[n] == 1 ) { yield return (n, item); continue; }
			seen.TryGetValue( n, out var i );
			seen[n] = i + 1;
			yield return ($"{n}[{i}]", item);
		}
	}

	/// <summary>The path Walk gives an object, for references from other objects' properties.</summary>
	public static string PathOf( GameObject go )
	{
		if ( go is null ) return null;
		if ( go is Sandbox.Scene ) return "";
		var siblings = go.Parent is null ? new List<GameObject>() : go.Parent.Children.Where( c => c.Name == go.Name ).ToList();
		var name = siblings.Count > 1 ? $"{go.Name}[{siblings.IndexOf( go )}]" : go.Name;
		var parent = go.Parent is null or Sandbox.Scene ? "" : PathOf( go.Parent ) + "/";
		return parent + name;
	}
}
