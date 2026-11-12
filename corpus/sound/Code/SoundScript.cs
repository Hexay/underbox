using Sandbox;

/// <summary>
/// The audio differential's script (docs/differential-audio.md, tools/diff/sound.py): every event is
/// one PlayFile with a sample-accurate delay from a single start, so onsets don't depend on frame
/// timing. Only the loop's stop is timed by the main thread. Keep the schedule in step with sound.py.
/// </summary>
public sealed class SoundScript : Component
{
	const float Lead = 1.0f;
	const float LoopStop = 9.5f;
	const float End = 11.0f;

	RealTimeSince _since;
	bool _started, _stopped, _ended;
	SoundHandle _loop;

	protected override void OnStart() => _since = 0;

	protected override void OnUpdate()
	{
		if ( !_started && _since > Lead )
		{
			_started = true;
			_since = 0;
			Play();
		}
		if ( _started && !_stopped && _since > LoopStop )
		{
			_stopped = true;
			_loop?.Stop();
			Log.Info( $"SOUND:STOP loop at {_since.Relative:0.000}" );
		}
		if ( _started && !_ended && _since > End )
		{
			_ended = true;
			Log.Info( "SOUND:END" );
		}
	}

	void Play()
	{
		var sine = SoundFile.Load( "sounds/compiled/sine440.vsnd" );
		var noise = SoundFile.Load( "sounds/compiled/noise.vsnd" );
		var loop = SoundFile.Load( "sounds/compiled/loop330.vsnd" );
		Log.Info( $"SOUND:LOADED sine={sine?.IsValid()} noise={noise?.IsValid()} loop={loop?.IsValid()}" );

		var listener = Scene.Camera.WorldPosition;
		Ui( sine, 0, 1, 1 );
		At( sine, 1, listener + Vector3.Left * 200 );
		At( sine, 2, listener + Vector3.Right * 200 );
		At( sine, 3, listener + Vector3.Forward * 200 );
		At( sine, 4, listener + Vector3.Forward * 800 );
		Ui( sine, 5, 0.5f, 1 );
		Ui( sine, 6, 1, 1.5f );
		Ui( noise, 7, 1, 1 );
		_loop = Ui( loop, 8, 1, 1 );
		Log.Info( "SOUND:PLAY" );
	}

	static SoundHandle Ui( SoundFile file, float delay, float volume, float pitch )
	{
		var h = Sound.PlayFile( file, volume, pitch, delay );
		h.ListenLocal = true;
		h.SpacialBlend = 0;
		h.DistanceAttenuation = false;
		return h;
	}

	static SoundHandle At( SoundFile file, float delay, Vector3 position )
	{
		var h = Sound.PlayFile( file, 1, 1, delay );
		h.Position = position;
		return h;
	}
}
