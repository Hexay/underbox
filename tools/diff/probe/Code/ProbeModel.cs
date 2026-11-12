using Sandbox;
using System.Linq;

/// <summary>
/// Resource metadata managed reads off native (`MODEL`, `LIGHT`): core models' parts, skins,
/// skeleton and physics names and GUIDs, a procedural model's name, and new lights' radius
/// (docs/differential-tap.md).
/// </summary>
public static class ProbeModel
{
	static void Mark( string what ) => Log.Info( $"DIFF:{what}" );

	public static void Run( Scene scene )
	{
		foreach ( var path in new[] { "models/dev/box.vmdl", "models/dev/sphere.vmdl", "models/dev/plane.vmdl", "models/dev/error.vmdl" } )
			Describe( path, Model.Load( path ) );
		Describe( "procedural", Model.Builder.Create() );

		var spot = new SceneSpotLight( scene.SceneWorld );
		var sun = new SceneDirectionalLight( scene.SceneWorld, Rotation.Identity, Color.White );
		Mark( $"LIGHT spot={spot.Radius} sun={sun.Radius}" );
		spot.Delete();
		sun.Delete();
	}

	static void Describe( string what, Model m )
	{
		var parts = string.Join( ";", m.Parts.All.Select( p => $"{p.Name}:{p.Mask}[{string.Join( ",", p.Choices.Select( c => $"{c.Name}={c.Mask}" ) )}]" ) );
		Mark( $"MODEL {what} name={m.ResourceName} error={m.IsError} parts={m.BodyGroupCount} {parts} skins={m.MaterialGroupCount} skeleton='{m.SkeletonName}' physics='{m.Physics?.Name}' meshes={m.MeshCount}" );
	}
}
