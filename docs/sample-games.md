# sbox-public's sample games

Part of the real-games runs (docs/real-games.md). sbox-public carries three runnable games, used in
place and never copied into this repo: `game/samples/sweeper` and the project templates
`game/templates/game.minimal` and `game/templates/game.playercontroller`. `game/addons/menu` is
also a game: it is the main menu every client loads.

None of them ships compiled assets. sbox-public's only `_c` files are three `.scene_c`, and
`game/core` and the addons hold sources and `.meta` files. Even the menu's music is `.meta` only.
So no sample gives a textured frame. What they do give is real geometry with no error models. The
scenes use `models/dev/box.vmdl` and `plane.vmdl`, which engine2 builds from their `.vmdl` source
(`error_models=0`). Their materials stay untextured.

| Game | Server | Client frame | Scripted interaction |
|---|---|---|---|
| Sweeper (`scenes/minimal.scene`) | passes: 72 rigidbody cubes settle, `GAME:STANDING ... on=Player Cube`, 0 stubs | 76 renderers, 0 error models: tinted cubes, plane, spotlight cone, flat sky | `MODE=play PLAY_VIA=host`: W drives `CubeController`, `GAME:CAMERA_MOVED view dist=58`, the mouse turns it, `GAME:CAMERA_TURNED view yaw=-0->-19` |
| `game.minimal` | passes: `bodies=3`, `GAME:STANDING ... on=Plane` | `MODE=render` passes: 3 magenta boxes on a green plane, flat blue sky | none (no game code) |
| `game.playercontroller` (`basic_example.scene`) | passes: `GAME:STANDING RealGameWatchController@128,128,0 on=Plane`. The scene's own player stands at the origin, so the probe drops beside it | third person over the Citizen, which is an error model (`human.vmdl` is source only) | `MODE=play PLAY_VIA=host`: `GAME:CAMERA_MOVED view dist=42`, `GAME:CAMERA_TURNED view yaw=0->-11` |

Commands: `tools/game.sh <sbox-public>/game/samples/sweeper` (likewise
`templates/game.minimal`, `templates/game.playercontroller`), with `MODE=render` or
`MODE=play PLAY_VIA=host`.

What these runs show:
- Every client logs `OnEnabled on Sandbox.EnvmapProbe failed ... ITexture was null when calling
  IsStrongHandleValid`. `SceneCubemap.Texture` wraps a null native texture when the envmap
  (`textures/cubemaps/default2.vtex`) has no compiled file. Handed to the renderer lane.
- The menu logs `LoadFromFile: Couldn't find scenes/menu-main.scene`, though the source is in
  `addons/menu/Assets/scenes`. Not triaged. It doesn't block `+game` runs.
