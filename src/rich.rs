use crate::note::NoteColor;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

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
    /// `(rendered offset, source offset)` pairs, one per piece of text added
    /// to the block, in order: where the block's text (its spans joined)
    /// comes from in the note. See [`source_offset`].
    pub map: Vec<(usize, usize)>,
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum OpenTag {
    Color(Option<NoteColor>),
    Background(Option<NoteColor>),
    Size(f32),
    Highlight,
}

fn resolve_color(name: &str, palette: &[NoteColor]) -> Option<Option<NoteColor>> {
    if name.starts_with('#') {
        return NoteColor::parse_hex(name).map(Some);
    }
    COLOR_NAMES
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|slot| palette.get(slot).copied())
}

/// Parses the inside of a `{...}` tag; `None` means it is not a tag.
pub(crate) fn parse_tag(inner: &str, palette: &[NoteColor]) -> Option<OpenTag> {
    if let Some(value) = inner.strip_prefix("size:") {
        let size: f32 = value.parse().ok()?;
        return size
            .is_finite()
            .then(|| OpenTag::Size(size.clamp(8.0, 48.0)));
    }
    if let Some(value) = inner.strip_prefix("bg:") {
        return resolve_color(value, palette).map(OpenTag::Background);
    }
    resolve_color(inner, palette).map(OpenTag::Color)
}

struct Builder<'a> {
    content: &'a str,
    /// Byte offset where each source line starts, for `line_at`.
    line_starts: Vec<usize>,
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
    /// An image just split its block; the line break after it belongs to it.
    after_image: bool,
    in_code_block: bool,
    /// Source ranges of the tags and `==` pairs consumed as markup.
    removed: Vec<(usize, usize)>,
}

impl<'a> Builder<'a> {
    fn line_at(&self, offset: usize) -> usize {
        self.line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
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
        self.after_image = false;
        self.cur = Some(Block {
            kind,
            spans: Vec::new(),
            source_line: self.line_at(offset),
            map: Vec::new(),
        });
    }

    fn ensure_block(&mut self, offset: usize) {
        if self.cur.is_none() {
            self.start(self.text_kind(), offset);
        }
    }

    fn text_kind(&self) -> BlockKind {
        if self.quote_depth > 0 {
            BlockKind::Quote
        } else {
            BlockKind::Paragraph
        }
    }

    /// Starts a paragraph (or quote) block, unless it continues a list item.
    fn start_paragraph(&mut self, offset: usize) {
        let in_item = matches!(
            self.cur.as_ref().map(|c| &c.kind),
            Some(BlockKind::ListItem { .. })
        );
        if !in_item {
            self.start(self.text_kind(), offset);
        }
    }

    /// Ends the current block at an image, without the line break that led
    /// up to the image.
    fn split_at_image(&mut self) {
        if let Some(block) = self.cur.as_mut() {
            if let Some(last) = block.spans.last_mut() {
                if last.text.ends_with('\n') {
                    last.text.pop();
                    if last.text.is_empty() {
                        block.spans.pop();
                    }
                }
            }
        }
        self.flush();
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
        let mut text = text;
        let mut offset = offset;
        if self.cur.is_none() && std::mem::take(&mut self.after_image) {
            // The text after an image starts on the line below it.
            if let Some(rest) = text.strip_prefix('\n') {
                text = rest;
                offset += 1;
            }
        }
        if text.is_empty() {
            return;
        }
        self.ensure_block(offset);
        span.text = text.to_string();
        let Some(block) = self.cur.as_mut() else {
            return;
        };
        let rendered: usize = block.spans.iter().map(|s| s.text.len()).sum();
        block.map.push((rendered, offset));
        match block.spans.last_mut() {
            Some(last) if last.same_style(&span) => last.text.push_str(text),
            _ => block.spans.push(span),
        }
    }

