//! MP4/MOV through `re_mp4`: AV1 and H.264 video, AAC audio; first track of each kind.

use super::{AudioCodec, AudioTrack, AvcConfig, Builder, Kind, Media, VideoCodec, VideoTrack};
use re_mp4::{Mp4, StsdBoxContent, Track};

/// MPEG-4 sampling frequency index → Hz.
const AAC_RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

pub fn open(bytes: Vec<u8>) -> Result<Media, String> {
    let mp4 = Mp4::read_bytes(&bytes).map_err(|e| format!("mp4: {e}"))?;
    let (mut video, mut audio, mut unsupported) = (None, None, Vec::new());
    let mut b = Builder::default();
    for track in mp4.tracks().values() {
        let kind = match &track.trak(&mp4).mdia.minf.stbl.stsd.contents {
            StsdBoxContent::Av01(_) if video.is_none() => {
                video = Some(VideoTrack { codec: VideoCodec::Av1, width: track.width.into(), height: track.height.into() });
                Kind::Video
            }
            StsdBoxContent::Avc1(c) if video.is_none() => {
                let a = &c.avcc.contents;
                let sps = a.sequence_parameter_sets.iter().map(|n| n.bytes.clone()).collect();
                let pps = a.picture_parameter_sets.iter().map(|n| n.bytes.clone()).collect();
                let config = AvcConfig { length_size: a.length_size_minus_one as usize + 1, sps, pps };
                video = Some(VideoTrack { codec: VideoCodec::H264(config), width: track.width.into(), height: track.height.into() });
                Kind::Video
            }
            StsdBoxContent::Mp4a(m) if audio.is_none() => {
                let Some(d) = m.esds.as_ref().map(|e| &e.es_desc.dec_config.dec_specific) else { continue };
                let rate = AAC_RATES.get(d.freq_index as usize).copied().unwrap_or(m.samplerate.value().into());
                let config = asc(d.profile, d.freq_index, d.chan_conf);
                audio = Some(AudioTrack { codec: AudioCodec::Aac { config }, rate, channels: m.channelcount.max(1).into() });
                Kind::Audio
            }
            _ => {
                unsupported.push(track.codec_string(&mp4).unwrap_or_else(|| "unknown".into()));
                continue;
            }
        };
        add_samples(&mut b, track, kind, bytes.len());
    }
    let mvhd = &mp4.moov.mvhd;
    let duration = (mvhd.timescale > 0).then(|| mvhd.duration as f64 / mvhd.timescale as f64);
    let mut media = b.finish(bytes, video, audio, duration);
    media.unsupported = unsupported;
    Ok(media)
}

fn add_samples(b: &mut Builder, track: &Track, kind: Kind, len: usize) {
    let scale = track.timescale.max(1) as f64;
    for s in track.samples.iter().filter(|s| s.byte_range().end <= len) {
        b.at(s.decode_timestamp as f64 / scale, kind, s.composition_timestamp as f64 / scale, s.is_sync, s.byte_range());
    }
}

/// A two-byte AudioSpecificConfig.
fn asc(object_type: u8, freq_index: u8, channels: u8) -> Vec<u8> {
    let v = (object_type as u16 & 0x1F) << 11 | (freq_index as u16 & 0xF) << 7 | (channels as u16 & 0xF) << 3;
    v.to_be_bytes().to_vec()
}
