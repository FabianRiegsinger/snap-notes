//! A note's reminder as an iCalendar (RFC 5545) event, for the system
//! calendar app.

use crate::note::Note;
use crate::reminder;
use crate::rich;
use chrono::{DateTime, Duration, Local, Utc};
use std::io;
use std::path::{Path, PathBuf};

/// How long the calendar event lasts.
const EVENT_MINUTES: i64 = 15;

/// The title a note without one goes by outside the app.
pub const UNTITLED: &str = "Untitled note";

/// The note's display title, or [`UNTITLED`].
pub fn summary(note: &Note) -> String {
    let title = reminder::display(&note.title);
    let title = title.trim();
    if title.is_empty() {
        UNTITLED.to_string()
    } else {
        title.to_string()
    }
}

/// The note's body as trimmed plain text.
pub fn description(note: &Note) -> String {
    rich::plain_text(&note.content).trim().to_string()
}

/// A VCALENDAR with one event at `due` for the note, plus an alert at its
/// start. The UID is stable per note, so importing again updates the event.
pub fn event(note: &Note, due: DateTime<Local>, now: DateTime<Utc>) -> String {
    let stamp = |at: DateTime<Utc>| at.format("%Y%m%dT%H%M%SZ").to_string();
    let start = due.with_timezone(&Utc);
    let summary = escape(&summary(note));
    let description = description(note);
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//Snap Notes//EN".into(),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{}@snap-notes", note.id),
        format!("DTSTAMP:{}", stamp(now)),
        format!("DTSTART:{}", stamp(start)),
        format!("DTEND:{}", stamp(start + Duration::minutes(EVENT_MINUTES))),
        format!("SUMMARY:{summary}"),
    ];
    if !description.is_empty() {
        lines.push(format!("DESCRIPTION:{}", escape(&description)));
    }
    lines.extend([
        "BEGIN:VALARM".into(),
        "ACTION:DISPLAY".into(),
        format!("DESCRIPTION:{summary}"),
        "TRIGGER:PT0M".into(),
        "END:VALARM".into(),
        "END:VEVENT".into(),
        "END:VCALENDAR".into(),
    ]);
    lines.iter().map(|line| fold(line) + "\r\n").collect()
}

