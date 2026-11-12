using Sandbox;

/// <summary>
/// Emits its burst on a fixed grid in the object's Y/Z plane, with fixed colours, sizes and rolls and no
/// velocity: the stock emitters draw positions from an unseeded random, so the frame would differ every run.
/// </summary>
public sealed class GridEmitter : ParticleEmitter
{
	[Property] public int Columns { get; set; } = 5;
	[Property] public float Spacing { get; set; } = 30.0f;
	[Property] public float Size { get; set; } = 24.0f;
	[Property] public float SizeStep { get; set; } = 0.0f;
	[Property] public float RollStep { get; set; } = 0.0f;
	[Property] public float Alpha { get; set; } = 1.0f;
	[Property] public float Brightness { get; set; } = 1.0f;

	static readonly Color[] Palette =
	{
		new( 1.0f, 0.35f, 0.2f ), new( 1.0f, 0.8f, 0.2f ), new( 0.4f, 1.0f, 0.3f ), new( 0.2f, 0.8f, 1.0f ),
		new( 0.35f, 0.4f, 1.0f ), new( 0.9f, 0.3f, 1.0f ), new( 1.0f, 1.0f, 1.0f ),
	};

	int index;

	public override bool Emit( ParticleEffect target )
	{
		int i = index++;
		var local = new Vector3( 0, ((Columns - 1) * 0.5f - i % Columns) * Spacing, (i / Columns) * Spacing );
		var p = target.Emit( WorldTransform.PointToWorld( local ), Delta );
		var size = Size + SizeStep * i;
		var c = Palette[i % Palette.Length];
		p.Velocity = Vector3.Zero;
		p.Size = new Vector3( size, size, size );
		p.Angles = new Angles( 0, RollStep * i * 0.5f, RollStep * i );
		p.Color = new Color( c.r * Brightness, c.g * Brightness, c.b * Brightness, 1.0f );
		p.Alpha = Alpha;
		return true;
	}
}
