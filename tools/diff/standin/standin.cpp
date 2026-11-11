// Steam stand-in for Facepunch's engine2 client on a host without a Steam client: see docs/interop.md.
// LD_PRELOADed over the real libsteam_api.so. Steam's own networking sockets come from its
// steamclient.so started as an anonymous game server (no Steam client needed, as on a dedicated
// server); the user is a fixed offline identity whose auth ticket only an offline server accepts.
#include <steam/steam_api.h>
#include <steam/steam_gameserver.h>
#include <dlfcn.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace {

// By handle, not RTLD_NEXT: libengine2 pulls libsteam_api in locally, outside the global scope.
template <typename F> F real(const char *name) {
    static void *lib = dlopen("libsteam_api.so", RTLD_NOW | RTLD_NOLOAD);
    void *p = lib ? dlsym(lib, name) : nullptr;
    if (!p) std::fprintf(stderr, "standin: no %s in libsteam_api\n", name);
    return reinterpret_cast<F>(p);
}

const char TICKET[] = "underbox-standin-ticket";
const uint64 USER_ID = 76561197960265729ull;

class User final : public ISteamUser {
public:
    HSteamUser GetHSteamUser() override { return 1; }
    bool BLoggedOn() override { return false; }
    CSteamID GetSteamID() override { return CSteamID(USER_ID); }
    int InitiateGameConnection_DEPRECATED(void *, int, CSteamID, uint32, uint16, bool) override { return 0; }
    void TerminateGameConnection_DEPRECATED(uint32, uint16) override {}
    void TrackAppUsageEvent(CGameID, int, const char *) override {}
    bool GetUserDataFolder(char *, int) override { return false; }
    void StartVoiceRecording() override {}
    void StopVoiceRecording() override {}
    EVoiceResult GetAvailableVoice(uint32 *c, uint32 *u, uint32) override {
        if (c) *c = 0;
        if (u) *u = 0;
        return k_EVoiceResultNoData;
    }
    EVoiceResult GetVoice(bool, void *, uint32, uint32 *n, bool, void *, uint32, uint32 *un, uint32) override {
        if (n) *n = 0;
        if (un) *un = 0;
        return k_EVoiceResultNoData;
    }
    EVoiceResult DecompressVoice(const void *, uint32, void *, uint32, uint32 *n, uint32) override {
        if (n) *n = 0;
        return k_EVoiceResultNoData;
    }
    uint32 GetVoiceOptimalSampleRate() override { return 24000; }
    HAuthTicket GetAuthSessionTicket(void *out, int max, uint32 *len, const SteamNetworkingIdentity *) override {
        if (!out || max < (int)sizeof TICKET) {
            if (len) *len = 0;
            return k_HAuthTicketInvalid;
        }
        std::memcpy(out, TICKET, sizeof TICKET);
        if (len) *len = sizeof TICKET;
        std::fprintf(stderr, "standin: handed out the offline auth ticket\n");
        return 1;
    }
    HAuthTicket GetAuthTicketForWebApi(const char *) override { return k_HAuthTicketInvalid; }
    EBeginAuthSessionResult BeginAuthSession(const void *, int, CSteamID) override { return k_EBeginAuthSessionResultInvalidTicket; }
    void EndAuthSession(CSteamID) override {}
    void CancelAuthTicket(HAuthTicket) override {}
    EUserHasLicenseForAppResult UserHasLicenseForApp(CSteamID, AppId_t) override { return k_EUserHasLicenseResultHasLicense; }
    bool BIsBehindNAT() override { return false; }
    void AdvertiseGame(CSteamID, uint32, uint16) override {}
    SteamAPICall_t RequestEncryptedAppTicket(void *, int) override { return k_uAPICallInvalid; }
    bool GetEncryptedAppTicket(void *, int, uint32 *len) override {
        if (len) *len = 0;
        return false;
    }
    int GetGameBadgeLevel(int, bool) override { return 0; }
    int GetPlayerSteamLevel() override { return 0; }
    SteamAPICall_t RequestStoreAuthURL(const char *) override { return k_uAPICallInvalid; }
    bool BIsPhoneVerified() override { return false; }
    bool BIsTwoFactorEnabled() override { return false; }
    bool BIsPhoneIdentifying() override { return false; }
    bool BIsPhoneRequiringVerification() override { return false; }
    SteamAPICall_t GetMarketEligibility() override { return k_uAPICallInvalid; }
    SteamAPICall_t GetDurationControl() override { return k_uAPICallInvalid; }
    bool BSetDurationControlOnlineState(EDurationControlOnlineState) override { return false; }
};

