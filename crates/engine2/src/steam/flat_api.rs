//! The slice of Valve's flat `steam_api` that managed `[DllImport("steam_api64")]`s directly
//! (`Platform/Steam/Generated`), answered for the offline platform: no Steam client, so every
//! interface accessor is null and every method on those interfaces returns zero (no friends, no
//! lobbies, not on a Steam Deck). tools/install-engine2.sh links `libsteam_api.so` to libengine2.
//! With the `steam` feature the real libsteam_api provides these instead.
//!
//! Gotcha: one zero-returning signature serves them all. That is sound on the C ABI because the
//! caller owns its arguments and every import returns an integer, bool, pointer or 8-byte struct
//! in a register (no floats, no sret); a new import returning either needs its own function.
//! The value helpers (`SteamNetworkingIdentity_*`, `servernetadr_t_*`) do real work and are not here.

macro_rules! zero {
    ($($name:ident),* $(,)?) => {$(
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() -> u64 {
            0
        }
    )*};
}

zero!(SteamAPI_SteamFriends_v018, SteamAPI_SteamMatchmaking_v009, SteamAPI_SteamUtils_v010, SteamAPI_SteamGameServerUtils_v010);

zero!(
    SteamAPI_ISteamFriends_ActivateGameOverlay, SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialog,
    SteamAPI_ISteamFriends_ActivateGameOverlayInviteDialogConnectString, SteamAPI_ISteamFriends_ActivateGameOverlayRemotePlayTogetherInviteDialog,
    SteamAPI_ISteamFriends_ActivateGameOverlayToStore, SteamAPI_ISteamFriends_ActivateGameOverlayToUser,
    SteamAPI_ISteamFriends_ActivateGameOverlayToWebPage, SteamAPI_ISteamFriends_ClearRichPresence,
    SteamAPI_ISteamFriends_CloseClanChatWindowInSteam, SteamAPI_ISteamFriends_DownloadClanActivityCounts,
    SteamAPI_ISteamFriends_EnumerateFollowingList, SteamAPI_ISteamFriends_GetChatMemberByIndex,
    SteamAPI_ISteamFriends_GetClanActivityCounts, SteamAPI_ISteamFriends_GetClanByIndex, SteamAPI_ISteamFriends_GetClanChatMemberCount,
    SteamAPI_ISteamFriends_GetClanChatMessage, SteamAPI_ISteamFriends_GetClanCount, SteamAPI_ISteamFriends_GetClanName,
    SteamAPI_ISteamFriends_GetClanOfficerByIndex, SteamAPI_ISteamFriends_GetClanOfficerCount, SteamAPI_ISteamFriends_GetClanOwner,
    SteamAPI_ISteamFriends_GetClanTag, SteamAPI_ISteamFriends_GetCoplayFriend, SteamAPI_ISteamFriends_GetCoplayFriendCount,
    SteamAPI_ISteamFriends_GetFollowerCount, SteamAPI_ISteamFriends_GetFriendByIndex, SteamAPI_ISteamFriends_GetFriendCoplayGame,
    SteamAPI_ISteamFriends_GetFriendCoplayTime, SteamAPI_ISteamFriends_GetFriendCount, SteamAPI_ISteamFriends_GetFriendCountFromSource,
    SteamAPI_ISteamFriends_GetFriendFromSourceByIndex, SteamAPI_ISteamFriends_GetFriendGamePlayed, SteamAPI_ISteamFriends_GetFriendMessage,
    SteamAPI_ISteamFriends_GetFriendPersonaName, SteamAPI_ISteamFriends_GetFriendPersonaNameHistory,
    SteamAPI_ISteamFriends_GetFriendPersonaState, SteamAPI_ISteamFriends_GetFriendRelationship, SteamAPI_ISteamFriends_GetFriendRichPresence,
    SteamAPI_ISteamFriends_GetFriendRichPresenceKeyByIndex, SteamAPI_ISteamFriends_GetFriendRichPresenceKeyCount,
    SteamAPI_ISteamFriends_GetFriendSteamLevel, SteamAPI_ISteamFriends_GetFriendsGroupCount, SteamAPI_ISteamFriends_GetFriendsGroupIDByIndex,
    SteamAPI_ISteamFriends_GetFriendsGroupMembersCount, SteamAPI_ISteamFriends_GetFriendsGroupMembersList,
    SteamAPI_ISteamFriends_GetFriendsGroupName, SteamAPI_ISteamFriends_GetLargeFriendAvatar, SteamAPI_ISteamFriends_GetMediumFriendAvatar,
    SteamAPI_ISteamFriends_GetNumChatsWithUnreadPriorityMessages, SteamAPI_ISteamFriends_GetPersonaName,
    SteamAPI_ISteamFriends_GetPersonaState, SteamAPI_ISteamFriends_GetPlayerNickname, SteamAPI_ISteamFriends_GetSmallFriendAvatar,
    SteamAPI_ISteamFriends_HasFriend, SteamAPI_ISteamFriends_InviteUserToGame, SteamAPI_ISteamFriends_IsClanChatAdmin,
    SteamAPI_ISteamFriends_IsClanChatWindowOpenInSteam, SteamAPI_ISteamFriends_IsClanOfficialGameGroup, SteamAPI_ISteamFriends_IsClanPublic,
    SteamAPI_ISteamFriends_IsFollowing, SteamAPI_ISteamFriends_IsUserInSource, SteamAPI_ISteamFriends_JoinClanChatRoom,
    SteamAPI_ISteamFriends_LeaveClanChatRoom, SteamAPI_ISteamFriends_OpenClanChatWindowInSteam,
    SteamAPI_ISteamFriends_RegisterProtocolInOverlayBrowser, SteamAPI_ISteamFriends_ReplyToFriendMessage,
    SteamAPI_ISteamFriends_RequestClanOfficerList, SteamAPI_ISteamFriends_RequestFriendRichPresence,
    SteamAPI_ISteamFriends_RequestUserInformation, SteamAPI_ISteamFriends_SendClanChatMessage, SteamAPI_ISteamFriends_SetInGameVoiceSpeaking,
    SteamAPI_ISteamFriends_SetListenForFriendsMessages, SteamAPI_ISteamFriends_SetPlayedWith, SteamAPI_ISteamFriends_SetRichPresence,
);

