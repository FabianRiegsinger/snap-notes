//! Markdown styling for the body editor: markup stays visible but faint,
//! and the text it marks takes the style it will have when formatted.
//! Works line by line; only a fenced code block carries over between lines.

use crate::note::NoteColor;
use crate::rich::{self, OpenTag};
use crate::theme;
use iced::advanced::text::highlighter::{self, Format};
use iced::font;
use iced::{Color, Font};
use std::ops::Range;

/// How a range of an editor line is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Style {
    Plain,
    /// Markup: drawn faint.
    Marker,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Color(NoteColor),
    Link,
    Heading,
    /// A line inside a fenced code block.
    CodeBlock,
}

/// Styles one editor line. `in_code_block` says whether a fenced code block
/// is open before the line; the returned flag says whether one is open after
/// it. Unstyled text gets no range.
pub fn spans(
    line: &str,
    in_code_block: bool,
    palette: &[NoteColor],
) -> (Vec<(Range<usize>, Style)>, bool) {
    let mut scan = Scan {
        line,
        palette,
        out: Vec::new(),
        heading: false,
        bold: None,
        italic: None,
        strike: false,
        tags: Vec::new(),
        link: None,
    };
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let trimmed = &line[indent..];
    let fence = indent <= 3 && (trimmed.starts_with("```") || trimmed.starts_with("~~~"));
    if fence {
        scan.push(0..line.len(), Style::Marker);
        return (scan.out, !in_code_block);
    }
    if in_code_block {
        scan.push(0..line.len(), Style::CodeBlock);
        return (scan.out, true);
    }
    if is_rule(trimmed) {
        scan.push(0..line.len(), Style::Marker);
        return (scan.out, false);
    }
    let start = scan.block_prefix(indent);
    scan.inline(start);
    (scan.out, false)
}

/// A thematic break: three or more `-`, `*` or `_`, optionally spaced.
fn is_rule(trimmed: &str) -> bool {
    let Some(c) = trimmed
        .chars()
        .next()
        .filter(|c| matches!(c, '-' | '*' | '_'))
    else {
        return false;
    };
    trimmed.chars().all(|ch| ch == c || ch == ' ' || ch == '\t')
        && trimmed.chars().filter(|&ch| ch == c).count() >= 3
}

struct Scan<'a> {
    line: &'a str,
    palette: &'a [NoteColor],
    out: Vec<(Range<usize>, Style)>,
    heading: bool,
    /// The delimiter character of the open bold or italic run.
    bold: Option<char>,
    italic: Option<char>,
    strike: bool,
    tags: Vec<OpenTag>,
    /// An open link: where its text ends (at `]`) and where `(url)` ends.
    link: Option<(usize, usize)>,
}

