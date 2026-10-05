//! Hover peek: after resting on a bar, the bar itself widens to the left
//! into a small preview of the note: paper, title on top, a divider, and the
//! first lines of the body below, laid out like the open note.

use crate::animation::{ease_out_cubic, lerp};
use crate::app::NOTE_MARGIN;
use crate::icons::{Icon, ICON_FONT};
use crate::note::Note;
use crate::note_panel::{morph_paper, PLACEHOLDER};
use crate::rich;
use crate::theme::{self, Theme, TITLE_FONT};

use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::Text;
use iced::alignment;
use iced::border;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Border, Color, Font, Pixels, Point, Rectangle, Shadow, Size};

/// Same insets and type sizes as the open note.
const PADDING_X: f32 = 18.0;
/// Under the adhesive band, plus the open note's 2 px title padding.
const PADDING_TOP: f32 = BAND_HEIGHT + 2.0;
const BAND_HEIGHT: f32 = 16.0;
const PADDING_BOTTOM: f32 = 14.0;
const TITLE_SIZE: f32 = theme::TEXT_MD;
const TITLE_LINE: f32 = 21.0;
const BODY_SIZE: f32 = theme::TEXT_SM;
/// The open note's body font, for the body, confirmation and buttons.
const BODY_FONT: Font = theme::BODY_FONT;
/// The open note's body line height, as whole pixels for layout.
const BODY_LINE: f32 = (theme::TEXT_SM * theme::BODY_LINE_HEIGHT + 0.5).floor();
/// Space between the title and the divider, and the divider and the body.
const DIVIDER_GAP: f32 = 8.0;
const BODY_GAP: f32 = 8.0;
/// The divider is inset like the note's.
const DIVIDER_INSET: f32 = 14.0;
const MAX_LINES: usize = 3;
/// The delete button in the header's top-right corner.
const TRASH_SIZE: f32 = 22.0;
const TRASH_INSET: f32 = 8.0;
/// The "Delete this note?" question and its two buttons below it.
const CONFIRM_BUTTON: Size = Size::new(68.0, 24.0);
const CONFIRM_GAP: f32 = 8.0;
const CONFIRM_HEIGHT: f32 = BODY_LINE + CONFIRM_GAP + 24.0;
/// Rough glyph widths as a share of the font size, kept generous so the
/// estimated text height never cuts text off.
const TITLE_GLYPH: f32 = 0.62;
const BODY_GLYPH: f32 = 0.55;

pub struct PeekText {
    pub title: Option<String>,
    pub lines: Vec<String>,
    /// The peek's width, which the text was cut and wrapped for.
    pub width: f32,
}

/// Where the peek and its parts are. Text and button positions are those of
/// the fully open peek, so nothing moves while the peek grows.
pub struct PeekLayout {
    pub rect: Rectangle,
    pub title: Point,
    pub divider_y: f32,
    pub body: Point,
    /// The delete button in the header.
    pub trash: Rectangle,
    /// The confirmation's buttons, used while it shows.
    pub delete: Rectangle,
    pub cancel: Rectangle,
}

/// The width the peek of `note` asks for: the open note's, or the default
/// note width when it has none of its own.
pub fn peek_width(note: &Note, default: f32) -> f32 {
    note.size.map_or(default, |s| s[0])
}

/// `width` kept on screen: the peek grows left from `bar`, so it stops
/// `NOTE_MARGIN` short of the window's left edge, and never gets narrower
/// than the bar.
pub fn fit_peek_width(width: f32, bar: Rectangle) -> f32 {
    width.min(bar.x + bar.width - NOTE_MARGIN).max(bar.width)
}

/// The width the peek of `note` on `bar` is drawn and hit-tested with.
pub fn note_peek_width(note: &Note, default: f32, bar: Rectangle) -> f32 {
    fit_peek_width(peek_width(note, default), bar)
}

fn inner_width(width: f32) -> f32 {
    width - 2.0 * PADDING_X
}

fn body_chars_per_line(width: f32) -> usize {
    (inner_width(width) / (BODY_SIZE * BODY_GLYPH)).floor() as usize
}

/// The title leaves room for the delete button on its right.
fn title_width(width: f32) -> f32 {
    inner_width(width) - (TRASH_SIZE + TRASH_INSET - PADDING_X).max(0.0) - 4.0
}

