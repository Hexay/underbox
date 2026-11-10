using Sandbox;

/// <summary>A scripted player: steers its rigidbody at the goal and reports what happens to it.</summary>
public sealed class OrbBot : Component, Component.ICollisionListener
{
	public int Team { get; set; }
	public Vector3 Home { get; set; }
	public bool Moved { get; private set; }

	[Property] public float Speed { get; set; } = 200f;

	bool stopped;
	bool collided;

	Rigidbody Body => Components.Get<Rigidbody>();

	protected override void OnFixedUpdate()
	{
		if ( IsProxy || stopped || Body is null ) return;
		var dir = (Vector3.Zero - WorldPosition).WithZ( 0 ).Normal;
		Body.Velocity = dir * Speed;
		if ( !Moved && WorldPosition.Distance( Home ) > 32f )
		{
			Moved = true;
			MatchDirector.Mark( $"MOVE {GameObject.Name} pos={WorldPosition}" );
		}
	}

	public void OnCollisionStart( Collision collision )
	{
		if ( collided || collision.Other.GameObject?.Components.Get<OrbBot>() is null ) return;
		collided = true;
		MatchDirector.Mark( $"COLLIDE {GameObject.Name} with={collision.Other.GameObject.Name}" );
	}

	public void Respawn()
	{
		WorldPosition = Home;
		if ( Body is not null ) Body.Velocity = Vector3.Zero;
	}

	public void Stop()
	{
		stopped = true;
		if ( Body is not null ) Body.Velocity = Vector3.Zero;
	}
}
