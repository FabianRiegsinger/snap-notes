use std::collections::HashSet;
use std::ops::Range;

use uuid::Uuid;

use crate::note::Note;

pub const MAX_RESULTS: usize = 50;

const CONTEXT_CHARS: usize = 30;
const FALLBACK_CHARS: usize = 60;

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct Hit {
    pub note_id: Uuid,
    pub title: String,
    pub snippet: String,
    /// Byte range of the match inside `snippet`; empty for title-only hits.
    pub highlight: Range<usize>,
    /// Byte range of the first match in the note's content.
    pub body_match: Option<Range<usize>>,
}

/// Lowercased text plus, for every folded byte, the byte range of the
/// original char it came from. Lowercasing can change lengths (`İ` becomes
/// two chars), so matches are mapped back to original char boundaries.
struct Folded {
    text: String,
    starts: Vec<usize>,
    ends: Vec<usize>,
}

fn fold(source: &str) -> Folded {
    let mut text = String::with_capacity(source.len());
    let mut starts = Vec::with_capacity(source.len());
    let mut ends = Vec::with_capacity(source.len());
    for (start, ch) in source.char_indices() {
        let end = start + ch.len_utf8();
        for lower in ch.to_lowercase() {
            text.push(lower);
            for _ in 0..lower.len_utf8() {
                starts.push(start);
                ends.push(end);
            }
        }
    }
    Folded { text, starts, ends }
}

/// The byte range in `source` of the first match of the folded `needle`.
fn find_in(source: &str, needle: &str) -> Option<Range<usize>> {
    if source.is_ascii() && needle.is_ascii() {
        find_ascii(source, needle)
    } else {
        find_folded(source, needle)
    }
}

/// ASCII lowercasing keeps every byte where it is, so a match's offsets
/// are its offsets in `source` and no maps are needed.
fn find_ascii(source: &str, needle: &str) -> Option<Range<usize>> {
    let at = source
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))?;
    Some(at..at + needle.len())
}

fn find_folded(source: &str, needle: &str) -> Option<Range<usize>> {
    let folded = fold(source);
    let at = folded.text.find(needle)?;
    let last = at + needle.len() - 1;
    Some(folded.starts[at]..folded.ends[last])
}

/// The folded query, or `None` when it is blank (S3).
fn needle(query: &str) -> Option<String> {
    let query = query.trim();
    (!query.is_empty()).then(|| fold(query).text)
}

/// The first `MAX_RESULTS` hits, in strip order, and the ids of every
/// matching note (uncapped, for dimming). Empty for a blank query.
pub struct SearchResult {
    pub hits: Vec<Hit>,
    pub matches: HashSet<Uuid>,
}

pub fn search(notes: &[Note], query: &str) -> SearchResult {
    search_with(notes, query, find_in)
}

/// [`search`] without the ASCII fast path.
#[cfg(test)]
fn search_general(notes: &[Note], query: &str) -> SearchResult {
    search_with(notes, query, find_folded)
}

/// One pass over the notes: each title and body is searched once.
fn search_with(
    notes: &[Note],
    query: &str,
    find: fn(&str, &str) -> Option<Range<usize>>,
) -> SearchResult {
    let mut result = SearchResult {
        hits: Vec::new(),
        matches: HashSet::new(),
    };
    let Some(needle) = needle(query) else {
        return result;
    };
    for note in notes {
        let body_match = find(&note.content, &needle);
        if body_match.is_none() && find(&note.title, &needle).is_none() {
            continue;
        }
        result.matches.insert(note.id);
        if result.hits.len() < MAX_RESULTS {
            result.hits.push(hit(note, body_match));
        }
    }
    result
}

