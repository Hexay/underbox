using Sandbox;
using System.Linq;

/// <summary>
/// Waits for the scene's MapInstance (maps/tiny.vpk, written by engine2's world tests from the
/// world crate's fixture) and logs what it made as `MAP:` markers that tools/map.sh asserts.
/// </summary>
public sealed class MapProbe : Component
{
	[Property] public float Timeout { get; set; } = 20f;

	TimeSince sinceStart;
	bool done;

	public static void Mark( string what ) => Log.Info( $"MAP:{what}" );

	protected override void OnStart() => sinceStart = 0;

	protected override void OnUpdate()
	{
		if ( done ) return;
		var map = Scene.GetAllComponents<MapInstance>().FirstOrDefault();
		var loaded = map is not null && map.IsLoaded;
		if ( !loaded && sinceStart < Timeout ) return;
		done = true;

		Mark( $"LOADED {loaded}" );
		if ( loaded )
		{
			foreach ( var go in map.GameObject.GetAllObjects( false ).Where( x => x != map.GameObject ) )
				Mark( $"ENTITY {go.Name} @ {go.WorldPosition}" );
			Mark( $"SPAWNPOINTS {Scene.GetAllComponents<SpawnPoint>().Count()}" );
			Mark( $"LIGHTS {Scene.GetAllComponents<PointLight>().Count()}" );
			var models = Scene.SceneWorld.SceneObjects.Where( x => x.Model is not null ).ToArray();
			foreach ( var so in models )
				Mark( $"SCENEOBJECT {so.Model.ResourceName} @ {so.Position}" );
			Mark( $"SCENEOBJECTS {models.Length}" );
		}
		Mark( "END" );
	}
}
