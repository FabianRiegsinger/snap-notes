//! A note's reminder as a new email, through a `mailto:` link.

use crate::ics;
use crate::note::Note;

/// Longest `mailto:` link handed to the mail app; longer ones get cut off
/// or refused by some of them.
const MAX_URL: usize = 1900;

/// Where a shortened text ends.
const ELLIPSIS: &str = "…";

/// A `mailto:` link with the note's title as the subject and, as the body,
/// `Reminder: <due_label>`, a blank line and the note as plain text. A body
/// too long for [`MAX_URL`] is shortened and ends in "…".
pub fn mailto(note: &Note, due_label: &str) -> String {
    let mut body = format!("Reminder: {due_label}");
    let text = ics::description(note);
    if !text.is_empty() {
        body.push_str("\n\n");
        body.push_str(&text);
    }
    let body = body.replace("\r\n", "\n").replace('\n', "\r\n");
    let fixed = "mailto:?subject=&body=".len();
    // A title alone never takes more than half the room.
    let subject = fit(&ics::summary(note), (MAX_URL - fixed) / 2);
    let room = MAX_URL - fixed - subject.len();
    format!("mailto:?subject={subject}&body={}", fit(&body, room))
}

/// `text` percent-encoded in at most `room` bytes: cut at a character (not
/// inside an escape) and ended with "…" when it is longer.
fn fit(text: &str, room: usize) -> String {
    let full = encode(text);
    if full.len() <= room {
        return full;
    }
    let ellipsis = encode(ELLIPSIS);
    let room = room.saturating_sub(ellipsis.len());
    let mut out = String::new();
    let mut buf = [0; 4];
    for c in text.chars() {
        let piece = encode(c.encode_utf8(&mut buf));
        if out.len() + piece.len() > room {
            break;
        }
        out.push_str(&piece);
    }
    out + &ellipsis
}

/// RFC 3986 percent-encoding of the UTF-8 bytes; only unreserved
/// characters (`A-Z a-z 0-9 - . _ ~`) stay bare.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for &b in text.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    fn note(title: &str, content: &str) -> Note {
        let mut n = Note::new(PALETTE[0]);
        n.title = title.into();
        n.content = content.into();
        n
    }

    /// Every `%` starts a two-hex-digit escape, and only unreserved
    /// characters appear bare.
    fn valid_escapes(query: &str) -> bool {
        let bytes = query.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' => {
                    let hex = |b: u8| b.is_ascii_hexdigit();
                    if i + 2 >= bytes.len() {
                        return false;
                    }
                    if !(hex(bytes[i + 1]) && hex(bytes[i + 2])) {
                        return false;
                    }
                    i += 3;
                }
                b if b.is_ascii_alphanumeric() || b"-._~=&".contains(&b) => i += 1,
                _ => return false,
            }
        }
        true
    }

    #[test]
    fn subject_and_body_are_percent_encoded() {
        let url = mailto(
            &note("Tea & cake? #1+2 grüß", "line one\nline two"),
            "Thu 15 Oct 16:30",
        );
        assert_eq!(
            url,
            "mailto:?subject=Tea%20%26%20cake%3F%20%231%2B2%20gr%C3%BC%C3%9F\
             &body=Reminder%3A%20Thu%2015%20Oct%2016%3A30%0D%0A%0D%0Aline%20one%0D%0Aline%20two"
        );
    }

    #[test]
    fn unreserved_characters_stay_bare() {
        let url = mailto(&note("a-b_c.d~e", ""), "x");
        assert_eq!(url, "mailto:?subject=a-b_c.d~e&body=Reminder%3A%20x");
    }

    #[test]
    fn untitled_note_gets_a_subject() {
        let url = mailto(&note("", "**hi**"), "Mon 9:00");
        assert!(
            url.starts_with("mailto:?subject=Untitled%20note&body="),
            "{url}"
        );
        assert!(url.ends_with("%0D%0A%0D%0Ahi"), "{url}");
    }

    #[test]
    fn long_body_is_shortened_to_fit() {
        let body = "ünd 😀 text; ".repeat(800);
        assert!(body.len() > 10_000);
        let url = mailto(&note("Long", &body), "Thu 15 Oct 16:30");
        assert!(url.len() <= MAX_URL, "{} chars", url.len());
        assert!(url.len() > MAX_URL - 20, "uses the room: {}", url.len());
        assert!(url.ends_with("%E2%80%A6"), "ends with an ellipsis");
        let query = url.strip_prefix("mailto:?").unwrap();
        assert!(valid_escapes(query));
        assert!(query.contains("body=Reminder%3A%20Thu%2015%20Oct%2016%3A30%0D%0A%0D%0A%C3%BCnd"));
    }

    #[test]
    fn long_title_still_fits() {
        let url = mailto(&note(&"ü".repeat(2000), "body"), "x");
        assert!(url.len() <= MAX_URL, "{} chars", url.len());
        assert!(valid_escapes(url.strip_prefix("mailto:?").unwrap()));
    }
}
