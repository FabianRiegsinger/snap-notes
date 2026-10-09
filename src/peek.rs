//! Hover peek: after resting on a bar, the bar itself widens away from the
//! screen edge into a small preview of the note: paper, title on top, a
//! divider, and the first lines of the body below, laid out like the open
//! note.

use crate::animation::{ease_out_cubic, lerp};
use crate::edge::Edge;
use crate::icons::{Icon, ICON_FONT};
use crate::note::Note;
use crate::note_panel::{morph_paper, PLACEHOLDER};
use crate::reminder;
use crate::rich;
use crate::strip_model::{self, Entry};
use crate::theme::{self, Theme, TITLE_FONT};

use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::Renderer as _;
use iced::advanced::Text;
use iced::alignment;
use iced::border;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Border, Color, Font, Pixels, Point, Rectangle, Shadow, Size};
use uuid::Uuid;

/// Same insets and type sizes as the open note.
const PADDING_X: f32 = 18.0;
/// Under the adhesive band, plus the open note's 2 px title padding.
const PADDING_TOP: f32 = BAND_HEIGHT + 2.0;
const BAND_HEIGHT: f32 = 16.0;
const PADDING_BOTTOM: f32 = 14.0;
const TITLE_SIZE: f32 = theme::TEXT_MD;
const TITLE_LINE: f32 = 21.0;
const BODY_SIZE: f32 = theme::TEXT_SM;
/// The open note's body font, for the body and buttons.
const BODY_FONT: Font = theme::BODY_FONT;
/// The open note's body line height, as whole pixels for layout.
const BODY_LINE: f32 = (theme::TEXT_SM * theme::BODY_LINE_HEIGHT + 0.5).floor();
/// Space between the title and the divider, and the divider and the body.
const DIVIDER_GAP: f32 = 8.0;
const BODY_GAP: f32 = 8.0;
/// The divider is inset like the note's.
const DIVIDER_INSET: f32 = 14.0;
/// Body lines shown at once; the wheel scrolls through the rest.
const MAX_LINES: usize = 3;
/// Body lines kept for scrolling, so a huge note stays cheap to peek.
const MAX_SCROLL_LINES: usize = 500;
/// The delete button in the header's top-right corner.
const TRASH_SIZE: f32 = 22.0;
const TRASH_INSET: f32 = 8.0;
/// The stack peek's "Unstack" button below its list, this far below it.
const UNSTACK_GAP: f32 = 8.0;
const UNSTACK_BUTTON: Size = Size::new(76.0, 24.0);
/// Shown in a stack's list for a note without a title.
const UNTITLED: &str = "Untitled";
/// A stack's list shows at most this many notes, then "+N more".
const MAX_STACK_ROWS: usize = 8;
/// Rough glyph widths as a share of the font size, kept generous so the
/// estimated text height never cuts text off.
const TITLE_GLYPH: f32 = 0.62;
const BODY_GLYPH: f32 = 0.55;

pub struct PeekText {
    pub title: Option<String>,
    /// The body's lines; the first `MAX_LINES` show, the rest scroll in.
    pub lines: Vec<String>,
    /// Done and total task items, shown in the header.
    pub progress: Option<(usize, usize)>,
    /// The title's reminder time (`Tue 15:00`), shown after a bell.
    pub reminder: Option<String>,
    /// The peek's width, which the text was cut and wrapped for.
    pub width: f32,
    /// A stack's notes, top first, by id and title: listed in place of the
    /// body lines. Empty for a single note.
    pub stack: Vec<(Uuid, String)>,
    /// How many of the stack's notes the list leaves out (`+N more`).
    pub stack_more: usize,
}

/// Where the peek and its parts are. Text and button positions are those of
/// the fully open peek, so nothing moves while the peek grows.
pub struct PeekLayout {
    pub rect: Rectangle,
    pub title: Point,
    /// The `done/total` text in the header, left of the delete button.
    pub progress: Rectangle,
    /// The bell and reminder time, left of the progress.
    pub reminder: Rectangle,
    pub divider_y: f32,
    pub body: Point,
    /// The body's shown rows, which scrolled lines are clipped to.
    pub body_rect: Rectangle,
    /// The delete button in the header.
    pub trash: Rectangle,
    /// A stack's list rows, one per note of `PeekText::stack`.
    pub stack_rows: Vec<Rectangle>,
    /// The faint `+N more` row below a long stack's list; not clickable.
    pub stack_more: Option<Rectangle>,
    /// A stack's "Unstack" button below its list, kept inside the peek.
    pub unstack: Option<Rectangle>,
}