fn title_rows(title: &str, width: f32) -> usize {
    let per_row = (title_width(width) / (TITLE_SIZE * TITLE_GLYPH))
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
pub fn peek_text(note: &Note, width: f32) -> PeekText {
    let title = Some(note.title.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let max_chars = body_chars_per_line(width);
    PeekText {
        width,
        title,
        lines: rich::plain_lines(&note.content, MAX_LINES)
            .iter()
            .map(|l| truncate(l, max_chars))
            .collect(),
    }
}

fn title_height(text: &PeekText) -> f32 {
    text.title
        .as_deref()
        .map_or(1, |t| title_rows(t, text.width)) as f32
        * TITLE_LINE
}

/// Height of the open peek: header, divider and body (at least one line,
/// which shows a placeholder for an empty body, and room for the delete
/// confirmation while `confirming`).
pub fn peek_height(text: &PeekText, confirming: bool) -> f32 {
    let mut body = text.lines.len().max(1) as f32 * BODY_LINE;
    if confirming {
        body = body.max(CONFIRM_HEIGHT);
    }
    PADDING_TOP + title_height(text) + DIVIDER_GAP + 1.0 + BODY_GAP + body + PADDING_BOTTOM
}

/// The peek grown from `bar` by `progress`: it widens to the left and
/// grows to fit its text, centered on the bar and kept inside `bounds`.
pub fn peek_layout(
    bar: Rectangle,
    bounds: Rectangle,
    progress: f32,
    text: &PeekText,
    confirming: bool,
    width: f32,
) -> PeekLayout {
    let full_width = width.max(bar.width);
    let full_height = peek_height(text, confirming).max(bar.height);
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
    let body = Point::new(title.x, divider_y + 1.0 + BODY_GAP);
    let buttons_y = body.y + BODY_LINE + CONFIRM_GAP;
    PeekLayout {
        rect: rect_at(ease_out_cubic(progress)),
        title,
        divider_y,
        body,
        trash: Rectangle::new(
            Point::new(
                full.x + full.width - TRASH_INSET - TRASH_SIZE,
                title.y + (TITLE_LINE - TRASH_SIZE) / 2.0,
            ),
            Size::new(TRASH_SIZE, TRASH_SIZE),
        ),
        delete: Rectangle::new(Point::new(body.x, buttons_y), CONFIRM_BUTTON),
        cancel: Rectangle::new(
            Point::new(body.x + CONFIRM_BUTTON.width + CONFIRM_GAP, buttons_y),
            CONFIRM_BUTTON,
        ),
    }
}

/// Corner radius while opening: the bar's, rounding into the note's.
fn peek_radius(bar_radius: f32, progress: f32) -> f32 {
    lerp(bar_radius, theme::RADIUS_SURFACE, ease_out_cubic(progress))
}

/// Draws `label` centered in `rect`, clipped to `clip`.
fn draw_label(
    renderer: &mut iced::Renderer,
    label: &str,
    rect: Rectangle,
    font: Font,
    size: f32,
    color: Color,
    clip: Rectangle,
) {
    renderer.fill_text(
        Text {
            content: label.to_string(),
            bounds: rect.size(),
            size: Pixels(size),
            line_height: LineHeight::default(),
            font,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            shaping: Shaping::Advanced,
            wrapping: Wrapping::None,
        },
        rect.center(),
        color,
        clip,
    );
}

/// A rounded button face: `fill` behind a centered `label`.
fn draw_button(
    renderer: &mut iced::Renderer,
    label: &str,
    rect: Rectangle,
    fill: Color,
    text_color: Color,
    clip: Rectangle,
) {
    renderer::Renderer::fill_quad(
        renderer,
        Quad {
            bounds: rect,
            border: Border {
                radius: theme::RADIUS_CONTROL.into(),
                ..Default::default()
            },
            shadow: Shadow::default(),
            snap: true,
        },
        fill,
    );
    draw_label(
        renderer,
        label,
        rect,
        BODY_FONT,
        theme::TEXT_SM,
        text_color,
        clip,
    );
}

/// Draws the peek: the bar's color turning into paper, then the title,
/// divider and body fading in once the peek is nearly fully open. The
/// header has a delete button; while `confirming`, the body asks first.
/// `cursor` darkens whichever button it is over.
#[allow(clippy::too_many_arguments)]
pub fn draw_peek(
    renderer: &mut iced::Renderer,
    note: &Note,
    bar: Rectangle,
    bounds: Rectangle,
    theme: &Theme,
    radius: f32,
    progress: f32,
    paper_tint: f32,
    confirming: bool,
    cursor: Option<Point>,
    width: f32,
) {
    let text = peek_text(note, width);
    let layout = peek_layout(bar, bounds, progress, &text, confirming, width);
    let rect = layout.rect;
    let t = ease_out_cubic(progress);
    // Gradient quads draw no shadow, so each shadow sits on its own solid
    // paper quad under the gradient one: ambient first, then contact.
    let paper = morph_paper(theme, note.color, paper_tint, progress);
    let corner = peek_radius(radius, progress);
    let [contact, ambient] = theme.shadows(progress);
    for shadow in [ambient, contact] {
        renderer::Renderer::fill_quad(
            renderer,
            Quad {
                bounds: rect,
                border: Border {
                    radius: corner.into(),
                    ..Default::default()
                },
                shadow,
                snap: true,
            },
            paper,
        );
    }
    renderer::Renderer::fill_quad(
        renderer,
        Quad {
            bounds: rect,
            border: Border {
                radius: corner.into(),
                ..Default::default()
            },
            shadow: Shadow::default(),
            snap: true,
        },
        theme.paper_gradient(paper, 1.0),
    );
    // The adhesive band and 1 px highlight fade in as the bar becomes paper.
    let fade = |c: Color| Color { a: c.a * t, ..c };
    renderer::Renderer::fill_quad(
        renderer,
        Quad {
            bounds: Rectangle {
                height: BAND_HEIGHT.min(rect.height),
                ..rect
            },
            border: Border {
                radius: border::top(corner),
                ..Default::default()
            },
            shadow: Shadow::default(),
            snap: true,
        },
        fade(theme.band()),
    );
    renderer::Renderer::fill_quad(
        renderer,
        Quad {
            bounds: Rectangle::new(
                Point::new(rect.x + corner, rect.y),
                Size::new((rect.width - 2.0 * corner).max(0.0), 1.0),
            ),
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        },
        fade(theme.highlight()),
    );

    let alpha = ease_out_cubic((progress - 0.7) / 0.3);
    if alpha <= 0.0 {
        return;
    }

    let hovered = |r: Rectangle| cursor.is_some_and(|c| r.contains(c));
    let mut draw_text = |content: String,
                         at: Point,
                         font: Font,
                         size: f32,
                         line: f32,
                         line_height: LineHeight,
                         rows: usize,
                         color| {
        // The title stays clear of the delete button.
        let width = if font == TITLE_FONT {
            title_width(width)
        } else {
            inner_width(width)
        };
        renderer.fill_text(
            Text {
                content,
                bounds: Size::new(width, rows as f32 * line),
                size: Pixels(size),
                line_height,
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
            LineHeight::Absolute(Pixels(TITLE_LINE)),
            title_rows(title, width),
            theme.ink(alpha),
        ),
        None => draw_text(
            "Title".into(),
            layout.title,
            TITLE_FONT,
            TITLE_SIZE,
            TITLE_LINE,
            LineHeight::Absolute(Pixels(TITLE_LINE)),
            1,
            theme.ink(0.35 * alpha),
        ),
    }

    // Body: the delete confirmation, the first lines, or the placeholder.
    if confirming {
        draw_text(
            "Delete this note?".into(),
            layout.body,
            BODY_FONT,
            theme::TEXT_SM,
            BODY_LINE,
            LineHeight::Relative(theme::BODY_LINE_HEIGHT),
            1,
            theme.ink(alpha),
        );
    } else if text.lines.is_empty() {
        draw_text(
            PLACEHOLDER.into(),
            layout.body,
            BODY_FONT,
            BODY_SIZE,
            BODY_LINE,
            LineHeight::Relative(theme::BODY_LINE_HEIGHT),
            1,
            theme.ink(0.35 * alpha),
        );
    }
    for (i, content) in text
        .lines
        .into_iter()
        .enumerate()
        .take_while(|_| !confirming)
    {
        let at = Point::new(layout.body.x, layout.body.y + i as f32 * BODY_LINE);
        draw_text(
            content,
            at,
            BODY_FONT,
            BODY_SIZE,
            BODY_LINE,
            LineHeight::Relative(theme::BODY_LINE_HEIGHT),
            1,
            theme.ink(0.8 * alpha),
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
        theme.ink(0.12 * alpha),
    );

    let trash_alpha = if hovered(layout.trash) || confirming {
        0.9
    } else {
        0.45
    };
    draw_label(
        renderer,
        &Icon::Trash.codepoint().to_string(),
        layout.trash,
        ICON_FONT,
        14.0,
        theme.ink(trash_alpha * alpha),
        rect,
    );
    if confirming {
        let shade_if = |r: Rectangle, base: f32| if hovered(r) { base + 0.08 } else { base };
        draw_button(
            renderer,
            "Delete",
            layout.delete,
            theme.danger(shade_if(layout.delete, 0.87) * alpha),
            Color::WHITE,
            rect,
        );
        draw_button(
            renderer,
            "Cancel",
            layout.cancel,
            theme.ink(shade_if(layout.cancel, 0.12) * alpha),
            theme.ink(alpha),
            rect,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::NoteColor;

    const PEEK_WIDTH: f32 = 260.0;

    fn note(title: &str, content: &str) -> Note {
        let mut n = Note::new(NoteColor::new(1.0, 0.85, 0.24));
        n.title = title.to_string();
        n.content = content.to_string();
        n
    }

    #[test]
    fn peek_body_line_follows_type_scale() {
        assert_eq!(
            BODY_LINE,
            (theme::TEXT_SM * theme::BODY_LINE_HEIGHT).round()
        );
        assert_eq!(TITLE_SIZE, theme::TEXT_MD);
    }

    #[test]
    fn peek_uses_bundled_font() {
        assert_eq!(BODY_FONT, theme::BODY_FONT);
    }

    #[test]
    fn shows_title_and_next_three_lines() {
        let t = peek_text(
            &note("Groceries", "milk\n\neggs\nbread\nbutter\njam"),
            PEEK_WIDTH,
        );
        assert_eq!(t.title.as_deref(), Some("Groceries"));
        assert_eq!(t.lines, ["milk", "eggs", "bread"]);
    }

    #[test]
    fn missing_title_keeps_first_line_in_body() {
        let t = peek_text(&note("  ", "Call Anna\nre: offsite\nbook room"), PEEK_WIDTH);
        assert_eq!(t.title, None);
        assert_eq!(t.lines, ["Call Anna", "re: offsite", "book room"]);
    }

    #[test]
    fn markup_is_stripped_in_peek() {
        let t = peek_text(&note("", "**Buy**\n- [ ] milk"), PEEK_WIDTH);
        assert_eq!(t.lines, ["Buy", "☐ milk"]);
    }

    #[test]
    fn long_lines_are_truncated_with_ellipsis() {
        let long = "x".repeat(200);
        let t = peek_text(&note("T", &long), PEEK_WIDTH);
        let line = &t.lines[0];
        assert!(line.ends_with('…'));
        assert!(line.chars().count() <= body_chars_per_line(PEEK_WIDTH));
    }

    #[test]
    fn long_titles_get_more_height() {
        let short = peek_height(&peek_text(&note("Short", ""), PEEK_WIDTH), false);
        let long = peek_height(
            &peek_text(&note(&"word ".repeat(30), ""), PEEK_WIDTH),
            false,
        );
        assert!(long > short);
    }

    fn screen() -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(1000.0, 900.0))
    }

    #[test]
    fn peek_corners_round_like_the_note() {
        assert_eq!(peek_radius(3.0, 0.0), 3.0);
        assert_eq!(peek_radius(3.0, 1.0), theme::RADIUS_SURFACE);
    }

    #[test]
    fn peek_starts_as_the_bar() {
        let bar = Rectangle::new(Point::new(960.0, 300.0), Size::new(30.0, 150.0));
        let text = peek_text(&note("T", "a\nb"), PEEK_WIDTH);
        assert_eq!(
            peek_layout(bar, screen(), 0.0, &text, false, PEEK_WIDTH).rect,
            bar
        );
    }

    #[test]
    fn open_peek_widens_leftward_and_fits_content() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let text = peek_text(&note("Groceries", "milk\neggs\nbread"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, false, PEEK_WIDTH);
        assert!((l.rect.width - PEEK_WIDTH).abs() < 0.01);
        assert!((l.rect.x + l.rect.width - 990.0).abs() < 0.01);
        assert!((l.rect.height - peek_height(&text, false)).abs() < 0.01);
        let center = |r: Rectangle| r.y + r.height / 2.0;
        assert!((center(l.rect) - center(bar)).abs() < 0.01);
    }

    #[test]
    fn header_sits_on_top_and_body_below() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 200.0));
        let text = peek_text(&note("Groceries", "milk\neggs"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, false, PEEK_WIDTH);
        assert!((l.title.y - (l.rect.y + PADDING_TOP)).abs() < 0.01);
        assert!(l.divider_y >= l.title.y + TITLE_LINE);
        assert!(l.body.y > l.divider_y);
        assert!(l.body.y + 2.0 * BODY_LINE <= l.rect.y + l.rect.height);
    }

    fn inside(outer: Rectangle, inner: Rectangle) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.width <= outer.x + outer.width
            && inner.y + inner.height <= outer.y + outer.height
    }

    #[test]
    fn trash_sits_in_the_header_beside_the_title() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let text = peek_text(&note(&"word ".repeat(30), "milk"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, false, PEEK_WIDTH);
        assert!(inside(l.rect, l.trash));
        assert!(l.trash.y + l.trash.height <= l.divider_y);
        assert!(l.title.x + title_width(PEEK_WIDTH) <= l.trash.x);
    }

    #[test]
    fn confirmation_buttons_fit_below_the_divider() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        // An empty body is one line tall; the confirmation needs more.
        let text = peek_text(&note("T", ""), PEEK_WIDTH);
        let open = peek_layout(bar, screen(), 1.0, &text, false, PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, true, PEEK_WIDTH);
        assert!(l.rect.height > open.rect.height);
        for button in [l.delete, l.cancel] {
            assert!(inside(l.rect, button));
            assert!(button.y > l.divider_y);
        }
        assert!(!l.delete.intersects(&l.cancel));
        assert!(!l.delete.intersects(&l.trash));
    }

    #[test]
    fn peek_stays_on_screen() {
        let bar = Rectangle::new(Point::new(960.0, 0.0), Size::new(30.0, 30.0));
        let text = peek_text(&note("T", "a\nb\nc"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, false, PEEK_WIDTH);
        assert!(l.rect.y >= 0.0);
    }

    #[test]
    fn peek_width_follows_note_or_default() {
        let mut n = note("T", "");
        assert_eq!(peek_width(&n, 500.0), 500.0);
        n.size = Some([320.0, 400.0]);
        assert_eq!(peek_width(&n, 500.0), 320.0);
    }

    #[test]
    fn open_peek_uses_given_width() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let width = 480.0;
        let text = peek_text(&note("Groceries", "milk"), width);
        let l = peek_layout(bar, screen(), 1.0, &text, false, width);
        assert!((l.rect.width - width).abs() < 0.01);
        assert!((l.rect.x + l.rect.width - 990.0).abs() < 0.01);
        assert!(l.rect.contains(l.trash.position()));
        assert!(l.rect.contains(Point::new(
            l.trash.x + l.trash.width,
            l.trash.y + l.trash.height
        )));
    }

    #[test]
    fn wider_peek_shows_more_of_a_long_line() {
        let long = "x".repeat(200);
        let narrow = peek_text(&note("T", &long), 260.0);
        let wide = peek_text(&note("T", &long), 500.0);
        assert!(wide.lines[0].chars().count() > narrow.lines[0].chars().count());
    }

    #[test]
    fn peek_width_stays_on_screen() {
        let bar = Rectangle::new(Point::new(300.0, 0.0), Size::new(30.0, 40.0));
        assert_eq!(fit_peek_width(900.0, bar), 330.0 - NOTE_MARGIN);
        assert_eq!(fit_peek_width(100.0, bar), 100.0);
        let edge = Rectangle::new(Point::new(0.0, 0.0), Size::new(30.0, 40.0));
        assert_eq!(fit_peek_width(900.0, edge), 30.0);
    }
}
