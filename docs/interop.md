# Interop with Facepunch's engine2

Mixed sessions over GameNetworkingSockets: a stock s&box client (Facepunch's native engine2 from the
public artifacts, docs/differential.md) against an underbox dedicated server, and the reverse.

| Direction | State | Check |
| --- | --- | --- |
| Facepunch client joins an underbox server | works: connects, loads, spawns; both sides log the player standing | `MODE=interop tools/game.sh <checkout>` |
| underbox client joins a Facepunch server | blocked: the real server needs a Steam logon before it listens | none (see below) |

Passing checkouts: `corpus/nethost` and Facepunch/sandbox (`zoo.scene`, standing on `Block`).

## Running

```sh
REAL_ENGINE2_HOST=<base url> tools/diff/fetch.sh "" game/bin/linuxsteamrt64/ game/core/ game/addons/menu/ game/addons/base/
MODE=interop tools/game.sh corpus/nethost
MODE=interop tools/game.sh ../sandbox
```

`MODE=interop` is `MODE=join` with `CLIENT_ENGINE2` set to the newest artifact root in
`$REAL_ENGINE2_CACHE` that has the addons (or `UNDERBOX_REAL_ENGINE2`/`CLIENT_ENGINE2`). It prints
`SKIP` and exits 0 when there is none. It adds one check: the client's stderr shows the stand-in's
Steam game-server context starting, so the pass really came from Facepunch's natives.

`CLIENT_ENGINE2=<root>` and `SERVER_ENGINE2=<root>` pick Facepunch's engine2 per side for any
`tools/game.sh` mode; `UNDERBOX_REAL_ENGINE2` sets both. Underneath, `tools/boot-client.sh` and
`tools/boot-server.sh` take `UNDERBOX_REAL_ENGINE2`.

## What the real client needs headless

- **Launch args:** `-nosteam` (otherwise it retries `SteamAPI_Init` forever) and `-vulkan_allow_cpu`
  (lavapipe is a CPU device).
- **Display:** an X server. Xvfb is started with `-displayfd` when `DISPLAY` is unset. SDL's
  offscreen driver has no swapchain format, so it can't be used.
- **Compiled addons:** `game/addons/menu` and `game/addons/base` from the artifacts, or
  `menu-main.scene` is missing. `tools/diff/real-game.sh` overlays them.
- **steamclient.so:** `libsteam_api` dlopens a bare `steamclient.so` before trying `~/.steam/sdk64`,
  so `LD_LIBRARY_PATH` must start with the game's `bin/linuxsteamrt64`. Without it, a real *server*
  has no networking at all ("ISteamNetworkingSockets was null").

## The Steam stand-in (`tools/diff/standin/`)

With `-nosteam` the client has no `ISteamUser` and no networking interfaces, so `+connect` does
nothing. `tools/diff/standin.sh` builds `libsteam_standin.so` against the Steamworks headers that
`steamworks-sys` vendors, and `boot-client.sh` `LD_PRELOAD`s it. The library interposes the SDK's
inline accessors (`SteamInternal_ContextInit`, `SteamInternal_FindOrCreateUserInterface`):

- **Sockets:** `SteamNetworkingSockets*`/`SteamNetworkingUtils*` come from Valve's own
  `steamclient.so`, started as an anonymous game server (`SteamInternal_GameServer_Init_V2`,
  `eServerModeNoAuthentication`, `SteamAppId=590830`). No Steam client process or login is needed.
  The wire protocol is Steam's own.
- **Auth config:** Steam's sockets default `IP_AllowWithoutAuth` to 0, and without a login the
  client then refuses to connect ("Unable to determine local identity, and auth required"). The
  stand-in sets it to 2.
- **User:** `SteamUser023` is a fixed offline identity (`76561197960265729`) whose
  `GetAuthSessionTicket` hands out a dummy ticket. Our offline server accepts it; a real server
  would not.

`libsteam_api` is loaded with local symbols, so `RTLD_NEXT` can't find the real functions; the
stand-in looks them up through `dlopen(..., RTLD_NOLOAD)`.

## Bugs this found

- **App id 0 in our certs.** Open GameNetworkingSockets never sets its app id
  (`CSteamNetworkingUtils::SetAppID` is internal and nothing calls it). Our self-signed certs
  therefore named app 0, and any peer whose cert names an app was refused: "Cert is not authorized
  for appid 0, only 590830". Every stock s&box client sends 590830. The fix is
  `gns::set_app_id`, which writes the field through a layout mirror (`crates/gns/src/shim.cpp`,
  test `app_id_reaches_certs_and_peers_still_connect`). `engine2::net` sets it from the config's
  Steam app id, defaulting to 590830.
- **Corpus org `local`.** A joining client looks the announced package up by ident, and the offline
  package record can't stand in for a `local.*` package ("Package local.nethost wasn't found!").
  Corpus games meant to be joined use another org (`corpus/nethost` uses `underbox`).
- **Leaked manifest server.** An interrupted `MODE=join` left `tools/game.sh`'s loopback HTTP
  server holding its port. The next join's client then read a stale cached manifest and failed with
  "Unable to download package". It is now killed on exit.

## Facepunch server, underbox client: what needs Steam

Booting the real dedicated server (`SERVER_ENGINE2=<root> MODE=join tools/game.sh corpus/nethost`)
gets the game loaded, but our client only ever logs "Couldn't connect, retrying". Two Steam
dependencies block it, both in managed code:

1. **Logon before listening.** `GameNetworkSystem.Static.cs` (around line 380) calls
   `ISteamGameServer::LogOnAnonymous` and waits for `BLoggedOn()`. Only after that does it create
   its `IpListenSocket` and `IdListenSocket`. If logon fails it logs "Unable to connect to Steam in
   a reasonable time!" and never listens. Anonymous logon needs no account, only a route to Steam's
   connection managers. This devbox has none: "Connection to Steam Failed: NoConnection".
2. **Ticket validation per client.** Even once it listens, `SteamConnections.cs`
   (`OnReceiveUserInfo`) calls `BeginAuthSession` with the client's ticket. It then waits up to 30 s
   for Steam's `ValidateAuthTicketResponse_t` and kicks on anything but OK. That needs a ticket
   issued to a logged-on Steam user that owns s&box. An offline underbox client can't produce one.

Faking both in a server-side stand-in (`BLoggedOn` true, a synthetic validation callback) would
test our client against a server that no longer behaves like Facepunch's, so it isn't done. To test
this direction for real, you need Steam network access plus a logged-on Steam client that owns
s&box, running on the underbox client's side (`--features steam`, docs/steam.md).

## Risk for stock clients logged on to Steam

This was not verified here, because there is no Steam login. A stock client that *is* logged on has
a Steam-signed cert and keeps `IP_AllowWithoutAuth` at 0. In GameNetworkingSockets'
`AllowRemoteUnsignedCert`, that value refuses a peer's unsigned cert ("Unsigned certs are not
allowed"). Our server's cert is self-signed, so such a client may refuse an underbox server unless
the server gets a Steam-signed cert, i.e. a Steam game-server logon.
