//! Hover peek: after resting on a bar, the bar itself widens to the left and
//! shows the note's full title and its first few lines inside it.

use crate::animation::{ease_out_cubic, lerp};
use crate::note::Note;
use crate::note_panel::{ink, TITLE_FONT};

use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::Text;
use iced::alignment;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Border, Color, Font, Pixels, Point, Rectangle, Shadow, Size, Vector};

pub const PEEK_WIDTH: f32 = 260.0;
const PADDING: f32 = 14.0;
const TITLE_SIZE: f32 = 14.0;
const TITLE_LINE: f32 = 19.0;
const BODY_SIZE: f32 = 13.0;
const BODY_LINE: f32 = 18.0;
const GAP: f32 = 6.0;
const MAX_LINES: usize = 3;
/// Rough glyph widths as a share of the font size, kept generous so the
/// estimated text height never cuts text off.
const TITLE_GLYPH: f32 = 0.62;
const BODY_GLYPH: f32 = 0.55;

pub struct PeekText {
    pub title: Option<String>,
    pub lines: Vec<String>,
}

fn inner_width() -> f32 {
    PEEK_WIDTH - 2.0 * PADDING
}

fn body_chars_per_line() -> usize {
    (inner_width() / (BODY_SIZE * BODY_GLYPH)).floor() as usize
}

fn title_rows(title: &str) -> usize {
    let per_row = (inner_width() / (TITLE_SIZE * TITLE_GLYPH))
        .floor()
        .max(1.0) as usize;
    title.chars().count().div_ceil(per_row).max(1)
}

fn truncate(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        line.to_string()
    } else {
        let kept: String = line.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", kept.trim_end())
    }
}

/// The full title and the next lines of text. Without a title, the first
/// line of the body stands in for it.
pub fn peek_text(note: &Note) -> PeekText {
    let mut lines = note
        .content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty());
    let title = if note.title.trim().is_empty() {
        lines.next().map(str::to_string)
    } else {
        Some(note.title.trim().to_string())
    };
    let max_chars = body_chars_per_line();
    PeekText {
        title,
        lines: lines
            .take(MAX_LINES)
            .map(|l| truncate(l, max_chars))
            .collect(),
    }
}

pub fn peek_height(text: &PeekText) -> f32 {
    let title = text
        .title
        .as_deref()
        .map_or(TITLE_LINE, |t| title_rows(t) as f32 * TITLE_LINE);
    let body = if text.lines.is_empty() {
        0.0
    } else {
        GAP + text.lines.len() as f32 * BODY_LINE
    };
    2.0 * PADDING + title + body
}

/// The bar widened by the peek: same height and right edge, growing left.
pub fn peek_bar(bar: Rectangle, progress: f32) -> Rectangle {
    let width = lerp(
        bar.width,
        PEEK_WIDTH.max(bar.width),
        ease_out_cubic(progress),
    );
    Rectangle::new(
        Point::new(bar.x + bar.width - width, bar.y),
        Size::new(width, bar.height),
    )
}

/// Draws the widened bar in the note's color with its text. The text sits at
/// its final position and fades in once the bar is nearly full width, so it
/// never reflows while the bar grows.
pub fn draw_peek(
    renderer: &mut iced::Renderer,
    note: &Note,
    bar: Rectangle,
    radius: f32,
    progress: f32,
) {
    let rect = peek_bar(bar, progress);
    let [r, g, b, _] = note.color.rgba;
    renderer::Renderer::fill_quad(
        renderer,
        Quad {
            bounds: rect,
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25 * progress),
                offset: Vector::new(-2.0, 4.0),
                blur_radius: 12.0,
            },
            snap: true,
        },
        Color::from_rgb(r, g, b),
    );

    let alpha = ease_out_cubic((progress - 0.7) / 0.3);
    if alpha <= 0.0 {
        return;
    }

    let parts = peek_text(note);
    let left = bar.x + bar.width - PEEK_WIDTH.max(bar.width) + PADDING;
    let mut y = bar.y + ((bar.height - peek_height(&parts)) / 2.0).max(0.0) + PADDING;
    // Draws one text block below the previous one, after `gap` pixels.
    let mut line =
        |gap: f32, content: String, font: Font, size: f32, line: f32, rows: usize, color| {
            y += gap;
            let height = rows as f32 * line;
            renderer.fill_text(
                Text {
                    content,
                    bounds: Size::new(inner_width(), height),
                    size: Pixels(size),
                    line_height: LineHeight::Absolute(Pixels(line)),
                    font,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Top,
                    shaping: Shaping::Advanced,
                    wrapping: if rows > 1 {
                        Wrapping::WordOrGlyph
                    } else {
                        Wrapping::None
                    },
                },
                Point::new(left, y),
                color,
                rect,
            );
            y += height;
        };

    match parts.title {
        Some(title) => {
            let rows = title_rows(&title);
            line(
                0.0,
                title,
                TITLE_FONT,
                TITLE_SIZE,
                TITLE_LINE,
                rows,
                ink(alpha),
            );
        }
        None => line(
            0.0,
            "Empty note".into(),
            Font::DEFAULT,
            TITLE_SIZE,
            TITLE_LINE,
            1,
            ink(0.4 * alpha),
        ),
    }
    for (i, text) in parts.lines.into_iter().enumerate() {
        let gap = if i == 0 { GAP } else { 0.0 };
        line(
            gap,
            text,
            Font::DEFAULT,
            BODY_SIZE,
            BODY_LINE,
            1,
            ink(0.8 * alpha),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::NoteColor;

    fn note(title: &str, content: &str) -> Note {
        let mut n = Note::new(NoteColor::new(1.0, 0.85, 0.24));
        n.title = title.to_string();
        n.content = content.to_string();
        n
    }

    #[test]
    fn shows_title_and_next_three_lines() {
        let t = peek_text(&note("Groceries", "milk\n\neggs\nbread\nbutter\njam"));
        assert_eq!(t.title.as_deref(), Some("Groceries"));
        assert_eq!(t.lines, ["milk", "eggs", "bread"]);
    }

    #[test]
    fn first_line_stands_in_for_missing_title() {
        let t = peek_text(&note("  ", "Call Anna\nre: offsite\nbook room"));
        assert_eq!(t.title.as_deref(), Some("Call Anna"));
        assert_eq!(t.lines, ["re: offsite", "book room"]);
    }

    #[test]
    fn long_lines_are_truncated_with_ellipsis() {
        let long = "x".repeat(200);
        let t = peek_text(&note("T", &long));
        let line = &t.lines[0];
        assert!(line.ends_with('…'));
        assert!(line.chars().count() <= body_chars_per_line());
    }

    #[test]
    fn long_titles_get_more_height() {
        let short = peek_height(&peek_text(&note("Short", "")));
        let long = peek_height(&peek_text(&note(&"word ".repeat(30), "")));
        assert!(long > short);
    }

    #[test]
    fn peek_widens_bar_leftward_only() {
        let bar = Rectangle::new(Point::new(960.0, 300.0), Size::new(30.0, 150.0));
        assert_eq!(peek_bar(bar, 0.0), bar);
        let full = peek_bar(bar, 1.0);
        assert!((full.width - PEEK_WIDTH).abs() < 0.01);
        assert!((full.x + full.width - 990.0).abs() < 0.01);
        assert_eq!((full.y, full.height), (bar.y, bar.height));
    }
}
