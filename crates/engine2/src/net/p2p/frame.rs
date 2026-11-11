//! The P2P transport's datagrams. Every frame starts `'U', VERSION, kind`; integers are little-endian.
//! `dst` is always the receiver's own connection handle, so each side routes by its local id.

pub const VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// Open a connection to `vport`; `src` is the caller's handle. Resent until answered.
    Connect { src: u32, vport: i32 },
    /// The listener took it: `src` is the listener's handle for the connection.
    Accept { dst: u32, src: u32 },
    /// The sender closed (or refused) the connection.
    Close { dst: u32, reason: i32, debug: String },
    Unreliable { dst: u32, payload: Vec<u8> },
    /// One segment of the reliable stream; `last` ends a message.
    Reliable { dst: u32, seq: u32, last: bool, payload: Vec<u8> },
    /// Every segment before `next` arrived, plus `next + 1 + i` for each set bit i of `mask`.
    Ack { dst: u32, next: u32, mask: u64 },
    /// Keeps an idle connection from timing out.
    Ping { dst: u32 },
}

mod kind {
    pub const CONNECT: u8 = 1;
    pub const ACCEPT: u8 = 2;
    pub const CLOSE: u8 = 3;
    pub const UNRELIABLE: u8 = 4;
    pub const RELIABLE: u8 = 5;
    pub const RELIABLE_LAST: u8 = 6;
    pub const ACK: u8 = 7;
    pub const PING: u8 = 8;
}

/// Longest close reason kept (Steam's `m_szEndDebug` is 128 bytes with its NUL).
const MAX_DEBUG: usize = 127;

impl Frame {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(16);
        let head = |b: &mut Vec<u8>, k: u8, dst: u32| {
            b.extend_from_slice(&[b'U', VERSION, k]);
            b.extend_from_slice(&dst.to_le_bytes());
        };
        match self {
            Frame::Connect { src, vport } => {
                head(&mut b, kind::CONNECT, *src);
                b.extend_from_slice(&vport.to_le_bytes());
            }
            Frame::Accept { dst, src } => {
                head(&mut b, kind::ACCEPT, *dst);
                b.extend_from_slice(&src.to_le_bytes());
            }
            Frame::Close { dst, reason, debug } => {
                head(&mut b, kind::CLOSE, *dst);
                b.extend_from_slice(&reason.to_le_bytes());
                let mut end = debug.len().min(MAX_DEBUG);
                while !debug.is_char_boundary(end) {
                    end -= 1;
                }
                b.extend_from_slice(&debug.as_bytes()[..end]);
            }
            Frame::Unreliable { dst, payload } => {
                head(&mut b, kind::UNRELIABLE, *dst);
                b.extend_from_slice(payload);
            }
            Frame::Reliable { dst, seq, last, payload } => {
                head(&mut b, if *last { kind::RELIABLE_LAST } else { kind::RELIABLE }, *dst);
                b.extend_from_slice(&seq.to_le_bytes());
                b.extend_from_slice(payload);
            }
            Frame::Ack { dst, next, mask } => {
                head(&mut b, kind::ACK, *dst);
                b.extend_from_slice(&next.to_le_bytes());
                b.extend_from_slice(&mask.to_le_bytes());
            }
            Frame::Ping { dst } => head(&mut b, kind::PING, *dst),
        }
        b
    }

    /// `None` for anything that isn't a whole frame of this version.
    pub fn decode(b: &[u8]) -> Option<Frame> {
        let (&[b'U', VERSION, k], rest) = b.split_first_chunk::<3>()? else { return None };
        let (id, rest) = rest.split_first_chunk::<4>()?;
        let id = u32::from_le_bytes(*id);
        let word = |r: &[u8]| r.first_chunk::<4>().map(|w| u32::from_le_bytes(*w));
        let exact = |r: &[u8], n: usize| (r.len() == n).then_some(());
        Some(match k {
            kind::CONNECT => {
                exact(rest, 4)?;
                Frame::Connect { src: id, vport: word(rest)? as i32 }
            }
            kind::ACCEPT => {
                exact(rest, 4)?;
                Frame::Accept { dst: id, src: word(rest)? }
            }
            kind::CLOSE => {
                let reason = word(rest)? as i32;
                Frame::Close { dst: id, reason, debug: String::from_utf8_lossy(&rest[4..]).into_owned() }
            }
            kind::UNRELIABLE => Frame::Unreliable { dst: id, payload: rest.to_vec() },
            kind::RELIABLE | kind::RELIABLE_LAST => {
                Frame::Reliable { dst: id, seq: word(rest)?, last: k == kind::RELIABLE_LAST, payload: rest[4..].to_vec() }
            }
            kind::ACK => {
                exact(rest, 12)?;
                Frame::Ack { dst: id, next: word(rest)?, mask: u64::from_le_bytes(*rest[4..].first_chunk::<8>()?) }
            }
            kind::PING => {
                exact(rest, 0)?;
                Frame::Ping { dst: id }
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_round_trips() {
        let frames = [
            Frame::Connect { src: 7, vport: 77 },
            Frame::Accept { dst: 7, src: 0x8000_0001 },
            Frame::Close { dst: 3, reason: 1000, debug: "Server Full".into() },
            Frame::Unreliable { dst: 3, payload: vec![1, 2, 3] },
            Frame::Unreliable { dst: 3, payload: vec![] },
            Frame::Reliable { dst: 9, seq: u32::MAX, last: true, payload: vec![0xAB; 1100] },
            Frame::Reliable { dst: 9, seq: 4, last: false, payload: vec![5] },
            Frame::Ack { dst: 9, next: 12, mask: 0x8000_0000_0000_0005 },
            Frame::Ping { dst: 1 },
        ];
        for f in frames {
            assert_eq!(Frame::decode(&f.encode()), Some(f.clone()), "{f:?}");
        }
    }

    #[test]
    fn layout_is_fixed() {
        assert_eq!(Frame::Connect { src: 0x0403_0201, vport: 77 }.encode(), [b'U', 1, 1, 1, 2, 3, 4, 77, 0, 0, 0]);
        assert_eq!(Frame::Ping { dst: 2 }.encode(), [b'U', 1, 8, 2, 0, 0, 0]);
        let r = Frame::Reliable { dst: 1, seq: 2, last: true, payload: vec![9] }.encode();
        assert_eq!(r, [b'U', 1, 6, 1, 0, 0, 0, 2, 0, 0, 0, 9]);
    }

    #[test]
    fn rejects_foreign_and_short_datagrams() {
        assert_eq!(Frame::decode(b""), None);
        assert_eq!(Frame::decode(b"lobby voice"), None);
        assert_eq!(Frame::decode(&[b'U', 2, 8, 0, 0, 0, 0]), None, "other version");
        assert_eq!(Frame::decode(&[b'U', 1, 99, 0, 0, 0, 0]), None, "unknown kind");
        let ack = Frame::Ack { dst: 1, next: 2, mask: 3 }.encode();
        assert_eq!(Frame::decode(&ack[..ack.len() - 1]), None);
        assert_eq!(Frame::decode(&[b'U', 1, 5, 1, 0, 0, 0, 2]), None, "reliable without a full seq");
    }

    #[test]
    fn close_reason_is_cut_to_steams_buffer() {
        let long = "é".repeat(100);
        let Some(Frame::Close { debug, .. }) = Frame::decode(&Frame::Close { dst: 1, reason: 0, debug: long }.encode()) else { panic!() };
        assert_eq!(debug.len(), 126, "cut on a char boundary at or below 127 bytes");
    }
}
