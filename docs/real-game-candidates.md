# Real-game candidates

More MIT-licensed s&box games on GitHub, for the user to approve (docs/real-games.md). Nothing was
cloned or downloaded. The data comes from the GitHub API on 2026-11-11: licence, the default
branch's last commit, and file counts from its recursive tree. `_c` counts compiled files.
vmdl/vmat/img/snd counts source assets (img: png/tga/jpg; snd: wav/mp3/ogg/.sound). "Scene" means the
current scene system: a `.sbproj`, `.scene`/`.prefab` files and no `.addon`.

| Repo | Licence | Last commit | `_c` | vmdl/vmat/img/snd | API | Notes |
|---|---|---|---|---|---|---|
| Facepunch/sbox-walker | MIT | 2026-01-29 | 0 | 0/0/0/6 | Scene | Running: docs/walker.md |
| timmybo5/simple-weapon-base | MIT | 2026-11-10 | 3 | 27/30/146/109 | Scene | FPS weapon base and demo. Sets `DedicatedServerStartupScene`. Active |
| Titanovsky/Driving-Drunk | MIT | 2026-07-01 | 0 | 0/5/5/6 | Scene | Small vehicle game, MaxPlayers 8. Sets `DedicatedServerStartupScene` |
| Facepunch/sbox-spritetools | MIT | 2026-01-07 | 0 | 3/5/42/0 | Scene | 2D sprite library and demo game: the sprite path |
| Facepunch/sbox-gpu-voxels | MIT | 2026-06-13 | 5 | 1/2/20/0 | Scene | GPU voxel demo: compute shaders |
| CarsonKompon/sbox-scenebox-2 | MIT | 2024-09-28 | 1 | 0/3/66/71 | Scene | Menu and game scenes. Heavy on UI, sound and prefabs |
| Nolankicks/sbox-OpenWorld | MIT | 2024-12-06 | 0 | 17/13/56/8 | Scene | Open world. Bundles Facepunch's libsdf |
| Small-Fish-Dev/blubber_runner | MIT | 2024-11-29 | 0 | 3/9/1/4 | Scene | Tiny jam game |
| openframeworkRP/core | MIT | 2026-05-04 | 0 | 38/128/1048/578 | Scene | Large roleplay gamemode. Main `.sbproj` not located (unverified) |
| Facepunch/sbox-tabletennis-vr | MIT | 2025-04-15 | 0 | 18/20/73/11 | Scene | VR, 2 players. Needs OpenXR |
| ShadowBrian/sbox-scene-VRcontroller | MIT | 2024-08-06 | 0 | 0/0/0/0 | Scene | VR controller test, 20 scenes |
| shishkabob27/hls2 | MIT | 2023-08-30 | 93 | 213/718/1454/2573 | legacy | Half-Life remake. Its HL1-derived content may not be the author's to license, whatever the repo's licence says |
| Facepunch/sbox-spire | MIT | 2022-12-06 | 73 | 3/3/18/12 | legacy (`.addon`, archived) | Old entity API only |

Every `.sbproj` read has empty `PackageReferences`. Scenes may still name cloud models, which this
survey didn't check. Few commit compiled assets: simple-weapon-base (3), gpu-voxels (5) and
scenebox-2 (1). None would give a fully textured frame.

Excluded: Facepunch/sbox-hc1, sbox-jumpgame, sbox-bombroyale, sbox-battlebugs and
sbox-scenestaging have no licence. TTTReborn/tttreborn is GPL-3.0. wiremod/wirebox is an addon,
sbox-sdf and sbox-juicebox are libraries, and Advisor and sandboxplusplus are legacy addons.
