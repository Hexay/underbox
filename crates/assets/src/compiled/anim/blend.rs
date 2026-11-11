//! Blend sequences: a fetch's references laid out on a grid of pose keys (`m_nGroupSize`,
//! `m_poseKeyArray0/1`), each axis driven by a pose parameter (`m_nLocalPose`). Nothing sets
//! pose parameters at runtime, so each sits at 0, normalized into its range
//! (`m_localPoseParamArray[] {m_flStart, m_flEnd}`), and the grid bakes to one animation.

use super::super::kv::{self, float, ints, objects};
use super::{Animation, Key};
use ::kv::Object;

/// (key index, weight) pairs for `v` (0..1) between ascending `keys`; evenly spaced if absent.
fn axis(v: f32, keys: &[f32], n: usize) -> Vec<(usize, f32)> {
    let at = |i: usize| keys.get(i).copied().unwrap_or(if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 });
    if n <= 1 {
        return vec![(0, 1.0)];
    }
    let hi = (1..n).find(|&i| at(i) >= v).unwrap_or(n - 1);
    let span = at(hi) - at(hi - 1);
    let t = if span > 0.0 { ((v - at(hi - 1)) / span).clamp(0.0, 1.0) } else { 0.0 };
    vec![(hi - 1, 1.0 - t), (hi, t)]
}

/// Each reference's weight (by position in `m_localReferenceArray`).
pub fn weights(fetch: &Object, seq: &Object) -> Vec<(usize, f32)> {
    let refs = ints(fetch, "m_localReferenceArray").len();
    let size = ints(fetch, "m_nGroupSize");
    let (nx, ny) = (size.first().map_or(refs, |&n| n.max(1) as usize), size.get(1).map_or(1, |&n| n.max(1) as usize));
    let params: Vec<&Object> = objects(seq, "m_localPoseParamArray").collect();
    let pose = ints(fetch, "m_nLocalPose");
    let value = |k: usize| {
        let p = pose.get(k).and_then(|&i| params.get(usize::try_from(i).ok()?));
        p.map_or(0.0, |p| {
            let (start, end) = (float(p, "m_flStart"), float(p, "m_flEnd"));
            if end != start { ((0.0 - start) / (end - start)).clamp(0.0, 1.0) } else { 0.0 }
        })
    };
    let keys = |name: &str| kv::array(fetch, name).iter().filter_map(|v| kv::int_of(v).map(|i| i as f32).or(v.as_f64().map(|f| f as f32))).collect::<Vec<f32>>();
    let (xs, ys) = (axis(value(0), &keys("m_poseKeyArray0"), nx), axis(value(1), &keys("m_poseKeyArray1"), ny));
    let mut out: Vec<(usize, f32)> = ys.iter().flat_map(|&(y, wy)| xs.iter().map(move |&(x, wx)| (y * nx + x, wx * wy))).filter(|&(i, w)| i < refs && w > 0.0).collect();
    if out.is_empty() && refs > 0 {
        out.push((0, 1.0));
    }
    out
}

fn mix(keys: &[(Key, f32)]) -> Key {
    let first = keys[0].0.rotation;
    let mut k = Key { position: [0.0; 3], rotation: [0.0; 4], scale: 0.0 };
    for &(key, w) in keys {
        let dot: f32 = key.rotation.iter().zip(first).map(|(a, b)| a * b).sum();
        let sign = if dot < 0.0 { -w } else { w };
        (0..3).for_each(|i| k.position[i] += key.position[i] * w);
        (0..4).for_each(|i| k.rotation[i] += key.rotation[i] * sign);
        k.scale += key.scale * w;
    }
    let len = k.rotation.iter().map(|c| c * c).sum::<f32>().sqrt();
    k.rotation = if len > 0.0 { k.rotation.map(|c| c / len) } else { Key::IDENTITY.rotation };
    k
}

/// `parts` blended frame by frame over the heaviest part's frames, each sampled at the same
/// cycle; its events and flags are kept.
pub fn blend(parts: &[(&Animation, f32)]) -> Option<Animation> {
    let total: f32 = parts.iter().map(|p| p.1).sum();
    // Reversed so ties keep the first part (max_by returns the last maximum).
    let &(main, _) = parts.iter().rev().max_by(|a, b| a.1.total_cmp(&b.1))?;
    if parts.len() == 1 {
        return Some(main.clone());
    }
    let n = main.frames.len();
    let at = |a: &Animation, f: usize| (if n > 1 { f * a.frames.len().saturating_sub(1) / (n - 1) } else { 0 }).min(a.frames.len().saturating_sub(1));
    let frames = (0..n)
        .map(|f| {
            let bones = main.frames[f].len();
            (0..bones)
                .map(|b| mix(&parts.iter().filter_map(|&(a, w)| Some((*a.frames.get(at(a, f))?.get(b)?, w / total))).collect::<Vec<_>>()))
                .collect()
        })
        .collect();
    Some(Animation { frames, ..main.clone() })
}
