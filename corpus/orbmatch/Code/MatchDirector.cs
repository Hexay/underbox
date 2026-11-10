using Sandbox;
using System.Collections.Generic;
using System.Linq;

/// <summary>
/// Orb Match: bots spawn on two sides, roll toward a central goal trigger, bump each other on the
/// way and score on entry. First to <see cref="ScoreToWin"/> ends the match. Every step logs a
/// `MATCH:` marker that tools/match.sh asserts.
/// </summary>
public sealed class MatchDirector : Component
{
	[Property] public int BotsPerTeam { get; set; } = 2;
	[Property] public int ScoreToWin { get; set; } = 3;
	[Property] public float TimeLimit { get; set; } = 30f;

	readonly List<OrbBot> bots = new();
	readonly Dictionary<int, int> scores = new();
	TimeSince sinceStart;
	bool ended;

	public static void Mark( string what ) => Log.Info( $"MATCH:{what}" );

	protected override void OnStart()
	{
		if ( IsProxy ) return;
		Mark( "START" );
		SpawnGoal();
		for ( int team = 0; team < 2; team++ )
		{
			scores[team] = 0;
			for ( int i = 0; i < BotsPerTeam; i++ )
				bots.Add( SpawnBot( team, i ) );
		}
		Mark( $"SPAWNED {bots.Count}" );
		sinceStart = 0;
	}

	void SpawnGoal()
	{
		var go = new GameObject( true, "Goal" );
		go.WorldPosition = Vector3.Zero;
		var box = go.Components.Create<BoxCollider>();
		box.Scale = new Vector3( 64, 64, 64 );
		box.IsTrigger = true;
		go.Components.Create<GoalZone>().Director = this;
	}

	OrbBot SpawnBot( int team, int index )
	{
		var side = team == 0 ? -1f : 1f;
		var go = new GameObject( true, $"Bot{team}_{index}" );
		go.WorldPosition = new Vector3( side * (400 + index * 40), index * 8f, 32 );
		go.Components.Create<SphereCollider>().Radius = 16f;
		var body = go.Components.Create<Rigidbody>();
		body.Gravity = false;
		var bot = go.Components.Create<OrbBot>();
		bot.Team = team;
		bot.Home = go.WorldPosition;
		// Trailing bots run faster so they rear-end their teammate before anyone reaches the goal and respawns.
		bot.Speed = 200f + index * 100f;
		return bot;
	}

	public void Scored( OrbBot bot )
	{
		if ( ended ) return;
		scores[bot.Team]++;
		Mark( $"SCORE team={bot.Team} bot={bot.GameObject.Name} total={scores[bot.Team]}" );
		bot.Respawn();
		if ( scores[bot.Team] >= ScoreToWin ) End( $"winner={bot.Team}" );
	}

	protected override void OnFixedUpdate()
	{
		if ( IsProxy || ended || bots.Count == 0 ) return;
		if ( sinceStart > TimeLimit )
			End( $"timeout scores={string.Join( ",", scores.Values )} moved={bots.Count( b => b.Moved )}" );
	}

	void End( string why )
	{
		ended = true;
		foreach ( var b in bots )
		{
			Mark( $"FINAL {b.GameObject.Name} pos={b.WorldPosition}" );
			b.Stop();
		}
		Mark( $"END {why}" );
	}
}
