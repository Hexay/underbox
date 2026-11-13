//! Matroska/WebM through `matroska-demuxer`: AV1 and H.264 video, Opus and AAC audio; first
//! track of each kind. Frames are copied out, so the file and its packets are held once each.

use super::{AudioCodec, AudioTrack, AvcConfig, Builder, Kind, Media, VideoCodec, VideoTrack};
use matroska_demuxer::{Frame, MatroskaFile, TrackType};
use std::io::Cursor;

pub fn open(bytes: Vec<u8>) -> Result<Media, String> {
    let mut file = MatroskaFile::open(Cursor::new(&bytes[..])).map_err(|e| format!("matroska: {e}"))?;
    let (mut video, mut audio, mut unsupported) = (None, None, Vec::new());
    let (mut video_track, mut audio_track) = (None, None);
    for t in file.tracks() {
        let number = t.track_number().get();
        match (t.track_type(), t.codec_id()) {
            (TrackType::Video, id) if video.is_none() => {
                let codec = match id {
                    "V_AV1" => VideoCodec::Av1,
                    "V_MPEG4/ISO/AVC" => match t.codec_private().and_then(AvcConfig::parse) {
                        Some(c) => VideoCodec::H264(c),
                        None => {
                            unsupported.push(format!("{id} without avcC"));
                            continue;
                        }
                    },
                    _ => {
                        unsupported.push(id.into());
                        continue;
                    }
                };
                let (w, h) = t.video().map_or((0, 0), |v| (v.pixel_width().get() as u32, v.pixel_height().get() as u32));
                video = Some(VideoTrack { codec, width: w, height: h });
                video_track = Some(number);
            }
            (TrackType::Audio, id) if audio.is_none() => {
                let private = t.codec_private().unwrap_or_default();
                let codec = match id {
                    "A_OPUS" => AudioCodec::Opus { pre_skip: private.get(10..12).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as u32) },
                    "A_AAC" if !private.is_empty() => AudioCodec::Aac { config: private.to_vec() },
                    _ => {
                        unsupported.push(id.into());
                        continue;
                    }
                };
                let (rate, channels) = t.audio().map_or((48000, 2), |a| (a.sampling_frequency() as u32, a.channels().get() as u32));
                audio = Some(AudioTrack { codec, rate, channels });
                audio_track = Some(number);
            }
            (_, id) => unsupported.push(id.into()),
        }
    }
    let scale = file.info().timestamp_scale().get() as f64 / 1e9;
    let duration = file.info().duration().map(|d| d * scale);
    let mut metadata: Vec<(String, String)> = file.info().title().map(|t| ("title".to_string(), t.to_string())).into_iter().collect();
    for tag in file.tags().unwrap_or_default() {
        metadata.extend(tag.simple_tags().iter().filter_map(|s| Some((s.name().to_ascii_lowercase(), s.string()?.to_string()))));
    }

    let mut b = Builder::default();
    let mut frame = Frame::default();
    let mut order = 0.0;
    while file.next_frame(&mut frame).map_err(|e| format!("matroska: {e}"))? {
        let kind = match Some(frame.track) {
            t if t == video_track => Kind::Video,
            t if t == audio_track => Kind::Audio,
            _ => continue,
        };
        // File order is decode order; Matroska stores presentation times.
        b.copy(order, kind, frame.timestamp as f64 * scale, frame.is_keyframe.unwrap_or(true), &frame.data);
        order += 1.0;
    }
    let mut media = b.finish(Vec::new(), video, audio, duration);
    media.metadata = metadata;
    media.unsupported = unsupported;
    Ok(media)
}
