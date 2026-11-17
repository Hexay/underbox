//! LZ4 HC: the plain LZ4 block format, with matches from a hash chain searched as deep as
//! liblz4's HC levels search and one step of lazy matching (no optimal parser for 10-12), and
//! frames of independent 64 KiB HC blocks.

const MIN_MATCH: usize = 4;
/// The format's end rules: the last 5 bytes are literals, the last match starts 12 before the end.
const LAST_LITERALS: usize = 5;
const MF_LIMIT: usize = 12;
const MAX_DISTANCE: usize = 65_535;
const HASH_LOG: u32 = 15;
const NONE: usize = usize::MAX;

/// Chain steps per position at `level` (liblz4's `nbSearches`; levels below 1 are its default 9).
fn attempts(level: i32) -> usize {
    match if level < 1 { 9 } else { level } {
        ..=2 => 2,
        l @ 3..=9 => 1 << (l - 1),
        10 => 1024,
        11 => 4096,
        _ => 16_384,
    }
}

fn read4(s: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([s[i], s[i + 1], s[i + 2], s[i + 3]])
}

struct Chain {
    head: Vec<usize>,
    /// Distance back to the previous position with the same hash, by position mod 64 KiB; 0 ends.
    prev: Vec<u16>,
    next: usize,
}

impl Chain {
    fn new() -> Self {
        Self { head: vec![NONE; 1 << HASH_LOG], prev: vec![0; MAX_DISTANCE + 1], next: 0 }
    }

    fn hash(v: u32) -> usize {
        (v.wrapping_mul(2_654_435_761) >> (32 - HASH_LOG)) as usize
    }

    fn insert_until(&mut self, s: &[u8], end: usize) {
        for p in self.next..end {
            let h = Self::hash(read4(s, p));
            let q = self.head[h];
            self.prev[p & MAX_DISTANCE] = if q == NONE || p - q > MAX_DISTANCE { 0 } else { (p - q) as u16 };
            self.head[h] = p;
        }
        self.next = self.next.max(end);
    }

    /// The longest match for `i` (length, distance), ending by `limit`.
    fn best(&mut self, s: &[u8], i: usize, limit: usize, mut tries: usize) -> Option<(usize, usize)> {
        self.insert_until(s, i);
        let mut cand = self.head[Self::hash(read4(s, i))];
        let mut best: Option<(usize, usize)> = None;
        while cand != NONE && i - cand <= MAX_DISTANCE && tries > 0 {
            tries -= 1;
            let len = s[cand..limit].iter().zip(&s[i..limit]).take_while(|(a, b)| a == b).count();
            if len >= MIN_MATCH && best.is_none_or(|(l, _)| len > l) {
                best = Some((len, i - cand));
            }
            let d = usize::from(self.prev[cand & MAX_DISTANCE]);
            if d == 0 || d > cand {
                break;
            }
            cand -= d;
        }
        best
    }
}

fn length(out: &mut Vec<u8>, mut n: usize) {
    while n >= 255 {
        out.push(255);
        n -= 255;
    }
    out.push(n as u8);
}

fn sequence(out: &mut Vec<u8>, literals: &[u8], m: Option<(usize, usize)>) {
    let (ll, ml) = (literals.len(), m.map_or(0, |(l, _)| l - MIN_MATCH));
    out.push(((ll.min(15) as u8) << 4) | ml.min(15) as u8);
    if ll >= 15 {
        length(out, ll - 15);
    }
    out.extend_from_slice(literals);
    if let Some((_, distance)) = m {
        out.extend_from_slice(&(distance as u16).to_le_bytes());
        if ml >= 15 {
            length(out, ml - 15);
        }
    }
}

/// One LZ4 block of `src`, HC-searched at `level`.
pub fn compress(src: &[u8], level: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() / 2 + 16);
    let (mut chain, tries, limit) = (Chain::new(), attempts(level), src.len().saturating_sub(LAST_LITERALS));
    let (mut i, mut anchor) = (0, 0);
    while i + MF_LIMIT <= src.len() {
        let Some(mut m) = chain.best(src, i, limit, tries) else {
            i += 1;
            continue;
        };
        while i + 1 + MF_LIMIT <= src.len() {
            match chain.best(src, i + 1, limit, tries) {
                Some(n) if n.0 > m.0 => (i, m) = (i + 1, n),
                _ => break,
            }
        }
        sequence(&mut out, &src[anchor..i], Some(m));
        i += m.0;
        anchor = i;
    }
    sequence(&mut out, &src[anchor..], None);
    out
}

/// An LZ4 frame (independent 64 KiB blocks, content size and checksum) of HC blocks.
pub fn compress_frame(src: &[u8], level: i32) -> Vec<u8> {
    let xxh32 = |b: &[u8]| twox_hash::XxHash32::oneshot(0, b);
    let mut out = 0x184D_2204u32.to_le_bytes().to_vec();
    let descriptor = [[0x6C, 0x40].as_slice(), &(src.len() as u64).to_le_bytes()].concat();
    out.extend_from_slice(&descriptor);
    out.push((xxh32(&descriptor) >> 8) as u8);
    for block in src.chunks(64 * 1024) {
        let packed = compress(block, level);
        let (bytes, flag) = if packed.len() < block.len() { (packed.as_slice(), 0) } else { (block, 0x8000_0000u32) };
        out.extend_from_slice(&(bytes.len() as u32 | flag).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&xxh32(src).to_le_bytes());
    out
}