    fn push_plain(&mut self, text: &str, offset: usize) {
        let span = self.style();
        self.push_span(span, text, offset);
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
        // Entities and the like make a text event differ from its source;
        // then offsets into the text are not offsets into the note.
        let verbatim = self.content.get(range_start..range_end) == Some(text);
        let mut literal = String::new();
        // Where the pending literal starts in the source.
        let mut literal_at = range_start;
        let mut rest = text;
        let mut first = true;
        while !rest.is_empty() {
            if rest.starts_with('{') && !(first && escaped) {
                if let Some(end) = rest.find('}') {
                    let inner = &rest[1..end];
                    if inner == "/" {
                        self.mark_removed(verbatim, range_start, text, rest, end + 1);
                        self.flush_literal(&mut literal, literal_at);
                        self.tags.pop();
                        rest = &rest[end + 1..];
                        first = false;
                        continue;
                    }
                    if let Some(tag) = parse_tag(inner, self.palette) {
                        self.mark_removed(verbatim, range_start, text, rest, end + 1);
                        self.flush_literal(&mut literal, literal_at);
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
                    self.mark_removed(verbatim, range_start, text, rest, 2);
                    self.flush_literal(&mut literal, literal_at);
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
            if literal.is_empty() {
                literal_at = range_start + (text.len() - rest.len());
            }
            literal.push(ch);
            rest = &rest[ch.len_utf8().max(1)..];
            first = false;
        }
        self.flush_literal(&mut literal, literal_at);
    }

    /// Records the `len` bytes at the start of `rest` (a tail of `text`) as markup.
    fn mark_removed(
        &mut self,
        verbatim: bool,
        range_start: usize,
        text: &str,
        rest: &str,
        len: usize,
    ) {
        if verbatim {
            let at = range_start + (text.len() - rest.len());
            self.removed.push((at, at + len));
        }
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
    scan(content, palette).0
}

/// Parses like [`parse`] and also returns the source ranges of the tags and
/// `==` pairs it consumed, in order.
fn scan(content: &str, palette: &[NoteColor]) -> (Doc, Vec<(usize, usize)>) {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let line_starts = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let mut b = Builder {
        content,
        line_starts,
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
        after_image: false,
        in_code_block: false,
        removed: Vec::new(),
    };

    for (event, range) in Parser::new_ext(content, options).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                // Paragraphs inside a list item continue that item; raw HTML
                // shows as typed in a paragraph of its own.
                Tag::Paragraph | Tag::HtmlBlock => b.start_paragraph(range.start),
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
                Tag::Link {
                    link_type: LinkType::Email,
                    dest_url,
                    ..
                } => b.links.push(format!("mailto:{dest_url}")),
                Tag::Link { dest_url, .. } => b.links.push(dest_url.to_string()),
                Tag::Image { dest_url, .. } => {
                    b.split_at_image();
                    b.image = Some((dest_url.to_string(), String::new(), b.line_at(range.start)));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item => b.flush(),
                TagEnd::CodeBlock | TagEnd::HtmlBlock => {
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
                            map: Vec::new(),
                        });
                        b.after_image = true;
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
            // Markup the parser doesn't render stays as typed.
            Event::Html(html) | Event::InlineHtml(html) => b.push_plain(&html, range.start),
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
                    map: Vec::new(),
                });
            }
            _ => {}
        }
    }
    b.flush();
    (Doc { blocks: b.blocks }, b.removed)
}

/// The note with the `{...}` tags and `==` highlights the formatted view
/// consumes removed; Markdown, escapes and anything shown literally stay.
pub fn strip_tags(content: &str) -> String {
    let (_, removed) = scan(content, &[]);
    let mut out = String::with_capacity(content.len());
    let mut at = 0;
    for (start, end) in removed {
        out.push_str(&content[at..start]);
        at = end;
    }
    out.push_str(&content[at..]);
    out
}

/// The source offset of the character at byte `text_offset` of `block`'s
/// rendered text (its spans joined), clamped to the end of the block.
pub fn source_offset(block: &Block, text_offset: usize) -> usize {
    let rendered: usize = block.spans.iter().map(|s| s.text.len()).sum();
    let text_offset = text_offset.min(rendered);
    let at = block
        .map
        .partition_point(|&(rendered, _)| rendered <= text_offset);
    match at.checked_sub(1).and_then(|i| block.map.get(i)) {
        Some(&(rendered, source)) => source + (text_offset - rendered),
        None => block.map.first().map_or(0, |&(_, source)| source),
    }
}

