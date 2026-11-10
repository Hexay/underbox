using Sandbox;

public sealed class GoalZone : Component, Component.ITriggerListener
{
	public MatchDirector Director { get; set; }

	public void OnTriggerEnter( Collider other )
	{
		var bot = other.GameObject?.Components.Get<OrbBot>();
		if ( bot is not null ) Director?.Scored( bot );
	}
}
