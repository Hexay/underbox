//! Animation segment decoders (`CCompressed*`): a segment's elements, keyed per frame or once,
//! as full floats, half floats or 48-bit quaternions. Each encoder is the inverse its fixture
//! writer uses.

use half::f16;

/// What one decoded element sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Vec3([f32; 3]),
    Quat([f32; 4]),
    Float(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoder {
    StaticFullVector3,
    StaticVector3,
    StaticQuaternion,
    StaticFloat,
    StaticBool,
    FullVector3,
    DeltaVector3,
    AnimVector3,
    AnimQuaternion,
    FullQuaternion,
    FullFloat,
    FullBool,
}

const ALL: [Decoder; 12] = [
    Decoder::StaticFullVector3,
    Decoder::StaticVector3,
    Decoder::StaticQuaternion,
    Decoder::StaticFloat,
    Decoder::StaticBool,
    Decoder::FullVector3,
    Decoder::DeltaVector3,
    Decoder::AnimVector3,
    Decoder::AnimQuaternion,
    Decoder::FullQuaternion,
    Decoder::FullFloat,
    Decoder::FullBool,
];

impl Decoder {
    /// The `m_decoderArray[].m_szName` it answers to.
    pub fn name(self) -> &'static str {
        match self {
            Decoder::StaticFullVector3 => "CCompressedStaticFullVector3",
            Decoder::StaticVector3 => "CCompressedStaticVector3",
            Decoder::StaticQuaternion => "CCompressedStaticQuaternion",
            Decoder::StaticFloat => "CCompressedStaticFloat",
            Decoder::StaticBool => "CCompressedStaticBool",
            Decoder::FullVector3 => "CCompressedFullVector3",
            Decoder::DeltaVector3 => "CCompressedDeltaVector3",
            Decoder::AnimVector3 => "CCompressedAnimVector3",
            Decoder::AnimQuaternion => "CCompressedAnimQuaternion",
            Decoder::FullQuaternion => "CCompressedFullQuaternion",
            Decoder::FullFloat => "CCompressedFullFloat",
            Decoder::FullBool => "CCompressedFullBool",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        ALL.into_iter().find(|d| d.name() == name)
    }

    /// Element `pos` of a segment of `count` elements at block-relative `frame`.
    pub fn read(self, data: &[u8], count: usize, frame: usize, pos: usize) -> Option<Value> {
        let keyed = frame * count + pos;
        Some(match self {
            Decoder::StaticFullVector3 => Value::Vec3(vec3(data, pos)?),
            Decoder::StaticVector3 => Value::Vec3(half3(data, pos)?),
            Decoder::StaticQuaternion => Value::Quat(quaternion(data.get(pos * 6..pos * 6 + 6)?)),
            Decoder::StaticFloat => Value::Float(f32_at(data, pos)?),
            Decoder::StaticBool => Value::Float(bool_at(data, pos)?),
            Decoder::FullVector3 => Value::Vec3(vec3(data, keyed)?),
            Decoder::DeltaVector3 => {
                let (base, delta) = (vec3(data, pos)?, half3(data.get(count * 12..)?, keyed)?);
                Value::Vec3(std::array::from_fn(|i| base[i] + delta[i]))
            }
            Decoder::AnimVector3 => Value::Vec3(half3(data, keyed)?),
            Decoder::AnimQuaternion => Value::Quat(quaternion(data.get(keyed * 6..keyed * 6 + 6)?)),
            Decoder::FullQuaternion => Value::Quat(std::array::from_fn(|i| f32_at(data, keyed * 4 + i).unwrap_or(0.0))),
            Decoder::FullFloat => Value::Float(f32_at(data, keyed)?),
            Decoder::FullBool => Value::Float(bool_at(data, keyed)?),
        })
    }
}

fn f32_at(data: &[u8], i: usize) -> Option<f32> {
    data.get(i * 4..i * 4 + 4).map(|b| f32::from_le_bytes(b.try_into().unwrap()))
}

fn bool_at(data: &[u8], i: usize) -> Option<f32> {
    data.get(i).map(|&b| if b != 0 { 1.0 } else { 0.0 })
}

fn vec3(data: &[u8], i: usize) -> Option<[f32; 3]> {
    data.get(i * 12 + 11)?;
    Some(std::array::from_fn(|c| f32_at(data, i * 3 + c).unwrap()))
}

fn half3(data: &[u8], i: usize) -> Option<[f32; 3]> {
    let b = data.get(i * 6..i * 6 + 6)?;
    Some(std::array::from_fn(|c| f16::from_le_bytes([b[c * 2], b[c * 2 + 1]]).to_f32()))
}

/// sin(π/4) / 16384: the step of each stored 14-bit component.
const STEP: f32 = std::f32::consts::FRAC_1_SQRT_2 / 16384.0;

/// A 48-bit quaternion: three 14-bit components (bit 14 picks the non-negative half), the
/// largest one dropped and rebuilt. Word 0/1's sign bits say which slot it was; word 2's, its sign.
pub fn quaternion(b: &[u8]) -> [f32; 4] {
    let w: [u16; 3] = std::array::from_fn(|i| u16::from_le_bytes([b[i * 2], b[i * 2 + 1]]));
    let c = w.map(|w| {
        let v = f32::from(w & 0x3FFF);
        if w & 0x4000 == 0 { STEP * (v - 16384.0) } else { STEP * v }
    });
    let mut big = (1.0 - c[0] * c[0] - c[1] * c[1] - c[2] * c[2]).max(0.0).sqrt();
    if w[2] & 0x8000 != 0 {
        big = -big;
    }
    match (w[0] & 0x8000 != 0, w[1] & 0x8000 != 0) {
        (false, false) => [c[0], c[1], c[2], big],
        (false, true) => [big, c[0], c[1], c[2]],
        (true, false) => [c[2], big, c[0], c[1]],
        (true, true) => [c[1], c[2], big, c[0]],
    }
}

/// The inverse of [`quaternion`] for a unit quaternion (to within one step per component).
pub fn encode_quaternion(q: [f32; 4]) -> [u8; 6] {
    let big = (0..4).max_by(|&a, &b| q[a].abs().total_cmp(&q[b].abs())).unwrap();
    let q = if q[big] < 0.0 { q.map(|v| -v) } else { q };
    let (rest, flags) = match big {
        3 => ([q[0], q[1], q[2]], (0, 0)),
        0 => ([q[1], q[2], q[3]], (0, 0x8000)),
        1 => ([q[2], q[3], q[0]], (0x8000, 0)),
        _ => ([q[3], q[0], q[1]], (0x8000, 0x8000)),
    };
    let word = |v: f32| -> u16 {
        if v < 0.0 { ((v / STEP + 16384.0).round().clamp(0.0, 16383.0)) as u16 } else { 0x4000 | (v / STEP).round().min(16383.0) as u16 }
    };
    let words = [word(rest[0]) | flags.0, word(rest[1]) | flags.1, word(rest[2])];
    let mut out = [0u8; 6];
    for (i, w) in words.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&w.to_le_bytes());
    }
    out
}

