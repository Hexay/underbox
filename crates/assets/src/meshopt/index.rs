//! The index codecs: triangle lists through a 16-entry edge FIFO and vertex FIFO (one code byte per
//! triangle, free indices as zigzag varint deltas, a 16-byte code table at the end), and plain index
//! sequences as deltas against two alternating baselines.

use super::{Error, decode_vbyte, encode_vbyte, unzigzag_delta, zigzag_delta};

const INDEX_HEADER: u8 = 0xe0;
const SEQUENCE_HEADER: u8 = 0xd0;
const VERSION: u8 = 1;
const CODEAUX_TABLE: [u8; 16] = [0x00, 0x76, 0x87, 0x56, 0x67, 0x78, 0xa9, 0x86, 0x65, 0x89, 0x68, 0x98, 0x01, 0x69, 0, 0];

#[derive(Default)]
struct Fifos {
    edges: [[u32; 2]; 16],
    vertices: [u32; 16],
    edge_at: usize,
    vertex_at: usize,
}

impl Fifos {
    fn new() -> Self {
        Self { edges: [[u32::MAX; 2]; 16], vertices: [u32::MAX; 16], ..Self::default() }
    }
    fn push_edge(&mut self, a: u32, b: u32) {
        self.edges[self.edge_at] = [a, b];
        self.edge_at = (self.edge_at + 1) & 15;
    }
    fn push_vertex(&mut self, v: u32, cond: bool) {
        self.vertices[self.vertex_at] = v;
        self.vertex_at = (self.vertex_at + usize::from(cond)) & 15;
    }
    /// The vertex `back` entries before the newest (0 = newest).
    fn vertex(&self, back: usize) -> u32 {
        self.vertices[(self.vertex_at + 15 - back) & 15]
    }
    fn edge(&self, back: usize) -> [u32; 2] {
        self.edges[(self.edge_at + 15 - back) & 15]
    }
    fn find_vertex(&self, v: u32) -> Option<usize> {
        (0..16).find(|&i| self.vertex(i) == v)
    }
    /// (age, rotation) of a stored edge matching one of the triangle's edges.
    fn find_edge(&self, t: [u32; 3]) -> Option<(usize, usize)> {
        (0..16).find_map(|i| {
            let e = self.edge(i);
            (0..3).find(|&r| e == [t[r], t[(r + 1) % 3]]).map(|r| (i, r))
        })
    }
}

fn header_version(data: &[u8], header: u8) -> Result<u8, Error> {
    let h = *data.first().ok_or(Error::Size)?;
    if h & 0xf0 != header || h & 0x0f > VERSION {
        return Err(Error::Header(h));
    }
    Ok(h & 0x0f)
}

/// A triangle list of `count` indices, as u32s.
pub fn decode_index_buffer(data: &[u8], count: usize) -> Result<Vec<u32>, Error> {
    if !count.is_multiple_of(3) {
        return Err(Error::Shape);
    }
    if data.len() < 1 + count / 3 + 16 {
        return Err(Error::Size);
    }
    let version = header_version(data, INDEX_HEADER)?;
    let fecmax = if version >= 1 { 13 } else { 15 };
    let safe_end = data.len() - 16;
    let table = &data[safe_end..];
    let (mut f, mut next, mut last) = (Fifos::new(), 0u32, 0u32);
    let mut at = 1 + count / 3;
    let mut out = Vec::with_capacity(count);
    let free = |at: &mut usize, last: u32| last.wrapping_add(unzigzag_delta(decode_vbyte(data, at)));
    for &code in &data[1..1 + count / 3] {
        if code < 0xf0 {
            let [a, b] = f.edge(usize::from(code >> 4));
            let fec = code & 15;
            let c = if fec < fecmax {
                let c = if fec == 0 { next } else { f.vertex(usize::from(fec)) };
                next += u32::from(fec == 0);
                f.push_vertex(c, fec == 0);
                c
            } else {
                if at > safe_end {
                    return Err(Error::Size);
                }
                last = if fec != 15 { last.wrapping_add((i32::from(fec) * 2 - 27) as u32) } else { free(&mut at, last) };
                f.push_vertex(last, true);
                last
            };
            f.push_edge(c, b);
            f.push_edge(a, c);
            out.extend([a, b, c]);
            continue;
        }
        let (fea, codeaux) = if code < 0xfe {
            (0, table[usize::from(code & 15)])
        } else {
            if at > safe_end {
                return Err(Error::Size);
            }
            at += 1;
            (if code == 0xfe { 0 } else { 15 }, data[at - 1])
        };
        let (feb, fec) = (usize::from(codeaux >> 4), usize::from(codeaux & 15));
        if code >= 0xfe && codeaux == 0 {
            next = 0;
        }
        let mut take = |fe: usize, f: &Fifos| match fe {
            0 => {
                next += 1;
                next - 1
            }
            15 => 0,
            n => f.vertex(n - 1),
        };
        let (mut a, mut b, mut c) = (take(fea, &f), take(feb, &f), take(fec, &f));
        for (fe, v) in [(fea, &mut a), (feb, &mut b), (fec, &mut c)] {
            if fe == 15 {
                last = free(&mut at, last);
                *v = last;
            }
        }
        f.push_vertex(a, true);
        f.push_vertex(b, feb == 0 || feb == 15);
        f.push_vertex(c, fec == 0 || fec == 15);
        f.push_edge(b, a);
        f.push_edge(c, b);
        f.push_edge(a, c);
        out.extend([a, b, c]);
    }
    if at != safe_end {
        return Err(Error::Size);
    }
    Ok(out)
}

