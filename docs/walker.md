# sbox-walker

Part of the real-games runs (docs/real-games.md). Facepunch/sbox-walker (MIT, `95f5b5a`) is
Facepunch's minimal multiplayer walking game: a `PlayerController` player with a Citizen body,
inventory, HUD, chat and a few map entities (`FuncButton`, `FuncDoor`, `TriggerTeleport`,
`TriggerHurt`, `CollectableItem`). It ships code, scenes, prefabs and three `.ogg` UI sounds, and no
compiled assets. Run from a separate checkout (`../sbox-walker`); nothing is copied into this repo.

## How it starts

- `minimal.scene` (the `StartupScene`) holds a camera and a `MapInstance` with
  `UseMapFromLaunch`. Its default map, `facepunch.datacore`, is a package and stays refused.
- `GameManager.OnHostInitialize` loads `engine.scene` (camera, HUD, chat, scoreboard) additively and
  creates a lobby. Every connection gets `/player.prefab` at a `SpawnPoint`.
- `scenemap.scene` is walker's own map: a scene map (its `.meta` says `Type: map`). The real launch
  is `+game <sbproj> scenes/scenemap.scene`, which `MAP=` passes, so the MapInstance loads it under
  `Map Loader`. Without `MAP=` there is no floor, and players spawn at the origin and fall.

## Runs

| Run | Command | Result |
|---|---|---|
| Server, map | `MAP=scenes/scenemap.scene tools/game.sh <walker>` | passes: 0 compile errors, 0 missing components, 0 stubs, `GAME:STANDING RealGameWatchController@0,-0,12 on=Box` |
| Server, each scene as startup | `tools/game.sh <walker> scenes/{minimal,engine,scenemap}.scene` | all load and compile with 0 stubs. scenemap: `STANDING ... on=Box`. minimal and engine have no floor, so the probe has nothing to stand on |
| Host render | `MODE=render [MAP=..] tools/game.sh <walker> [scene]` | passes for all three. The host's own player spawns and stands (`PLAYER_STANDING ... local=True ... on=Box`) |
| Join | `MODE=walker tools/game.sh <walker>` (a join with `MAP` defaulting to scenemap) | passes: the client finds `facepunch.walker` offline, `PLAYER_STANDING ... local=False ... on=Box` on the server and `local=True` on the client |
| Play | `MODE=play MAP=scenes/scenemap.scene tools/game.sh <walker>` | passes: `GAME:PLAYER_MOVED owner=Player local=False dist=37` and `PLAYER_TURNED ... yaw=0->-11` on the server |

`MAP` places the map 12 units up, at the Map Loader's offset: `NoOrigin` keeps the map objects'
local positions under it, as native does.

## Fixed root causes

- **Mixed-case folders.** Resource keys are lowercase and `fs::resolve` joined them onto each root
  as is, so `Assets/Audio/Ui/*.ogg` (and `Assets/Scenes`) never resolved on Linux. Each root now
  falls back to a case-blind walk (`engine2/src/fs.rs`). tools/game/prepare.py finds the scene the
  same way.
- **The map teleported itself.** Sensors had every rapier `ActiveCollisionTypes`, so the static
  teleport trigger reported a coin resting 0.27 units inside it. The coin is a SphereCollider without
  a Rigidbody, so its body is keyframed. `TriggerTeleport` moves `other.GameObject.Root`, which
  under a MapInstance is the map itself, so the whole map moved twice by the target's offset and
  the floor left the origin. Triggers now report only pairs with a dynamic body
  (`physics/src/shape.rs`, test `static_trigger_ignores_static_and_keyframed_but_sees_dynamic`).
  Native must behave the same way, or walker's map would move when it loads. Walker's triggers act
  only on the owner, whose body is dynamic. Found with the watcher's collider probe
  (`GAME:COLLIDER`, logged when the drop trace misses).

## Open (renderer lane)

- **7 of 13 MeshComponents fail to deserialize**, on servers and host clients:
  `Error when deserializing Sandbox.MeshComponent.Mesh (ITexture was null when calling
  IsStrongHandleValid)`. `PolygonMesh.CalculateTextureSize` reads `material.FirstTexture`, and
  `Texture.FromNative` calls `IsStrongHandleValid()` on the handle, which throws on null.
  `render/material.rs` (`GET_FIRST_TEXTURE_ATTRIBUTE`, `GET_TEXTURE`) returns `Texture::NULL` for
  a material without textures, as the error material that stands in for a missing `.vmat` is. The
  meshes that fail are the ones that list a missing material: the brick walls, the door frame and
  the `TriggerHurt` volume. They have neither collision nor a mesh. Same class as the samples'
  `EnvmapProbe` error (docs/sample-games.md).
- The health HUD wraps "100" onto two lines ("10" / "0") in every client frame.

## Expected gaps

No textures or models: the Citizen, coins, road cone and door are error models, and the floor and
boxes are untextured. Neither scene has a sky. A joined client lacks walker's sounds
(`audio/ui/inventory-*.vsnd`): the game's content isn't downloaded, only its code archive.
