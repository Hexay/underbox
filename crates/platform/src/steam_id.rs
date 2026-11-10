//! Steam's 64-bit `CSteamID`: universe (8 bits), account type (4), instance (20), account (32).

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SteamId(pub u64);

const UNIVERSE_PUBLIC: u64 = 1;
const TYPE_INDIVIDUAL: u64 = 1;
const TYPE_ANON_GAME_SERVER: u64 = 4;
/// A user account's instance (desktop).
const INSTANCE_DESKTOP: u64 = 1;

impl SteamId {
    pub fn individual(account: u32) -> Self {
        Self(UNIVERSE_PUBLIC << 56 | TYPE_INDIVIDUAL << 52 | INSTANCE_DESKTOP << 32 | account as u64)
    }

    pub fn anonymous_game_server(account: u32) -> Self {
        Self(UNIVERSE_PUBLIC << 56 | TYPE_ANON_GAME_SERVER << 52 | account as u64)
    }

    pub fn account_type(self) -> u64 {
        self.0 >> 52 & 0xF
    }

    pub fn account(self) -> u32 {
        self.0 as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_fields() {
        let id = SteamId::anonymous_game_server(1234);
        assert_eq!((id.0 >> 56, id.account_type(), id.account()), (1, 4, 1234));
        assert_eq!(id.0, 0x0140_0000_0000_04D2);
    }
}
