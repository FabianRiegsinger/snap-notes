//! Draws a parsed note (`rich::Doc`) as styled, clickable blocks.

use crate::app::Message;
use crate::hit_text::hit_text;
use crate::note::NoteColor;
use crate::note_panel::PLACEHOLDER;
use crate::rich::{self, Block, BlockKind, Doc, Span};
use crate::theme::{self, BODY_FONT, TEXT_LG, TEXT_MD, TEXT_SM, TEXT_XL};

use iced::widget::{button, column, container, image, mouse_area, rich_text, row, text, Space};
use iced::{border, font, Color, ContentFit, Element, Fill, Font, Padding, Theme};
use std::collections::HashSet;
use std::path::Path;

const BODY_SIZE: f32 = TEXT_SM;
/// Every line of the formatted view, its markers and `hit_text`'s layout
/// share this line height, so hits land where the text is drawn.
pub(crate) const LINE_HEIGHT: text::LineHeight =
    text::LineHeight::Relative(theme::BODY_LINE_HEIGHT);
const LIST_INDENT: f32 = 16.0;
/// Space below each block; part of the block, so clicks there still hit it.
const BLOCK_GAP: f32 = 6.0;

/// The trimmed link if it may be handed to the system opener (`http`,
/// `https` and `mailto` only); pass on exactly this string.
pub fn openable(url: &str) -> Option<&str> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        .then_some(url)
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => TEXT_XL,
        2 => TEXT_LG,
        _ => TEXT_MD,
    }
}

fn note_color(color: NoteColor, alpha: f32) -> Color {
    let [r, g, b, a] = color.rgba;
    Color::from_rgba(r, g, b, a * alpha)
}

/// The rendered note body: one clickable element per block.
/// Image references in `broken` (missing, undecodable or too large, checked
/// when the note was parsed) show a placeholder; nothing here touches the disk.
pub fn view<'a>(
    doc: &'a Doc,
    dir: &'a Path,
    broken: &'a HashSet<String>,
    theme: theme::Theme,
    alpha: f32,
) -> Element<'a, Message> {
    if doc.blocks.is_empty() {
        // Same placeholder as the empty editor.
        return text(PLACEHOLDER)
            .size(BODY_SIZE)
            .line_height(LINE_HEIGHT)
            .color(theme.ink(0.35 * alpha))
            .into();
    }
    column(
        doc.blocks
            .iter()
            .map(|b| block(b, dir, broken, theme, alpha)),
    )
    .width(Fill)
    .into()
}

fn block<'a>(
    block: &'a Block,
    dir: &'a Path,
    broken: &'a HashSet<String>,
    theme: theme::Theme,
    alpha: f32,
) -> Element<'a, Message> {
    let ink = move |a: f32| theme.ink(a);
    let size = match block.kind {
        BlockKind::Heading(level) => heading_size(level),
        _ => BODY_SIZE,
    };
    let bold = matches!(block.kind, BlockKind::Heading(_));
    let mono = block.kind == BlockKind::CodeBlock;
    let styled = || lines(block, size, bold, mono, theme, alpha);

    let content: Element<'a, Message> = match &block.kind {
        BlockKind::Paragraph | BlockKind::Heading(_) => styled(),
        BlockKind::ListItem {
            ordered,
            depth,
            task,
        } => {
            let marker: Element<'a, Message> = match (task, ordered) {
                (Some(done), _) => button(
                    text(if *done { "☑" } else { "☐" })
                        .size(size)
                        .line_height(LINE_HEIGHT),
                )
                .on_press(Message::ToggleTask(block.source_line))
                .padding(0)
                .style(move |_theme: &Theme, status| button::Style {
                    background: None,
                    text_color: ink(if matches!(status, button::Status::Hovered) {
                        1.0
                    } else {
                        0.75
                    } * alpha),
                    ..Default::default()
                })
                .into(),
                (None, Some(n)) => text(format!("{n}."))
                    .size(size)
                    .line_height(LINE_HEIGHT)
                    .color(ink(0.75 * alpha))
                    .into(),
                (None, None) => text("•")
                    .size(size)
                    .line_height(LINE_HEIGHT)
                    .color(ink(0.75 * alpha))
                    .into(),
            };
            row![
                Space::new().width(LIST_INDENT * *depth as f32),
                marker,
                styled()
            ]
            .spacing(6)
            .into()
        }
        BlockKind::Quote => container(styled())
            .width(Fill)
            .padding(Padding::new(2.0).left(10))
            .style(move |_theme: &Theme| container::Style {
                background: Some(ink(0.06 * alpha).into()),
                border: border::rounded(theme::RADIUS_BAR),
                ..Default::default()
            })
            .into(),
        BlockKind::CodeBlock => container(styled())
            .width(Fill)
            .padding(Padding::new(4.0).left(8).right(8))
            .style(move |_theme: &Theme| container::Style {
                background: Some(ink(0.08 * alpha).into()),
                border: border::rounded(theme::RADIUS_CONTROL),
                ..Default::default()
            })
            .into(),
        BlockKind::Image { path, .. } if broken.contains(path) => text("image not found")
            .size(BODY_SIZE)
            .line_height(LINE_HEIGHT)
            .color(ink(0.45 * alpha))
            .into(),
        BlockKind::Image { path, .. } => image(dir.join(path))
            .content_fit(ContentFit::ScaleDown)
            .into(),
        BlockKind::Rule => container(container(Space::new().width(Fill).height(1)).style(
            move |_theme: &Theme| container::Style {
                background: Some(ink(0.2 * alpha).into()),
                ..Default::default()
            },
        ))
        .padding(Padding::ZERO.top(8).bottom(8))
        .into(),
    };

    mouse_area(
        container(content)
            .width(Fill)
            .padding(Padding::ZERO.bottom(BLOCK_GAP)),
    )
    .on_press(Message::BodyClicked(Some(block.source_line)))
    .into()
}

