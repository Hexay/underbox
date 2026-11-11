# Steam services

`platform` has two backends: `Offline` (default) and `Steam` (`--features steam`, steamworks-sys flat
API). engine2's `steam/` binds managed's native Steam surface over whichever is installed. Null accessors
mean "not available"; managed checks them.

## Coverage

| Service | Offline | Steam |
|---|---|---|
| Game server logon, server-browser info, auth sessions | accepts any ticket | real (`ISteamGameServer`) |
| Server's own tickets (`GetAuthSessionTicket`) | empty | real |
| User, friends, apps, cloud files, text filter | stub user / null | real |
| Web API tickets (`EngineGlue.*WebAuthTicket`) | none | real; identity from `UNDERBOX_WEB_API_IDENTITY` (unset = none) |
| Workshop queries, details, votes | null query/vote | real; every managed filter and sort |
| Subscribe, download, installed items, playtime | empty | real (client and game server) |
| Publish / update (`CUgcUpdate`) | create fails, submit fails `NotLoggedOn` | real; all 17 edits, progress |
| Lobbies (`ISteamMatchmaking` slots + LobbyManager callbacks) | null | real; managed calls Create/Join/List/Chat itself via DllImport |
| Peer messages, networking utils | null | real (`ISteamNetworkingMessages`, relay init, ping) |
| Server list (internet, LAN) | starts and finishes empty | real (`ISteamMatchmakingServers`) |
| Inventory, prices, checkout | empty | real (client); `Deserialize`/owner checks on a game server |
| Screenshots | false | real (`ISteamScreenshots::WriteScreenshot`) |
| HTML surface | null | null (see below) |

## Mechanics

- Both pipes use manual dispatch (`platform::steam::pump`): our call results go to their handlers, every other
  callback is forwarded raw to managed's `Steamworks.Dispatch`, which decodes it. Server callbacks we decode
  (logon, auth result) become `Event`s instead. steamworks-rs's own pump is never run: it swallows call
  results it didn't make.
- Callback structs are read with `offset_of!` + `read_unaligned`; flat functions returning enums are
  redeclared with `i32` (`steam/ffi.rs`), since an out-of-range value in a Rust enum is UB.
- The client starts at `SourceEnginePreInit` (`app/platform.rs`), before managed's `SteamClient.Init`, and
  sets `SteamAppId` from the app dict if unset.
- Completions check the managed-facing object is still the one they were made for (`ugc_query::completed`).

## Joining a server

`connect <ip>` needs only `ISteamNetworkingSockets` (our GameNetworkingSockets). `connect <steamid>` goes
through `Glue.ConnectToSteamId`, which `net` can't serve (open-source GNS has no Steam relay), so it fails
on either backend. Lobby games instead carry traffic over `ISteamNetworkingMessages`, which the Steam
backend provides. Matchmaking, peer messages and FakeIP are not on the dedicated-server join path.

Auth is: the client sends `ISteamUser::GetAuthSessionTicket`'s ticket (an empty one makes it drop itself,
"Invalid Authentication Ticket"); the server runs `BeginAuthSession` and waits up to 30 s for
`ValidateAuthTicketResponse_t`. So a stock s&box client joining an underbox server needs the server on the
Steam backend; an offline server accepts any ticket, and an offline client's stub ticket is only accepted
by an offline server.

## HTML surface: unavailable

`ISteamHTMLSurface` renders through Steam's `steamwebhelper` (CEF) process, owned by the running Steam
client, and hands back BGRA frames plus ~30 callback kinds (paint, cursor, popups, JS dialogs, file
dialogs). It can't work without a Steam client, and wiring it means a texture upload path in `render` and
input routing, outside this lane. Managed already treats a null surface as "unavailable" (WebSurface logs
and returns), so it stays null. A future web surface should be a separate CEF/Ultralight backend, not Steam.

## Manual checks (need a logged-in Steam account)

Nothing here is tested against live Steam; unit tests cover query planning, result mapping, callback
decoding, dispatch layout and offline behaviour. With `--features steam` and Steam running:

1. Client starts: `SteamClient.Init` succeeds, persona name and Steam id are the account's.
2. Dedicated server with a GSLT logs on (`SteamServersConnected`), shows in the internet server list.
3. A stock s&box client joins it by IP; the server's auth completes `OK`; a bad ticket is kicked.
4. Workshop: browse (popular, recent, search text, tags), open an item, vote up and read it back.
5. Subscribe, download, see it in installed items with a folder; unsubscribe.
6. Publish a new item and update it (title, tags, preview, content); progress reaches 100.
7. Lobbies: create, see it in the list, join from a second account, chat, set/read data, leave.
8. Server list LAN query finds a local server.
9. Inventory: items load, definitions and prices appear, a checkout opens the overlay and reports.
10. Screenshot (F12 path) lands in the Steam screenshot library.
11. Web ticket: with `UNDERBOX_WEB_API_IDENTITY` set, `GetWebAuthTicket` returns hex after a few frames.
