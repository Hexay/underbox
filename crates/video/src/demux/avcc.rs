//! H.264 in MP4/Matroska: the `avcC` record and length-prefixed samples, turned into the Annex B
//! byte stream openh264 reads.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AvcConfig {
    pub length_size: usize,
    pub sps: Vec<Vec<u8>>,
    pub pps: Vec<Vec<u8>>,
}

const START: [u8; 4] = [0, 0, 0, 1];

impl AvcConfig {
    /// An `AVCDecoderConfigurationRecord` (the `avcC` box payload, Matroska's CodecPrivate).
    pub fn parse(b: &[u8]) -> Option<Self> {
        let length_size = (*b.get(4)? & 3) as usize + 1;
        let mut at = 5;
        let mut sets = |count_mask: u8| -> Option<Vec<Vec<u8>>> {
            let count = (*b.get(at)? & count_mask) as usize;
            at += 1;
            let mut out = Vec::with_capacity(count);
            for _ in 0..count {
                let len = u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]) as usize;
                out.push(b.get(at + 2..at + 2 + len)?.to_vec());
                at += 2 + len;
            }
            Some(out)
        };
        let sps = sets(0x1F)?;
        let pps = sets(0xFF)?;
        Some(Self { length_size, sps, pps })
    }

    /// The record for one SPS and PPS, 4-byte lengths.
    pub fn record(sps: &[u8], pps: &[u8]) -> Vec<u8> {
        let mut out = vec![1, sps.get(1).copied().unwrap_or(66), sps.get(2).copied().unwrap_or(0), sps.get(3).copied().unwrap_or(30), 0xFF, 0xE1];
        out.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        out.extend_from_slice(sps);
        out.push(1);
        out.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        out.extend_from_slice(pps);
        out
    }

    /// SPS and PPS as Annex B, sent ahead of the first sample after a (re)start.
    pub fn parameter_sets(&self) -> Vec<u8> {
        self.sps.iter().chain(&self.pps).flat_map(|n| START.iter().chain(n.iter()).copied()).collect()
    }

    /// A length-prefixed sample as Annex B; `None` if a length runs past the end.
    pub fn annex_b(&self, sample: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(sample.len() + 16);
        let mut at = 0;
        while at < sample.len() {
            let len = sample.get(at..at + self.length_size)?.iter().fold(0usize, |n, b| n << 8 | *b as usize);
            at += self.length_size;
            out.extend_from_slice(&START);
            out.extend_from_slice(sample.get(at..at + len)?);
            at += len;
        }
        Some(out)
    }
}

/// NAL units of an Annex B stream, start codes removed.
pub fn nal_units(stream: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= stream.len() {
        if stream[i..i + 3] == [0, 0, 1] {
            starts.push((i, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    let ends: Vec<usize> = starts.iter().skip(1).map(|&(s, _)| if s > 0 && stream[s - 1] == 0 { s - 1 } else { s }).chain([stream.len()]).collect();
    starts.into_iter().zip(ends).map(move |((_, body), end)| &stream[body..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips_and_samples_become_annex_b() {
        let c = AvcConfig::parse(&AvcConfig::record(&[0x67, 66, 0, 30, 9], &[0x68, 7])).unwrap();
        assert_eq!(c, AvcConfig { length_size: 4, sps: vec![vec![0x67, 66, 0, 30, 9]], pps: vec![vec![0x68, 7]] });
        assert_eq!(c.annex_b(&[0, 0, 0, 2, 0x65, 1, 0, 0, 0, 1, 0x41]).unwrap(), [0, 0, 0, 1, 0x65, 1, 0, 0, 0, 1, 0x41]);
        assert!(c.annex_b(&[0, 0, 0, 9, 1]).is_none());
        let sets = c.parameter_sets();
        let nals: Vec<&[u8]> = nal_units(&sets).collect();
        assert_eq!(nals, [&[0x67u8, 66, 0, 30, 9][..], &[0x68, 7]]);
    }
}