pub fn half3_bytes(v: [f32; 3]) -> [u8; 6] {
    let mut out = [0u8; 6];
    for (i, c) in v.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&f16::from_f32(*c).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        let dot: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        (dot.abs() - 1.0).abs() < 1e-6
    }

    #[test]
    fn quaternions_round_trip_through_every_dropped_slot() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        for q in [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0], [0.0, -s, 0.0, s], [0.1, 0.2, -0.9, 0.37], [-0.5, 0.5, 0.5, -0.5]] {
            let n = (q.iter().map(|v: &f32| v * v).sum::<f32>()).sqrt();
            let q = q.map(|v| v / n);
            assert!(close(quaternion(&encode_quaternion(q)), q), "{q:?} -> {:?}", quaternion(&encode_quaternion(q)));
        }
    }

    #[test]
    fn identity_is_three_offset_zeros() {
        assert_eq!(encode_quaternion([0.0, 0.0, 0.0, 1.0]), [0x00, 0x40, 0x00, 0x40, 0x00, 0x40]);
        assert_eq!(quaternion(&[0x00, 0x40, 0x00, 0x40, 0x00, 0x40]), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn keyed_decoders_index_frame_major() {
        // Two elements over two frames: frame f, element e holds 10f + e.
        let floats: Vec<u8> = [0.0f32, 1.0, 10.0, 11.0].iter().flat_map(|f| f.to_le_bytes()).collect();
        assert_eq!(Decoder::FullFloat.read(&floats, 2, 1, 0), Some(Value::Float(10.0)));
        assert_eq!(Decoder::StaticFloat.read(&floats, 2, 1, 1), Some(Value::Float(1.0)), "static ignores the frame");
        let vecs: Vec<u8> = (0..6).flat_map(|i| (i as f32).to_le_bytes()).collect();
        assert_eq!(Decoder::FullVector3.read(&vecs, 1, 1, 0), Some(Value::Vec3([3.0, 4.0, 5.0])));
        let halves: Vec<u8> = [[0.5, 1.0, 2.0], [4.0, 8.0, 16.0]].iter().flat_map(|v| half3_bytes(*v)).collect();
        assert_eq!(Decoder::AnimVector3.read(&halves, 1, 1, 0), Some(Value::Vec3([4.0, 8.0, 16.0])));
        assert_eq!(Decoder::StaticVector3.read(&halves, 2, 5, 1), Some(Value::Vec3([4.0, 8.0, 16.0])));
        let mut delta: Vec<u8> = [100.0f32, 0.0, -1.0].iter().flat_map(|f| f.to_le_bytes()).collect();
        delta.extend(half3_bytes([0.0, 0.0, 0.0]).iter().chain(&half3_bytes([2.0, 0.5, 1.0])));
        assert_eq!(Decoder::DeltaVector3.read(&delta, 1, 1, 0), Some(Value::Vec3([102.0, 0.5, 0.0])));
        assert_eq!(Decoder::FullBool.read(&[0, 1, 1, 0], 2, 1, 1), Some(Value::Float(0.0)));
        let full: Vec<u8> = [0.0f32, 0.0, 1.0, 0.0].iter().flat_map(|f| f.to_le_bytes()).collect();
        assert_eq!(Decoder::FullQuaternion.read(&full, 1, 0, 0), Some(Value::Quat([0.0, 0.0, 1.0, 0.0])));
        assert_eq!(Decoder::FullFloat.read(&floats, 2, 2, 0), None, "past the data");
    }

    #[test]
    fn names_round_trip() {
        assert!(ALL.iter().all(|d| Decoder::from_name(d.name()) == Some(*d)));
        assert_eq!(Decoder::from_name("CCompressedNope"), None);
    }
}
