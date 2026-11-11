// Flat C front for GameNetworkingSockets: every Steam struct with packing rules stays on this
// side; Rust sees handles, opaque message pointers and gns_status (see src/ffi.rs).
#include <steam/steamnetworkingsockets.h>
#include <steam/isteamnetworkingutils.h>
#include <cstring>

extern "C" {

struct gns_status {
    uint32 conn;
    uint32 listen_socket;
    int32 state;
    int32 old_state;
    int32 end_reason;
    uint64 remote_steam_id;
    char end_debug[128];
    char description[128];
};

typedef void (*gns_status_fn)(const gns_status *);

static gns_status_fn g_status_sink;

static void on_status(SteamNetConnectionStatusChangedCallback_t *cb) {
    if (!g_status_sink) return;
    gns_status s{};
    s.conn = cb->m_hConn;
    s.listen_socket = cb->m_info.m_hListenSocket;
    s.state = cb->m_info.m_eState;
    s.old_state = cb->m_eOldState;
    s.end_reason = cb->m_info.m_eEndReason;
    s.remote_steam_id = cb->m_info.m_identityRemote.GetSteamID64();
    std::strncpy(s.end_debug, cb->m_info.m_szEndDebug, sizeof s.end_debug - 1);
    std::strncpy(s.description, cb->m_info.m_szConnectionDescription, sizeof s.description - 1);
    g_status_sink(&s);
}

bool gns_init(char *err, size_t err_len) {
    SteamNetworkingErrMsg msg;
    if (!GameNetworkingSockets_Init(nullptr, msg)) {
        std::strncpy(err, msg, err_len - 1);
        err[err_len - 1] = 0;
        return false;
    }
    SteamNetworkingUtils()->SetGlobalCallback_SteamNetConnectionStatusChanged(on_status);
    return true;
}

void gns_kill() { GameNetworkingSockets_Kill(); }

// Open GNS has no API for its app id (CSteamNetworkingUtils::SetAppID is internal and never called),
// so certs carry app id 0 and any peer whose cert names an app is refused (docs/interop.md). Mirror of
// that class's layout: an interface (vptr only) followed by AppId_t m_nAppID (csteamnetworkingsockets.h).
struct UtilsLayout {
    void *vptr;
    AppId_t app_id;
};

void gns_set_app_id(uint32 app_id) { reinterpret_cast<UtilsLayout *>(SteamNetworkingUtils())->app_id = app_id; }
uint32 gns_app_id() { return reinterpret_cast<UtilsLayout *>(SteamNetworkingUtils())->app_id; }

void gns_run_callbacks(gns_status_fn sink) {
    g_status_sink = sink;
    SteamNetworkingSockets()->RunCallbacks();
    g_status_sink = nullptr;
}

void gns_set_debug_output(int level, FSteamNetworkingSocketsDebugOutput f) {
    SteamNetworkingUtils()->SetDebugOutputFunction((ESteamNetworkingSocketsDebugOutputType)level, f);
}

bool gns_set_config_int(int key, int32 value) {
    return SteamNetworkingUtils()->SetGlobalConfigValueInt32((ESteamNetworkingConfigValue)key, value);
}

bool gns_set_config_string(int key, const char *value) {
    return SteamNetworkingUtils()->SetGlobalConfigValueString((ESteamNetworkingConfigValue)key, value);
}

uint32 gns_listen_ip(uint16 port) {
    SteamNetworkingIPAddr addr;
    addr.Clear();
    addr.m_port = port;
    return SteamNetworkingSockets()->CreateListenSocketIP(addr, 0, nullptr);
}

uint32 gns_listen_p2p(int virtual_port) {
    return SteamNetworkingSockets()->CreateListenSocketP2P(virtual_port, 0, nullptr);
}

bool gns_listen_address(uint32 sock, char *buf, size_t len) {
    SteamNetworkingIPAddr addr;
    if (!SteamNetworkingSockets()->GetListenSocketAddress(sock, &addr)) return false;
    addr.ToString(buf, len, true);
    return true;
}

bool gns_close_listen(uint32 sock) { return SteamNetworkingSockets()->CloseListenSocket(sock); }

bool gns_identity(char *buf, size_t len) {
    SteamNetworkingIdentity id;
    if (!SteamNetworkingSockets()->GetIdentity(&id)) return false;
    id.ToString(buf, len);
    return true;
}

uint32 gns_connect_ip(const char *address) {
    SteamNetworkingIPAddr addr;
    if (!addr.ParseString(address)) return k_HSteamNetConnection_Invalid;
    return SteamNetworkingSockets()->ConnectByIPAddress(addr, 0, nullptr);
}

int gns_accept(uint32 conn) { return SteamNetworkingSockets()->AcceptConnection(conn); }

bool gns_close(uint32 conn, int reason, const char *debug, bool linger) {
    return SteamNetworkingSockets()->CloseConnection(conn, reason, debug, linger);
}

uint32 gns_create_poll_group() { return SteamNetworkingSockets()->CreatePollGroup(); }
bool gns_destroy_poll_group(uint32 pg) { return SteamNetworkingSockets()->DestroyPollGroup(pg); }
bool gns_set_poll_group(uint32 conn, uint32 pg) { return SteamNetworkingSockets()->SetConnectionPollGroup(conn, pg); }

int gns_receive_poll_group(uint32 pg, void **out, int max) {
    return SteamNetworkingSockets()->ReceiveMessagesOnPollGroup(pg, (SteamNetworkingMessage_t **)out, max);
}

int gns_receive_connection(uint32 conn, void **out, int max) {
    return SteamNetworkingSockets()->ReceiveMessagesOnConnection(conn, (SteamNetworkingMessage_t **)out, max);
}

void *gns_allocate_message(uint32 conn, int size, int flags) {
    SteamNetworkingMessage_t *m = SteamNetworkingUtils()->AllocateMessage(size);
    if (!m) return nullptr;
    m->m_conn = conn;
    m->m_nFlags = flags;
    return m;
}

void *gns_message_data(void *msg) { return ((SteamNetworkingMessage_t *)msg)->m_pData; }
int gns_message_size(void *msg) { return ((SteamNetworkingMessage_t *)msg)->m_cbSize; }
uint32 gns_message_connection(void *msg) { return ((SteamNetworkingMessage_t *)msg)->m_conn; }
void gns_release_message(void *msg) { ((SteamNetworkingMessage_t *)msg)->Release(); }

// Takes ownership of every message (failed ones are freed too); results go to `out` if non-null.
void gns_send_messages(void **msgs, int n, int64 *out) {
    SteamNetworkingSockets()->SendMessages(n, (SteamNetworkingMessage_t **)msgs, out, true);
}

int gns_send(uint32 conn, const void *data, uint32 size, int flags) {
    return SteamNetworkingSockets()->SendMessageToConnection(conn, data, size, flags, nullptr);
}

int gns_flush(uint32 conn) { return SteamNetworkingSockets()->FlushMessagesOnConnection(conn); }

int gns_connection_state(uint32 conn) {
    SteamNetConnectionInfo_t info;
    return SteamNetworkingSockets()->GetConnectionInfo(conn, &info) ? info.m_eState : 0;
}

bool gns_connection_description(uint32 conn, char *buf, size_t len) {
    SteamNetConnectionInfo_t info;
    if (!SteamNetworkingSockets()->GetConnectionInfo(conn, &info)) return false;
    std::strncpy(buf, info.m_szConnectionDescription, len - 1);
    buf[len - 1] = 0;
    return true;
}

uint64 gns_connection_steam_id(uint32 conn) {
    SteamNetConnectionInfo_t info;
    return SteamNetworkingSockets()->GetConnectionInfo(conn, &info) ? info.m_identityRemote.GetSteamID64() : 0;
}

size_t gns_realtime_status_size() { return sizeof(SteamNetConnectionRealTimeStatus_t); }
size_t gns_message_struct_size() { return sizeof(SteamNetworkingMessage_t); }

// Fills `out`, a `size`-byte mirror of SteamNetConnectionRealTimeStatus_t (managed's drops some of
// the trailing reserved words), or zeroes it.
bool gns_realtime_status(uint32 conn, void *out, size_t size) {
    SteamNetConnectionRealTimeStatus_t s{};
    bool ok = SteamNetworkingSockets()->GetConnectionRealTimeStatus(conn, &s, 0, nullptr) == k_EResultOK;
    std::memset(out, 0, size);
    if (ok) std::memcpy(out, &s, size < sizeof s ? size : sizeof s);
    return ok;
}

}