/// The width the peek of `note` asks for: the open note's, or the default
/// note width when it has none of its own.
pub fn peek_width(note: &Note, default: f32) -> f32 {
    note.size.map_or(default, |s| s[0])
}

/// Narrowest peek, the width before it followed the note. In a window too
/// narrow for more, the window clips the peek.
const MIN_PEEK_WIDTH: f32 = 260.0;
/// Space kept free left of the peek.
const PEEK_MARGIN: f32 = 24.0;

/// `width` kept on screen: the peek grows left from `bar`, so it stops
/// `PEEK_MARGIN` short of the window's left edge, but never gets narrower
/// than `MIN_PEEK_WIDTH`.
pub fn fit_peek_width(width: f32, bar: Rectangle) -> f32 {
    width
        .min(bar.x + bar.width - PEEK_MARGIN)
        .max(MIN_PEEK_WIDTH)
}

/// The width the peek of `note` on `bar` is drawn and hit-tested with. On
/// Right it stops short of the window's left edge (see `fit_peek_width`);
/// elsewhere the room is the window's to provide.
pub fn note_peek_width(note: &Note, default: f32, bar: Rectangle, edge: Edge) -> f32 {
    let width = peek_width(note, default);
    match edge {
        Edge::Right => fit_peek_width(width, bar),
        Edge::Left | Edge::Top => width.max(MIN_PEEK_WIDTH),
    }
}

fn inner_width(width: f32) -> f32 {
    width - 2.0 * PADDING_X
}

fn body_chars_per_line(width: f32) -> usize {
    (inner_width(width) / (BODY_SIZE * BODY_GLYPH)).floor() as usize
}

/// Space the header's `done/total` text takes left of the delete button.
const PROGRESS_GAP: f32 = 6.0;

/// The `done/total` text of a checklist.
fn progress_label((done, total): (usize, usize)) -> String {
    format!("{done}/{total}")
}

/// Width reserved for the progress text, with the gap before it; generous
/// like the other glyph estimates.
fn progress_reserve(progress: Option<(usize, usize)>) -> f32 {
    progress.map_or(0.0, |p| {
        progress_label(p).chars().count() as f32 * BODY_SIZE * BODY_GLYPH + PROGRESS_GAP
    })
}

/// The bell's size in the header, and the space between it and the time.
const BELL_SIZE: f32 = 14.0;
const BELL_GAP: f32 = 4.0;

/// Width reserved for the bell and reminder time, with the gap before them.
fn reminder_reserve(reminder: Option<&str>) -> f32 {
    reminder.map_or(0.0, |label| {
        BELL_SIZE + BELL_GAP + label.chars().count() as f32 * BODY_SIZE * BODY_GLYPH + PROGRESS_GAP
    })
}

/// The title leaves room for the delete button, the progress and the
/// reminder on its right.
fn title_width(text: &PeekText) -> f32 {
    inner_width(text.width)
        - (TRASH_SIZE + TRASH_INSET - PADDING_X).max(0.0)
        - 4.0
        - progress_reserve(text.progress)
        - reminder_reserve(text.reminder.as_deref())
}

