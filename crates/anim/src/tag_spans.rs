//! AnimGraph1 tag spans: the tags a sequence node raises over parts of its cycle
//! (`CSequenceAnimNode.m_tagSpans`), raised while that sequence plays.

#[derive(Debug, Clone, PartialEq)]
pub struct TagSpan {
    /// The tag's `m_name` in the graph's tag manager.
    pub tag: String,
    /// Cycle (0..1) the span starts at.
    pub start: f32,
    /// Cycles it lasts; a span past 1 wraps into the next lap.
    pub duration: f32,
}

/// The spans of the sequence nodes playing `sequence`.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceTags {
    pub sequence: String,
    pub spans: Vec<TagSpan>,
}

/// Tags whose spans `[from, to]` (cycles, `to` past 1 after a wrap) touched, deduplicated.
pub fn swept(spans: &[TagSpan], from: f32, to: f32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in spans {
        let end = s.start + s.duration.max(0.0);
        // The span this lap, the next lap's (after a wrap), and the previous lap's tail.
        let hit = [-1.0, 0.0, 1.0].iter().any(|lap| s.start + lap <= to && end + lap >= from);
        if hit && !out.iter().any(|t| t.eq_ignore_ascii_case(&s.tag)) {
            out.push(s.tag.clone());
        }
    }
    out
}

/// The spans for `sequence`, if any node plays it.
pub fn for_sequence<'a>(all: &'a [SequenceTags], sequence: &str) -> &'a [TagSpan] {
    all.iter().find(|s| s.sequence.eq_ignore_ascii_case(sequence)).map_or(&[], |s| &s.spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(tag: &str, start: f32, duration: f32) -> TagSpan {
        TagSpan { tag: tag.into(), start, duration }
    }

    #[test]
    fn spans_touched_by_the_swept_cycles_are_active() {
        let spans = [span("early", 0.0, 0.2), span("late", 0.6, 0.1), span("wrap", 0.9, 0.2)];
        assert_eq!(swept(&spans, 0.05, 0.15), ["early", "wrap"], "the wrapping span's tail covers 0..0.1");
        assert_eq!(swept(&spans, 0.3, 0.5), Vec::<String>::new());
        assert_eq!(swept(&spans, 0.5, 0.8), ["late"], "passed over in one update still counts");
        assert_eq!(swept(&spans, 0.95, 1.05), ["early", "wrap"], "across the wrap");
        assert_eq!(for_sequence(&[SequenceTags { sequence: "Walk".into(), spans: spans.to_vec() }], "walk").len(), 3);
    }
}
