//! A minimal Matroska/WebM writer for synthetic clips: header, info, tracks, tags and
//! SimpleBlock clusters (one per second), no cues.

pub struct Track {
    pub codec_id: &'static str,
    pub private: Vec<u8>,
    /// Pixel width and height of a video track.
    pub video: Option<(u32, u32)>,
    /// Sampling rate and channels of an audio track.
    pub audio: Option<(f64, u32)>,
    /// (seconds, keyframe, data) in decode order.
    pub packets: Vec<(f64, bool, Vec<u8>)>,
}

fn id_bytes(id: u32) -> Vec<u8> {
    let b = id.to_be_bytes();
    b[b.iter().position(|v| *v != 0).unwrap_or(3)..].to_vec()
}

fn el(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = id_bytes(id);
    // Sizes as 8-byte vints: marker byte 0x01, then 7 bytes.
    out.push(1);
    out.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
    out.extend_from_slice(body);
    out
}

fn uint(id: u32, v: u64) -> Vec<u8> {
    let b = v.to_be_bytes();
    el(id, &b[b.iter().position(|v| *v != 0).unwrap_or(7)..])
}

fn float(id: u32, v: f64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

fn master(id: u32, children: &[Vec<u8>]) -> Vec<u8> {
    el(id, &children.concat())
}

fn track_entry(number: u64, t: &Track) -> Vec<u8> {
    let mut c = vec![uint(0xD7, number), uint(0x73C5, number), uint(0x83, if t.video.is_some() { 1 } else { 2 }), el(0x86, t.codec_id.as_bytes())];
    if !t.private.is_empty() {
        c.push(el(0x63A2, &t.private));
    }
    if let Some((w, h)) = t.video {
        c.push(master(0xE0, &[uint(0xB0, w.into()), uint(0xBA, h.into())]));
    }
    if let Some((rate, channels)) = t.audio {
        c.push(master(0xE1, &[float(0xB5, rate), uint(0x9F, channels.into())]));
    }
    master(0xAE, &c)
}

/// A whole file. `tags` become one Tag of SimpleTags.
pub fn write(doc_type: &str, tracks: &[Track], tags: &[(&str, &str)]) -> Vec<u8> {
    let header = master(0x1A45DFA3, &[uint(0x4286, 1), uint(0x42F7, 1), uint(0x42F2, 4), uint(0x42F3, 8), el(0x4282, doc_type.as_bytes()), uint(0x4287, 4), uint(0x4285, 2)]);
    let ms = |t: f64| (t * 1000.0).round() as i64;
    let end = tracks.iter().flat_map(|t| t.packets.iter().map(|p| p.0)).fold(0.0, f64::max);
    let info = master(0x1549A966, &[uint(0x2AD7B1, 1_000_000), float(0x4489, (ms(end) + 40) as f64), el(0x4D80, b"underbox"), el(0x5741, b"underbox")]);
    let entries: Vec<Vec<u8>> = tracks.iter().enumerate().map(|(i, t)| track_entry(i as u64 + 1, t)).collect();
    let mut body = vec![info, master(0x1654AE6B, &entries)];
    if !tags.is_empty() {
        let simple: Vec<Vec<u8>> = tags.iter().map(|(k, v)| master(0x67C8, &[el(0x45A3, k.as_bytes()), el(0x4487, v.as_bytes())])).collect();
        body.push(master(0x1254C367, &[master(0x7373, &[[master(0x63C0, &[])].as_slice(), &simple].concat())]));
    }

    let mut blocks: Vec<(i64, u64, bool, &[u8])> = tracks.iter().enumerate().flat_map(|(i, t)| t.packets.iter().map(move |p| (ms(p.0), i as u64 + 1, p.1, p.2.as_slice()))).collect();
    blocks.sort_by_key(|b| b.0);
    let mut cluster: Option<(i64, Vec<Vec<u8>>)> = None;
    for (time, track, key, data) in blocks {
        if cluster.as_ref().is_some_and(|(start, _)| time - start >= 1000) {
            let (start, c) = cluster.take().unwrap();
            body.push(master(0x1F43B675, &[[uint(0xE7, start as u64)].as_slice(), &c].concat()));
        }
        let (start, c) = cluster.get_or_insert_with(|| (time, Vec::new()));
        let mut block = vec![0x80 | track as u8];
        block.extend_from_slice(&((time - *start) as i16).to_be_bytes());
        block.push(if key { 0x80 } else { 0 });
        block.extend_from_slice(data);
        c.push(el(0xA3, &block));
    }
    if let Some((start, c)) = cluster {
        body.push(master(0x1F43B675, &[[uint(0xE7, start as u64)].as_slice(), &c].concat()));
    }
    [header, master(0x18538067, &body)].concat()
}

/// Opus's CodecPrivate: an `OpusHead` for mapping family 0.
pub fn opus_head(channels: u8, pre_skip: u16) -> Vec<u8> {
    let mut h = b"OpusHead".to_vec();
    h.extend_from_slice(&[1, channels]);
    h.extend_from_slice(&pre_skip.to_le_bytes());
    h.extend_from_slice(&48000u32.to_le_bytes());
    h.extend_from_slice(&[0, 0, 0]);
    h
}
