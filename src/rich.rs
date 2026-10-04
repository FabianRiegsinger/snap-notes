use crate::note::NoteColor;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Palette slot used by the `==text==` highlight shorthand.
const HIGHLIGHT_SLOT: usize = 5;

/// Tag color names, in palette slot order.
pub const COLOR_NAMES: [&str; 20] = [
    "coral",
    "rose",
    "blush",
    "peach",
    "tangerine",
    "amber",
    "lemon",
    "sand",
    "lime",
    "sage",
    "mint",
    "teal",
    "aqua",
    "sky",
    "cornflower",
    "periwinkle",
    "lavender",
    "orchid",
    "mocha",
    "slate",
];

/// A parsed note: a flat list of blocks.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

/// What kind of block a `Block` is.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Paragraph,
    /// Level 1 to 3; deeper Markdown headings are clamped to 3.
    Heading(u8),
    ListItem {
        ordered: Option<u64>,
        depth: usize,
        task: Option<bool>,
    },
    Quote,
    CodeBlock,
    Image {
        path: String,
        alt: String,
    },
    Rule,
}

/// One block of styled text and the 0-based source line it starts on.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
    pub source_line: usize,
}

/// A run of text with uniform styling.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub color: Option<NoteColor>,
    pub background: Option<NoteColor>,
    pub size: Option<f32>,
    pub link: Option<String>,
}

impl Span {
    fn same_style(&self, other: &Span) -> bool {
        Span {
            text: String::new(),
            ..self.clone()
        } == Span {
            text: String::new(),
            ..other.clone()
        }
    }
}

/// An open `{...}` tag. A color naming a slot the palette lacks is `None`.
enum OpenTag {
    Color(Option<NoteColor>),
    Background(Option<NoteColor>),
    Size(f32),
    Highlight,
}

struct Builder<'a> {
    content: &'a str,
    palette: &'a [NoteColor],
    blocks: Vec<Block>,
    cur: Option<Block>,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
    tags: Vec<OpenTag>,
    list_stack: Vec<Option<u64>>,
    quote_depth: u32,
    image: Option<(String, String, usize)>,
    in_code_block: bool,
}

impl<'a> Builder<'a> {
    fn line_at(&self, offset: usize) -> usize {
        self.content
            .as_bytes()
            .iter()
            .take(offset)
            .filter(|&&b| b == b'\n')
            .count()
    }

    /// Ends the current block and drops any open tags.
    fn flush(&mut self) {
        if let Some(block) = self.cur.take() {
            // An image splits its paragraph and can leave an empty one behind.
            let empty_text = matches!(block.kind, BlockKind::Paragraph | BlockKind::Quote)
                && block.spans.is_empty();
            if !empty_text {
                self.blocks.push(block);
            }
        }
        self.tags.clear();
    }

    fn start(&mut self, kind: BlockKind, offset: usize) {
        self.flush();
        self.cur = Some(Block {
            kind,
            spans: Vec::new(),
            source_line: self.line_at(offset),
        });
    }

    fn ensure_block(&mut self, offset: usize) {
        if self.cur.is_none() {
            let kind = if self.quote_depth > 0 {
                BlockKind::Quote
            } else {
                BlockKind::Paragraph
            };
            self.start(kind, offset);
        }
    }

    fn style(&self) -> Span {
        let mut span = Span {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            link: self.links.last().cloned(),
            ..Span::default()
        };
        for tag in &self.tags {
            match tag {
                OpenTag::Color(c) => span.color = *c,
                OpenTag::Background(c) => span.background = *c,
                OpenTag::Size(s) => span.size = Some(*s),
                OpenTag::Highlight => span.background = self.palette.get(HIGHLIGHT_SLOT).copied(),
            }
        }
        span
    }

    fn push_span(&mut self, mut span: Span, text: &str, offset: usize) {
        if text.is_empty() {
            return;
        }
        self.ensure_block(offset);
        span.text = text.to_string();
        let Some(block) = self.cur.as_mut() else {
            return;
        };
        match block.spans.last_mut() {
            Some(last) if last.same_style(&span) => last.text.push_str(text),
            _ => block.spans.push(span),
        }
    }

    fn push_plain(&mut self, text: &str, offset: usize) {
        let span = self.style();
        self.push_span(span, text, offset);
    }

    fn resolve_color(&self, name: &str) -> Option<Option<NoteColor>> {
        if name.starts_with('#') {
            return NoteColor::parse_hex(name).map(Some);
        }
        COLOR_NAMES
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .map(|slot| self.palette.get(slot).copied())
    }

