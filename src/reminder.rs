//! Reminders typed into a note's title: `@15:00`, `@tomorrow`, `@fri 10:30`,
//! `@2026-10-07`. Pure: the current time is always passed in.

use crate::note::Note;

use chrono::{
    DateTime, Datelike, Days, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday,
};
use std::ops::Range;

/// The time of day a tag without one reminds at.
const DEFAULT_HOUR: u32 = 9;

/// A tag's day, before it is resolved against the current time.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Day {
    /// `@HH:MM`: today, or tomorrow once the time has passed.
    Next,
    Tomorrow,
    Weekday(Weekday),
    Date(NaiveDate),
}

/// The first `@…` tag in a title.
struct Tag {
    /// Where it is: the tag and an optional time after it.
    range: Range<usize>,
    /// What it says; `None` for an invalid tag.
    when: Option<(Day, NaiveTime)>,
}

/// The first tag in `title`, if it has one.
fn tag(title: &str) -> Option<Tag> {
    // A tag starts a word, so an address like `bob@x.com` isn't one.
    let start = title
        .char_indices()
        .find(|&(i, c)| {
            c == '@'
                && title[..i]
                    .chars()
                    .next_back()
                    .is_none_or(char::is_whitespace)
        })
        .map(|(i, _)| i)?;
    let word_end = |from: usize| {
        title[from..]
            .find(char::is_whitespace)
            .map_or(title.len(), |n| from + n)
    };
    let end = word_end(start);
    let word = title[start + 1..end].to_ascii_lowercase();
    let day = if let Some(time) = parse_time(&word) {
        return Some(Tag {
            range: start..end,
            when: Some((Day::Next, time)),
        });
    } else if word == "tomorrow" {
        Day::Tomorrow
    } else if let Some(weekday) = parse_weekday(&word) {
        Day::Weekday(weekday)
    } else if let Ok(date) = NaiveDate::parse_from_str(&word, "%Y-%m-%d") {
        Day::Date(date)
    } else {
        return Some(Tag {
            range: start..end,
            when: None,
        });
    };
    // An optional time follows after whitespace.
    let rest = &title[end..];
    let time_start = end + (rest.len() - rest.trim_start().len());
    if time_start > end && time_start < title.len() {
        let time_end = word_end(time_start);
        if let Some(time) = parse_time(&title[time_start..time_end]) {
            return Some(Tag {
                range: start..time_end,
                when: Some((day, time)),
            });
        }
    }
    let nine = NaiveTime::from_hms_opt(DEFAULT_HOUR, 0, 0).expect("valid time");
    Some(Tag {
        range: start..end,
        when: Some((day, nine)),
    })
}

/// `H:MM` or `HH:MM`, 24-hour.
fn parse_time(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.split_once(':')?;
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || h.len() > 2 || !digits(m) || m.len() != 2 {
        return None;
    }
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    })
}

/// `naive` as local time. In a daylight-saving gap it moves on an hour.
fn to_local(naive: NaiveDateTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&naive).earliest().or_else(|| {
        Local
            .from_local_datetime(&(naive + chrono::Duration::hours(1)))
            .earliest()
    })
}

/// The reminder time in `title`, resolved against `now`: `@HH:MM` today or
/// tomorrow once past, `@tomorrow [HH:MM]`, `@mon`…`@sun [HH:MM]` on the next
/// such day (today while the time is ahead), `@YYYY-MM-DD [HH:MM]`. Without a
/// time it is 09:00. Only the first tag counts; an invalid one is none.
pub fn parse(title: &str, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let (day, time) = tag(title)?.when?;
    let today = now.date_naive();
    let on = |date: NaiveDate| to_local(date.and_time(time));
    let first_after_now =
        |dates: &mut dyn Iterator<Item = NaiveDate>| dates.filter_map(&on).find(|t| *t > now);
    match day {
        Day::Next => first_after_now(&mut today.iter_days().take(2)),
        Day::Tomorrow => on(today.checked_add_days(Days::new(1))?),
        Day::Weekday(weekday) => {
            first_after_now(&mut today.iter_days().take(8).filter(|d| d.weekday() == weekday))
        }
        Day::Date(date) => on(date),
    }
}

