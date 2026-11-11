using Sandbox;
using Sandbox.UI;

/// <summary>
/// Static filled, bordered, rounded and gradient boxes and text, so the frame settles.
/// </summary>
public sealed class Hud : PanelComponent
{
	protected override void OnTreeFirstBuilt()
	{
		Panel.Style.Set( "position: absolute; left: 0; top: 0; width: 100%; height: 100%;" );

		Box( "left: 40px; top: 40px; width: 320px; height: 140px; background-color: #d03020;" );
		Box( "left: 400px; top: 40px; width: 320px; height: 140px; background-color: #203080; border: 8px solid #f0f0f0;" );
		Box( "left: 760px; top: 40px; width: 220px; height: 220px; background-color: #20a040; border-radius: 60px;" );
		Box( "left: 40px; top: 220px; width: 680px; height: 120px; background-image: linear-gradient( to right, #ff8000, #0080ff );" );
		Box( "left: 40px; top: 380px; width: 940px; height: 200px; background-color: rgba( 0, 0, 0, 0.5 ); border: 4px solid #ffd000; border-radius: 24px;" );

		var label = new Label( "UNDERBOX HUD 42", null ) { Parent = Panel };
		label.Style.Set( "position: absolute; left: 80px; top: 420px; font-size: 72px; font-weight: 700; color: #ffffff;" );
		var small = new Label( "panels, borders, gradients, text", null ) { Parent = Panel };
		small.Style.Set( "position: absolute; left: 80px; top: 520px; font-size: 32px; color: #ffd000;" );
	}

	private void Box( string style )
	{
		var box = new Panel( Panel );
		box.Style.Set( "position: absolute; " + style );
	}
}