/// The word (letters, digits and `_`) at byte `offset` of `content`, as a
/// byte range; empty at `offset` when no word starts or continues there.
pub fn word_at(content: &str, offset: usize) -> (usize, usize) {
    let offset = snap(content, offset);
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if !content[offset..].chars().next().is_some_and(is_word) {
        return (offset, offset);
    }
    let start = content[..offset]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word(c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = content[offset..]
        .char_indices()
        .find(|&(_, c)| !is_word(c))
        .map_or(content.len(), |(i, _)| offset + i);
    (start, end)
}

/// The line holding byte `offset` of `content`, without its line break.
pub fn line_bounds(content: &str, offset: usize) -> (usize, usize) {
    let offset = snap(content, offset);
    let start = content[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = content[offset..]
        .find('\n')
        .map_or(content.len(), |i| offset + i);
    let end = if content[..end].ends_with('\r') {
        end - 1
    } else {
        end
    };
    (start, end.max(start))
}

/// The note as plain text: markers and tags removed, one line per block
/// (plus one per line break inside a block).
pub fn plain_text(content: &str) -> String {
    let doc = parse(content, &[]);
    let lines: Vec<String> = doc
        .blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Image { .. } => "Image".to_string(),
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
                let text: String = block.spans.iter().map(|s| s.text.as_str()).collect();
                format!("{prefix}{text}")
            }
        })
        .collect();
    lines.join("\n")
}

/// The first `max` non-blank lines of [`plain_text`], trimmed. Parses only
/// the start of the note, so long notes stay cheap.
pub fn plain_lines(content: &str, max: usize) -> Vec<String> {
    // Source lines parsed past the last one shown, so markup that looks
    // ahead (a setext underline, a closing `**`) renders as in the whole note.
    const MARGIN: usize = 8;
    let mut want = max.saturating_add(MARGIN);
    loop {
        let mut taken = 0;
        let end = content
            .split_inclusive('\n')
            .scan(0, |len, line| {
                *len += line.len();
                Some((*len, line))
            })
            .find(|(_, line)| {
                taken += usize::from(!line.trim().is_empty());
                taken >= want
            })
            .map_or(content.len(), |(len, _)| len);
        let text = plain_text(&content[..end]);
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if end == content.len() || lines.len() >= max.saturating_add(MARGIN) {
            return lines.into_iter().take(max).map(str::to_string).collect();
        }
        want = want.saturating_mul(2);
    }
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

/// A formatting action the toolbar applies to the selected text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Bold,
    Italic,
    Strike,
    Code,
    /// Palette slot, written as its `COLOR_NAMES` tag.
    Color(usize),
    Highlight,
    Size(f32),
    Link,
}