impl Scan<'_> {
    /// Adds a styled range, merging it into the previous one when they touch
    /// and match. Plain text gets no range.
    fn push(&mut self, range: Range<usize>, style: Style) {
        if range.is_empty() || style == Style::Plain {
            return;
        }
        match self.out.last_mut() {
            Some((last, s)) if *s == style && last.end == range.start => last.end = range.end,
            _ => self.out.push((range, style)),
        }
    }

    fn style(&self) -> Style {
        let mut color = None;
        for tag in &self.tags {
            if let OpenTag::Color(c) = tag {
                color = *c;
            }
        }
        if let Some(c) = color {
            return Style::Color(c);
        }
        if self.link.is_some() {
            return Style::Link;
        }
        match (self.bold.is_some(), self.italic.is_some()) {
            (true, true) => Style::BoldItalic,
            (true, false) => Style::Bold,
            (false, true) if self.heading => Style::BoldItalic,
            (false, true) => Style::Italic,
            (false, false) if self.heading => Style::Heading,
            (false, false) => Style::Plain,
        }
    }

    fn literal(&mut self, range: Range<usize>) {
        let style = self.style();
        self.push(range, style);
    }

    /// Marks the quote, heading, list and task markers at the start of the
    /// line, returning where its text starts.
    fn block_prefix(&mut self, indent: usize) -> usize {
        let bytes = self.line.as_bytes();
        let mut pos = indent;
        while bytes.get(pos) == Some(&b'>') {
            let quote = pos;
            pos += 1;
            while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
                pos += 1;
            }
            self.push(quote..pos, Style::Marker);
        }
        let hashes = bytes[pos..].iter().take_while(|&&b| b == b'#').count();
        if (1..=6).contains(&hashes) && matches!(bytes.get(pos + hashes), None | Some(b' ')) {
            let end = (pos + hashes + 1).min(bytes.len());
            self.push(pos..end, Style::Marker);
            self.heading = true;
            return end;
        }
        if matches!(bytes.get(pos), Some(b'-' | b'*' | b'+')) && bytes.get(pos + 1) == Some(&b' ') {
            self.push(pos..pos + 2, Style::Marker);
            pos += 2;
            let rest = &self.line[pos..];
            for task in ["[ ]", "[x]", "[X]"] {
                if rest == task || rest.starts_with(&format!("{task} ")) {
                    let end = (pos + 4).min(bytes.len());
                    self.push(pos..end, Style::Marker);
                    return end;
                }
            }
        }
        pos
    }

    fn inline(&mut self, start: usize) {
        let line = self.line;
        let mut i = start;
        while i < line.len() {
            if let Some((text_end, end)) = self.link {
                if i >= text_end {
                    self.push(i..end.max(i), Style::Marker);
                    self.link = None;
                    i = i.max(end);
                    continue;
                }
            }
            let rest = &line[i..];
            let Some(c) = rest.chars().next() else {
                break;
            };
            i = match c {
                '\\' => self.escape(i),
                '`' => self.code(i),
                '*' | '_' => self.emphasis(i, c),
                '~' if rest.starts_with("~~") => self.strike(i),
                '=' if rest.starts_with("==") => self.highlight(i),
                '{' => self.tag(i),
                '[' => self.link_start(i),
                '!' if rest.starts_with("![") => self.image(i),
                '<' => self.autolink(i),
                _ => {
                    self.literal(i..i + c.len_utf8());
                    i + c.len_utf8()
                }
            };
        }
    }

    fn prev_char(&self, i: usize) -> Option<char> {
        self.line[..i].chars().next_back()
    }

    fn next_char(&self, i: usize) -> Option<char> {
        self.line[i..].chars().next()
    }

    /// A backslash before ASCII punctuation is markup; the punctuation is text.
    fn escape(&mut self, i: usize) -> usize {
        match self.next_char(i + 1) {
            Some(c) if c.is_ascii_punctuation() => {
                self.push(i..i + 1, Style::Marker);
                self.literal(i + 1..i + 2);
                i + 2
            }
            _ => {
                self.literal(i..i + 1);
                i + 1
            }
        }
    }

    fn code(&mut self, i: usize) -> usize {
        let line = self.line;
        let n = line[i..].bytes().take_while(|&b| b == b'`').count();
        let body = i + n;
        let mut from = body;
        while let Some(found) = line[from..].find('`') {
            let at = from + found;
            let run = line[at..].bytes().take_while(|&b| b == b'`').count();
            if run == n {
                self.push(i..body, Style::Marker);
                self.push(body..at, Style::Code);
                self.push(at..at + n, Style::Marker);
                return at + n;
            }
            from = at + run;
        }
        self.literal(i..body);
        body
    }

    /// Whether `delim` closes later in the line, after `from`: preceded by
    /// non-whitespace and, for `_`, not followed by a word character.
    fn closes(&self, from: usize, delim: &str) -> bool {
        let tail = &self.line[from..];
        tail.match_indices(delim).any(|(at, _)| {
            let at = from + at;
            let flanked = at > from && self.prev_char(at).is_some_and(|c| !c.is_whitespace());
            let word_after = self
                .next_char(at + delim.len())
                .is_some_and(char::is_alphanumeric);
            flanked && !(delim.starts_with('_') && word_after)
        })
    }

    fn emphasis(&mut self, i: usize, c: char) -> usize {
        let n = self.line[i..].chars().take_while(|&ch| ch == c).count();
        let prev = self.prev_char(i);
        let next = self.next_char(i + n);
        let underscore = c == '_';
        let can_close = prev.is_some_and(|p| !p.is_whitespace())
            && !(underscore && next.is_some_and(char::is_alphanumeric));
        let can_open = next.is_some_and(|ch| !ch.is_whitespace())
            && !(underscore && prev.is_some_and(char::is_alphanumeric));
        if can_close {
            let close = if n >= 3 && self.bold == Some(c) && self.italic == Some(c) {
                self.bold = None;
                self.italic = None;
                3
            } else if n >= 2 && self.bold == Some(c) {
                self.bold = None;
                2
            } else if self.italic == Some(c) {
                self.italic = None;
                1
            } else {
                0
            };
            if close > 0 {
                self.push(i..i + close, Style::Marker);
                return i + close;
            }
        }
        if can_open {
            let delim = |k: usize| c.to_string().repeat(k);
            let open = if n >= 3
                && self.bold.is_none()
                && self.italic.is_none()
                && self.closes(i + 3, &delim(3))
            {
                self.bold = Some(c);
                self.italic = Some(c);
                3
            } else if n >= 2 && self.bold.is_none() && self.closes(i + 2, &delim(2)) {
                self.bold = Some(c);
                2
            } else if self.italic.is_none() && self.closes(i + 1, &delim(1)) {
                self.italic = Some(c);
                1
            } else {
                0
            };
            if open > 0 {
                self.push(i..i + open, Style::Marker);
                return i + open;
            }
        }
        self.literal(i..i + n);
        i + n
    }

    fn strike(&mut self, i: usize) -> usize {
        let flanked_before = self.prev_char(i).is_some_and(|c| !c.is_whitespace());
        let flanked_after = self.next_char(i + 2).is_some_and(|c| !c.is_whitespace());
        if self.strike && flanked_before {
            self.strike = false;
        } else if !self.strike && flanked_after && self.closes(i + 2, "~~") {
            self.strike = true;
        } else {
            self.literal(i..i + 2);
            return i + 2;
        }
        self.push(i..i + 2, Style::Marker);
        i + 2
    }

    /// `==text==`, as the parser reads it: it opens before non-whitespace
    /// that a `==` closes, and closes after non-whitespace.
    fn highlight(&mut self, i: usize) -> usize {
        let open = self
            .tags
            .iter()
            .rposition(|t| matches!(t, OpenTag::Highlight));
        let flanks = match open {
            Some(_) => self.prev_char(i).is_some_and(|c| !c.is_whitespace()),
            None => {
                self.next_char(i + 2).is_some_and(|c| !c.is_whitespace())
                    && self.closes(i + 2, "==")
            }
        };
        if !flanks {
            self.literal(i..i + 2);
            return i + 2;
        }
        match open {
            Some(at) => {
                self.tags.remove(at);
            }
            None => self.tags.push(OpenTag::Highlight),
        }
        self.push(i..i + 2, Style::Marker);
        i + 2
    }

    /// A `{...}` tag or `{/}`; anything else in braces is text.
    fn tag(&mut self, i: usize) -> usize {
        if let Some(len) = self.line[i..].find('}') {
            let inner = &self.line[i + 1..i + len];
            let tag = if inner == "/" {
                self.tags.pop();
                true
            } else if let Some(tag) = rich::parse_tag(inner, self.palette) {
                self.tags.push(tag);
                true
            } else {
                false
            };
            if tag {
                self.push(i..i + len + 1, Style::Marker);
                return i + len + 1;
            }
        }
        self.literal(i..i + 1);
        i + 1
    }

    /// Where `[text](url)` starting at `i` has its `]` and ends.
    fn link_bounds(&self, i: usize) -> Option<(usize, usize)> {
        let text_end = i + self.line[i..].find(']')?;
        if self.line.as_bytes().get(text_end + 1) != Some(&b'(') {
            return None;
        }
        let end = text_end + 1 + self.line[text_end + 1..].find(')')? + 1;
        Some((text_end, end))
    }

    fn link_start(&mut self, i: usize) -> usize {
        match self.link.is_none().then(|| self.link_bounds(i)).flatten() {
            Some(bounds) => {
                self.push(i..i + 1, Style::Marker);
                self.link = Some(bounds);
            }
            None => self.literal(i..i + 1),
        }
        i + 1
    }

    /// An image shows as itself in the formatted view; all of its source is markup.
    fn image(&mut self, i: usize) -> usize {
        match self.link_bounds(i + 1) {
            Some((_, end)) => {
                self.push(i..end, Style::Marker);
                end
            }
            None => {
                self.literal(i..i + 1);
                i + 1
            }
        }
    }

    /// `<https://…>` or `<name@host>`.
    fn autolink(&mut self, i: usize) -> usize {
        let inner_end = self.line[i + 1..].find('>').map(|len| i + 1 + len);
        if let Some(end) = inner_end {
            let inner = &self.line[i + 1..end];
            let valid = !inner.is_empty()
                && !inner.contains(|c: char| c.is_whitespace() || c == '<')
                && (inner.contains("://") || inner.starts_with("mailto:") || inner.contains('@'));
            if valid {
                self.push(i..i + 1, Style::Marker);
                self.push(i + 1..end, Style::Link);
                self.push(end..end + 1, Style::Marker);
                return end + 1;
            }
        }
        self.literal(i..i + 1);
        i + 1
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HighlightSettings {
    pub palette: Vec<NoteColor>,
    /// Not read while highlighting; a mode change re-highlights the text so
    /// [`format`] picks up the new theme.
    pub mode: theme::Mode,
}

/// The editor's highlighter, built on [`spans`].
pub struct Highlighter {
    palette: Vec<NoteColor>,
    /// Whether a code block is open after each line highlighted so far.
    open_after: Vec<bool>,
}

impl highlighter::Highlighter for Highlighter {
    type Settings = HighlightSettings;
    type Highlight = Style;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Style)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            palette: settings.palette.clone(),
            open_after: Vec::new(),
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.palette = new_settings.palette.clone();
        self.open_after.clear();
    }

    fn change_line(&mut self, line: usize) {
        self.open_after.truncate(line);
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let open_before = self.open_after.last().copied().unwrap_or(false);
        let (spans, open_after) = spans(line, open_before, &self.palette);
        self.open_after.push(open_after);
        spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.open_after.len()
    }
}

