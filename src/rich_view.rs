//! Draws a parsed note (`rich::Doc`) as styled, clickable blocks.

use crate::app::Message;
use crate::note::NoteColor;
use crate::rich::{Block, BlockKind, Doc, Span};

use iced::widget::{button, column, container, image, mouse_area, rich_text, row, text, Space};
use iced::{font, Border, Color, ContentFit, Element, Fill, Font, Padding, Theme};
use std::collections::HashSet;
use std::path::Path;

const BODY_SIZE: f32 = 14.0;
const LIST_INDENT: f32 = 16.0;
/// Space below each block; part of the block, so clicks there still hit it.
const BLOCK_GAP: f32 = 6.0;

/// Whether a link may be handed to the system opener.
pub fn is_openable(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 22.0,
        2 => 18.0,
        _ => 16.0,
    }
}

fn note_color(color: NoteColor, alpha: f32) -> Color {
    let [r, g, b, a] = color.rgba;
    Color::from_rgba(r, g, b, a * alpha)
}

/// The rendered note body: one clickable element per block.
/// Image references in `broken` (missing or undecodable, checked when the
/// note was parsed) show a placeholder; nothing here touches the disk.
pub fn view<'a>(
    doc: &'a Doc,
    dir: &'a Path,
    broken: &'a HashSet<String>,
    ink: impl Fn(f32) -> Color + Copy + 'a,
    alpha: f32,
) -> Element<'a, Message> {
    if doc.blocks.is_empty() {
        // Same placeholder as the empty editor.
        return text("Write something…")
            .size(BODY_SIZE)
            .color(ink(0.35 * alpha))
            .into();
    }
    column(doc.blocks.iter().map(|b| block(b, dir, broken, ink, alpha)))
        .width(Fill)
        .into()
}

fn block<'a>(
    block: &'a Block,
    dir: &'a Path,
    broken: &'a HashSet<String>,
    ink: impl Fn(f32) -> Color + Copy + 'a,
    alpha: f32,
) -> Element<'a, Message> {
    let size = match block.kind {
        BlockKind::Heading(level) => heading_size(level),
        _ => BODY_SIZE,
    };
    let bold = matches!(block.kind, BlockKind::Heading(_));
    let mono = block.kind == BlockKind::CodeBlock;
    let styled = || spans(&block.spans, size, bold, mono, ink, alpha);

    let content: Element<'a, Message> = match &block.kind {
        BlockKind::Paragraph | BlockKind::Heading(_) => styled(),
        BlockKind::ListItem {
            ordered,
            depth,
            task,
        } => {
            let marker: Element<'a, Message> = match (task, ordered) {
                (Some(done), _) => button(text(if *done { "☑" } else { "☐" }).size(size))
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
                    .color(ink(0.75 * alpha))
                    .into(),
                (None, None) => text("•").size(size).color(ink(0.75 * alpha)).into(),
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
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
        BlockKind::CodeBlock => container(styled())
            .width(Fill)
            .padding(Padding::new(4.0).left(8).right(8))
            .style(move |_theme: &Theme| container::Style {
                background: Some(ink(0.08 * alpha).into()),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into(),
        BlockKind::Image { path, .. } if broken.contains(path) => text("image not found")
            .size(BODY_SIZE)
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

fn spans<'a>(
    spans: &'a [Span],
    size: f32,
    bold: bool,
    mono: bool,
    ink: impl Fn(f32) -> Color + Copy + 'a,
    alpha: f32,
) -> Element<'a, Message> {
    let spans: Vec<text::Span<'a, String>> = spans
        .iter()
        .map(|s| {
            let font = Font {
                family: if mono || s.code {
                    font::Family::Monospace
                } else {
                    Font::DEFAULT.family
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
            let color = s
                .color
                .map(|c| note_color(c, alpha))
                .unwrap_or_else(|| ink(0.9 * alpha));
            let background = s
                .background
                .map(|c| note_color(c, alpha))
                .or_else(|| (s.code && !mono).then(|| ink(0.08 * alpha)));
            text::Span::new(s.text.as_str())
                .size(s.size.unwrap_or(size))
                .font(font)
                .color(color)
                .background_maybe(background)
                .strikethrough(s.strike)
                .underline(s.link.is_some())
                .link_maybe(s.link.clone())
        })
        .collect();
    rich_text(spans)
        .on_link_click(Message::LinkClicked)
        .width(Fill)
        .into()
}