    /// Parses the inside of a `{...}` tag; `None` means it is not a tag.
    fn parse_tag(&self, inner: &str) -> Option<OpenTag> {
        if let Some(value) = inner.strip_prefix("size:") {
            let size: f32 = value.parse().ok()?;
            return size
                .is_finite()
                .then(|| OpenTag::Size(size.clamp(8.0, 48.0)));
        }
        if let Some(value) = inner.strip_prefix("bg:") {
            return self.resolve_color(value).map(OpenTag::Background);
        }
        self.resolve_color(inner).map(OpenTag::Color)
    }

    /// Scans a Markdown text event for `{...}` tags and `==` highlights.
    fn push_text(&mut self, text: &str, range_start: usize, range_end: usize) {
        let escaped = text.starts_with('{') && {
            let backslashes = self.content.as_bytes()[..range_start]
                .iter()
                .rev()
                .take_while(|&&b| b == b'\\')
                .count();
            backslashes % 2 == 1
        };
        let mut literal = String::new();
        let mut rest = text;
        let mut first = true;
        while !rest.is_empty() {
            if rest.starts_with('{') && !(first && escaped) {
                if let Some(end) = rest.find('}') {
                    let inner = &rest[1..end];
                    if inner == "/" {
                        self.flush_literal(&mut literal, range_start);
                        self.tags.pop();
                        rest = &rest[end + 1..];
                        first = false;
                        continue;
                    }
                    if let Some(tag) = self.parse_tag(inner) {
                        self.flush_literal(&mut literal, range_start);
                        self.tags.push(tag);
                        rest = &rest[end + 1..];
                        first = false;
                        continue;
                    }
                }
            }
            if rest.starts_with("==") {
                let consumed = text.len() - rest.len();
                let prev = text[..consumed]
                    .chars()
                    .next_back()
                    .or_else(|| self.content[..range_start].chars().next_back());
                let open = self
                    .tags
                    .iter()
                    .rposition(|t| matches!(t, OpenTag::Highlight));
                let flanks = match open {
                    Some(_) => prev.is_some_and(|c| !c.is_whitespace()),
                    None => {
                        let next = rest[2..]
                            .chars()
                            .next()
                            .or_else(|| self.content[range_end..].chars().next());
                        next.is_some_and(|c| !c.is_whitespace())
                            && self.has_closing_highlight(range_start + consumed + 2)
                    }
                };
                if flanks {
                    self.flush_literal(&mut literal, range_start);
                    match open {
                        Some(i) => {
                            self.tags.remove(i);
                        }
                        None => self.tags.push(OpenTag::Highlight),
                    }
                    rest = &rest[2..];
                    first = false;
                    continue;
                }
            }
            let ch = rest.chars().next().unwrap_or('\0');
            literal.push(ch);
            rest = &rest[ch.len_utf8().max(1)..];
            first = false;
        }
        self.flush_literal(&mut literal, range_start);
    }

    /// Whether a `==` preceded by non-whitespace follows `from` within the same block.
    fn has_closing_highlight(&self, from: usize) -> bool {
        let Some(tail) = self.content.get(from..) else {
            return false;
        };
        let block = tail.split("\n\n").next().unwrap_or(tail);
        block.match_indices("==").any(|(i, _)| {
            let before = if i == 0 {
                &self.content[..from]
            } else {
                &block[..i]
            };
            before
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
        })
    }

    fn flush_literal(&mut self, literal: &mut String, offset: usize) {
        if !literal.is_empty() {
            self.push_plain(literal, offset);
            literal.clear();
        }
    }
}