/// Snaps `offset` down to a char boundary within `content`.
fn snap(content: &str, offset: usize) -> usize {
    let mut offset = offset.min(content.len());
    while !content.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// The selected byte range, given the editor's cursor (`head`) and selection
/// anchor (`anchor`) as offsets and its selected text. A double- or
/// triple-click selection is reported by iced as its anchor only, so when
/// the two offsets meet, the range is the occurrence of `selected` around
/// the cursor.
pub fn selection_range(
    content: &str,
    head: usize,
    anchor: usize,
    selected: Option<&str>,
) -> (usize, usize) {
    if head != anchor {
        return (head.min(anchor), head.max(anchor));
    }
    selected
        .filter(|s| !s.is_empty())
        .and_then(|s| {
            content
                .match_indices(s)
                .map(|(start, _)| (start, start + s.len()))
                .find(|&(start, end)| start <= head && head <= end)
        })
        .unwrap_or((head, head))
}

/// Wraps `start..end` (byte offsets, clamped and snapped to char boundaries)
/// in the markup for `format`. Returns the new text and the new selection:
/// the wrapped text, or for a link the spot inside `()`. With an empty
/// selection the cursor lands between the markers.
pub fn wrap_selection(
    content: &str,
    start: usize,
    end: usize,
    format: Format,
) -> (String, usize, usize) {
    let (a, b) = (snap(content, start), snap(content, end));
    let (start, end) = (a.min(b), a.max(b));
    let (open, close) = match format {
        Format::Bold => ("**".to_string(), "**".to_string()),
        Format::Italic => ("*".to_string(), "*".to_string()),
        Format::Strike => ("~~".to_string(), "~~".to_string()),
        Format::Code => ("`".to_string(), "`".to_string()),
        Format::Highlight => ("==".to_string(), "==".to_string()),
        Format::Color(slot) => (
            format!("{{{}}}", COLOR_NAMES[slot.min(COLOR_NAMES.len() - 1)]),
            "{/}".to_string(),
        ),
        Format::Size(size) => (format!("{{size:{size}}}"), "{/}".to_string()),
        Format::Link => ("[".to_string(), "]()".to_string()),
    };
    let mut out = String::with_capacity(content.len() + open.len() + close.len());
    out.push_str(&content[..start]);
    out.push_str(&open);
    out.push_str(&content[start..end]);
    out.push_str(&close);
    out.push_str(&content[end..]);
    let inner = (start + open.len(), end + open.len());
    if format == Format::Link {
        // Between the parentheses, ready for the address.
        let at = inner.1 + close.len() - 1;
        (out, at, at)
    } else {
        (out, inner.0, inner.1)
    }
}

/// Byte offset in `content` of an editor position. The editor counts
/// `column` in bytes within the line; out-of-range values are clamped.
pub fn offset_of(content: &str, line: usize, column: usize) -> usize {
    let mut line_start = 0;
    for _ in 0..line {
        match content[line_start..].find('\n') {
            Some(i) => line_start += i + 1,
            None => return content.len(),
        }
    }
    let line_end = content[line_start..]
        .find('\n')
        .map_or(content.len(), |i| line_start + i);
    line_start + snap(&content[line_start..line_end], column)
}

/// The editor position `(line, column)` of a byte offset, the inverse of
/// [`offset_of`].
pub fn position_of(content: &str, offset: usize) -> (usize, usize) {
    let offset = snap(content, offset);
    let before = &content[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, offset - line_start)
}

/// Inserts `block` on its own line at byte `offset` and returns the new text
/// with the offset right after the block.
pub fn insert_block(content: &str, offset: usize, block: &str) -> (String, usize) {
    let offset = snap(content, offset);
    let (before, after) = content.split_at(offset);
    let lead = if before.is_empty() || before.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let trail = if after.is_empty() { "" } else { "\n" };
    let end = offset + lead.len() + block.len();
    (format!("{before}{lead}{block}{trail}{after}"), end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    #[test]
    fn insert_block_puts_block_on_its_own_line() {
        assert_eq!(insert_block("a\nb", 2, "X"), ("a\nX\nb".into(), 3));
        assert_eq!(insert_block("ab", 1, "X"), ("a\nX\nb".into(), 3));
        assert_eq!(insert_block("hi", 2, "X"), ("hi\nX".into(), 4));
        assert_eq!(insert_block("hi\n", 3, "X"), ("hi\nX".into(), 4));
        assert_eq!(insert_block("", 0, "X"), ("X".into(), 1));
        assert_eq!(insert_block("ab", 0, "X"), ("X\nab".into(), 1));
    }

    fn texts_of(block: &Block) -> String {
        block.spans.iter().map(|s| s.text.as_str()).collect()
    }

    fn texts(doc: &Doc) -> Vec<String> {
        doc.blocks.iter().map(texts_of).collect()
    }

    fn span<'a>(doc: &'a Doc, text: &str) -> &'a Span {
        doc.blocks
            .iter()
            .flat_map(|b| &b.spans)
            .find(|s| s.text == text)
            .unwrap_or_else(|| panic!("no span {text:?} in {doc:?}"))
    }

    /// The source offset of the rendered character at `text_offset` in the
    /// first block, checked against where `expect` sits in `content`.
    fn maps(content: &str, text_offset: usize, expect: &str) {
        let doc = parse(content, &PALETTE);
        let block = &doc.blocks[0];
        let at = source_offset(block, text_offset);
        assert!(
            content[at..].starts_with(expect),
            "{content:?}: rendered {text_offset} -> {at}, expected {expect:?}"
        );
    }

    #[test]
    fn source_offsets_skip_markup() {
        // Rendered text: "a bold red c"
        maps("a **bold** {coral}red{/} c", 2, "bold");
        maps("a **bold** {coral}red{/} c", 7, "red");
        maps("a **bold** {coral}red{/} c", 11, "c");
        maps("x ==hi== y", 2, "hi");
        maps("x ==hi== y", 5, "y");
    }

    #[test]
    fn source_offsets_in_headings_lists_and_lines() {
        maps("# Title", 0, "Title");
        maps("- [ ] task", 0, "task");
        maps("1. first", 0, "first");
        maps("one\ntwo", 4, "two");
        maps("> quoted", 0, "quoted");
    }

    #[test]
    fn source_offset_clamps_past_the_end() {
        let doc = parse("ab", &PALETTE);
        assert_eq!(source_offset(&doc.blocks[0], 99), 2);
    }

    #[test]
    fn word_at_finds_word_without_markup() {
        assert_eq!(word_at("a **bold** c", 5), (4, 8));
        assert_eq!(word_at("a **bold** c", 4), (4, 8));
        assert_eq!(word_at("héllo wörld", 8), (7, 13));
        assert_eq!(word_at("a  b", 2), (2, 2));
    }

    #[test]
    fn line_bounds_exclude_the_line_break() {
        assert_eq!(line_bounds("x\nabc\ny", 3), (2, 5));
        assert_eq!(line_bounds("abc", 1), (0, 3));
        assert_eq!(line_bounds("a\r\nb", 0), (0, 1));
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
            "<",
            "<!--",
            "- <div>\n  x",
            "> a\n> ![](x)\n> b",
            "a\r\n![](x)\r\nb",
            "![](x)\n![](y)\n",
            "<a@b>",
        ] {
            parse(input, &PALETTE);
            parse(input, &[]);
        }
    }

    #[test]
    fn plain_text_strips_markup() {
        let got = plain_text("# T\n**b** {coral}c{/}\n- [ ] x\n- [x] y\n![a](images/a.png)");
        assert_eq!(got, "T\nb c\n☐ x\n☑ y\nImage");
    }

    #[test]
    fn strip_tags_keeps_markdown() {
        assert_eq!(
            strip_tags("{coral}**b**{/} ==hi== [l](https://x) {size:20}t{/}"),
            "**b** hi [l](https://x) t"
        );
    }

    #[test]
    fn strip_tags_follows_the_parser() {
        for same in [
            "{foo} {size:abc} {/",
            "\\{coral}x",
            "a == b",
            "x ==y",
            "`{coral}x{/}`",
            "```\n{coral}x{/}\n```",
        ] {
            assert_eq!(strip_tags(same), same, "{same:?}");
        }
        assert_eq!(strip_tags("{/}x"), "x");
        assert_eq!(strip_tags("a ==b== c ==d"), "a b c ==d");
    }

    #[test]
    fn strip_tags_adjacent_tags() {
        assert_eq!(strip_tags("{coral}{size:20}x{/}{/}"), "x");
    }

    #[test]
    fn strip_tags_multibyte() {
        assert_eq!(strip_tags("{coral}é😀{/} ü"), "é😀 ü");
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

    #[test]
    fn selection_range_uses_cursor_range_when_present() {
        assert_eq!(selection_range("a word b", 6, 2, Some("word")), (2, 6));
    }

    #[test]
    fn selection_range_finds_word_selection_around_cursor() {
        // A double-click reports only its anchor; the word comes from the text.
        assert_eq!(selection_range("hello world", 0, 0, Some("hello")), (0, 5));
        assert_eq!(selection_range("hello world", 8, 8, Some("world")), (6, 11));
        assert_eq!(selection_range("ab ab ab", 4, 4, Some("ab")), (3, 5));
        assert_eq!(
            selection_range("x\nline two\n", 4, 4, Some("line two\n")),
            (2, 11)
        );
    }

    #[test]
    fn selection_range_without_selection_is_the_cursor() {
        assert_eq!(selection_range("abc", 1, 1, None), (1, 1));
        assert_eq!(selection_range("abc", 1, 1, Some("")), (1, 1));
        assert_eq!(selection_range("abc", 1, 1, Some("zz")), (1, 1));
    }

    #[test]
    fn wrap_bold_selection() {
        assert_eq!(
            wrap_selection("a word b", 2, 6, Format::Bold),
            ("a **word** b".to_string(), 4, 8)
        );
    }

    #[test]
    fn wrap_empty_selection_places_cursor_inside() {
        assert_eq!(
            wrap_selection("ab", 1, 1, Format::Italic),
            ("a**b".to_string(), 2, 2)
        );
    }

    #[test]
    fn wrap_color_size_link() {
        let wrapped = |f| wrap_selection("x", 0, 1, f);
        assert_eq!(wrapped(Format::Color(0)).0, "{coral}x{/}");
        assert_eq!(wrapped(Format::Size(20.0)).0, "{size:20}x{/}");
        assert_eq!(wrapped(Format::Highlight).0, "==x==");
        assert_eq!(wrapped(Format::Strike).0, "~~x~~");
        assert_eq!(wrapped(Format::Code).0, "`x`");
        let (text, a, b) = wrapped(Format::Link);
        assert_eq!(text, "[x]()");
        assert_eq!((a, b), (4, 4));
    }

    #[test]
    fn wrap_selection_with_multibyte_text() {
        // The editor's column counts bytes: é is 2, the emoji 4.
        let text = "é😀z";
        let end = offset_of(text, 0, 6);
        assert_eq!(wrap_selection(text, 0, end, Format::Bold).0, "**é😀**z");
    }

    #[test]
    fn wrap_clamps_and_snaps_bad_offsets() {
        assert_eq!(wrap_selection("é", 1, 99, Format::Bold).0, "**é**");
        assert_eq!(wrap_selection("", 5, 2, Format::Link).0, "[]()");
        assert_eq!(wrap_selection("ab", 2, 0, Format::Bold).0, "**ab**");
        assert_eq!(
            wrap_selection("a", 0, 1, Format::Color(99)).0,
            "{slate}a{/}"
        );
        assert_eq!(offset_of("ab\ncd", 9, 0), 5);
        assert_eq!(offset_of("ab\ncd", 0, 99), 2);
        assert_eq!(offset_of("é", 0, 1), 0);
        assert_eq!(position_of("ab", 99), (0, 2));
    }

    #[test]
    fn inline_html_stays_literal() {
        let doc = parse("Meet <Anna> at 5", &PALETTE);
        assert_eq!(texts(&doc), ["Meet <Anna> at 5"]);
        assert_eq!(plain_text("Meet <Anna> at 5"), "Meet <Anna> at 5");
    }

    #[test]
    fn html_block_stays_literal() {
        let doc = parse("<div>hi</div>", &PALETTE);
        assert_eq!(texts(&doc), ["<div>hi</div>"]);
        assert_eq!(doc.blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(plain_text("<div>hi</div>\n\nafter"), "<div>hi</div>\nafter");
    }

    #[test]
    fn html_comment_stays_literal() {
        let doc = parse("<!-- todo -->\nnext", &PALETTE);
        assert_eq!(texts(&doc), ["<!-- todo -->", "next"]);
        let lines: Vec<_> = doc.blocks.iter().map(|b| b.source_line).collect();
        assert_eq!(lines, [0, 1]);
        assert_eq!(plain_text("<!-- todo -->"), "<!-- todo -->");
    }

    #[test]
    fn image_between_lines_has_no_blank_lines() {
        let doc = parse("a\n![](images/x.png)\nb", &PALETTE);
        let got: Vec<_> = doc
            .blocks
            .iter()
            .map(|b| (b.kind.clone(), texts_of(b), b.source_line))
            .collect();
        let image = BlockKind::Image {
            path: "images/x.png".into(),
            alt: String::new(),
        };
        assert_eq!(
            got,
            [
                (BlockKind::Paragraph, "a".to_string(), 0),
                (image, String::new(), 1),
                (BlockKind::Paragraph, "b".to_string(), 2),
            ]
        );
    }

    #[test]
    fn email_autolink_opens_as_mailto() {
        let doc = parse("<a@b.com>", &PALETTE);
        assert_eq!(
            span(&doc, "a@b.com").link.as_deref(),
            Some("mailto:a@b.com")
        );
    }

    #[test]
    fn plain_lines_stops_early() {
        let content: String = (0..10_000).map(|i| format!("line {i}\n")).collect();
        assert_eq!(plain_lines(&content, 3), ["line 0", "line 1", "line 2"]);
        assert_eq!(plain_lines("\n\n  # T\n\n- [ ] x", 3), ["T", "☐ x"]);
    }

    #[test]
    fn offset_position_roundtrip() {
        let text = "ab\ncé\n😀";
        for o in (0..=text.len()).filter(|&o| text.is_char_boundary(o)) {
            let (line, column) = position_of(text, o);
            assert_eq!(offset_of(text, line, column), o);
        }
    }
}