/// The title's valid reminder tag as typed (with its time, if any).
pub fn tag_text(title: &str) -> Option<&str> {
    let tag = tag(title)?;
    tag.when.map(|_| &title[tag.range])
}

/// Sets the note's title. A changed tag counts from `now`: `@15:00` typed at
/// 16:00 means tomorrow. The same tag keeps counting from when it was typed.
pub fn retitle(note: &mut Note, title: String, now: DateTime<Local>) {
    if tag_text(&note.title) != tag_text(&title) || note.reminder_set_at.is_none() {
        note.reminder_set_at = tag_text(&title).map(|_| now.with_timezone(&Utc));
    }
    note.title = title;
}

/// Gives a tagged note without an anchor (written before anchors existed)
/// its `updated_at` as one, so later edits can't move its reminder. True if
/// it changed the note.
pub fn freeze_anchor(note: &mut Note) -> bool {
    let freeze = note.reminder_set_at.is_none() && tag_text(&note.title).is_some();
    if freeze {
        note.reminder_set_at = Some(note.updated_at);
    }
    freeze
}

/// The note's reminder time, if its title sets one. Relative tags count from
/// when the tag was typed.
pub fn at(note: &Note) -> Option<DateTime<Local>> {
    let set_at = note.reminder_set_at?;
    parse(&note.title, set_at.with_timezone(&Local))
}

/// The reminder time while it hasn't fired yet, due or not.
pub fn pending(note: &Note) -> Option<DateTime<Utc>> {
    let time = at(note)?.with_timezone(&Utc);
    (note.reminder_fired != Some(time)).then_some(time)
}

/// The reminder time once it has come and hasn't fired for that time.
pub fn due(note: &Note, now: DateTime<Local>) -> Option<DateTime<Utc>> {
    pending(note).filter(|time| *time <= now.with_timezone(&Utc))
}

/// The title without its reminder tag, for the notification.
pub fn display(title: &str) -> String {
    match tag(title) {
        Some(Tag {
            range,
            when: Some(_),
        }) => {
            let before = title[..range.start].trim_end();
            let after = title[range.end..].trim_start();
            let sep = if before.is_empty() || after.is_empty() {
                ""
            } else {
                " "
            };
            format!("{before}{sep}{after}")
        }
        _ => title.trim().to_string(),
    }
}

/// Whether the title has something that looks like a reminder tag: an `@`
/// at the start or after whitespace, followed by a letter or digit. It may
/// still fail to parse.
pub fn candidate(title: &str) -> bool {
    title.char_indices().any(|(i, c)| {
        c == '@'
            && title[..i]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace)
            && title[i + 1..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric)
    })
}

/// What a note's title says about a reminder, for the header.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// No tag.
    None,
    /// A reminder that hasn't fired yet, with its label.
    Pending(String),
    /// A reminder that has fired for its current time, with its label.
    Fired(String),
    /// A tag candidate that isn't a reminder.
    Invalid,
}

/// The note's reminder status. A tag without an anchor is no reminder.
pub fn status(note: &Note) -> Status {
    match at(note) {
        Some(time) => {
            let text = label(time);
            if note.reminder_fired == Some(time.with_timezone(&Utc)) {
                Status::Fired(text)
            } else {
                Status::Pending(text)
            }
        }
        None if candidate(&note.title) => Status::Invalid,
        None => Status::None,
    }
}

