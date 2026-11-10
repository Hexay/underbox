//! Compiled maps: the VPK a map ships in, and the world (`.vwrld_c`), world node (`.vwnod_c`) and
//! entity lump (`.vents_c`) resources inside it. Format sources and coverage: research/world.md.

pub mod entities;
pub mod fixture;
pub mod map;
pub mod node;
pub mod resource;
#[cfg(test)]
mod tests;
mod values;
pub mod vpk;

pub use map::Map;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Vpk(vpk::Error),
    Container(assets::container::ReadError),
    Kv3(assets::kv3bin::Error),
    /// A file the map names isn't there.
    Missing(String),
    /// A resource decoded but doesn't have the shape its type needs.
    Format(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Vpk(e) => write!(f, "{e}"),
            Error::Container(e) => write!(f, "resource container: {e:?}"),
            Error::Kv3(e) => write!(f, "{e}"),
            Error::Missing(p) => write!(f, "{p} is missing"),
            Error::Format(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<vpk::Error> for Error {
    fn from(e: vpk::Error) -> Self {
        Error::Vpk(e)
    }
}