/// Folds a content line into physical lines of at most 75 octets: CRLF and
/// a space before each continuation, never inside a UTF-8 character.
fn fold(line: &str) -> String {
    const MAX: usize = 75;
    let mut out = String::with_capacity(line.len() + line.len() / MAX * 3);
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > MAX {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out
}

/// RFC 5545 TEXT escaping; any line break becomes a literal `\n`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.replace("\r\n", "\n").chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' | '\r' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Writes the note's event to `<dir>/snap-notes-<id>.ics` (replacing an
/// older one) and returns its path.
pub fn calendar_file(
    dir: &Path,
    note: &Note,
    due: DateTime<Local>,
    now: DateTime<Utc>,
) -> io::Result<PathBuf> {
    let path = dir.join(format!("snap-notes-{}.ics", note.id));
    crate::export::write(&path, &event(note, due, now))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;
    use chrono::TimeZone;

    fn note(title: &str, content: &str) -> Note {
        let mut n = Note::new(PALETTE[0]);
        n.title = title.into();
        n.content = content.into();
        n
    }

    fn due() -> DateTime<Local> {
        Utc.with_ymd_and_hms(2026, 10, 15, 14, 30, 0)
            .unwrap()
            .with_timezone(&Local)
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 11, 9, 5, 7).unwrap()
    }

    /// Content lines with folding undone.
    fn unfolded(ics: &str) -> Vec<String> {
        ics.replace("\r\n ", "")
            .split("\r\n")
            .map(str::to_string)
            .collect()
    }

    fn value<'a>(lines: &'a [String], name: &str) -> Option<&'a str> {
        lines
            .iter()
            .find_map(|l| l.strip_prefix(&format!("{name}:")))
    }

    #[test]
    fn event_has_calendar_structure_in_order() {
        let n = note("Call", "body");
        let ics = event(&n, due(), now());
        assert!(ics.ends_with("\r\n"));
        assert!(!ics.replace("\r\n", "").contains('\n'), "CRLF only");
        let id = n.id;
        let expected = [
            "BEGIN:VCALENDAR".to_string(),
            "VERSION:2.0".into(),
            "PRODID:-//Snap Notes//EN".into(),
            "CALSCALE:GREGORIAN".into(),
            "METHOD:PUBLISH".into(),
            "BEGIN:VEVENT".into(),
            format!("UID:{id}@snap-notes"),
            "DTSTAMP:20261011T090507Z".into(),
            "DTSTART:20261015T143000Z".into(),
            "DTEND:20261015T144500Z".into(),
            "SUMMARY:Call".into(),
            "DESCRIPTION:body".into(),
            "BEGIN:VALARM".into(),
            "ACTION:DISPLAY".into(),
            "DESCRIPTION:Call".into(),
            "TRIGGER:PT0M".into(),
            "END:VALARM".into(),
            "END:VEVENT".into(),
            "END:VCALENDAR".into(),
            String::new(),
        ];
        assert_eq!(unfolded(&ics), expected);
    }

    #[test]
    fn times_are_utc_and_end_fifteen_minutes_later() {
        // 23:50 UTC ends on the next day.
        let due = Utc
            .with_ymd_and_hms(2026, 12, 31, 23, 50, 0)
            .unwrap()
            .with_timezone(&Local);
        let lines = unfolded(&event(&note("x", ""), due, now()));
        assert_eq!(value(&lines, "DTSTART"), Some("20261231T235000Z"));
        assert_eq!(value(&lines, "DTEND"), Some("20270101T000500Z"));
    }

    #[test]
    fn text_is_escaped() {
        let n = note("a\\b; c, d", "one;two\nthree, four\\");
        let lines = unfolded(&event(&n, due(), now()));
        assert_eq!(value(&lines, "SUMMARY"), Some("a\\\\b\\; c\\, d"));
        assert_eq!(
            value(&lines, "DESCRIPTION"),
            Some("one\\;two\\nthree\\, four\\\\")
        );
        assert_eq!(escape("a\r\nb\rc"), "a\\nb\\nc");
    }

    #[test]
    fn long_lines_fold_at_75_octets_on_char_boundaries() {
        let body = "ü😀x".repeat(40);
        let n = note(&"ü".repeat(60), &body);
        let ics = event(&n, due(), now());
        for line in ics.split("\r\n") {
            assert!(line.len() <= 75, "{} octets: {line}", line.len());
        }
        // Splitting into &str above would have panicked on a broken char;
        // unfolding restores the exact text.
        let lines = unfolded(&ics);
        assert_eq!(value(&lines, "DESCRIPTION"), Some(body.as_str()));
        assert_eq!(value(&lines, "SUMMARY"), Some("ü".repeat(60).as_str()));
        assert!(ics.contains("\r\n "), "something folded");
    }

    #[test]
    fn uid_is_stable_per_note() {
        let mut n = note("Call", "a");
        let first = unfolded(&event(&n, due(), now()));
        n.title = "Other".into();
        n.content = "b".into();
        let later = now() + Duration::days(1);
        let second = unfolded(&event(&n, due(), later));
        assert_eq!(value(&first, "UID"), value(&second, "UID"));
        let other = unfolded(&event(&note("Call", "a"), due(), now()));
        assert_ne!(value(&first, "UID"), value(&other, "UID"));
    }

    #[test]
    fn summary_hides_tag_and_names_untitled() {
        let mut n = note("Call @2026-10-15 16:30", "");
        n.reminder_set_at = Some(now());
        let lines = unfolded(&event(&n, due(), now()));
        assert_eq!(value(&lines, "SUMMARY"), Some("Call"));
        let lines = unfolded(&event(&note("  ", ""), due(), now()));
        assert_eq!(value(&lines, "SUMMARY"), Some("Untitled note"));
        let mut tag_only = note("@2026-10-15 16:30", "");
        tag_only.reminder_set_at = Some(now());
        let lines = unfolded(&event(&tag_only, due(), now()));
        assert_eq!(value(&lines, "SUMMARY"), Some("Untitled note"));
    }

    #[test]
    fn description_is_plain_text_and_omitted_when_empty() {
        let lines = unfolded(&event(&note("t", "**bold** {coral}x{/}"), due(), now()));
        assert_eq!(value(&lines, "DESCRIPTION"), Some("bold x"));
        let lines = unfolded(&event(&note("t", "  \n "), due(), now()));
        // Only the alarm's description remains.
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.starts_with("DESCRIPTION:"))
                .count(),
            1
        );
        assert_eq!(value(&lines, "DESCRIPTION"), Some("t"));
    }

    #[test]
    fn calendar_file_writes_and_replaces_the_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut n = note("Call", "first");
        let path = calendar_file(dir.path(), &n, due(), now()).unwrap();
        assert_eq!(path, dir.path().join(format!("snap-notes-{}.ics", n.id)));
        n.content = "second".into();
        let again = calendar_file(dir.path(), &n, due(), now()).unwrap();
        assert_eq!(again, path);
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, event(&n, due(), now()));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let missing = dir.path().join("missing");
        assert!(calendar_file(&missing, &n, due(), now()).is_err());
    }
}