/// A reminder time as shown in the header and peek: `Tue 15:00`.
pub fn label(at: DateTime<Local>) -> String {
    at.format("%a %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;
    use chrono::TimeZone;

    /// Monday 5 October 2026, 14:00 local time.
    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 5, 14, 0, 0).unwrap()
    }

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn note(title: &str, set_at: DateTime<Local>) -> Note {
        let mut n = Note::new(PALETTE[0]);
        n.title = title.into();
        n.reminder_set_at = Some(set_at.with_timezone(&Utc));
        n
    }

    #[test]
    fn parses_time_today_or_tomorrow() {
        assert_eq!(
            parse("Call Anna @15:00", now()),
            Some(local(2026, 10, 5, 15, 0))
        );
        assert_eq!(
            parse("@9:30 standup", now()),
            Some(local(2026, 10, 6, 9, 30))
        );
        // The current minute itself is already past.
        assert_eq!(parse("@14:00", now()), Some(local(2026, 10, 6, 14, 0)));
        assert_eq!(parse("@23:59", now()), Some(local(2026, 10, 5, 23, 59)));
    }

    #[test]
    fn parses_tomorrow_and_weekdays() {
        assert_eq!(parse("@tomorrow", now()), Some(local(2026, 10, 6, 9, 0)));
        assert_eq!(
            parse("Pay rent @Tomorrow 18:15", now()),
            Some(local(2026, 10, 6, 18, 15))
        );
        assert_eq!(parse("@tue", now()), Some(local(2026, 10, 6, 9, 0)));
        assert_eq!(parse("@fri 10:30", now()), Some(local(2026, 10, 9, 10, 30)));
        assert_eq!(parse("@sun", now()), Some(local(2026, 10, 11, 9, 0)));
        // Today's weekday counts while its time is still ahead, else next week.
        assert_eq!(parse("@mon 16:00", now()), Some(local(2026, 10, 5, 16, 0)));
        assert_eq!(parse("@mon", now()), Some(local(2026, 10, 12, 9, 0)));
    }

    #[test]
    fn parses_dates() {
        assert_eq!(
            parse("Dentist @2026-10-07", now()),
            Some(local(2026, 10, 7, 9, 0))
        );
        assert_eq!(
            parse("@2026-12-24 17:00 gifts", now()),
            Some(local(2026, 12, 24, 17, 0))
        );
        // A date in the past is a reminder that is already due.
        assert_eq!(parse("@2026-01-02", now()), Some(local(2026, 1, 2, 9, 0)));
        // A time that isn't one is not part of the tag.
        assert_eq!(
            parse("@2026-10-07 at noon", now()),
            Some(local(2026, 10, 7, 9, 0))
        );
    }

    #[test]
    fn invalid_tags_are_ignored() {
        for title in [
            "",
            "No reminder",
            "@",
            "@25:00",
            "@12:60",
            "@12:5",
            "@noon",
            "@2026-02-30",
            "@2026-13-01",
            "@monday",
            "@15:00,",
            "@tomorrow25:00",
        ] {
            assert_eq!(parse(title, now()), None, "{title}");
        }
        // An address is not a tag.
        assert_eq!(parse("mail bob@15:00", now()), None);
        // Only the first tag counts, even when it is invalid.
        assert_eq!(parse("@later @15:00", now()), None);
        assert_eq!(
            parse("@15:00 @16:00", now()),
            Some(local(2026, 10, 5, 15, 0))
        );
    }

    #[test]
    fn display_drops_the_tag() {
        assert_eq!(display("Call Anna @15:00"), "Call Anna");
        assert_eq!(display("Pay @tomorrow 18:15 rent"), "Pay rent");
        assert_eq!(display("@fri"), "");
        assert_eq!(display("Mail bob@x.com"), "Mail bob@x.com");
        assert_eq!(display("Plan @noon"), "Plan @noon");
    }

    #[test]
    fn candidate_detects_tags_not_emails() {
        assert!(candidate("@15:00"));
        assert!(candidate("Call @noon"));
        assert!(candidate("x @2026-10-07"));
        assert!(!candidate(""));
        assert!(!candidate("@"));
        assert!(!candidate("@ later"));
        assert!(!candidate("mail bob@x.com"));
        assert!(!candidate("@,"));
        assert!(!candidate("é@x"));
        assert!(candidate("@é"));
        assert!(!candidate("a @"));
    }

    #[test]
    fn status_pending_fired_invalid_none() {
        let mut n = note("Call @15:00", local(2026, 10, 5, 10, 0));
        assert_eq!(status(&n), Status::Pending("Mon 15:00".into()));
        n.reminder_fired = Some(local(2026, 10, 5, 15, 0).with_timezone(&Utc));
        assert_eq!(status(&n), Status::Fired("Mon 15:00".into()));
        let n = note("Plan @noon", now());
        assert_eq!(status(&n), Status::Invalid);
        let n = note("Plain", now());
        assert_eq!(status(&n), Status::None);
        // Without an anchor a tag is no reminder: invalid, not counted
        // from now.
        let mut n = Note::new(PALETTE[0]);
        n.title = "@15:00".into();
        assert_eq!(status(&n), Status::Invalid);
        n.title = "Plain".into();
        assert_eq!(status(&n), Status::None);
    }

    #[test]
    fn label_shows_weekday_and_time() {
        assert_eq!(label(local(2026, 10, 6, 15, 0)), "Tue 15:00");
    }

    #[test]
    fn relative_tags_count_from_when_they_were_set() {
        let n = note("@15:00", local(2026, 10, 5, 16, 0));
        assert_eq!(at(&n), Some(local(2026, 10, 6, 15, 0)));
        let n = note("@15:00", local(2026, 10, 5, 10, 0));
        assert_eq!(at(&n), Some(local(2026, 10, 5, 15, 0)));
    }

    #[test]
    fn due_respects_fired() {
        let mut n = note("Call @13:30", local(2026, 10, 5, 9, 0));
        let time = local(2026, 10, 5, 13, 30).with_timezone(&Utc);
        assert_eq!(due(&n, local(2026, 10, 5, 13, 29)), None);
        assert_eq!(pending(&n), Some(time));
        assert_eq!(due(&n, now()), Some(time));
        n.reminder_fired = Some(time);
        assert_eq!(due(&n, now()), None);
        assert_eq!(pending(&n), None);
        // Still set, so the header keeps its bell.
        assert!(at(&n).is_some());
        assert_eq!(due(&note("Plain", now()), now()), None);
    }

    #[test]
    fn retyping_a_different_tag_rearms_from_now() {
        // Typed Monday 10:00, fired Tuesday 09:00.
        let mut n = note("Call @tomorrow", local(2026, 10, 5, 10, 0));
        n.reminder_fired = due(&n, local(2026, 10, 6, 9, 0));
        assert!(n.reminder_fired.is_some());
        // Wednesday 10:00: `@9:00` resolves to Thursday 09:00 like
        // `@tomorrow` would now, but it is a new tag.
        let wed = local(2026, 10, 7, 10, 0);
        retitle(&mut n, "Call @9:00".into(), wed);
        assert_eq!(n.reminder_set_at, Some(wed.with_timezone(&Utc)));
        assert_eq!(
            pending(&n),
            Some(local(2026, 10, 8, 9, 0).with_timezone(&Utc))
        );
        // Other title edits keep the anchor.
        retitle(&mut n, "Call Bob @9:00".into(), local(2026, 10, 7, 11, 0));
        assert_eq!(n.reminder_set_at, Some(wed.with_timezone(&Utc)));
        // No tag, no anchor.
        retitle(&mut n, "Call Bob".into(), wed);
        assert_eq!(n.reminder_set_at, None);
    }

    #[test]
    fn legacy_anchor_freezes_at_updated_at() {
        let mut n = note("Call @15:00", now());
        n.reminder_set_at = None;
        assert_eq!(at(&n), None);
        assert!(freeze_anchor(&mut n));
        assert_eq!(n.reminder_set_at, Some(n.updated_at));
        assert!(!freeze_anchor(&mut n));
        let mut plain = note("Plain", now());
        plain.reminder_set_at = None;
        assert!(!freeze_anchor(&mut plain));
    }

    #[test]
    fn editing_reminder_time_rearms() {
        let mut n = note("Call @13:30", local(2026, 10, 5, 9, 0));
        n.reminder_fired = due(&n, now());
        assert!(n.reminder_fired.is_some());
        // Retyped to a later time today: pending again, due once it passes.
        n.title = "Call @14:30".into();
        n.reminder_set_at = Some(now().with_timezone(&Utc));
        assert!(pending(&n).is_some());
        assert_eq!(due(&n, now()), None);
        assert_eq!(
            due(&n, local(2026, 10, 5, 14, 30)),
            Some(local(2026, 10, 5, 14, 30).with_timezone(&Utc))
        );
    }
}
