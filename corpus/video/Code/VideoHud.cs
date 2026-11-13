using Sandbox;
using Sandbox.UI;

/// <summary>
/// A panel whose background is a looping H.264 still (videos/still.webm, four palette quadrants),
/// loaded through managed's video texture loader, so the frame settles once the first picture is up.
/// </summary>
public sealed class VideoHud : PanelComponent
{
	protected override void OnTreeFirstBuilt()
	{
		Panel.Style.Set( "position: absolute; left: 0; top: 0; width: 100%; height: 100%;" );

		var screen = new Panel( Panel );
		screen.Style.Set( "position: absolute; left: 80px; top: 60px; width: 640px; height: 360px; border: 6px solid #f0f0f0; background-image: url( videos/still.webm ); background-size: 100% 100%;" );
	}
}
