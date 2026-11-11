//! `.anim` clip documents (KV3 `CClipDocument`, the editor's `ClipDocument`): the sync track.
//! Pose data comes from the FBX/DMX the document names, which we don't import.
//!
//! The sync track is the first enabled `m_eventTracks` entry with `m_bIsSyncTrack`. Each event
//! marks where a sync event starts (`m_startTime`, a frame, normalized over the last frame as the
//! compiler does); its ID is a `CIDEvent`'s `m_ID`, a `CFootEvent`'s phase name, else `m_name`.

use crate::clip::SyncEvent;
use kv::{Object, Value};

const PHASES: [&str; 4] = ["LeftFootDown", "RightFootPassing", "RightFootDown", "LeftFootPassing"];

#[derive(Debug)]
pub enum Error {
    Parse(kv::Error),
    NotAClip,
}

fn marker_id(class: &str, e: &Object) -> String {
    let text = |k: &str| e.str(k).unwrap_or_default().to_owned();
    match class {
        "CIDEvent" => text("m_ID"),
        "CFootEvent" => e.get("m_phase").and_then(Value::as_f64).and_then(|p| PHASES.get(p as usize)).map_or_else(String::new, |p| (*p).to_owned()),
        _ => text("m_name"),
    }
}

/// The sync events of the document in `src`, over a clip whose last frame is `last_frame`;
/// empty without a sync track (the clip then has the default one).
pub fn sync_track(src: &str, last_frame: f32) -> Result<Vec<SyncEvent>, Error> {
    let (_, root) = kv::kv3::parse(src).map_err(Error::Parse)?;
    let root = root.as_object().filter(|o| o.str("_class").is_none_or(|c| c == "CClipDocument")).ok_or(Error::NotAClip)?;
    let flag = |o: &Object, k: &str| matches!(o.get(k), Some(Value::Bool(true)));
    let tracks = root.get("m_eventTracks").and_then(Value::as_array).unwrap_or_default();
    let Some(track) = tracks.iter().filter_map(Value::as_object).find(|t| flag(t, "m_bIsSyncTrack") && !flag(t, "m_bIsDisabled")) else { return Ok(Vec::new()) };
    let class = track.str("m_eventClassName").unwrap_or_default();
    let span = last_frame.max(f32::EPSILON);
    let mut markers: Vec<(f32, String)> = track
        .get("m_events")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_object)
        .map(|e| ((e.get("m_startTime").and_then(Value::as_f64).unwrap_or(0.0) as f32 / span).clamp(0.0, 1.0), marker_id(class, e)))
        .collect();
    markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = markers.first().map_or(0.0, |m| m.0);
    let next = |i: usize| markers.get(i + 1).map_or(first + 1.0, |m| m.0);
    Ok(markers.iter().enumerate().map(|(i, (start, id))| SyncEvent { id: id.clone(), start: *start, duration: next(i) - start }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<!-- kv3 encoding:text:version{e21c7f3c-8a33-41c5-9977-a76d3a32aa0d} format:generic:version{7412167c-06e9-4698-aff2-e63eb59037e7} -->
{
    _class = "CClipDocument"
    m_eventTracks = [
        { m_eventClassName = "CIDEvent" m_bIsSyncTrack = true m_bIsDisabled = true m_events = [ { m_startTime = 0.0 m_ID = "ignored" } ] },
        { m_eventClassName = "CFootEvent" m_bIsSyncTrack = true m_bIsDisabled = false m_events = [
            { m_startTime = 30.0 m_duration = 0.0 m_phase = 2 },
            { m_startTime = 5.0 m_duration = 0.0 m_phase = 0 },
        ] },
        { m_eventClassName = "CIDEvent" m_bIsSyncTrack = true m_events = [ { m_startTime = 0.0 m_ID = "second" } ] },
    ]
}"#;

    #[test]
    fn first_enabled_sync_track_divides_the_clip() {
        let s = sync_track(DOC, 50.0).unwrap();
        let ids: Vec<_> = s.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["LeftFootDown", "RightFootDown"], "sorted by start; the disabled track skipped");
        let spans: Vec<_> = s.iter().map(|e| (e.start, e.duration)).collect();
        assert!(spans.iter().zip([(0.1, 0.5), (0.6, 0.5)]).all(|(a, b)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6), "{spans:?}: the last wraps to the first");
        assert!(sync_track("{ m_eventTracks = [] }", 10.0).unwrap().is_empty());
    }
}