/// Parses note content (Markdown plus `{color}`/`{size:N}` tags) into blocks.
/// Palette lookups are by slot, so a short palette yields no color rather than an error.
pub fn parse(content: &str, palette: &[NoteColor]) -> Doc {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut b = Builder {
        content,
        palette,
        blocks: Vec::new(),
        cur: None,
        bold: 0,
        italic: 0,
        strike: 0,
        links: Vec::new(),
        tags: Vec::new(),
        list_stack: Vec::new(),
        quote_depth: 0,
        image: None,
        in_code_block: false,
    };

    for (event, range) in Parser::new_ext(content, options).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    // Paragraphs inside a list item continue that item.
                    let in_item = matches!(
                        b.cur.as_ref().map(|c| &c.kind),
                        Some(BlockKind::ListItem { .. })
                    );
                    if !in_item {
                        let kind = if b.quote_depth > 0 {
                            BlockKind::Quote
                        } else {
                            BlockKind::Paragraph
                        };
                        b.start(kind, range.start);
                    }
                }
                Tag::Heading { level, .. } => {
                    b.start(BlockKind::Heading((level as u8).min(3)), range.start)
                }
                Tag::BlockQuote(_) => {
                    b.flush();
                    b.quote_depth += 1;
                }
                Tag::CodeBlock(_) => {
                    b.start(BlockKind::CodeBlock, range.start);
                    b.in_code_block = true;
                }
                Tag::List(first) => {
                    b.flush();
                    b.list_stack.push(first);
                }
                Tag::Item => {
                    let depth = b.list_stack.len().saturating_sub(1);
                    let ordered = match b.list_stack.last_mut() {
                        Some(Some(n)) => {
                            let current = *n;
                            *n += 1;
                            Some(current)
                        }
                        _ => None,
                    };
                    b.start(
                        BlockKind::ListItem {
                            ordered,
                            depth,
                            task: None,
                        },
                        range.start,
                    );
                }
                Tag::Emphasis => b.italic += 1,
                Tag::Strong => b.bold += 1,
                Tag::Strikethrough => b.strike += 1,
                Tag::Link { dest_url, .. } => b.links.push(dest_url.to_string()),
                Tag::Image { dest_url, .. } => {
                    b.flush();
                    b.image = Some((dest_url.to_string(), String::new(), b.line_at(range.start)));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item => b.flush(),
                TagEnd::CodeBlock => {
                    b.in_code_block = false;
                    if let Some(block) = b.cur.as_mut() {
                        if let Some(last) = block.spans.last_mut() {
                            if last.text.ends_with('\n') {
                                last.text.pop();
                            }
                        }
                    }
                    b.flush();
                }
                TagEnd::BlockQuote(_) => {
                    b.flush();
                    b.quote_depth = b.quote_depth.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    b.flush();
                    b.list_stack.pop();
                }
                TagEnd::Emphasis => b.italic = b.italic.saturating_sub(1),
                TagEnd::Strong => b.bold = b.bold.saturating_sub(1),
                TagEnd::Strikethrough => b.strike = b.strike.saturating_sub(1),
                TagEnd::Link => {
                    b.links.pop();
                }
                TagEnd::Image => {
                    if let Some((path, alt, line)) = b.image.take() {
                        b.blocks.push(Block {
                            kind: BlockKind::Image { path, alt },
                            spans: Vec::new(),
                            source_line: line,
                        });
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some((_, alt, _)) = b.image.as_mut() {
                    alt.push_str(&text);
                } else if b.in_code_block {
                    let span = Span {
                        code: true,
                        ..Span::default()
                    };
                    b.push_span(span, &text, range.start);
                } else {
                    b.push_text(&text, range.start, range.end);
                }
            }
            Event::Code(text) => {
                let span = Span {
                    code: true,
                    ..b.style()
                };
                b.push_span(span, &text, range.start);
            }
            Event::SoftBreak | Event::HardBreak => b.push_plain("\n", range.start),
            Event::TaskListMarker(done) => {
                if let Some(Block {
                    kind: BlockKind::ListItem { task, .. },
                    ..
                }) = b.cur.as_mut()
                {
                    *task = Some(done);
                }
            }
            Event::Rule => {
                b.flush();
                let line = b.line_at(range.start);
                b.blocks.push(Block {
                    kind: BlockKind::Rule,
                    spans: Vec::new(),
                    source_line: line,
                });
            }
            _ => {}
        }
    }
    b.flush();
    Doc { blocks: b.blocks }
}

/// The note as plain text: markers and tags removed, one line per block
/// (plus one per line break inside a block).
pub fn plain_text(content: &str) -> String {
    let doc = parse(content, &[]);
    let lines: Vec<String> = doc
        .blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Image { .. } => "🖼".to_string(),
            kind => {
                let prefix = match kind {
                    BlockKind::ListItem {
                        task: Some(done), ..
                    } => {
                        if *done {
                            "☑ "
                        } else {
                            "☐ "
                        }
                    }
                    _ => "",
                };
                // An image next to text leaves the line break before it behind.
                let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
                format!("{prefix}{}", text.trim_end_matches('\n'))
            }
        })
        .collect();
    lines.join("\n")
}

