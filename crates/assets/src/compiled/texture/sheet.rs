//! A vtex's SHEET extra data: sprite-sheet sequences. Layout per ValveResourceFormat
//! `Texture.GetSpriteSheetData` (unchecked against a shipped file), every offset relative to its
//! own field: `i32 sequences, u32 count`; a 32-byte sequence (`u32 id`, `bool clamp, alpha_crop,
//! no_color, no_alpha`, `i32 frames, u32 count`, `f32 total time`, `i32 name`, `i32 float params,
//! u32 count`); 12-byte frames (`f32 display time`, `i32 images, u32 count`); 32-byte images
//! (cropped UV min, max, then uncropped min, max, each a Vector2).

use super::super::{Error, format as error};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub display_time: f32,
    /// Per image: cropped UV min/max, then uncropped UV min/max.
    pub images: Vec<[f32; 8]>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sequence {
    pub id: u32,
    pub clamp: bool,
    pub alpha_crop: bool,
    pub no_color: bool,
    pub no_alpha: bool,
    pub total_time: f32,
    pub name: String,
    pub frames: Vec<Frame>,
}

const SEQUENCE: usize = 32;
const FRAME: usize = 12;
const IMAGE: usize = 32;

fn word(b: &[u8], at: usize) -> Result<[u8; 4], Error> {
    b.get(at..at + 4).map(|s| s.try_into().unwrap()).ok_or_else(|| error("sheet truncated"))
}

/// The table an (offset, count) pair at `at` points to: its start and count.
fn table(b: &[u8], at: usize) -> Result<(usize, usize), Error> {
    let offset = i32::from_le_bytes(word(b, at)?);
    let start = (at as i64 + i64::from(offset)).try_into().map_err(|_| error("sheet offset before its data"))?;
    Ok((start, u32::from_le_bytes(word(b, at + 4)?) as usize))
}

pub fn read(b: &[u8]) -> Result<Vec<Sequence>, Error> {
    let f32_at = |at: usize| word(b, at).map(f32::from_le_bytes);
    let (start, count) = table(b, 0)?;
    (0..count)
        .map(|i| {
            let s = start + i * SEQUENCE;
            let flags = b.get(s + 4..s + 8).ok_or_else(|| error("sheet truncated"))?;
            let (frames_at, frames) = table(b, s + 8)?;
            let name_at = (s as i64 + 20 + i64::from(i32::from_le_bytes(word(b, s + 20)?))) as usize;
            let name = b.get(name_at..).and_then(|n| n.split(|c| *c == 0).next()).map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
            let frames = (0..frames)
                .map(|j| {
                    let f = frames_at + j * FRAME;
                    let (images_at, images) = table(b, f + 4)?;
                    let images = (0..images).map(|k| (0..8).map(|c| f32_at(images_at + k * IMAGE + c * 4)).collect::<Result<Vec<_>, _>>().map(|v| v.try_into().unwrap())).collect::<Result<_, _>>()?;
                    Ok(Frame { display_time: f32_at(f)?, images })
                })
                .collect::<Result<_, Error>>()?;
            let [clamp, alpha_crop, no_color, no_alpha] = [0, 1, 2, 3].map(|k| flags[k] != 0);
            Ok(Sequence { id: u32::from_le_bytes(word(b, s)?), clamp, alpha_crop, no_color, no_alpha, total_time: f32_at(s + 16)?, name, frames })
        })
        .collect()
}

/// The SHEET payload for `sequences`: header, sequences, frames, images, names.
pub fn write(sequences: &[Sequence]) -> Vec<u8> {
    let frames: usize = sequences.iter().map(|s| s.frames.len()).sum();
    let images: usize = sequences.iter().flat_map(|s| &s.frames).map(|f| f.images.len()).sum();
    let mut out = vec![0u8; 8 + sequences.len() * SEQUENCE + frames * FRAME + images * IMAGE];
    let put = |out: &mut Vec<u8>, at: usize, v: [u8; 4]| out[at..at + 4].copy_from_slice(&v);
    let rel = |field: usize, target: usize| (target as i32 - field as i32).to_le_bytes();
    put(&mut out, 0, rel(0, 8));
    put(&mut out, 4, (sequences.len() as u32).to_le_bytes());
    let (mut frame_at, mut image_at) = (8 + sequences.len() * SEQUENCE, 8 + sequences.len() * SEQUENCE + frames * FRAME);
    for (i, s) in sequences.iter().enumerate() {
        let at = 8 + i * SEQUENCE;
        put(&mut out, at, s.id.to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&[s.clamp, s.alpha_crop, s.no_color, s.no_alpha].map(u8::from));
        put(&mut out, at + 8, rel(at + 8, frame_at));
        put(&mut out, at + 12, (s.frames.len() as u32).to_le_bytes());
        put(&mut out, at + 16, s.total_time.to_le_bytes());
        let name_at = out.len();
        out.extend(s.name.as_bytes());
        out.push(0);
        put(&mut out, at + 20, rel(at + 20, name_at));
        put(&mut out, at + 24, rel(at + 24, at + 24));
        for f in &s.frames {
            put(&mut out, frame_at, f.display_time.to_le_bytes());
            put(&mut out, frame_at + 4, rel(frame_at + 4, image_at));
            put(&mut out, frame_at + 8, (f.images.len() as u32).to_le_bytes());
            for img in &f.images {
                img.iter().enumerate().for_each(|(c, v)| put(&mut out, image_at + c * 4, v.to_le_bytes()));
                image_at += IMAGE;
            }
            frame_at += FRAME;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences_round_trip() {
        let quad = |x: f32| [x, 0.0, x + 0.5, 0.5, x, 0.0, x + 0.5, 0.5];
        let seqs = vec![
            Sequence { id: 0, total_time: 0.2, name: "walk".into(), frames: vec![Frame { display_time: 0.1, images: vec![quad(0.0)] }, Frame { display_time: 0.1, images: vec![quad(0.5)] }], ..Default::default() },
            Sequence { id: 3, clamp: true, no_alpha: true, total_time: 1.0, name: String::new(), frames: vec![Frame { display_time: 1.0, images: vec![] }], ..Default::default() },
        ];
        let bytes = write(&seqs);
        assert_eq!(read(&bytes).unwrap(), seqs);
        assert!(read(&bytes[..20]).is_err(), "truncated");
        assert_eq!(read(&write(&[])).unwrap(), []);
    }
}
