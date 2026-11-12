// A friends interface for the Steam stand-in (standin.cpp): an offline user with no friends, clans or
// overlay. Every call is a no-op. See standin.cpp's friends().
#include <steam/steam_api.h>

namespace {

class Friends final : public ISteamFriends {
public:
    const char *GetPersonaName() override { return "standin"; }
    EPersonaState GetPersonaState() override { return k_EPersonaStateOffline; }
    int GetFriendCount(int) override { return 0; }
    CSteamID GetFriendByIndex(int, int) override { return CSteamID(); }
    EFriendRelationship GetFriendRelationship(CSteamID) override { return k_EFriendRelationshipNone; }
    EPersonaState GetFriendPersonaState(CSteamID) override { return k_EPersonaStateOffline; }
    const char *GetFriendPersonaName(CSteamID) override { return ""; }
    bool GetFriendGamePlayed(CSteamID, FriendGameInfo_t *) override { return false; }
    const char *GetFriendPersonaNameHistory(CSteamID, int) override { return ""; }
    int GetFriendSteamLevel(CSteamID) override { return 0; }
    const char *GetPlayerNickname(CSteamID) override { return nullptr; }
    int GetFriendsGroupCount() override { return 0; }
    FriendsGroupID_t GetFriendsGroupIDByIndex(int) override { return k_FriendsGroupID_Invalid; }
    const char *GetFriendsGroupName(FriendsGroupID_t) override { return ""; }
    int GetFriendsGroupMembersCount(FriendsGroupID_t) override { return 0; }
    void GetFriendsGroupMembersList(FriendsGroupID_t, CSteamID *, int) override {}
    bool HasFriend(CSteamID, int) override { return false; }
    int GetClanCount() override { return 0; }
    CSteamID GetClanByIndex(int) override { return CSteamID(); }
    const char *GetClanName(CSteamID) override { return ""; }
    const char *GetClanTag(CSteamID) override { return ""; }
    bool GetClanActivityCounts(CSteamID, int *, int *, int *) override { return false; }
    SteamAPICall_t DownloadClanActivityCounts(CSteamID *, int) override { return k_uAPICallInvalid; }
    int GetFriendCountFromSource(CSteamID) override { return 0; }
    CSteamID GetFriendFromSourceByIndex(CSteamID, int) override { return CSteamID(); }
    bool IsUserInSource(CSteamID, CSteamID) override { return false; }
    void SetInGameVoiceSpeaking(CSteamID, bool) override {}
    void ActivateGameOverlay(const char *) override {}
    void ActivateGameOverlayToUser(const char *, CSteamID) override {}
    void ActivateGameOverlayToWebPage(const char *, EActivateGameOverlayToWebPageMode) override {}
    void ActivateGameOverlayToStore(AppId_t, EOverlayToStoreFlag) override {}
    void SetPlayedWith(CSteamID) override {}
    void ActivateGameOverlayInviteDialog(CSteamID) override {}
    int GetSmallFriendAvatar(CSteamID) override { return 0; }
    int GetMediumFriendAvatar(CSteamID) override { return 0; }
    int GetLargeFriendAvatar(CSteamID) override { return 0; }
    bool RequestUserInformation(CSteamID, bool) override { return false; }
    SteamAPICall_t RequestClanOfficerList(CSteamID) override { return k_uAPICallInvalid; }
    CSteamID GetClanOwner(CSteamID) override { return CSteamID(); }
    int GetClanOfficerCount(CSteamID) override { return 0; }
    CSteamID GetClanOfficerByIndex(CSteamID, int) override { return CSteamID(); }
    bool SetRichPresence(const char *, const char *) override { return true; }
    void ClearRichPresence() override {}
    const char *GetFriendRichPresence(CSteamID, const char *) override { return ""; }
    int GetFriendRichPresenceKeyCount(CSteamID) override { return 0; }
    const char *GetFriendRichPresenceKeyByIndex(CSteamID, int) override { return ""; }
    void RequestFriendRichPresence(CSteamID) override {}
    bool InviteUserToGame(CSteamID, const char *) override { return false; }
    int GetCoplayFriendCount() override { return 0; }
    CSteamID GetCoplayFriend(int) override { return CSteamID(); }
    int GetFriendCoplayTime(CSteamID) override { return 0; }
    AppId_t GetFriendCoplayGame(CSteamID) override { return 0; }
    SteamAPICall_t JoinClanChatRoom(CSteamID) override { return k_uAPICallInvalid; }
    bool LeaveClanChatRoom(CSteamID) override { return false; }
    int GetClanChatMemberCount(CSteamID) override { return 0; }
    CSteamID GetChatMemberByIndex(CSteamID, int) override { return CSteamID(); }
    bool SendClanChatMessage(CSteamID, const char *) override { return false; }
    int GetClanChatMessage(CSteamID, int, void *, int, EChatEntryType *, CSteamID *) override { return 0; }
    bool IsClanChatAdmin(CSteamID, CSteamID) override { return false; }
    bool IsClanChatWindowOpenInSteam(CSteamID) override { return false; }
    bool OpenClanChatWindowInSteam(CSteamID) override { return false; }
    bool CloseClanChatWindowInSteam(CSteamID) override { return false; }
    bool SetListenForFriendsMessages(bool) override { return false; }
    bool ReplyToFriendMessage(CSteamID, const char *) override { return false; }
    int GetFriendMessage(CSteamID, int, void *, int, EChatEntryType *) override { return 0; }
    SteamAPICall_t GetFollowerCount(CSteamID) override { return k_uAPICallInvalid; }
    SteamAPICall_t IsFollowing(CSteamID) override { return k_uAPICallInvalid; }
    SteamAPICall_t EnumerateFollowingList(uint32) override { return k_uAPICallInvalid; }
    bool IsClanPublic(CSteamID) override { return false; }
    bool IsClanOfficialGameGroup(CSteamID) override { return false; }
    int GetNumChatsWithUnreadPriorityMessages() override { return 0; }
    void ActivateGameOverlayRemotePlayTogetherInviteDialog(CSteamID) override {}
    bool RegisterProtocolInOverlayBrowser(const char *) override { return false; }
    void ActivateGameOverlayInviteDialogConnectString(const char *) override {}
    SteamAPICall_t RequestEquippedProfileItems(CSteamID) override { return k_uAPICallInvalid; }
    bool BHasEquippedProfileItem(CSteamID, ECommunityProfileItemType) override { return false; }
    const char *GetProfileItemPropertyString(CSteamID, ECommunityProfileItemType, ECommunityProfileItemProperty) override { return ""; }
    uint32 GetProfileItemPropertyUint(CSteamID, ECommunityProfileItemType, ECommunityProfileItemProperty) override { return 0; }
};

Friends g_friends;

}

ISteamFriends *standin_friends() { return &g_friends; }
