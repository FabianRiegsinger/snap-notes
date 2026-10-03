//! Hover peek: after resting on a bar, the bar itself widens to the left
//! into a small preview of the note: paper, title on top, a divider, and the
//! first lines of the body below, laid out like the open note.

use crate::animation::{ease_out_cubic, lerp};
use crate::note::Note;
use crate::note_panel::{ink, shade, TITLE_FONT};

use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::Text;
use iced::alignment;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Border, Color, Font, Pixels, Point, Rectangle, Shadow, Size, Vector};

pub const PEEK_WIDTH: f32 = 260.0;
/// Same insets and type sizes as the open note.
const PADDING_X: f32 = 18.0;
const PADDING_TOP: f32 = 12.0;
const PADDING_BOTTOM: f32 = 14.0;
const TITLE_SIZE: f32 = 16.0;
const TITLE_LINE: f32 = 21.0;
const BODY_SIZE: f32 = 14.0;
const BODY_LINE: f32 = 19.0;
/// Space between the title and the divider, and the divider and the body.
const DIVIDER_GAP: f32 = 6.0;
const BODY_GAP: f32 = 8.0;
/// The divider is inset like the note's.
const DIVIDER_INSET: f32 = 14.0;
const MAX_LINES: usize = 3;
/// Rough glyph widths as a share of the font size, kept generous so the
/// estimated text height never cuts text off.
const TITLE_GLYPH: f32 = 0.62;
const BODY_GLYPH: f32 = 0.55;

pub struct PeekText {
    pub title: Option<String>,
    pub lines: Vec<String>,
}

/// Where the peek and its parts are. Text positions are those of the fully
/// open peek, so the text never moves while the peek grows.
pub struct PeekLayout {
    pub rect: Rectangle,
    pub title: Point,
    pub divider_y: f32,
    pub body: Point,
}