User g_user;
// Bumped when the game server starts, so cached interface accessors refill (SteamInternal_ContextInit).
uintptr_t g_counter = 1;
bool g_server;

bool server() {
    static bool tried;
    if (!tried) {
        tried = true;
        // s&box's (managed Application.AppId); a client never sets it, a game server must.
        setenv("SteamAppId", "590830", 0);
        SteamErrMsg err{};
        auto init = real<decltype(&SteamInternal_GameServer_Init_V2)>("SteamInternal_GameServer_Init_V2");
        const char *versions = STEAMUTILS_INTERFACE_VERSION "\0" STEAMNETWORKINGUTILS_INTERFACE_VERSION
            "\0" STEAMNETWORKINGSOCKETS_INTERFACE_VERSION "\0";
        g_server = init && init(0, 0, STEAMGAMESERVER_QUERY_PORT_SHARED, eServerModeNoAuthentication, "1.0.0.0",
                                versions, &err) == k_ESteamAPIInitResult_OK;
        std::fprintf(stderr, "standin: Steam game-server context %s %s\n", g_server ? "up" : "failed:", err);
        g_counter++;
    }
    return g_server;
}

// Steam's sockets default IP_AllowWithoutAuth to 0 (open GNS: 2). A Steam-logged-on client has a signed
// identity; this one has none, so without it every IP connect fails "Unable to determine local identity".
void allow_without_auth(ISteamNetworkingUtils *utils) {
    int32 v = -1;
    size_t len = sizeof v;
    ESteamNetworkingConfigDataType type;
    utils->GetConfigValue(k_ESteamNetworkingConfig_IP_AllowWithoutAuth, k_ESteamNetworkingConfig_Global, 0, &type, &v, &len);
    if (v >= 2) return;
    std::fprintf(stderr, "standin: IP_AllowWithoutAuth was %d, set to 2\n", v);
    utils->SetGlobalConfigValueInt32(k_ESteamNetworkingConfig_IP_AllowWithoutAuth, 2);
}

bool networking(const char *ver) {
    return !std::strncmp(ver, "SteamNetworkingSockets", 22) || !std::strncmp(ver, "SteamNetworkingUtils", 20);
}

}

extern "C" {

// The engine prefers SteamGameServerNetworkingSockets() when it isn't null and pumps the game-server
// pipe itself, so once server() is up its own accessors find Steam's sockets with no help here.
S_API void *S_CALLTYPE SteamInternal_FindOrCreateUserInterface(HSteamUser user, const char *ver) {
    if (getenv("UNDERBOX_STANDIN_LOG")) std::fprintf(stderr, "standin: user %d %s\n", user, ver);
    if (!std::strcmp(ver, STEAMUSER_INTERFACE_VERSION)) return &g_user;
    if (networking(ver)) {
        if (!server()) return nullptr;
        static auto gs = real<decltype(&SteamInternal_FindOrCreateGameServerInterface)>("SteamInternal_FindOrCreateGameServerInterface");
        static auto gs_user = real<decltype(&SteamGameServer_GetHSteamUser)>("SteamGameServer_GetHSteamUser");
        void *p = gs(gs_user(), ver);
        std::fprintf(stderr, "standin: %s from the game-server context: %p\n", ver, p);
        if (p && !std::strcmp(ver, STEAMNETWORKINGUTILS_INTERFACE_VERSION)) allow_without_auth(static_cast<ISteamNetworkingUtils *>(p));
        return p;
    }
    static auto next = real<decltype(&SteamInternal_FindOrCreateUserInterface)>("SteamInternal_FindOrCreateUserInterface");
    return next(user, ver);
}

// struct { void (*fill)(void *ctx); uintptr_t counter; void *ctx; } (steam_api_internal.h).
// The real one never refills while SteamAPI_Init hasn't succeeded, which -nosteam guarantees.
S_API void *S_CALLTYPE SteamInternal_ContextInit(void *data) {
    auto slots = static_cast<uintptr_t *>(data);
    if (slots[1] != g_counter) {
        slots[1] = g_counter;
        reinterpret_cast<void (*)(void *)>(slots[0])(&slots[2]);
    }
    return &slots[2];
}

}
