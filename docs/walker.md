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
| Join | `MODE=walker tools/game.sh <walker>` (a join with `MAP` defaulting to scenemap) | passes with the map pinned (below): the client finds `facepunch.walker` offline, `PLAYER_STANDING ... local=False ... on=Box` on the server and `local=True` on the client |
| Play | `MODE=play MAP=scenes/scenemap.scene tools/game.sh <walker>` | passes: `GAME:PLAYER_MOVED owner=Player local=False dist=37` and `PLAYER_TURNED ... yaw=0->-11` on the server |

`MAP` places the map 12 units up, at the Map Loader's offset: `NoOrigin` keeps the map objects'
local positions under it, as native does.

## Fixed root causes

- **Mixed-case folders.** Resource keys are lowercase and `fs::resolve` joined them onto each root
  as is, so `Assets/Audio/Ui/*.ogg` (and `Assets/Scenes`) never resolved on Linux. Each root now
  falls back to a case-blind walk (`engine2/src/fs.rs`). tools/game/prepare.py finds the scene the
  same way.
- **The map teleports itself, on real too.** The static teleport trigger overlaps two coins, whose
  SphereColliders without a Rigidbody are keyframed. Real reports every trigger pair but
  static-static (probe `TRIGGER`, docs/differential-tap.md "Fixed"), and so does ours
  (`physics/src/shape.rs`). `TriggerTeleport` moves `other.GameObject.Root`, which under a
  MapInstance is the Map Loader, so the server's map moves twice by the target's offset at tick 1
  (real's tap shows the same `IPhysicsBody_Set*` calls). A joining client loads the moved map from
  the snapshot, and its own trigger moves it twice more (the same managed code runs there), so the
  host's spawn point puts the client's player in the air. Walker ships vpk maps without coins
  (`MapList`: flatgrass), so it never meets this. Runs with `MAP` pass `--pin-map` to
  tools/game/prepare.py, which keeps the scratch copy's `TriggerTeleport` off a MapInstance root.
  Found with the watcher's collider probe (`GAME:COLLIDER`, logged when the drop trace misses and
  on a joining client). As the startup scene, the coin is its own root and only it moves
  (docs/differential-state.md, 1).
- **Meshes with a missing material didn't load** (7 of 13: the brick walls, the door frame, the
  `TriggerHurt` volume). `PolygonMesh.CalculateTextureSize` reads `Material.FirstTexture`, whose
  `Texture.FromNative` calls `IsStrongHandleValid()` on the returned pointer and throws on null.
  The error material has no textures, and the getter returned null. Texture getters now return an
  empty strong handle (`resource::empty_handle`), which managed reads as no texture. All meshes
  load with collision (`meshes=12 colliders=45`), and no client logs the error.

## Expected gaps

The health HUD wraps "100" onto two lines ("10" / "0"). That's missing content, not a layout bug:
`.health` is `width: 100px` in `font-family: Poppins` at 64px, Poppins ships with s&box's Steam
content (`Font 'Poppins' not found`), and the system fallback, DejaVu Sans, sets "100" 122px wide
(Liberation Sans: 107px). RichTextKit breaks a word that overflows the line at a character, as in
docs/real-games.md's icon-font case.


No textures or models: the Citizen, coins, road cone and door are error models, and the floor and
boxes are untextured. Neither scene has a sky. A joined client lacks walker's sounds
(`audio/ui/inventory-*.vsnd`): the game's content isn't downloaded, only its code archive.
