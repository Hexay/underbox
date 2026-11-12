using Sandbox;

/// <summary>
/// Injected by tools/diff/perf.sh into a scratch copy of the scenario (docs/differential-perf.md). Logs
/// wall-clock timestamps (µs since the Unix epoch) that tools/diff/perf_sample.py lines up with its
/// per-thread CPU samples: `PERF:TICK n us` for each fixed tick after WarmupTicks, `PERF:BEGIN us` and
/// `PERF:END frames=n us` around Frames frames after WarmupFrames, then `PERF:DONE`.
/// Mode "tick" (a server) or "frame" (a client).
/// </summary>
public sealed class PerfMark : Component
{
	[Property] public string Mode { get; set; } = "tick";
	[Property] public int WarmupTicks { get; set; } = 50;
	[Property] public int Ticks { get; set; } = 500;
	[Property] public int WarmupFrames { get; set; } = 60;
	[Property] public int Frames { get; set; } = 200;

	int tick;
	int frame;
	bool done;

	static long Now() => (System.DateTime.UtcNow.Ticks - System.DateTime.UnixEpoch.Ticks) / 10;

	protected override void OnStart() => Log.Info( $"PERF:START {Now()}" );

	protected override void OnFixedUpdate()
	{
		if ( Mode != "tick" || done ) return;
		tick++;
		if ( tick <= WarmupTicks ) return;
		if ( tick > WarmupTicks + Ticks + 1 )
		{
			done = true;
			Log.Info( $"PERF:DONE ticks={Ticks}" );
			return;
		}
		Log.Info( $"PERF:TICK {tick - WarmupTicks} {Now()}" );
	}

	protected override void OnUpdate()
	{
		if ( Mode != "frame" || done ) return;
		frame++;
		if ( frame == WarmupFrames ) Log.Info( $"PERF:BEGIN {Now()}" );
		if ( frame != WarmupFrames + Frames ) return;
		Log.Info( $"PERF:END frames={Frames} {Now()}" );
		done = true;
		Log.Info( "PERF:DONE" );
	}
}