/// The hit for a matching note: its first body match, or else (a
/// title-only match) its first body line.
fn hit(note: &Note, body_match: Option<Range<usize>>) -> Hit {
    let (snippet, highlight) = match &body_match {
        Some(range) => body_snippet(&note.content, range.clone()),
        None => (first_line(&note.content), 0..0),
    };
    Hit {
        note_id: note.id,
        title: note.title.clone(),
        snippet,
        highlight,
        body_match,
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn body_snippet(content: &str, matched: Range<usize>) -> (String, Range<usize>) {
    let start = content[..matched.start]
        .char_indices()
        .rev()
        .nth(CONTEXT_CHARS - 1)
        .map_or(0, |(i, _)| i);
    let end = content[matched.end..]
        .char_indices()
        .nth(CONTEXT_CHARS)
        .map_or(content.len(), |(i, _)| matched.end + i);
    let lead = if start > 0 { "…" } else { "" };
    let trail = if end < content.len() { "…" } else { "" };
    let snippet = format!("{lead}{}{trail}", one_line(&content[start..end]));
    let from = lead.len() + (matched.start - start);
    (snippet, from..from + (matched.end - matched.start))
}

fn first_line(content: &str) -> String {
    let line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    match line.char_indices().nth(FALLBACK_CHARS) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::{Note, PALETTE};

    fn find(notes: &[Note], query: &str) -> Vec<Hit> {
        search(notes, query).hits
    }

    fn note(title: &str, content: &str) -> Note {
        let mut n = Note::new(PALETTE[0]);
        n.title = title.to_string();
        n.content = content.to_string();
        n
    }

    #[test]
    fn matches_title_and_body_case_insensitively() {
        let notes = vec![
            note("Groceries", "milk"),
            note("Other", "Café au lait"),
            note("Nope", "nothing"),
        ];
        let hits = find(&notes, "GROC");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note_id, notes[0].id);
        let hits = find(&notes, "CAFÉ");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note_id, notes[1].id);
        assert_eq!(&hits[0].snippet[hits[0].highlight.clone()], "Café");
        assert_eq!(find(&[note("x", "é")], "É").len(), 1);
    }

    #[test]
    fn title_only_match_shows_first_body_line() {
        let notes = vec![note("Plan", "\n  \nfirst line\nsecond")];
        let hits = find(&notes, "plan");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "first line");
        assert_eq!(hits[0].highlight, 0..0);
        assert_eq!(hits[0].body_match, None);

        let long = "x".repeat(100);
        let hits = find(&[note("Plan", &long)], "plan");
        assert_eq!(hits[0].snippet, format!("{}…", "x".repeat(60)));
    }

    #[test]
    fn snippet_trims_with_ellipsis_and_highlights_match() {
        let body = format!("{}NeedLe{}", "a".repeat(100), "b".repeat(94));
        assert_eq!(body.chars().count(), 200);
        let hits = find(&[note("t", &body)], "needle");
        let hit = &hits[0];
        assert!(hit.snippet.starts_with('…'));
        assert!(hit.snippet.ends_with('…'));
        assert_eq!(&hit.snippet[hit.highlight.clone()], "NeedLe");
        assert_eq!(hit.body_match, Some(100..106));
        assert_eq!(hit.snippet.chars().count(), 1 + 30 + 6 + 30 + 1);
    }

    #[test]
    fn snippet_is_one_line_and_highlight_survives() {
        let hits = find(&[note("t", "one\ntwo needle\nthree")], "needle");
        assert_eq!(hits[0].snippet, "one two needle three");
        assert_eq!(&hits[0].snippet[hits[0].highlight.clone()], "needle");
    }

    #[test]
    fn empty_or_blank_query_finds_nothing() {
        let notes = vec![note("a", "b")];
        assert!(find(&notes, "").is_empty());
        assert!(find(&notes, "   ").is_empty());
    }

    #[test]
    fn results_follow_strip_order_and_cap_at_50() {
        let notes: Vec<Note> = (0..60).map(|i| note(&format!("hit {i}"), "")).collect();
        let hits = find(&notes, "hit");
        assert_eq!(hits.len(), MAX_RESULTS);
        for (h, n) in hits.iter().zip(&notes) {
            assert_eq!(h.note_id, n.id);
        }
    }

    #[test]
    fn search_offsets_survive_case_folding() {
        let content = "İstanbul and Ünal";
        let hits = find(&[note("t", content)], "ünal");
        assert_eq!(hits.len(), 1);
        let at = hits[0].body_match.clone().unwrap();
        assert_eq!(&content[at], "Ünal");
        assert_eq!(&hits[0].snippet[hits[0].highlight.clone()], "Ünal");
        assert_eq!(find(&[note("t", content)], "i̇stanbul").len(), 1);
    }

    #[test]
    fn matching_is_uncapped_and_covers_titles() {
        let mut notes: Vec<Note> = (0..60).map(|_| note("", "hit")).collect();
        notes.push(note("Hit title", ""));
        notes.push(note("x", "y"));
        let m = search(&notes, "HIT").matches;
        assert_eq!(m.len(), 61);
        assert!(notes[..61].iter().all(|n| m.contains(&n.id)));
        assert!(!m.contains(&notes[61].id));
        assert!(search(&notes, " ").matches.is_empty());
    }

    #[test]
    fn matches_include_beyond_result_cap() {
        let notes: Vec<Note> = (0..60).map(|i| note("", &format!("match {i}"))).collect();
        let result = search(&notes, "match");
        assert_eq!(result.hits.len(), 50);
        assert_eq!(result.matches.len(), 60);
    }

    #[test]
    fn ascii_fast_path_matches_slow_path() {
        let table = [
            ("Groceries", "milk and EGGS", "eggs"),
            ("Plan", "first\nsecond", "PLAN"),
            ("t", "aaaa", "aa"),
            ("t", "abc", "abcd"),
            ("t", "x\r\ny needle", "NEEDLE"),
            ("", "Needle at the start", "needle"),
            ("", "at the end: NeEdLe", "needle"),
            ("t", "no match here", "zzz"),
            ("t", "punctuation: [a-b] {c}", "[A-B] {C"),
            (
                "t",
                &format!("{}needle{}", "a".repeat(100), "b".repeat(100)),
                "needle",
            ),
            ("t", "spaces  between", "  between"),
        ];
        for (title, content, query) in table {
            let notes = [note(title, content)];
            let fast = search(&notes, query);
            let slow = search_general(&notes, query);
            assert_eq!(fast.hits, slow.hits, "{title:?} {content:?} {query:?}");
            assert_eq!(
                fast.matches, slow.matches,
                "{title:?} {content:?} {query:?}"
            );
        }
    }
}