/// Byte offset of the task checkbox's inner character in `line`, if it is a task item.
fn task_box(line: &str) -> Option<usize> {
    let rest = line.trim_start();
    let mut chars = rest.char_indices();
    let marker_end = match chars.next()? {
        (_, '-' | '*' | '+') => 1,
        (_, c) if c.is_ascii_digit() => {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            match rest[digits..].chars().next()? {
                '.' | ')' => digits + 1,
                _ => return None,
            }
        }
        _ => return None,
    };
    let after = &rest[marker_end..];
    let spaced = after.trim_start();
    if spaced.len() == after.len() {
        return None;
    }
    let bytes = spaced.as_bytes();
    if bytes.len() >= 3
        && bytes[0] == b'['
        && matches!(bytes[1], b' ' | b'x' | b'X')
        && bytes[2] == b']'
    {
        Some(line.len() - spaced.len() + 1)
    } else {
        None
    }
}

/// Flips the task checkbox on 0-based source `line`; `None` if that line
/// is not a task item.
pub fn toggle_task(content: &str, line: usize) -> Option<String> {
    let mut out = String::with_capacity(content.len());
    let mut toggled = false;
    for (i, text) in content.split_inclusive('\n').enumerate() {
        match task_box(text).filter(|_| i == line) {
            Some(at) => {
                let flipped = if text.as_bytes()[at] == b' ' {
                    'x'
                } else {
                    ' '
                };
                out.push_str(&text[..at]);
                out.push(flipped);
                out.push_str(&text[at + 1..]);
                toggled = true;
            }
            None => out.push_str(text),
        }
    }
    toggled.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    fn texts(doc: &Doc) -> Vec<String> {
        doc.blocks
            .iter()
            .map(|b| b.spans.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    fn span<'a>(doc: &'a Doc, text: &str) -> &'a Span {
        doc.blocks
            .iter()
            .flat_map(|b| &b.spans)
            .find(|s| s.text == text)
            .unwrap_or_else(|| panic!("no span {text:?} in {doc:?}"))
    }

    #[test]
    fn plain_paragraphs_keep_text() {
        let doc = parse("hello\n\nworld", &PALETTE);
        assert_eq!(texts(&doc), ["hello", "world"]);
        assert!(doc.blocks.iter().all(|b| b.kind == BlockKind::Paragraph));
        let lines: Vec<_> = doc.blocks.iter().map(|b| b.source_line).collect();
        assert_eq!(lines, [0, 2]);
    }

    #[test]
    fn single_newlines_stay_line_breaks() {
        let doc = parse("Call Anna\nre: offsite\nbook room", &PALETTE);
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc.blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(texts(&doc), ["Call Anna\nre: offsite\nbook room"]);
    }

    #[test]
    fn emphasis_flags() {
        let doc = parse("**b** *i* ~~s~~ `c`", &PALETTE);
        assert!(span(&doc, "b").bold);
        assert!(span(&doc, "i").italic);
        assert!(span(&doc, "s").strike);
        assert!(span(&doc, "c").code);
    }

    #[test]
    fn headings_clamp_to_three() {
        let doc = parse("# a\n#### d", &PALETTE);
        assert_eq!(doc.blocks[0].kind, BlockKind::Heading(1));
        assert_eq!(doc.blocks[1].kind, BlockKind::Heading(3));
    }

    #[test]
    fn list_items_with_depth_and_tasks() {
        let doc = parse("- a\n  - b\n1. c\n- [ ] t\n- [x] u", &PALETTE);
        let items: Vec<_> = doc
            .blocks
            .iter()
            .map(|b| match &b.kind {
                BlockKind::ListItem {
                    ordered,
                    depth,
                    task,
                } => (*ordered, *depth, *task),
                other => panic!("not a list item: {other:?}"),
            })
            .collect();
        assert_eq!(
            items,
            [
                (None, 0, None),
                (None, 1, None),
                (Some(1), 0, None),
                (None, 0, Some(false)),
                (None, 0, Some(true)),
            ]
        );
        let lines: Vec<_> = doc.blocks.iter().map(|b| b.source_line).collect();
        assert_eq!(lines, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn image_block() {
        let doc = parse("![cat](images/a.png)", &PALETTE);
        assert_eq!(
            doc.blocks[0].kind,
            BlockKind::Image {
                path: "images/a.png".into(),
                alt: "cat".into()
            }
        );
    }

    #[test]
    fn link_span() {
        let doc = parse("[x](https://e.com)", &PALETTE);
        assert_eq!(span(&doc, "x").link.as_deref(), Some("https://e.com"));
    }

    #[test]
    fn color_tag_uses_current_slot() {
        let mut palette = PALETTE;
        palette[0] = PALETTE[13];
        let doc = parse("{coral}hot{/} cold", &palette);
        assert_eq!(span(&doc, "hot").color, Some(PALETTE[13]));
        assert_eq!(span(&doc, " cold").color, None);
    }

    #[test]
    fn hex_and_bg_and_size_tags() {
        let doc = parse(
            "{#00FF00}g{/}{bg:sky}h{/}{size:99}big{/}{size:2}tiny{/}",
            &PALETTE,
        );
        assert_eq!(span(&doc, "g").color, NoteColor::parse_hex("#00FF00"));
        assert_eq!(span(&doc, "h").background, Some(PALETTE[13]));
        assert_eq!(span(&doc, "big").size, Some(48.0));
        assert_eq!(span(&doc, "tiny").size, Some(8.0));
    }

    #[test]
    fn highlight_shorthand() {
        let doc = parse("==hi==", &PALETTE);
        assert_eq!(span(&doc, "hi").background, Some(PALETTE[5]));
    }

    #[test]
    fn equals_with_spaces_stay_literal() {
        let doc = parse("a == b", &PALETTE);
        assert_eq!(texts(&doc), ["a == b"]);
        assert_eq!(doc.blocks[0].spans[0].background, None);
    }

    #[test]
    fn unmatched_highlight_stays_literal() {
        let doc = parse("x ==y", &PALETTE);
        assert_eq!(texts(&doc), ["x ==y"]);
        assert_eq!(doc.blocks[0].spans[0].background, None);
    }

    #[test]
    fn tags_nest_and_unclosed_ends_at_block() {
        let doc = parse("{coral}{size:20}a{/}b\n\nc", &PALETTE);
        let a = span(&doc, "a");
        assert_eq!((a.color, a.size), (Some(PALETTE[0]), Some(20.0)));
        let b = span(&doc, "b");
        assert_eq!((b.color, b.size), (Some(PALETTE[0]), None));
        let c = span(&doc, "c");
        assert_eq!((c.color, c.size), (None, None));
    }

    #[test]
    fn stray_close_dropped_unknown_literal() {
        let doc = parse("{/}x {foo} {size:abc}", &PALETTE);
        assert_eq!(texts(&doc), ["x {foo} {size:abc}"]);
    }

    #[test]
    fn escaped_brace_is_literal() {
        let doc = parse("\\{coral}x", &PALETTE);
        assert_eq!(texts(&doc), ["{coral}x"]);
        assert_eq!(doc.blocks[0].spans[0].color, None);
    }

    #[test]
    fn tags_ignored_in_code() {
        let doc = parse("`{coral}`", &PALETTE);
        let s = span(&doc, "{coral}");
        assert!(s.code);
        assert_eq!(s.color, None);
    }

    #[test]
    fn braces_that_are_not_tags_stay_literal() {
        let doc = parse("{\"a\": 1}\n- item", &PALETTE);
        assert_eq!(texts(&doc), ["{\"a\": 1}", "item"]);
        assert!(matches!(doc.blocks[1].kind, BlockKind::ListItem { .. }));
    }

    #[test]
    fn empty_palette_consumes_tags_without_color() {
        let doc = parse("{coral}x{/}", &[]);
        assert_eq!(texts(&doc), ["x"]);
        assert_eq!(doc.blocks[0].spans[0].color, None);
    }

    #[test]
    fn hostile_inputs_do_not_panic() {
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
        ] {
            parse(input, &PALETTE);
            parse(input, &[]);
        }
    }

    #[test]
    fn plain_text_strips_markup() {
        let got = plain_text("# T\n**b** {coral}c{/}\n- [ ] x\n- [x] y\n![a](images/a.png)");
        assert_eq!(got, "T\nb c\n☐ x\n☑ y\n🖼");
    }

    #[test]
    fn toggle_task_flips_marker() {
        let c = "a\n  - [ ] b\n3. [x] c";
        assert_eq!(toggle_task(c, 1).as_deref(), Some("a\n  - [x] b\n3. [x] c"));
        assert_eq!(toggle_task(c, 2).as_deref(), Some("a\n  - [ ] b\n3. [ ] c"));
    }

    #[test]
    fn toggle_task_ignores_non_tasks() {
        assert_eq!(toggle_task("plain\n- [ ] t", 0), None);
        assert_eq!(toggle_task("plain\n- [ ] t", 9), None);
    }

    #[test]
    fn toggle_task_preserves_crlf() {
        assert_eq!(
            toggle_task("a\r\n- [ ] b\r\nc", 1).as_deref(),
            Some("a\r\n- [x] b\r\nc")
        );
    }
}