/// A port of `meshopt_encodeIndexBuffer` (version 1).
pub fn encode_index_buffer(indices: &[u32]) -> Vec<u8> {
    assert!(indices.len().is_multiple_of(3));
    let mut codes = Vec::with_capacity(indices.len() / 3);
    let mut data = Vec::new();
    let (mut f, mut next, mut last) = (Fifos::new(), 0u32, 0u32);
    let mut free = |data: &mut Vec<u8>, v: u32, last: &mut u32| {
        encode_vbyte(data, zigzag_delta(v.wrapping_sub(*last)));
        *last = v;
    };
    for t in indices.chunks(3).map(|t| [t[0], t[1], t[2]]) {
        if let Some((fe, r)) = f.find_edge(t).filter(|&(fe, _)| fe < 15) {
            let [a, b, c] = [t[r], t[(r + 1) % 3], t[(r + 2) % 3]];
            let fc = f.find_vertex(c);
            let mut fec = match fc {
                Some(i) if (1..13).contains(&i) => i as u8,
                _ if c == next => {
                    next += 1;
                    0
                }
                _ => 15,
            };
            if fec == 15 && c.wrapping_add(1) == last {
                (fec, last) = (13, c);
            } else if fec == 15 && c == last.wrapping_add(1) {
                (fec, last) = (14, c);
            }
            codes.push(((fe as u8) << 4) | fec);
            if fec == 15 {
                free(&mut data, c, &mut last);
            }
            if fec == 0 || fec >= 13 {
                f.push_vertex(c, true);
            }
            f.push_edge(c, b);
            f.push_edge(a, c);
            continue;
        }
        let r = if t[1] == next { 1 } else if t[2] == next { 2 } else { 0 };
        let [a, b, c] = [t[r], t[(r + 1) % 3], t[(r + 2) % 3]];
        let reset = [a, b, c] == [0, 1, 2] && next > 0;
        if reset {
            next = 0;
            f.vertices = [u32::MAX; 16];
        }
        let (fb, fc) = (f.find_vertex(b), f.find_vertex(c));
        let mut fe_of = |v: u32, found: Option<usize>| match found {
            Some(i) if i < 14 => i as u8 + 1,
            _ if v == next => {
                next += 1;
                0
            }
            _ => 15,
        };
        let fea = fe_of(a, None);
        let (feb, fec) = (fe_of(b, fb), fe_of(c, fc));
        let codeaux = (feb << 4) | fec;
        match CODEAUX_TABLE[..14].iter().position(|&x| x == codeaux) {
            Some(i) if fea == 0 && !reset => codes.push(0xf0 | i as u8),
            _ => {
                codes.push(0xf0 | 14 | fea);
                data.push(codeaux);
            }
        }
        for (fe, v) in [(fea, a), (feb, b), (fec, c)] {
            if fe == 15 {
                free(&mut data, v, &mut last);
            }
        }
        f.push_vertex(a, fea == 0 || fea == 15);
        f.push_vertex(b, feb == 0 || feb == 15);
        f.push_vertex(c, fec == 0 || fec == 15);
        f.push_edge(b, a);
        f.push_edge(c, b);
        f.push_edge(a, c);
    }
    let mut out = vec![INDEX_HEADER | VERSION];
    out.extend(codes);
    out.extend(data);
    out.extend(CODEAUX_TABLE);
    out
}

/// A non-triangle index list (strips, lines) of `count` indices.
pub fn decode_index_sequence(data: &[u8], count: usize) -> Result<Vec<u32>, Error> {
    if data.len() < 1 + count + 4 {
        return Err(Error::Size);
    }
    header_version(data, SEQUENCE_HEADER)?;
    let safe_end = data.len() - 4;
    let (mut at, mut last) = (1, [0u32; 2]);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if at >= safe_end {
            return Err(Error::Size);
        }
        let v = decode_vbyte(data, &mut at);
        let current = (v & 1) as usize;
        let index = last[current].wrapping_add(unzigzag_delta(v >> 1));
        last[current] = index;
        out.push(index);
    }
    if at != safe_end {
        return Err(Error::Size);
    }
    Ok(out)
}

pub fn encode_index_sequence(indices: &[u32]) -> Vec<u8> {
    let mut out = vec![SEQUENCE_HEADER | VERSION];
    let (mut last, mut current) = ([0u32; 2], 0usize);
    for &index in indices {
        let cd = index.wrapping_sub(last[current]) as i32;
        current ^= usize::from(cd.unsigned_abs() >= 30);
        let v = zigzag_delta(index.wrapping_sub(last[current]));
        encode_vbyte(&mut out, (v << 1) | current as u32);
        last[current] = index;
    }
    out.extend([0; 4]);
    out
}
