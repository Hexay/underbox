using Sandbox;
using System.Collections.Generic;

/// <summary>
/// Server-tick load for tools/diff/perf.sh (docs/differential-perf.md): a walled pit with Columns
/// stacks of StackHeight boxes and Rows×Rows mixed boxes and spheres dropped onto them in layers.
/// Every KickEvery ticks every body gets an upward kick, so the pile never falls asleep.
/// </summary>
public sealed class Pile : Component
{
	[Property] public int Rows { get; set; } = 12;
	[Property] public int Layers { get; set; } = 2;
	[Property] public int Columns { get; set; } = 4;
	[Property] public int StackHeight { get; set; } = 10;
	[Property] public int KickEvery { get; set; } = 100;

	readonly List<Rigidbody> bodies = new();
	int tick;

	protected override void OnStart()
	{
		if ( IsProxy ) return;
		Static( "Floor", new Vector3( 0, 0, -8 ), new Vector3( 1200, 1200, 16 ) );
		foreach ( var (x, y) in new[] { (1, 0), (-1, 0), (0, 1), (0, -1) } )
			Static( "Wall", new Vector3( 600 * x, 600 * y, 200 ), new Vector3( x == 0 ? 1200 : 16, y == 0 ? 1200 : 16, 400 ) );
		for ( int c = 0; c < Columns; c++ )
			for ( int h = 0; h < StackHeight; h++ )
				bodies.Add( Body( new Vector3( -300 + 200 * c, 300, 16 + 32 * h ), false ) );
		for ( int l = 0; l < Layers; l++ )
			for ( int i = 0; i < Rows; i++ )
				for ( int j = 0; j < Rows; j++ )
					bodies.Add( Body( new Vector3( -440 + 80 * i + 20 * l, -440 + 80 * j, 300 + 120 * l ), (i + j + l) % 2 == 0 ) );
		Log.Info( $"PILE:START bodies={bodies.Count}" );
	}

	static void Static( string name, Vector3 pos, Vector3 size )
	{
		var go = new GameObject( true, name );
		go.WorldPosition = pos;
		go.Components.Create<BoxCollider>().Scale = size;
	}

	static Rigidbody Body( Vector3 pos, bool sphere )
	{
		var go = new GameObject( true, sphere ? "Ball" : "Box" );
		go.WorldPosition = pos;
		go.WorldRotation = Rotation.From( pos.x % 30, pos.y % 45, 0 );
		if ( sphere ) go.Components.Create<SphereCollider>().Radius = 16f;
		else go.Components.Create<BoxCollider>().Scale = new Vector3( 32, 32, 32 );
		return go.Components.Create<Rigidbody>();
	}

	protected override void OnFixedUpdate()
	{
		if ( IsProxy || ++tick % KickEvery != 0 ) return;
		foreach ( var b in bodies )
			b.Velocity += Vector3.Up * 250;
	}
}
