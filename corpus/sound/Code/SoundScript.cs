using Sandbox;
using System.Threading.Tasks;

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
	bool _loaded, _started, _stopped, _ended;
	SoundFile _sine, _noise, _loopFile;
	SoundHandle _loop;

	protected override void OnStart()
	{
		_since = 0;
		_sine = SoundFile.Load( "sounds/compiled/sine440.vsnd" );
		_noise = SoundFile.Load( "sounds/compiled/noise.vsnd" );
		_loopFile = SoundFile.Load( "sounds/compiled/loop330.vsnd" );
		_ = WaitLoaded();
	}

	// Real engine2 loads sound data asynchronously and drops PlayFile's delay while it isn't playable yet.
	async Task WaitLoaded()
	{
		var ok = await _sine.LoadAsync() & await _noise.LoadAsync() & await _loopFile.LoadAsync();
		Log.Info( $"SOUND:LOADED sine={_sine.IsValid()} noise={_noise.IsValid()} loop={_loopFile.IsValid()} playable={ok}" );
		_loaded = true;
	}

	protected override void OnUpdate()
	{
		if ( !_started && _loaded && _since > Lead )
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
		var listener = Scene.Camera.WorldPosition;
		Ui( _sine, 0, 1, 1 );
		At( _sine, 1, listener + Vector3.Left * 200 );
		At( _sine, 2, listener + Vector3.Right * 200 );
		At( _sine, 3, listener + Vector3.Forward * 200 );
		At( _sine, 4, listener + Vector3.Forward * 800 );
		Ui( _sine, 5, 0.5f, 1 );
		Ui( _sine, 6, 1, 1.5f );
		Ui( _noise, 7, 1, 1 );
		_loop = Ui( _loopFile, 8, 1, 1 );
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