/// How a [`Style`] looks under the app's iced theme, which `main.rs` derives
/// from our light or dark mode. Plain and font-only styles keep the editor's
/// own text color.
pub fn format(style: &Style, theme: &iced::Theme) -> Format<Font> {
    let mode = theme::Mode::from(iced::theme::Base::mode(theme));
    let ours = theme::Theme::new(mode);
    let body = |weight, style| Font {
        weight,
        style,
        ..theme::BODY_FONT
    };
    let (color, font) = match *style {
        Style::Plain => (None, None),
        Style::Marker => (Some(ours.ink(0.35)), None),
        Style::Bold | Style::Heading => (None, Some(body(font::Weight::Bold, font::Style::Normal))),
        Style::Italic => (None, Some(body(font::Weight::Normal, font::Style::Italic))),
        Style::BoldItalic => (None, Some(body(font::Weight::Bold, font::Style::Italic))),
        Style::Code | Style::CodeBlock => (None, Some(Font::MONOSPACE)),
        Style::Color(c) => {
            let [r, g, b, a] = c.rgba;
            (Some(Color::from_rgba(r, g, b, a)), None)
        }
        // The formatted view draws link text in the body ink, underlined.
        Style::Link => (Some(ours.ink(0.9)), None),
    };
    Format { color, font }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    /// The style of each byte of `line`, `Plain` where no span covers it,
    /// and whether a code block is open after it.
    fn styles(line: &str, in_code_block: bool) -> (Vec<Style>, bool) {
        let (spans, open) = spans(line, in_code_block, &PALETTE);
        let mut out = vec![Style::Plain; line.len()];
        for (range, style) in spans {
            for s in &mut out[range] {
                *s = style;
            }
        }
        (out, open)
    }

    /// The style of `line[range]`, which must be uniform.
    fn style_at(line: &str, range: Range<usize>) -> Style {
        let (all, _) = styles(line, false);
        let first = all[range.start];
        assert!(
            all[range.clone()].iter().all(|s| *s == first),
            "{:?} in {line:?} is not uniformly styled: {:?}",
            &line[range.clone()],
            &all[range]
        );
        first
    }

    #[test]
    fn bold_text_and_faint_markers() {
        let line = "a **b** c";
        assert_eq!(style_at(line, 0..2), Style::Plain);
        assert_eq!(style_at(line, 2..4), Style::Marker);
        assert_eq!(style_at(line, 4..5), Style::Bold);
        assert_eq!(style_at(line, 5..7), Style::Marker);
        assert_eq!(style_at(line, 7..9), Style::Plain);
        assert!(!styles(line, false).1);
    }

    #[test]
    fn italic_code_and_color() {
        let line = "*i* `c` {coral}r{/}";
        assert_eq!(style_at(line, 0..1), Style::Marker);
        assert_eq!(style_at(line, 1..2), Style::Italic);
        assert_eq!(style_at(line, 2..3), Style::Marker);
        assert_eq!(style_at(line, 4..5), Style::Marker);
        assert_eq!(style_at(line, 5..6), Style::Code);
        assert_eq!(style_at(line, 6..7), Style::Marker);
        assert_eq!(style_at(line, 8..15), Style::Marker);
        assert_eq!(style_at(line, 15..16), Style::Color(PALETTE[0]));
        assert_eq!(style_at(line, 16..19), Style::Marker);
    }

    #[test]
    fn hex_and_unknown_tags() {
        let line = "{#ff0000}a{/} {nope}b";
        assert_eq!(style_at(line, 0..9), Style::Marker);
        assert_eq!(
            style_at(line, 9..10),
            Style::Color(NoteColor::parse_hex("#ff0000").unwrap())
        );
        // An unknown tag is literal text.
        assert_eq!(style_at(line, 14..21), Style::Plain);
        // An escaped tag is literal; only the backslash is markup.
        let line = "\\{coral}x";
        assert_eq!(style_at(line, 0..1), Style::Marker);
        assert_eq!(style_at(line, 1..9), Style::Plain);
        // A color naming a slot the palette lacks is still markup.
        let (spans, _) = spans("{slate}x", false, &PALETTE[..2]);
        assert!(spans.contains(&(0..7, Style::Marker)));
        assert!(!spans.iter().any(|(_, s)| matches!(s, Style::Color(_))));
    }

    #[test]
    fn heading_and_quote_markers() {
        let line = "## Title";
        assert_eq!(style_at(line, 0..3), Style::Marker);
        assert_eq!(style_at(line, 3..8), Style::Heading);
        let line = "> quoted";
        assert_eq!(style_at(line, 0..2), Style::Marker);
        assert_eq!(style_at(line, 2..8), Style::Plain);
        let line = "- [ ] task";
        assert_eq!(style_at(line, 0..6), Style::Marker);
        assert_eq!(style_at(line, 6..10), Style::Plain);
        // Without the space it is not a heading.
        assert_eq!(style_at("#tag", 0..4), Style::Plain);
    }

    #[test]
    fn link_text_and_url_markers() {
        let line = "[x](https://e.com)";
        assert_eq!(style_at(line, 0..1), Style::Marker);
        assert_eq!(style_at(line, 1..2), Style::Link);
        assert_eq!(style_at(line, 2..line.len()), Style::Marker);
        // Brackets without a destination are text.
        assert_eq!(style_at("[x] y", 0..5), Style::Plain);
    }

    #[test]
    fn fenced_code_block_spans_lines() {
        let (fence, open) = styles("```rust", false);
        assert!(open);
        assert!(fence.iter().all(|s| *s == Style::Marker));
        let (body, open) = styles("let **a** = 1;", true);
        assert!(open);
        assert!(body.iter().all(|s| *s == Style::CodeBlock));
        let (close, open) = styles("```", true);
        assert!(!open);
        assert!(close.iter().all(|s| *s == Style::Marker));
    }

    #[test]
    fn unmatched_markers_stay_plain() {
        for line in [
            "a ** b",
            "**a",
            "a*",
            "`a",
            "~~a",
            "==a",
            "{coral",
            "[a](b",
            "snake_case_name",
            "2 * 3 * 4",
        ] {
            let (all, open) = styles(line, false);
            assert!(!open);
            assert!(
                all.iter().all(|s| *s == Style::Plain),
                "{line:?} got {all:?}"
            );
        }
    }

    #[test]
    fn highlighter_tracks_code_blocks_across_edits() {
        use iced::advanced::text::Highlighter as _;
        let settings = HighlightSettings {
            palette: PALETTE.to_vec(),
            mode: theme::Mode::Light,
        };
        let mut h = Highlighter::new(&settings);
        let lines = ["a", "```", "**b**", "```", "**c**"];
        let mut all: Vec<Vec<(Range<usize>, Style)>> = lines
            .iter()
            .map(|l| h.highlight_line(l).collect())
            .collect();
        assert_eq!(h.current_line(), 5);
        assert!(all[2].iter().all(|(_, s)| *s == Style::CodeBlock));
        assert!(all[4].iter().any(|(_, s)| *s == Style::Bold));

        // Turning the opening fence into text reopens the rest from there.
        h.change_line(1);
        assert_eq!(h.current_line(), 1);
        let edited = ["x", "**b**", "```", "**c**"];
        for (i, l) in edited.iter().enumerate() {
            all[i + 1] = h.highlight_line(l).collect();
        }
        assert!(all[2].iter().any(|(_, s)| *s == Style::Bold));
        // The old closing fence now opens a block.
        assert!(all[4].iter().all(|(_, s)| *s == Style::CodeBlock));

        // New settings re-highlight from the top.
        h.update(&HighlightSettings {
            mode: theme::Mode::Dark,
            ..settings
        });
        assert_eq!(h.current_line(), 0);
    }

    #[test]
    fn format_maps_styles_to_theme() {
        let light = theme::Theme::new(theme::Mode::Light);
        let dark = theme::Theme::new(theme::Mode::Dark);
        assert_eq!(
            format(&Style::Plain, &iced::Theme::Light),
            Format::default()
        );
        assert_eq!(
            format(&Style::Marker, &iced::Theme::Light).color,
            Some(light.ink(0.35))
        );
        assert_eq!(
            format(&Style::Marker, &iced::Theme::Dark).color,
            Some(dark.ink(0.35))
        );
        let [r, g, b, _] = PALETTE[3].rgba;
        assert_eq!(
            format(&Style::Color(PALETTE[3]), &iced::Theme::Dark).color,
            Some(Color::from_rgb(r, g, b))
        );
        let bold = format(&Style::Bold, &iced::Theme::Light).font.unwrap();
        assert_eq!(bold.weight, font::Weight::Bold);
        assert_eq!(bold.family, theme::BODY_FONT.family);
        let italic = format(&Style::Italic, &iced::Theme::Light).font.unwrap();
        assert_eq!(italic.style, font::Style::Italic);
        assert_eq!(
            format(&Style::Code, &iced::Theme::Light).font,
            Some(Font::MONOSPACE)
        );
        assert_eq!(
            format(&Style::CodeBlock, &iced::Theme::Light).font,
            Some(Font::MONOSPACE)
        );
    }

    #[test]
    fn multibyte_ranges_are_char_boundaries() {
        for line in [
            "é**ü**é",
            "*ß* `ö` {coral}ä{/}",
            "## Überschrift",
            "[ä](ö)",
            "~~é~~ ==ü==",
            "→ *→",
            "\\é",
        ] {
            let (spans, _) = spans(line, false, &PALETTE);
            for (range, _) in spans {
                assert!(line.is_char_boundary(range.start), "{line:?} {range:?}");
                assert!(line.is_char_boundary(range.end), "{line:?} {range:?}");
                assert!(range.start < range.end && range.end <= line.len());
            }
        }
    }

    #[test]
    fn hostile_lines_do_not_panic() {
        for input in [
            "{",
            "}",
            "{/",
            "{size:",
            "{bg:}",
            "==",
            "====",
            "{#",
            "[](",
            "![",
            "- [",
            "\u{0}",
            "é{coral}é{/}é",
            "\r\n# a\r\n",
            "<",
            "<!--",
            "- <div>\n  x",
            "> a\n> ![](x)\n> b",
            "a\r\n![](x)\r\nb",
            "![](x)\n![](y)\n",
            "<a@b>",
            "***",
            "`",
            "``a`",
            "\\",
            "#",
            "[[x](y)](z)",
            "{/}{/}{/}",
        ] {
            for line in input.split('\n') {
                for in_code in [false, true] {
                    spans(line, in_code, &PALETTE);
                    spans(line, in_code, &[]);
                }
            }
        }
    }
}