/// A piece of one span on a line: the span's index and a byte range of its text.
type Piece = (usize, std::ops::Range<usize>);

/// A block's text split at its line breaks: each line's start (as a byte
/// offset into the block's joined text) and its pieces.
fn line_pieces(spans: &[Span]) -> Vec<(usize, Vec<Piece>)> {
    let mut lines = vec![(0, Vec::new())];
    let mut rendered = 0;
    for (i, span) in spans.iter().enumerate() {
        let mut from = 0;
        for (at, _) in span.text.match_indices('\n') {
            if at > from {
                lines.last_mut().unwrap().1.push((i, from..at));
            }
            lines.push((rendered + at + 1, Vec::new()));
            from = at + 1;
        }
        if span.text.len() > from {
            lines.last_mut().unwrap().1.push((i, from..span.text.len()));
        }
        rendered += span.text.len();
    }
    lines
}

/// The block's text, one hit-tested line each, so a press tells the app the
/// exact source offset under it.
fn lines<'a>(
    block: &'a Block,
    size: f32,
    bold: bool,
    mono: bool,
    theme: theme::Theme,
    alpha: f32,
) -> Element<'a, Message> {
    column(
        line_pieces(&block.spans)
            .into_iter()
            .map(|(start, pieces)| {
                let mut spans: Vec<text::Span<'a, String>> = pieces
                    .into_iter()
                    .map(|(i, range)| {
                        let s = &block.spans[i];
                        styled(s, &s.text[range], size, bold, mono, theme, alpha)
                    })
                    .collect();
                if spans.is_empty() {
                    // An empty line still takes a line's height.
                    spans.push(text::Span::new(" ").size(size));
                }
                let line = rich_text(spans.clone())
                    .line_height(LINE_HEIGHT)
                    .on_link_click(Message::LinkClicked)
                    .width(Fill);
                hit_text(line, spans, move |offset| {
                    Message::BodyPressed(rich::source_offset(block, start + offset))
                })
                .into()
            }),
    )
    .width(Fill)
    .into()
}

fn styled<'a>(
    s: &'a Span,
    content: &'a str,
    size: f32,
    bold: bool,
    mono: bool,
    theme: theme::Theme,
    alpha: f32,
) -> text::Span<'a, String> {
    let font = Font {
        family: if mono || s.code {
            font::Family::Monospace
        } else {
            BODY_FONT.family
        },
        weight: if bold || s.bold {
            font::Weight::Bold
        } else {
            font::Weight::Normal
        },
        style: if s.italic {
            font::Style::Italic
        } else {
            font::Style::Normal
        },
        ..Font::DEFAULT
    };
    let (color, background) = span_colors(s, mono, theme, alpha);
    text::Span::new(content)
        .size(s.size.unwrap_or(size))
        .font(font)
        .color(color)
        .background_maybe(background)
        .strikethrough(s.strike)
        .underline(s.link.is_some())
        .link_maybe(s.link.clone())
}

/// A span's text and background colors.
fn span_colors(s: &Span, mono: bool, theme: theme::Theme, alpha: f32) -> (Color, Option<Color>) {
    // Text on a background takes the ink that reads on it (a highlight is
    // light in both modes); a span's own color always wins.
    let color = match (s.color, s.background) {
        (Some(c), _) => note_color(c, alpha),
        (None, Some(bg)) => theme.text_on(note_color(bg, 1.0), alpha),
        (None, None) => theme.ink(0.9 * alpha),
    };
    let background = s
        .background
        .map(|c| note_color(c, alpha))
        .or_else(|| (s.code && !mono).then(|| theme.ink(0.08 * alpha)));
    (color, background)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    fn span(text: &str) -> Span {
        Span {
            text: text.into(),
            ..Span::default()
        }
    }

    #[test]
    fn line_pieces_split_at_line_breaks() {
        let pieces = line_pieces(&[span("a\nb"), span("c")]);
        assert_eq!(
            pieces,
            vec![(0, vec![(0, 0..1)]), (2, vec![(0, 2..3), (1, 0..1)])]
        );
    }

    #[test]
    fn highlight_text_contrasts_in_dark_mode() {
        let theme = theme::Theme::new(theme::Mode::Dark);
        let s = Span {
            background: Some(PALETTE[5]),
            ..span("hi")
        };
        let (fg, bg) = span_colors(&s, false, theme, 1.0);
        let bg = bg.unwrap();
        assert!(theme::contrast(fg, bg) >= 4.5);

        // A span's own text color is kept over a background.
        let s = Span {
            color: Some(PALETTE[0]),
            ..s
        };
        assert_eq!(
            span_colors(&s, false, theme, 1.0).0,
            note_color(PALETTE[0], 1.0)
        );

        // Without a background the text keeps the softened ink.
        assert_eq!(span_colors(&span("x"), false, theme, 1.0).0, theme.ink(0.9));
    }

    #[test]
    fn heading_sizes_follow_type_scale() {
        assert_eq!(heading_size(1), theme::TEXT_XL);
        assert_eq!(heading_size(2), theme::TEXT_LG);
        assert_eq!(heading_size(3), theme::TEXT_MD);
    }

    #[test]
    fn line_pieces_keep_empty_lines() {
        let pieces = line_pieces(&[span("a\n\nb")]);
        assert_eq!(
            pieces,
            vec![(0, vec![(0, 0..1)]), (2, vec![]), (3, vec![(0, 3..4)])]
        );
    }
}
