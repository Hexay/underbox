using Sandbox;

/// <summary>
/// Which collider pairs a trigger reports (`TRIGGER`): static, keyframed and dynamic triggers over
/// static, keyframed and dynamic boxes, all created overlapping; the `moved` set's trigger is
/// teleported 1 unit at tick 3 (docs/differential-tap.md, "Fixed").
/// </summary>
public sealed class ProbeTrigger : Component, Component.ITriggerListener
{
	public static int Tick;

	void ITriggerListener.OnTriggerEnter( Collider other ) =>
		Log.Info( $"DIFF:TRIGGER t={Tick} {GameObject.Name} enter {other.GameObject.Name}" );

	static GameObject Box( string name, Vector3 at, bool trigger, string kind )
	{
		var go = new GameObject( true, name );
		go.WorldPosition = at;
		var c = go.Components.Create<BoxCollider>();
		c.Scale = new Vector3( 16, 16, 16 );
		c.IsTrigger = trigger;
		c.Static = kind == "static";
		if ( kind == "dynamic" ) go.Components.Create<Rigidbody>().Gravity = false;
		if ( trigger ) go.Components.Create<ProbeTrigger>();
		return go;
	}

	/// One trigger of `kind` at `at`, with a static, a keyframed and a dynamic box inside it.
	static GameObject Set( string name, Vector3 at, string kind )
	{
		var trigger = Box( $"{name}-trigger", at, true, kind );
		foreach ( var other in new[] { "static", "keyframed", "dynamic" } )
			Box( $"{name}-{other}", at + new Vector3( 0, 0, 2 ), false, other );
		return trigger;
	}

	public static GameObject Spawn()
	{
		Set( "Tstatic", new Vector3( -1200, 1600, 100 ), "static" );
		Set( "Tkey", new Vector3( -1300, 1600, 100 ), "keyframed" );
		Set( "Tdyn", new Vector3( -1400, 1600, 100 ), "dynamic" );
		return Set( "Tmoved", new Vector3( -1500, 1600, 100 ), "keyframed" );
	}
}