fn title_rows(title: &str, text: &PeekText) -> usize {
    let per_row = (title_width(text) / (TITLE_SIZE * TITLE_GLYPH))
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

/// The title (if any) and the body's lines.
pub fn peek_text(note: &Note, width: f32) -> PeekText {
    let title = Some(note.title.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let max_chars = body_chars_per_line(width);
    PeekText {
        width,
        title,
        lines: rich::plain_lines(&note.content, MAX_SCROLL_LINES)
            .iter()
            .map(|l| truncate(l, max_chars))
            .collect(),
        progress: rich::task_progress(&note.content),
        reminder: reminder::at(note).map(reminder::label),
        stack: Vec::new(),
        stack_more: 0,
    }
}

/// The peek of `entry`'s bar: its top note's, and for a stack the titles of
/// its notes in place of the body (at most `MAX_STACK_ROWS`, the top first),
/// with the header counting the whole stack's tasks like its bar and showing
/// its earliest pending reminder.
pub fn entry_peek_text(notes: &[Note], entry: &Entry, width: f32) -> PeekText {
    let mut text = peek_text(&notes[entry.top], width);
    if entry.members.is_empty() {
        return text;
    }
    let max_chars = body_chars_per_line(width);
    text.lines.clear();
    text.progress = strip_model::progress(notes, std::slice::from_ref(entry))[0];
    let soonest = entry
        .notes()
        .filter_map(|i| Some((reminder::pending(&notes[i])?, i)))
        .min()
        .and_then(|(_, i)| reminder::at(&notes[i]));
    if let Some(at) = soonest {
        text.reminder = Some(reminder::label(at));
    }
    let count = 1 + entry.members.len();
    text.stack_more = count.saturating_sub(MAX_STACK_ROWS);
    text.stack = entry
        .notes()
        .take(MAX_STACK_ROWS)
        .map(|i| {
            let title = notes[i].title.trim();
            let title = if title.is_empty() { UNTITLED } else { title };
            (notes[i].id, truncate(title, max_chars))
        })
        .collect();
    text
}

fn title_height(text: &PeekText) -> f32 {
    text.title.as_deref().map_or(1, |t| title_rows(t, text)) as f32 * TITLE_LINE
}

/// Body rows the peek shows: up to `MAX_LINES`, at least one (for the
/// placeholder of an empty body).
fn shown_rows(text: &PeekText) -> usize {
    text.lines.len().clamp(1, MAX_LINES)
}

/// How far the body scrolls to show its last line, in pixels.
pub fn max_body_scroll(text: &PeekText) -> f32 {
    text.lines.len().saturating_sub(MAX_LINES) as f32 * BODY_LINE
}

/// The body's scroll after a wheel `dy` (positive scrolls up) from `scroll`.
pub fn scroll_body(text: &PeekText, scroll: f32, dy: f32) -> f32 {
    (scroll - dy).clamp(0.0, max_body_scroll(text))
}

/// Width of the body's scrollbar, centered in the peek's right padding.
const SCROLLBAR_WIDTH: f32 = 4.0;
/// The scrollbar's thumb never gets shorter than this.
const MIN_THUMB: f32 = 12.0;

/// The body's scrollbar track and thumb at `scroll`, when it has more lines
/// than it shows.
pub fn scrollbar(
    text: &PeekText,
    layout: &PeekLayout,
    scroll: f32,
) -> Option<(Rectangle, Rectangle)> {
    let max = max_body_scroll(text);
    if max <= 0.0 {
        return None;
    }
    let body = layout.body_rect;
    let track = Rectangle::new(
        Point::new(
            body.x + body.width + (PADDING_X - SCROLLBAR_WIDTH) / 2.0,
            body.y,
        ),
        Size::new(SCROLLBAR_WIDTH, body.height),
    );
    let thumb_height = (track.height * body.height / (body.height + max))
        .max(MIN_THUMB)
        .min(track.height);
    let travel = track.height - thumb_height;
    let thumb = Rectangle::new(
        Point::new(track.x, track.y + travel * (scroll / max).clamp(0.0, 1.0)),
        Size::new(SCROLLBAR_WIDTH, thumb_height),
    );
    Some((track, thumb))
}

/// Height of the open peek: header, divider and body (at least one line,
/// which shows a placeholder for an empty body).
pub fn peek_height(text: &PeekText) -> f32 {
    let body = if text.stack.is_empty() {
        shown_rows(text) as f32 * BODY_LINE
    } else {
        let rows = text.stack.len() + usize::from(text.stack_more > 0);
        rows as f32 * BODY_LINE + UNSTACK_GAP + UNSTACK_BUTTON.height
    };
    PADDING_TOP + title_height(text) + DIVIDER_GAP + 1.0 + BODY_GAP + body + PADDING_BOTTOM
}

/// The peek grown from `bar` by `progress`: it grows away from the screen
/// `edge` out of the bar's edge side and to fit its text, centred on the
/// bar along the edge and kept inside `bounds` along it (a peek longer than
/// `bounds` along the edge is cut to fit).
pub fn peek_layout(
    bar: Rectangle,
    bounds: Rectangle,
    progress: f32,
    text: &PeekText,
    width: f32,
    edge: Edge,
) -> PeekLayout {
    let full_width = width.max(bar.width);
    let full_height = peek_height(text).max(bar.height);
    let (full_width, full_height) = match edge {
        Edge::Right | Edge::Left => (full_width, full_height.min(bounds.height)),
        Edge::Top => (full_width.min(bounds.width), full_height),
    };
    // The bar's side on the screen edge, which the peek grows away from.
    let base = match edge {
        Edge::Right => Rectangle::new(
            Point::new(bar.x + bar.width, bar.y),
            Size::new(0.0, bar.height),
        ),
        Edge::Left => Rectangle::new(bar.position(), Size::new(0.0, bar.height)),
        Edge::Top => Rectangle::new(bar.position(), Size::new(bar.width, 0.0)),
    };
    let clamp = edge.along_only(bounds);
    let rect_at = |t: f32| {
        let size = Size::new(
            lerp(bar.width, full_width, t),
            lerp(bar.height, full_height, t),
        );
        edge.place_away(base, size, 0.0, clamp)
    };
    let full = rect_at(1.0);
    let title = Point::new(full.x + PADDING_X, full.y + PADDING_TOP);
    let trash = Rectangle::new(
        Point::new(
            full.x + full.width - TRASH_INSET - TRASH_SIZE,
            title.y + (TITLE_LINE - TRASH_SIZE) / 2.0,
        ),
        Size::new(TRASH_SIZE, TRASH_SIZE),
    );
    let progress_width = progress_reserve(text.progress) - PROGRESS_GAP;
    let reminder_width = reminder_reserve(text.reminder.as_deref()) - PROGRESS_GAP;
    let reminder_end = trash.x - 4.0 - progress_reserve(text.progress);
    let divider_y = title.y + title_height(text) + DIVIDER_GAP;
    let body = Point::new(title.x, divider_y + 1.0 + BODY_GAP);
    let stack_rows: Vec<Rectangle> = (0..text.stack.len())
        .map(|i| {
            Rectangle::new(
                Point::new(body.x, body.y + i as f32 * BODY_LINE),
                Size::new(inner_width(text.width), BODY_LINE),
            )
        })
        .collect();
    let stack_more = (text.stack_more > 0).then(|| {
        Rectangle::new(
            Point::new(body.x, body.y + stack_rows.len() as f32 * BODY_LINE),
            Size::new(inner_width(text.width), BODY_LINE),
        )
    });
    // In a peek cut to fit, the button rises over the list so it stays
    // reachable: it is the only way to dissolve the stack.
    let unstack = stack_more.or(stack_rows.last().copied()).map(|last| {
        let lowest = full.y + full.height - PADDING_BOTTOM - UNSTACK_BUTTON.height;
        Rectangle::new(
            Point::new(body.x, (last.y + last.height + UNSTACK_GAP).min(lowest)),
            UNSTACK_BUTTON,
        )
    });
    PeekLayout {
        rect: rect_at(ease_out_cubic(progress)),
        title,
        divider_y,
        body,
        body_rect: Rectangle::new(
            body,
            Size::new(inner_width(text.width), shown_rows(text) as f32 * BODY_LINE),
        ),
        progress: Rectangle::new(
            Point::new(trash.x - 4.0 - progress_width, title.y),
            Size::new(progress_width, TITLE_LINE),
        ),
        reminder: Rectangle::new(
            Point::new(reminder_end - reminder_width.max(0.0), title.y),
            Size::new(reminder_width.max(0.0), TITLE_LINE),
        ),
        trash,
        stack_rows,
        stack_more,
        unstack,
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
/// header has a delete button.
/// `cursor` darkens whichever button it is over.
#[allow(clippy::too_many_arguments)]
pub fn draw_peek(
    renderer: &mut iced::Renderer,
    note: &Note,
    text: &PeekText,
    bar: Rectangle,
    bounds: Rectangle,
    theme: &Theme,
    radius: f32,
    progress: f32,
    paper_tint: f32,
    cursor: Option<Point>,
    width: f32,
    scroll: f32,
    edge: Edge,
) {
    let layout = peek_layout(bar, bounds, progress, text, width, edge);
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
    // The stack row under the cursor is shaded, before any text draws.
    if let Some(row) = layout.stack_rows.iter().find(|r| hovered(**r)) {
        renderer::Renderer::fill_quad(
            renderer,
            Quad {
                bounds: row.expand(2.0),
                border: Border {
                    radius: theme::RADIUS_CONTROL.into(),
                    ..Default::default()
                },
                shadow: Shadow::default(),
                snap: true,
            },
            theme.ink(0.06 * alpha),
        );
    }
    if let Some((track, thumb)) = scrollbar(text, &layout, scroll) {
        for (bounds, ink) in [(track, 0.08), (thumb, 0.35)] {
            renderer::Renderer::fill_quad(
                renderer,
                Quad {
                    bounds,
                    border: Border {
                        radius: (SCROLLBAR_WIDTH / 2.0).into(),
                        ..Default::default()
                    },
                    shadow: Shadow::default(),
                    snap: true,
                },
                theme.ink(ink * alpha),
            );
        }
    }
    let title_width = title_width(text);
    let mut draw_text = |content: String,
                         at: Point,
                         font: Font,
                         size: f32,
                         line: f32,
                         line_height: LineHeight,
                         rows: usize,
                         color,
                         clip: Rectangle| {
        // The title stays clear of the delete button.
        let width = if font == TITLE_FONT {
            title_width
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
            clip,
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
            title_rows(title, text),
            theme.ink(alpha),
            rect,
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
            rect,
        ),
    }

    // Body: the lines scrolled into the shown rows, or the placeholder.
    if text.lines.is_empty() && text.stack.is_empty() {
        draw_text(
            PLACEHOLDER.into(),
            layout.body,
            BODY_FONT,
            BODY_SIZE,
            BODY_LINE,
            LineHeight::Relative(theme::BODY_LINE_HEIGHT),
            1,
            theme.ink(0.35 * alpha),
            rect,
        );
    }
    let body_clip = rect.intersection(&layout.body_rect).unwrap_or_default();
    let scroll = scroll.clamp(0.0, max_body_scroll(text));
    let first = (scroll / BODY_LINE).floor() as usize;
    for (i, content) in text
        .lines
        .iter()
        .enumerate()
        .skip(first)
        .take(MAX_LINES + 1)
    {
        let at = Point::new(layout.body.x, layout.body.y + i as f32 * BODY_LINE - scroll);
        draw_text(
            content.clone(),
            at,
            BODY_FONT,
            BODY_SIZE,
            BODY_LINE,
            LineHeight::Relative(theme::BODY_LINE_HEIGHT),
            1,
            theme.ink(0.8 * alpha),
            body_clip,
        );
    }

    // A stack: its notes' titles, each opening its note, then "Unstack".
    let shade_if = |r: Rectangle, base: f32| if hovered(r) { base + 0.08 } else { base };
    for ((_, title), row) in text.stack.iter().zip(&layout.stack_rows) {
        let faint = if title == UNTITLED { 0.35 } else { 0.8 };
        draw_text(
            title.clone(),
            row.position(),
            BODY_FONT,
            BODY_SIZE,
            BODY_LINE,
            LineHeight::Relative(theme::BODY_LINE_HEIGHT),
            1,
            theme.ink(faint * alpha),
            rect,
        );
    }
    if let Some(more) = layout.stack_more {
        draw_text(
            format!("+{} more", text.stack_more),
            more.position(),
            BODY_FONT,
            BODY_SIZE,
            BODY_LINE,
            LineHeight::Relative(theme::BODY_LINE_HEIGHT),
            1,
            theme.ink(0.35 * alpha),
            rect,
        );
    }
    if let Some(unstack) = layout.unstack {
        draw_button(
            renderer,
            "Unstack",
            unstack,
            theme.ink(shade_if(unstack, 0.12) * alpha),
            theme.ink(alpha),
            rect,
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

    if let Some(progress) = text.progress {
        draw_label(
            renderer,
            &progress_label(progress),
            layout.progress,
            BODY_FONT,
            BODY_SIZE,
            theme.ink(0.55 * alpha),
            rect,
        );
    }

    if let Some(when) = &text.reminder {
        let r = layout.reminder;
        let color = theme.ink(0.55 * alpha);
        let bell = Rectangle::new(r.position(), Size::new(BELL_SIZE, r.height));
        let bell_glyph = Icon::Bell.codepoint().to_string();
        draw_label(renderer, &bell_glyph, bell, ICON_FONT, 12.0, color, rect);
        let label = Rectangle {
            x: r.x + BELL_SIZE + BELL_GAP,
            width: (r.width - BELL_SIZE - BELL_GAP).max(0.0),
            ..r
        };
        draw_label(renderer, when, label, BODY_FONT, BODY_SIZE, color, rect);
    }

    let trash_alpha = if hovered(layout.trash) { 0.9 } else { 0.45 };
    draw_label(
        renderer,
        &Icon::Trash.codepoint().to_string(),
        layout.trash,
        ICON_FONT,
        14.0,
        theme.ink(trash_alpha * alpha),
        rect,
    );
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
    fn peek_header_shows_progress() {
        let text = peek_text(&note("Todo", "- [x] a\n- [ ] b\n- [ ] c"), PEEK_WIDTH);
        assert_eq!(text.progress, Some((1, 3)));
        assert_eq!(progress_label((1, 3)), "1/3");
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
        // Right of the title, left of the trash, and the title stops short of it.
        assert!(l.progress.x + l.progress.width <= l.trash.x);
        assert!(l.title.x + title_width(&text) <= l.progress.x);
        assert_eq!(peek_text(&note("T", "plain"), PEEK_WIDTH).progress, None);
    }

    #[test]
    fn peek_uses_bundled_font() {
        assert_eq!(BODY_FONT, theme::BODY_FONT);
    }

    #[test]
    fn shows_title_and_three_lines_and_keeps_the_rest_for_scrolling() {
        let t = peek_text(
            &note("Groceries", "milk\n\neggs\nbread\nbutter\njam"),
            PEEK_WIDTH,
        );
        assert_eq!(t.title.as_deref(), Some("Groceries"));
        assert_eq!(t.lines, ["milk", "eggs", "bread", "butter", "jam"]);
        let three = peek_text(&note("Groceries", "milk\neggs\nbread"), PEEK_WIDTH);
        assert_eq!(peek_height(&t), peek_height(&three));
        assert_eq!(max_body_scroll(&t), 2.0 * BODY_LINE);
        assert_eq!(max_body_scroll(&three), 0.0);
    }

    #[test]
    fn body_rect_spans_the_shown_rows() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let long = peek_text(&note("T", "a\nb\nc\nd\ne"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &long, PEEK_WIDTH, Edge::Right);
        assert_eq!(l.body_rect.position(), l.body);
        assert_eq!(l.body_rect.height, MAX_LINES as f32 * BODY_LINE);
        assert!(l.rect.contains(Point::new(
            l.body_rect.x,
            l.body_rect.y + l.body_rect.height - 1.0
        )));
        let short = peek_text(&note("T", "a"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &short, PEEK_WIDTH, Edge::Right);
        assert_eq!(l.body_rect.height, BODY_LINE);
    }

    #[test]
    fn scrollbar_shows_only_for_more_lines_and_follows_the_scroll() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let short = peek_text(&note("T", "a\nb"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &short, PEEK_WIDTH, Edge::Right);
        assert!(scrollbar(&short, &l, 0.0).is_none());

        let long = peek_text(&note("T", "a\nb\nc\nd\ne\nf"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &long, PEEK_WIDTH, Edge::Right);
        let (track, top) = scrollbar(&long, &l, 0.0).unwrap();
        // In the right padding, beside the shown rows.
        assert!(track.x >= l.body_rect.x + l.body_rect.width);
        assert!(track.x + track.width <= l.rect.x + l.rect.width);
        assert_eq!((track.y, track.height), (l.body_rect.y, l.body_rect.height));
        // Three of six lines show: the thumb is half the track, at the top.
        assert!((top.height - track.height / 2.0).abs() < 0.01);
        assert_eq!(top.y, track.y);
        let (_, bottom) = scrollbar(&long, &l, max_body_scroll(&long)).unwrap();
        assert!((bottom.y + bottom.height - (track.y + track.height)).abs() < 0.01);
    }

    #[test]
    fn scrolled_body_stays_within_its_lines() {
        let t = peek_text(&note("T", "a\nb\nc\nd\ne"), PEEK_WIDTH);
        assert_eq!(scroll_body(&t, 0.0, -30.0), 30.0);
        assert_eq!(scroll_body(&t, 30.0, -1000.0), max_body_scroll(&t));
        assert_eq!(scroll_body(&t, 30.0, 1000.0), 0.0);
        let short = peek_text(&note("T", "a"), PEEK_WIDTH);
        assert_eq!(scroll_body(&short, 0.0, -30.0), 0.0);
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
        let short = peek_height(&peek_text(&note("Short", ""), PEEK_WIDTH));
        let long = peek_height(&peek_text(&note(&"word ".repeat(30), ""), PEEK_WIDTH));
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
            peek_layout(bar, screen(), 0.0, &text, PEEK_WIDTH, Edge::Right).rect,
            bar
        );
    }

    #[test]
    fn open_peek_widens_leftward_and_fits_content() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let text = peek_text(&note("Groceries", "milk\neggs\nbread"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
        assert!((l.rect.width - PEEK_WIDTH).abs() < 0.01);
        assert!((l.rect.x + l.rect.width - 990.0).abs() < 0.01);
        assert!((l.rect.height - peek_height(&text)).abs() < 0.01);
        let center = |r: Rectangle| r.y + r.height / 2.0;
        assert!((center(l.rect) - center(bar)).abs() < 0.01);
    }

    #[test]
    fn header_sits_on_top_and_body_below() {
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 200.0));
        let text = peek_text(&note("Groceries", "milk\neggs"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
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
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
        assert!(inside(l.rect, l.trash));
        assert!(l.trash.y + l.trash.height <= l.divider_y);
        assert!(l.title.x + title_width(&text) <= l.trash.x);
    }

    #[test]
    fn peek_header_shows_reminder() {
        let mut n = note("Call @2026-10-06 15:00", "- [ ] a");
        n.reminder_set_at = Some(chrono::Utc::now());
        let text = peek_text(&n, PEEK_WIDTH);
        assert_eq!(text.reminder.as_deref(), Some("Tue 15:00"));
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
        // Title, then the bell and time, then the progress and the trash.
        assert!(l.title.x + title_width(&text) <= l.reminder.x);
        assert!(l.reminder.x + l.reminder.width <= l.progress.x);
        assert!(inside(l.rect, l.reminder));
        assert!(title_width(&text) < title_width(&peek_text(&note("Call", "- [ ] a"), PEEK_WIDTH)));
        assert_eq!(peek_text(&note("Call", ""), PEEK_WIDTH).reminder, None);
    }

    #[test]
    fn stack_peek_lists_its_notes_and_fits_them() {
        let notes = [
            note("Top", "- [x] a"),
            note("  ", "- [ ] b"),
            note("Third", "body"),
        ];
        let entry = Entry {
            top: 0,
            members: vec![1, 2],
        };
        let text = entry_peek_text(&notes, &entry, PEEK_WIDTH);
        let titles: Vec<_> = text.stack.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(titles, ["Top", "Untitled", "Third"]);
        let ids: Vec<_> = text.stack.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [notes[0].id, notes[1].id, notes[2].id]);
        // The header counts the whole stack's tasks, like its bar.
        assert_eq!(text.progress, Some((1, 2)));
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
        assert_eq!(l.stack_rows.len(), 3);
        for (i, row) in l.stack_rows.iter().enumerate() {
            assert!(inside(l.rect, *row));
            assert!(row.y > l.divider_y);
            assert_eq!(row.height, BODY_LINE);
            assert_eq!(row.y, l.body.y + i as f32 * BODY_LINE);
        }
        let unstack = l.unstack.expect("a stack's peek has an Unstack button");
        assert!(inside(l.rect, unstack));
        let last = l.stack_rows[2];
        assert!(unstack.y >= last.y + last.height);
        assert!(!unstack.intersects(&l.trash));
        // Taller than one note's peek.
        let single = peek_text(&notes[0], PEEK_WIDTH);
        assert!(peek_height(&text) > peek_height(&single));
    }

    #[test]
    fn stack_peek_shows_a_members_reminder() {
        let set = |mut n: Note| {
            n.reminder_set_at = Some(chrono::Utc::now());
            n
        };
        let notes = [
            note("Top", ""),
            set(note("Later @2026-10-08 09:00", "")),
            set(note("Call @2026-10-06 15:00", "")),
        ];
        let entry = Entry {
            top: 0,
            members: vec![1, 2],
        };
        let text = entry_peek_text(&notes, &entry, PEEK_WIDTH);
        assert_eq!(text.reminder.as_deref(), Some("Tue 15:00"));
        // A fired reminder no longer counts.
        let mut fired = notes.clone();
        fired[2].reminder_fired = reminder::pending(&fired[2]);
        let text = entry_peek_text(&fired, &entry, PEEK_WIDTH);
        assert_eq!(text.reminder.as_deref(), Some("Thu 09:00"));
    }

    #[test]
    fn long_stack_peek_stays_in_bounds() {
        let notes: Vec<Note> = (0..31).map(|i| note(&format!("N{i}"), "")).collect();
        let entry = Entry {
            top: 0,
            members: (1..31).collect(),
        };
        let text = entry_peek_text(&notes, &entry, PEEK_WIDTH);
        assert_eq!(text.stack.len(), MAX_STACK_ROWS);
        assert_eq!(text.stack[0].1, "N0");
        assert_eq!(text.stack_more, 31 - MAX_STACK_ROWS);
        let bar = Rectangle::new(Point::new(960.0, 860.0), Size::new(30.0, 40.0));
        for bounds in [
            screen(),
            Rectangle::new(Point::ORIGIN, Size::new(1000.0, 150.0)),
        ] {
            let l = peek_layout(bar, bounds, 1.0, &text, PEEK_WIDTH, Edge::Right);
            assert_eq!(l.stack_rows.len(), MAX_STACK_ROWS);
            assert!(l.stack_more.is_some());
            assert!(inside(bounds, l.rect));
            let unstack = l.unstack.unwrap();
            assert!(inside(l.rect, unstack) && inside(bounds, unstack));
        }
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
        let more = l.stack_more.unwrap();
        assert_eq!(more.y, l.stack_rows[MAX_STACK_ROWS - 1].y + BODY_LINE);
        assert!(l.unstack.unwrap().y >= more.y + more.height);
    }

    #[test]
    fn single_note_peek_has_no_stack_list() {
        let notes = [note("T", "milk")];
        let entry = Entry {
            top: 0,
            members: Vec::new(),
        };
        let text = entry_peek_text(&notes, &entry, PEEK_WIDTH);
        assert!(text.stack.is_empty());
        assert_eq!(text.lines, ["milk"]);
        let bar = Rectangle::new(Point::new(960.0, 400.0), Size::new(30.0, 40.0));
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
        assert!(l.stack_rows.is_empty() && l.unstack.is_none());
    }

    #[test]
    fn peek_stays_on_screen() {
        let bar = Rectangle::new(Point::new(960.0, 0.0), Size::new(30.0, 30.0));
        let text = peek_text(&note("T", "a\nb\nc"), PEEK_WIDTH);
        let l = peek_layout(bar, screen(), 1.0, &text, PEEK_WIDTH, Edge::Right);
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
        let l = peek_layout(bar, screen(), 1.0, &text, width, Edge::Right);
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
        assert_eq!(fit_peek_width(900.0, bar), 330.0 - PEEK_MARGIN);
        assert_eq!(fit_peek_width(280.0, bar), 280.0);
        assert_eq!(fit_peek_width(100.0, bar), 260.0);
        let edge = Rectangle::new(Point::new(0.0, 0.0), Size::new(30.0, 40.0));
        assert_eq!(fit_peek_width(900.0, edge), 260.0);
    }

    #[test]
    fn narrow_window_peek_keeps_minimum_width_and_sane_height() {
        let bar = Rectangle::new(Point::new(40.0, 400.0), Size::new(14.0, 40.0));
        let width = fit_peek_width(500.0, bar);
        let text = peek_text(&note(&"a".repeat(20), "milk"), width);
        let l = peek_layout(bar, screen(), 1.0, &text, width, Edge::Right);
        assert_eq!(l.rect.width, 260.0);
        assert!(peek_height(&text) < 200.0);
    }

    #[test]
    fn peek_sits_away_from_edge() {
        let text = peek_text(&note("T", "a\nb"), PEEK_WIDTH);
        let left = Rectangle::new(Point::new(10.0, 400.0), Size::new(30.0, 40.0));
        let l = peek_layout(left, screen(), 1.0, &text, PEEK_WIDTH, Edge::Left);
        assert_eq!(l.rect.x, left.x);
        assert!(l.rect.x + l.rect.width > left.x + left.width + 100.0);
        assert!((l.rect.center_y() - left.center_y()).abs() < 0.01);
        assert_eq!(
            peek_layout(left, screen(), 0.0, &text, PEEK_WIDTH, Edge::Left).rect,
            left
        );

        let top = Rectangle::new(Point::new(400.0, 10.0), Size::new(40.0, 30.0));
        let strip = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 64.0));
        let l = peek_layout(top, strip, 1.0, &text, PEEK_WIDTH, Edge::Top);
        assert_eq!(l.rect.y, top.y);
        // Its length grows downward, uncut by the thin strip; its width is
        // the note's.
        assert_eq!(l.rect.height, peek_height(&text));
        assert_eq!(l.rect.width, PEEK_WIDTH);
        assert!((l.rect.center_x() - top.center_x()).abs() < 0.01);
        assert_eq!(
            peek_layout(top, strip, 0.0, &text, PEEK_WIDTH, Edge::Top).rect,
            top
        );
    }

    #[test]
    fn top_peek_clamps_at_window_end() {
        let text = peek_text(&note("T", "a\nb"), PEEK_WIDTH);
        let strip = Rectangle::new(Point::ORIGIN, Size::new(1000.0, 64.0));
        let last = Rectangle::new(Point::new(960.0, 10.0), Size::new(40.0, 30.0));
        let l = peek_layout(last, strip, 1.0, &text, PEEK_WIDTH, Edge::Top);
        assert!(l.rect.x >= strip.x && l.rect.x + l.rect.width <= strip.x + strip.width);
        assert_eq!(l.rect.x + l.rect.width, 1000.0);
        assert!(l.rect.contains(l.trash.position()));
        let first = Rectangle::new(Point::new(0.0, 10.0), Size::new(40.0, 30.0));
        let l = peek_layout(first, strip, 1.0, &text, PEEK_WIDTH, Edge::Top);
        assert_eq!(l.rect.x, 0.0);
    }
}