zero!(
    SteamAPI_ISteamMatchmaking_AddFavoriteGame, SteamAPI_ISteamMatchmaking_AddRequestLobbyListCompatibleMembersFilter,
    SteamAPI_ISteamMatchmaking_AddRequestLobbyListDistanceFilter, SteamAPI_ISteamMatchmaking_AddRequestLobbyListFilterSlotsAvailable,
    SteamAPI_ISteamMatchmaking_AddRequestLobbyListNearValueFilter, SteamAPI_ISteamMatchmaking_AddRequestLobbyListNumericalFilter,
    SteamAPI_ISteamMatchmaking_AddRequestLobbyListResultCountFilter, SteamAPI_ISteamMatchmaking_AddRequestLobbyListStringFilter,
    SteamAPI_ISteamMatchmaking_CreateLobby, SteamAPI_ISteamMatchmaking_DeleteLobbyData, SteamAPI_ISteamMatchmaking_GetFavoriteGame,
    SteamAPI_ISteamMatchmaking_GetFavoriteGameCount, SteamAPI_ISteamMatchmaking_GetLobbyByIndex, SteamAPI_ISteamMatchmaking_GetLobbyChatEntry,
    SteamAPI_ISteamMatchmaking_GetLobbyData, SteamAPI_ISteamMatchmaking_GetLobbyDataByIndex, SteamAPI_ISteamMatchmaking_GetLobbyDataCount,
    SteamAPI_ISteamMatchmaking_GetLobbyGameServer, SteamAPI_ISteamMatchmaking_GetLobbyMemberByIndex,
    SteamAPI_ISteamMatchmaking_GetLobbyMemberData, SteamAPI_ISteamMatchmaking_GetLobbyMemberLimit, SteamAPI_ISteamMatchmaking_GetLobbyOwner,
    SteamAPI_ISteamMatchmaking_GetNumLobbyMembers, SteamAPI_ISteamMatchmaking_InviteUserToLobby, SteamAPI_ISteamMatchmaking_JoinLobby,
    SteamAPI_ISteamMatchmaking_LeaveLobby, SteamAPI_ISteamMatchmaking_RemoveFavoriteGame, SteamAPI_ISteamMatchmaking_RequestLobbyData,
    SteamAPI_ISteamMatchmaking_RequestLobbyList, SteamAPI_ISteamMatchmaking_SendLobbyChatMsg, SteamAPI_ISteamMatchmaking_SetLinkedLobby,
    SteamAPI_ISteamMatchmaking_SetLobbyData, SteamAPI_ISteamMatchmaking_SetLobbyGameServer, SteamAPI_ISteamMatchmaking_SetLobbyJoinable,
    SteamAPI_ISteamMatchmaking_SetLobbyMemberData, SteamAPI_ISteamMatchmaking_SetLobbyMemberLimit, SteamAPI_ISteamMatchmaking_SetLobbyOwner,
    SteamAPI_ISteamMatchmaking_SetLobbyType,
);

zero!(
    SteamAPI_ISteamUtils_GetAPICallResult, SteamAPI_ISteamUtils_GetAppID, SteamAPI_ISteamUtils_GetConnectedUniverse,
    SteamAPI_ISteamUtils_GetCurrentBatteryPower, SteamAPI_ISteamUtils_GetIPCountry, SteamAPI_ISteamUtils_GetImageRGBA,
    SteamAPI_ISteamUtils_GetImageSize, SteamAPI_ISteamUtils_GetSecondsSinceAppActive, SteamAPI_ISteamUtils_GetSecondsSinceComputerActive,
    SteamAPI_ISteamUtils_GetServerRealTime, SteamAPI_ISteamUtils_IsAPICallCompleted, SteamAPI_ISteamUtils_IsSteamRunningOnSteamDeck,
    SteamAPI_ISteamUtils_SetOverlayNotificationPosition,
);