fn inner_width() -> f32 {
    PEEK_WIDTH - 2.0 * PADDING_X
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

/// The title (if any) and the first lines of the body.
pub fn peek_text(note: &Note) -> PeekText {
    let title = Some(note.title.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let max_chars = body_chars_per_line();
    PeekText {
        title,
        lines: note
            .content
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(MAX_LINES)
            .map(|l| truncate(l, max_chars))
            .collect(),
    }
}

fn title_height(text: &PeekText) -> f32 {
    text.title.as_deref().map_or(1, title_rows) as f32 * TITLE_LINE
}

/// Height of the open peek: header, divider and body (at least one line,
/// which shows a placeholder for an empty body).
pub fn peek_height(text: &PeekText) -> f32 {
    let body = text.lines.len().max(1) as f32 * BODY_LINE;
    PADDING_TOP + title_height(text) + DIVIDER_GAP + 1.0 + BODY_GAP + body + PADDING_BOTTOM
}

/// The peek grown from `bar` by `progress`: it widens to the left and
/// grows to fit its text, centered on the bar and kept inside `bounds`.
pub fn peek_layout(
    bar: Rectangle,
    bounds: Rectangle,
    progress: f32,
    text: &PeekText,
) -> PeekLayout {
    let full_width = PEEK_WIDTH.max(bar.width);
    let full_height = peek_height(text).max(bar.height);
    let rect_at = |t: f32| {
        let width = lerp(bar.width, full_width, t);
        let height = lerp(bar.height, full_height, t);
        let center = bar.y + bar.height / 2.0;
        let max_y = (bounds.y + bounds.height - height).max(bounds.y);
        let y = (center - height / 2.0).clamp(bounds.y, max_y);
        Rectangle::new(
            Point::new(bar.x + bar.width - width, y),
            Size::new(width, height),
        )
    };
    let full = rect_at(1.0);
    let title = Point::new(full.x + PADDING_X, full.y + PADDING_TOP);
    let divider_y = title.y + title_height(text) + DIVIDER_GAP;
    PeekLayout {
        rect: rect_at(ease_out_cubic(progress)),
        title,
        divider_y,
        body: Point::new(title.x, divider_y + 1.0 + BODY_GAP),
    }
}

/// Draws the peek: the bar's color turning into paper, then the title,
/// divider and body fading in once the peek is nearly fully open.
pub fn draw_peek(
    renderer: &mut iced::Renderer,
    note: &Note,
    bar: Rectangle,
    bounds: Rectangle,
    radius: f32,
    progress: f32,
    paper_tint: f32,
) {
    let text = peek_text(note);
    let layout = peek_layout(bar, bounds, progress, &text);
    let rect = layout.rect;
    let t = ease_out_cubic(progress);
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
        shade(note, paper_tint * t, 1.0),
    );

    let alpha = ease_out_cubic((progress - 0.7) / 0.3);
    if alpha <= 0.0 {
        return;
    }

    let mut draw_text =
        |content: String, at: Point, font: Font, size: f32, line: f32, rows: usize, color| {
            renderer.fill_text(
                Text {
                    content,
                    bounds: Size::new(inner_width(), rows as f32 * line),
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
                at,
                color,
                rect,
            );
        };

    // Header: the title, or the note's faint placeholder.
    match &text.title {
        Some(title) => draw_text(
            title.clone(),
            layout.title,
            TITLE_FONT,
            TITLE_SIZE,
            TITLE_LINE,
            title_rows(title),
            ink(alpha),
        ),
        None => draw_text(
            "Title".into(),
            layout.title,
            TITLE_FONT,
            TITLE_SIZE,
            TITLE_LINE,
            1,
            ink(0.35 * alpha),
        ),
    }

    // Body: the first lines, or the note's placeholder.
    if text.lines.is_empty() {
        draw_text(
            "Write something…".into(),
            layout.body,
            Font::DEFAULT,
            BODY_SIZE,
            BODY_LINE,
            1,
            ink(0.35 * alpha),
        );
    }
    for (i, content) in text.lines.into_iter().enumerate() {
        let at = Point::new(layout.body.x, layout.body.y + i as f32 * BODY_LINE);
        draw_text(
            content,
            at,
            Font::DEFAULT,
            BODY_SIZE,
            BODY_LINE,
            1,
            ink(0.8 * alpha),
        );
    }

    renderer::Renderer::fill_quad(
        renderer,
        Quad {
            bounds: Rectangle::new(
                Point::new(rect.x + DIVIDER_INSET, layout.divider_y),
                Size::new((rect.width - 2.0 * DIVIDER_INSET).max(0.0), 1.0),
            ),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        },
        ink(0.12 * alpha),
    );
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
    fn missing_title_keeps_first_line_in_body() {
        let t = peek_text(&note("  ", "Call Anna\nre: offsite\nbook room"));
        assert_eq!(t.title, None);
        assert_eq!(t.lines, ["Call Anna", "re: offsite", "book room"]);
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

    fn screen() -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0))
    }

    #[test]
    fn peek_starts_as_the_bar() {
        let bar = Rectangle::new(Point::new(960.0, 300.0), Size::new(30.0, 150.0));
        let text = peek_text(&note("T", "a\nb"));
        assert_eq!(peek_layout(bar, screen(), 0.0, &text).rect, bar);
    }

    #[test]
    fn open_peek_widens_leftward_and_fits_content() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let text = peek_text(&note("Groceries", "milk\neggs\nbread"));
        let l = peek_layout(bar, screen(), 1.0, &text);
        assert!((l.rect.width - PEEK_WIDTH).abs() < 0.01);
        assert!((l.rect.x + l.rect.width - 990.0).abs() < 0.01);
        assert!((l.rect.height - peek_height(&text)).abs() < 0.01);
        let center = |r: Rectangle| r.y + r.height / 2.0;
        assert!((center(l.rect) - center(bar)).abs() < 0.01);
    }

    #[test]
    fn header_sits_on_top_and_body_below() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 200.0));
        let text = peek_text(&note("Groceries", "milk\neggs"));
        let l = peek_layout(bar, screen(), 1.0, &text);
        assert!((l.title.y - (l.rect.y + PADDING_TOP)).abs() < 0.01);
        assert!(l.divider_y >= l.title.y + TITLE_LINE);
        assert!(l.body.y > l.divider_y);
        assert!(l.body.y + 2.0 * BODY_LINE <= l.rect.y + l.rect.height);
    }

    #[test]
    fn peek_stays_on_screen() {
        let bar = Rectangle::new(Point::new(960.0, 0.0), Size::new(30.0, 30.0));
        let text = peek_text(&note("T", "a\nb\nc"));
        let l = peek_layout(bar, screen(), 1.0, &text);
        assert!(l.rect.y >= 0.0);
    }
}
