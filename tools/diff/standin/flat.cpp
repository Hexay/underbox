// Shim over Facepunch's libsteam_api for UNDERBOX_STANDIN_FRIENDS runs (tools/diff/standin.sh --shim).
// Managed P/Invokes the flat accessors through its own dlopen handle, so the LD_PRELOADed stand-in
// can't interpose them, and libsteam_api binds them internally to its never-initialised context.
// This lib takes libsteam_api.so's place in the run copy and needs the original (renamed); only
// these accessors are defined here, everything else resolves to the original.
#include <steam/steam_api.h>

extern "C" {

S_API void *S_CALLTYPE SteamInternal_FindOrCreateUserInterface(HSteamUser user, const char *ver);

S_API ISteamUtils *S_CALLTYPE SteamAPI_SteamUtils_v010() {
    return static_cast<ISteamUtils *>(SteamInternal_FindOrCreateUserInterface(0, STEAMUTILS_INTERFACE_VERSION));
}

S_API ISteamFriends *S_CALLTYPE SteamAPI_SteamFriends_v018() {
    return static_cast<ISteamFriends *>(SteamInternal_FindOrCreateUserInterface(0, STEAMFRIENDS_INTERFACE_VERSION));
}

}
