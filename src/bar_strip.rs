use crate::animation::MagnificationState;
use crate::app::{DragState, Message};
use crate::edge::{Edge, LocalRect};
use crate::icons::{Icon, ICON_FONT};
use crate::note::Note;
use crate::peek::{
    draw_peek, entry_peek_text, max_body_scroll, note_peek_width, peek_layout, scroll_body,
    PeekText,
};
use crate::settings::BarSettings;
use crate::strip_model::{self, Entry};
use crate::theme;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Renderer as _};
use iced::advanced::widget::{tree, Tree};
use iced::advanced::{self, Clipboard, Shell};
use iced::alignment;
use iced::event::Event;
use iced::keyboard;
use iced::mouse;
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Size, Theme, Vector};
use std::time::{Duration, Instant};

pub const STRIP_WIDTH: f32 = 64.0;
/// Horizontal space between the screen edge and the bars.
pub const EDGE_MARGIN: f32 = 10.0;
/// Extra room a hidden strip moves past the strip width, so no shadow, halo
/// or notch is left on screen.
pub const HIDE_MARGIN: f32 = 32.0;
/// How far right a fully hidden strip is drawn.
pub const HIDE_SHIFT: f32 = STRIP_WIDTH + HIDE_MARGIN;
const ADD_BUTTON_GAP: f32 = 20.0;
/// Gap between the three Actions children when the slot is expanded.
const ACTION_GAP: f32 = 4.0;
/// Slots magnify with the bars, but their length along the edge grows less
/// so they stay compact chips rather than tall notes.
const ADD_MAX_HEIGHT_SCALE: f32 = 1.4;
/// Slot thickness away from the edge, as a multiple of bar width. At rest
/// three chips nearly fit the strip; magnified rows grow past it and the
/// widget/hit area widen to match.
const SLOT_AWAY_SCALE: f32 = 1.5;
/// Slot length along the edge, as a fraction of bar height (half → squat).
const SLOT_ALONG_SCALE: f32 = 0.5;
const EDGE_PADDING: f32 = 16.0;
const CORNER_RADIUS: f32 = theme::RADIUS_BAR;
/// Corner radius for action pills (Settings / Search / Add).
const GLASS_SLOT_RADIUS: f32 = 6.0;
/// Icon ink strength inside action pills.
const SLOT_ICON_INK: f32 = 0.85;
/// The open note's bar is this much wider than a docked one.
const OPEN_BAR_SCALE: f32 = 1.5;
/// Opacity factor for bars of notes that don't match the search.
const DIM_ALPHA: f32 = 0.3;

/// The strip's edge-local frame inside its widget `bounds`. `along` is the
/// window's own coordinate along the edge (y, or x on Top), so it is the
/// same in the frame and in window coordinates; `away` is measured from the
/// side of `bounds` on the screen edge.
#[derive(Debug, Clone, Copy)]
struct Frame {
    edge: Edge,
    bounds: Rectangle,
}

impl Frame {
    /// The "window" `Edge` maps in: from the origin to the bounds' far
    /// corner, so the along axis keeps its window values.
    fn window(&self) -> Size {
        Size::new(
            self.bounds.x + self.bounds.width,
            self.bounds.y + self.bounds.height,
        )
    }

    /// Moves Left's and Top's away axis (which starts at the origin) to the
    /// bounds' screen-edge side.
    fn shift(&self) -> Vector {
        match self.edge {
            Edge::Right => Vector::ZERO,
            Edge::Left => Vector::new(self.bounds.x, 0.0),
            Edge::Top => Vector::new(0.0, self.bounds.y),
        }
    }

    /// `r` in window coordinates.
    fn rect(&self, r: LocalRect) -> Rectangle {
        self.edge.rect_to_window(r, self.window()) + self.shift()
    }

    /// The window rect `r` in the frame.
    #[cfg(test)]
    fn local(&self, r: Rectangle) -> LocalRect {
        self.edge.rect_to_local(r - self.shift(), self.window())
    }

    /// The bounds themselves: where they start along the edge, their
    /// length and their thickness away from it.
    fn span(&self) -> LocalRect {
        let b = self.bounds;
        match self.edge {
            Edge::Right | Edge::Left => LocalRect {
                along: b.y,
                away: 0.0,
                length: b.height,
                thickness: b.width,
            },
            Edge::Top => LocalRect {
                along: b.x,
                away: 0.0,
                length: b.width,
                thickness: b.height,
            },
        }
    }
}

/// Where `r` starts along `edge` and how long it is along it.
pub(crate) fn along_span(r: Rectangle, edge: Edge) -> (f32, f32) {
    match edge {
        Edge::Right | Edge::Left => (r.y, r.height),
        Edge::Top => (r.x, r.width),
    }
}

/// The middle of `r` along `edge`.
pub(crate) fn along_center(r: Rectangle, edge: Edge) -> f32 {
    match edge {
        Edge::Right | Edge::Left => r.center_y(),
        Edge::Top => r.center_x(),
    }
}

/// The translation that draws the strip `offset` toward its screen `edge`,
/// as auto-hide slides it away.
pub fn hide_translation(edge: Edge, offset: f32) -> Vector {
    match edge {
        Edge::Right => Vector::new(offset, 0.0),
        Edge::Left => Vector::new(-offset, 0.0),
        Edge::Top => Vector::new(0.0, -offset),
    }
}

/// Away-from-edge offset for a reminder jump (into the screen).
pub fn jump_translation(edge: Edge, amount: f32) -> Vector {
    hide_translation(edge, -amount)
}

/// The `done` of `total` share of `rect` at its end along `edge` (the
/// bottom on Right and Left, the right end on Top), the checklist progress.
pub fn progress_fill(rect: Rectangle, done: usize, total: usize, edge: Edge) -> Rectangle {
    let share = if total == 0 {
        0.0
    } else {
        (done as f32 / total as f32).clamp(0.0, 1.0)
    };
    match edge {
        Edge::Right | Edge::Left => {
            let height = rect.height * share;
            Rectangle {
                y: rect.y + rect.height - height,
                height,
                ..rect
            }
        }
        Edge::Top => {
            let width = rect.width * share;
            Rectangle {
                x: rect.x + rect.width - width,
                width,
                ..rect
            }
        }
    }
}

/// A `depth` deep cap across `rect` at its start along `edge` (its top on
/// Right and Left, its left end on Top), inset by the corner radius: the
/// highlight and the pinned notch.
fn start_cap(rect: Rectangle, depth: f32, edge: Edge) -> Rectangle {
    let corner = CORNER_RADIUS;
    match edge {
        Edge::Right | Edge::Left => Rectangle::new(
            Point::new(rect.x + corner, rect.y),
            Size::new((rect.width - 2.0 * corner).max(0.0), rect.height.min(depth)),
        ),
        Edge::Top => Rectangle::new(
            Point::new(rect.x, rect.y + corner),
            Size::new(rect.width.min(depth), (rect.height - 2.0 * corner).max(0.0)),
        ),
    }
}

/// Factor on a bar's alpha: a checklist bar is faint under its progress
/// fill (including when every task is done and the fill covers the bar).
fn progress_alpha(progress: Option<(usize, usize)>) -> f32 {
    match progress {
        Some((_, total)) if total > 0 => 0.45,
        _ => 1.0,
    }
}

/// Height of the pinned bar's notch.
const NOTCH_HEIGHT: f32 = 3.0;

/// The pinned notch: the bar's colour darkened, at the bar's alpha.
fn notch_color(bar: Color, alpha: f32) -> Color {
    Color {
        a: alpha,
        ..theme::mix(bar, Color::BLACK, 0.25)
    }
}

/// How far a fired reminder's glow reaches past its bar at full pulse.
const PULSE_REACH: f32 = 3.0;

/// The glow behind a bar whose reminder fired, `pulse` (0..=1) of the way
/// out.
fn pulse_halo(rect: Rectangle, pulse: f32) -> Rectangle {
    rect.expand(PULSE_REACH * pulse.clamp(0.0, 1.0))
}

/// Thickness of each "card edge" line beside a stack's bar.
const STACK_EDGE_WIDTH: f32 = 2.0;
/// Step away from the screen edge from one stack edge to the next.
const STACK_EDGE_STEP: f32 = 4.0;

/// The two "card edge" lines on a stack's bar's away side (left of a Right
/// bar, right of a Left one, below a Top one), each further out and shorter
/// than the last.
fn stack_edges(rect: Rectangle, edge: Edge) -> [Rectangle; 2] {
    [1, 2].map(|n| {
        let inset = 3.0 * n as f32;
        let out = STACK_EDGE_STEP * n as f32;
        match edge {
            Edge::Right => Rectangle::new(
                Point::new(rect.x - out, rect.y + inset),
                Size::new(STACK_EDGE_WIDTH, (rect.height - 2.0 * inset).max(0.0)),
            ),
            Edge::Left => Rectangle::new(
                Point::new(rect.x + rect.width + out - STACK_EDGE_WIDTH, rect.y + inset),
                Size::new(STACK_EDGE_WIDTH, (rect.height - 2.0 * inset).max(0.0)),
            ),
            Edge::Top => Rectangle::new(
                Point::new(
                    rect.x + inset,
                    rect.y + rect.height + out - STACK_EDGE_WIDTH,
                ),
                Size::new((rect.width - 2.0 * inset).max(0.0), STACK_EDGE_WIDTH),
            ),
        }
    })
}

/// Where a dragged bar would land when dropped at a point along the edge
/// over `bar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Before,
    /// The middle half: stacks the dragged note under this bar's.
    Onto,
    After,
}

/// The zone of `bar` a drag released at `along` (the cursor's coordinate
/// along `edge`) falls in; before or after the bar counts as its end.
pub fn drop_zone(bar: Rectangle, along: f32, edge: Edge) -> DropZone {
    let (start, length) = along_span(bar, edge);
    let quarter = length / 4.0;
    if along < start + quarter {
        DropZone::Before
    } else if along > start + length - quarter {
        DropZone::After
    } else {
        DropZone::Onto
    }
}

/// The bar a drag of bar `dragged` released at `along` (the cursor's
/// coordinate along `edge`) stacks onto, if any.
pub fn stack_target(bars: &[Rectangle], dragged: usize, along: f32, edge: Edge) -> Option<usize> {
    bars.iter()
        .enumerate()
        .position(|(i, bar)| i != dragged && drop_zone(*bar, along, edge) == DropZone::Onto)
}

/// The bar's rectangle: the open note's bar grows away from the screen
/// `edge`, keeping its side on it, as the note unfolds (`progress` 0 is
/// docked, 1 fully open).
pub fn bar_rect_for(progress: f32, rect: Rectangle, edge: Edge) -> Rectangle {
    let grow = 1.0 + (OPEN_BAR_SCALE - 1.0) * progress;
    match edge {
        Edge::Right => {
            let width = rect.width * grow;
            Rectangle {
                x: rect.x + rect.width - width,
                width,
                ..rect
            }
        }
        Edge::Left => Rectangle {
            width: rect.width * grow,
            ..rect
        },
        Edge::Top => Rectangle {
            height: rect.height * grow,
            ..rect
        },
    }
}

/// The strip's bars and slots, in window coordinates, laid out along `edge`.
pub struct StripLayout {
    pub bars: Vec<Rectangle>,
    /// Collapsed Actions control, or the bounding box of the expanded row.
    pub actions_button: Rectangle,
    pub actions_hit_area: Rectangle,
    /// Whether the three action children are shown side by side (the
    /// target; see `actions_progress` for how far they have unfolded).
    pub actions_expanded: bool,
    /// How far New / Search / Settings have slid out of the Actions slot,
    /// eased: 0 collapsed, 1 fully expanded. Only then do they take clicks.
    pub actions_progress: f32,
    /// The collapsed Actions slot, where the pills slide out from.
    pub actions_slot: Rectangle,
    /// Where New / Search / Settings sit once fully expanded.
    pub pill_slots: [Rectangle; 3],
    pub add_button: Rectangle,
    pub add_hit_area: Rectangle,
    pub search_button: Rectangle,
    pub search_hit_area: Rectangle,
    pub settings_button: Rectangle,
    pub settings_hit_area: Rectangle,
    pub max_scroll: f32,
    pub edge: Edge,
}

impl StripLayout {
    /// Whether the expanded row has fully unfolded: only then do its pills
    /// take clicks and show tooltips.
    pub fn actions_settled(&self) -> bool {
        self.actions_expanded && self.actions_progress >= 1.0
    }

    /// Where New / Search / Settings are drawn at `actions_progress`: from
    /// the collapsed slot's center out to their slots, growing from half
    /// size.
    pub fn pill_rects(&self) -> [Rectangle; 3] {
        let t = self.actions_progress.clamp(0.0, 1.0);
        if t >= 1.0 {
            return self.pill_slots;
        }
        let from = self.actions_slot.center();
        let scale = crate::animation::lerp(0.5, 1.0, t);
        self.pill_slots.map(|slot| {
            let to = slot.center();
            let center = Point::new(
                crate::animation::lerp(from.x, to.x, t),
                crate::animation::lerp(from.y, to.y, t),
            );
            let size = Size::new(slot.width * scale, slot.height * scale);
            Rectangle::new(
                Point::new(center.x - size.width / 2.0, center.y - size.height / 2.0),
                size,
            )
        })
    }

    /// Where the settings panel unfolds from.
    pub fn settings_anchor(&self) -> Rectangle {
        if self.actions_expanded {
            self.settings_button
        } else {
            self.actions_button
        }
    }

    /// Where the search panel unfolds from.
    pub fn search_anchor(&self) -> Rectangle {
        if self.actions_expanded {
            self.search_button
        } else {
            self.actions_button
        }
    }

    /// Where the last hit area ends along the edge: its bottom on Right
    /// and Left, its right end on Top.
    pub fn hit_bottom(&self) -> f32 {
        let (start, length) = along_span(self.actions_hit_area, self.edge);
        start + length
    }

    /// Whether `pos` is over the Actions control (collapsed slot or expanded row).
    pub fn over_actions(&self, pos: Point) -> bool {
        self.actions_hit_area.contains(pos)
    }

    /// What a press at `pos` on one of the slots below the bars does.
    /// The collapsed Actions slot only expands on hover; presses do nothing
    /// until New / Search / Settings are showing.
    pub fn slot_message(&self, pos: Point) -> Option<Message> {
        if !self.actions_settled() {
            return None;
        }
        if self.add_hit_area.contains(pos) {
            Some(Message::AddNote)
        } else if self.search_hit_area.contains(pos) {
            Some(Message::ToggleSearch)
        } else if self.settings_hit_area.contains(pos) {
            Some(Message::ToggleSettings)
        } else {
            None
        }
    }

    /// The slot with a tooltip under `pos`, if any.
    pub fn slot_at(&self, pos: Point) -> Option<Slot> {
        if !self.actions_expanded {
            return self.actions_hit_area.contains(pos).then_some(Slot::Actions);
        }
        if !self.actions_settled() {
            return None;
        }
        if self.add_hit_area.contains(pos) {
            Some(Slot::Add)
        } else if self.search_hit_area.contains(pos) {
            Some(Slot::Search)
        } else if self.settings_hit_area.contains(pos) {
            Some(Slot::Settings)
        } else {
            None
        }
    }

    /// What a press at `pos` on a slot does with `modifiers` held: Alt
    /// (Option) on the add slot makes a note from the clipboard.
    pub fn press_message(&self, pos: Point, modifiers: keyboard::Modifiers) -> Option<Message> {
        match self.slot_message(pos) {
            Some(Message::AddNote) if modifiers.alt() => Some(Message::ClipboardNote),
            message => message,
        }
    }
}

/// A slot after the bars that explains itself after a hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Actions,
    Add,
    Search,
    Settings,
}

/// How long the cursor rests on a slot before its tooltip shows.
const TOOLTIP_DELAY: Duration = Duration::from_millis(600);
const ACTIONS_TOOLTIP: &str = "Actions";
const ADD_TOOLTIP: &str = if cfg!(target_os = "macos") {
    "New note · Option-click: from clipboard"
} else {
    "New note · Alt-click: from clipboard"
};
const SEARCH_TOOLTIP: &str = if cfg!(target_os = "macos") {
    "Search (Cmd+F)"
} else {
    "Search (Ctrl+F)"
};
const SETTINGS_TOOLTIP: &str = if cfg!(target_os = "macos") {
    "Settings (Cmd+,)"
} else {
    "Settings (Ctrl+,)"
};
const CLIPBOARD_EMPTY: &str = "Clipboard is empty";
/// The undo toast after a delete reads "Note deleted · Undo", the link
/// part in full ink.
const TOAST_TEXT: &str = "Note deleted · ";
const TOAST_ACCENT: &str = "Undo";

/// The tooltip of the slot hovered since `hover`'s instant, once it has
/// rested there for `TOOLTIP_DELAY`.
pub fn slot_tooltip(hover: Option<(Slot, Instant)>, now: Instant) -> Option<&'static str> {
    let (slot, since) = hover?;
    (now.saturating_duration_since(since) >= TOOLTIP_DELAY).then_some(match slot {
        Slot::Actions => ACTIONS_TOOLTIP,
        Slot::Add => ADD_TOOLTIP,
        Slot::Search => SEARCH_TOOLTIP,
        Slot::Settings => SETTINGS_TOOLTIP,
    })
}

/// What a hint chip sits beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HintAt {
    Bar(usize),
    Actions,
    Add,
    Search,
    Settings,
}

/// A hint chip: its label (with an accented tail) and what it explains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hint {
    text: &'static str,
    accent: Option<&'static str>,
    at: HintAt,
}

/// The undo toast: a chip beside the Actions / `+` slot.
const TOAST: Hint = Hint {
    text: TOAST_TEXT,
    accent: Some(TOAST_ACCENT),
    at: HintAt::Actions,
};

/// Whether a pressed bar has moved far enough to count as dragged. A
/// drag's `origin_y` and `current_y` are coordinates along the edge.
fn drag_moved(drag: &DragState) -> bool {
    (drag.current_y - drag.origin_y).abs() > 5.0
}

/// Which kind of hint chip the strip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    None,
    /// "Stack" beside the bar a drop would stack onto, if any.
    Stack,
    Clipboard,
    Toast,
    /// The hovered slot's tooltip, once it is due.
    Tooltip,
}

/// Which hint chip shows, by precedence: none with the peek; while a bar
/// is dragged (once it has moved) only the stack cue; otherwise the empty
/// clipboard, then the undo toast, then the hovered slot's tooltip, which
/// an open panel hides. The app asks too, so the toast takes clicks
/// exactly while it shows.
pub fn hint_kind(
    peek: Option<(usize, f32)>,
    drag: Option<&DragState>,
    clipboard_hint: bool,
    toast: bool,
    panel_open: bool,
) -> HintKind {
    if peek.is_some_and(|(_, progress)| progress > 0.0) {
        HintKind::None
    } else if drag.is_some_and(drag_moved) {
        HintKind::Stack
    } else if clipboard_hint {
        HintKind::Clipboard
    } else if toast {
        HintKind::Toast
    } else if panel_open {
        HintKind::None
    } else {
        HintKind::Tooltip
    }
}

/// The hint while a bar is dragged: "Stack" beside the bar a drop would
/// stack onto.
fn drag_hint(bars: &[Rectangle], drag: Option<&DragState>, edge: Edge) -> Option<Hint> {
    let drag = drag.filter(|d| drag_moved(d))?;
    stack_target(bars, drag.bar_index, drag.current_y, edge).map(|onto| Hint {
        text: "Stack",
        accent: None,
        at: HintAt::Bar(onto),
    })
}

/// The widget's own state: the modifiers held, for Alt-clicks, and the
/// timing of the slot tooltips and the hint chip.
#[derive(Default)]
struct StripState {
    modifiers: keyboard::Modifiers,
    /// The slot under the cursor and since when.
    slot_hover: Option<(Slot, Instant)>,
    /// The slot pressed last: its tooltip waits until the cursor leaves it
    /// and comes back.
    pressed_slot: Option<Slot>,
    /// The chip on screen and since when, for its fade.
    chip: Option<(Hint, Instant)>,
    /// When the latest frame was requested: `draw` shows the chip as of
    /// then, the same instant `update` noted it at.
    frame: Option<Instant>,
}

impl StripState {
    /// Tracks the held modifiers. Leaving the strip or the window losing
    /// focus resets them: their release may never reach the strip.
    fn observe(&mut self, event: &Event) {
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                self.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::CursorLeft)
            | Event::Window(iced::window::Event::Unfocused) => {
                self.modifiers = keyboard::Modifiers::default();
            }
            _ => {}
        }
    }

    /// The cursor is over `slot` (or none) at `now`: entering a slot starts
    /// its tooltip's delay, moving within it keeps it running.
    fn hover_slot(&mut self, slot: Option<Slot>, now: Instant) {
        if slot != self.pressed_slot {
            self.pressed_slot = None;
        }
        if slot.is_none() || self.pressed_slot.is_some() {
            self.slot_hover = None;
        } else if self.slot_hover.map(|(s, _)| s) != slot {
            self.slot_hover = slot.map(|s| (s, now));
        }
    }

    /// Any press hides the tooltip; the pressed `slot` (if the cursor is on
    /// one) needs a fresh hover to show it again.
    fn press(&mut self, slot: Option<Slot>) {
        self.slot_hover = None;
        self.pressed_slot = slot;
    }

    /// When the hovered slot's tooltip is due, if it is still to come.
    fn tooltip_due(&self, now: Instant) -> Option<Instant> {
        self.slot_hover
            .map(|(_, since)| since + TOOLTIP_DELAY)
            .filter(|due| *due > now)
    }

    /// Whether the chip is still fading in at `now`.
    fn chip_fading(&self, now: Instant) -> bool {
        self.chip.is_some_and(|(_, since)| now < since + CHIP_FADE)
    }

    /// Notes the chip shown at `now`; a new one starts fading in.
    fn show_chip(&mut self, hint: Option<Hint>, now: Instant) {
        if self.chip.map(|(h, _)| h) != hint {
            self.chip = hint.map(|h| (h, now));
        }
    }
}

/// How long a hint chip takes to fade in.
const CHIP_FADE: Duration = Duration::from_millis(120);
const CHIP_TEXT: f32 = theme::TEXT_XS;
const CHIP_PADDING: Size = Size::new(8.0, 4.0);
/// Space between a chip and what it sits beside.
const CHIP_GAP: f32 = theme::space(2);

/// A chip's opacity, fading in since `since`.
fn chip_alpha(since: Instant, now: Instant) -> f32 {
    (now.saturating_duration_since(since).as_secs_f32() / CHIP_FADE.as_secs_f32()).min(1.0)
}

type ChipParagraph = <iced::Renderer as text::Renderer>::Paragraph;

/// A chip's label: `text`, then `accent` (if any) in `accent_color`.
fn chip_paragraph(text: &str, accent: Option<&str>, accent_color: Color) -> ChipParagraph {
    let spans: Vec<text::Span<'_, (), iced::Font>> = std::iter::once(text::Span::new(text))
        .chain(accent.map(|a| text::Span::new(a).color(accent_color)))
        .collect();
    ChipParagraph::with_spans(text::Text {
        content: &spans[..],
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(CHIP_TEXT),
        line_height: text::LineHeight::default(),
        font: theme::BODY_FONT,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    })
}

/// A chip around a label of size `label`: on the away side of `anchor`
/// from the screen `edge`, where the peek grows, and centred along the edge
/// on `along`, kept inside `bounds` along the edge like the peek.
fn chip_frame(
    bounds: Rectangle,
    anchor: Rectangle,
    along: f32,
    label: Size,
    edge: Edge,
) -> Rectangle {
    let size = Size::new(
        label.width + 2.0 * CHIP_PADDING.width,
        label.height + 2.0 * CHIP_PADDING.height,
    );
    // The anchor's away extent, centred on `along`.
    let anchor = match edge {
        Edge::Right | Edge::Left => {
            Rectangle::new(Point::new(anchor.x, along), Size::new(anchor.width, 0.0))
        }
        Edge::Top => Rectangle::new(Point::new(along, anchor.y), Size::new(0.0, anchor.height)),
    };
    edge.place_away(anchor, size, CHIP_GAP, edge.along_only(bounds))
}

/// Where the chip labelled `text` (and `accent`) beside `anchor`, centred
/// on `along` (a coordinate along `edge`), is drawn in `bounds`, for
/// hit-testing without a renderer.
pub fn chip_rect(
    bounds: Rectangle,
    anchor: Rectangle,
    along: f32,
    text: &str,
    accent: Option<&str>,
    edge: Edge,
) -> Rectangle {
    let label = chip_paragraph(text, accent, Color::TRANSPARENT).min_bounds();
    chip_frame(bounds, anchor, along, label, edge)
}

/// Draws a hint chip beside `anchor` (a bar or slot) in `bounds`, on its
/// away side from the screen `edge` and centred on it along the edge: a
/// small card like the peek's paper with `text` at 0.75 ink and `accent`
/// after it at full ink, all at `alpha`. Returns its rect.
#[allow(clippy::too_many_arguments)]
pub fn draw_chip(
    renderer: &mut iced::Renderer,
    theme: &theme::Theme,
    bounds: Rectangle,
    anchor: Rectangle,
    text: &str,
    accent: Option<&str>,
    alpha: f32,
    edge: Edge,
) -> Rectangle {
    let paragraph = chip_paragraph(text, accent, theme.ink(alpha));
    let rect = chip_frame(
        bounds,
        anchor,
        along_center(anchor, edge),
        paragraph.min_bounds(),
        edge,
    );
    if alpha <= 0.0 {
        return rect;
    }
    let [contact, ambient] = theme.shadows(alpha);
    for shadow in [ambient, contact] {
        renderer::Renderer::fill_quad(
            renderer,
            renderer::Quad {
                bounds: rect,
                border: iced::Border {
                    radius: theme::RADIUS_CONTROL.into(),
                    width: 1.0,
                    color: theme.ink(0.15 * alpha),
                },
                shadow,
                snap: true,
            },
            theme.card().scale_alpha(alpha),
        );
    }
    // The renderer only holds a weak reference to a paragraph, and this one
    // is dropped when `draw` returns: fill_text keeps its own copy.
    let origin = Point::new(rect.x + CHIP_PADDING.width, rect.y + CHIP_PADDING.height);
    let main_width = if accent.is_some() {
        chip_paragraph(text, None, Color::TRANSPARENT)
            .min_bounds()
            .width
    } else {
        paragraph.min_bounds().width
    };
    let label_height = paragraph.min_bounds().height;
    draw_chip_text(
        renderer,
        text,
        origin,
        label_height,
        theme.ink(0.75 * alpha),
        rect,
    );
    if let Some(accent) = accent {
        let at = Point::new(origin.x + main_width, origin.y);
        draw_chip_text(renderer, accent, at, label_height, theme.ink(alpha), rect);
    }
    rect
}

/// One run of chip text with its top-left at `at`, clipped to `clip`.
fn draw_chip_text(
    renderer: &mut iced::Renderer,
    content: &str,
    at: Point,
    height: f32,
    color: Color,
    clip: Rectangle,
) {
    renderer.fill_text(
        text::Text {
            content: content.to_string(),
            bounds: Size::new((clip.x + clip.width - at.x).max(0.0), height),
            size: Pixels(CHIP_TEXT),
            line_height: text::LineHeight::default(),
            font: theme::BODY_FONT,
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        },
        at,
        color,
        clip,
    );
}

/// Where the undo toast shows on `strip` in `bounds`: the whole chip is
/// its hit area. It sits on the away side of its slot, from `strip.edge`.
pub fn toast_rect(strip: &StripLayout, bounds: Rectangle) -> Rectangle {
    let anchor = if strip.actions_expanded {
        strip.add_button
    } else {
        strip.actions_button
    };
    chip_rect(
        bounds,
        anchor,
        along_center(anchor, strip.edge),
        TOAST.text,
        TOAST.accent,
        strip.edge,
    )
}

/// The `fraction` of `bounds` the bars are laid out in: centred along the
/// `edge`, spanning the full extent away from it.
pub fn band(bounds: Rectangle, fraction: f32, edge: Edge) -> Rectangle {
    match edge {
        Edge::Right | Edge::Left => {
            let height = bounds.height * fraction;
            Rectangle::new(
                Point::new(bounds.x, bounds.y + (bounds.height - height) / 2.0),
                Size::new(bounds.width, height),
            )
        }
        Edge::Top => {
            let width = bounds.width * fraction;
            Rectangle::new(
                Point::new(bounds.x + (bounds.width - width) / 2.0, bounds.y),
                Size::new(width, bounds.height),
            )
        }
    }
}

/// Size of a slot (add, search or settings button) at magnification `scale`,
/// as Right lays it out (width away from the edge, height along it): a squat
/// chip, wider than a note bar and half as tall along the edge.
fn slot_size(bars: &BarSettings, scale: f32) -> Size {
    Size::new(
        bars.width * SLOT_AWAY_SCALE * scale,
        bars.height * SLOT_ALONG_SCALE * scale.min(ADD_MAX_HEIGHT_SCALE),
    )
}

/// Extra room past the expanded pills so the cursor can aim for Settings
/// without leaving the strip widget / interactive column.
pub const ACTIONS_HOVER_PAD: f32 = 8.0;

/// How far from the screen edge the expanded Actions row reaches (margin +
/// three chips + gaps + hover pad), at least [`STRIP_WIDTH`].
pub fn actions_row_extent(
    bars: &BarSettings,
    bar_count: usize,
    scale: impl Fn(usize) -> f32,
) -> f32 {
    let add = slot_size(bars, scale(bar_count));
    let search = slot_size(bars, scale(bar_count + 1));
    let gear = slot_size(bars, scale(bar_count + 2));
    (EDGE_MARGIN
        + add.width
        + ACTION_GAP
        + search.width
        + ACTION_GAP
        + gear.width
        + ACTIONS_HOVER_PAD)
        .max(STRIP_WIDTH)
}

/// Lays out the bars plus the Actions slot (collapsed or expanded into New /
/// Search / Settings side by side away from the edge), centred along the
/// `edge` in `bounds`. Centering on the *current* (magnified) length keeps
/// the hovered bar roughly in place while its neighbours grow. If the stack
/// is longer than the bounds, it starts at their start and is scrolled by
/// `scroll_offset`.
///
/// The geometry is worked out in the edge-local frame (see `Frame`) and the
/// rects come out in window coordinates.
///
/// When collapsed, `scale(count)` magnifies Actions. When expanded,
/// `scale(count)` / `count+1` / `count+2` magnify New / Search / Settings.
///
/// `collapse` is a deleted bar's index and how far it has collapsed (0..=1):
/// its length and one gap next to it shrink by that share.
#[allow(clippy::too_many_arguments)]
pub fn compute_layout(
    count: usize,
    scale: impl Fn(usize) -> f32,
    bounds: Rectangle,
    scroll_offset: f32,
    bars_settings: &BarSettings,
    actions_expanded: bool,
    collapse: Option<(usize, f32)>,
    edge: Edge,
) -> StripLayout {
    let frame = Frame { edge, bounds };
    let span = frame.span();
    let gap = bars_settings.gap;
    let shrink = |i: usize| match collapse {
        Some((c, t)) if c == i => 1.0 - t.clamp(0.0, 1.0),
        _ => 1.0,
    };
    let lengths: Vec<f32> = (0..count)
        .map(|i| bars_settings.height * scale(i) * shrink(i))
        .collect();
    // Gap `i` follows bar `i`; the last bar collapses into the gap before it.
    let gaps: Vec<f32> = (0..count.saturating_sub(1))
        .map(|i| {
            let owner = match collapse {
                Some((c, _)) if c + 1 == count => i + 1,
                _ => i,
            };
            gap * shrink(owner)
        })
        .collect();
    let bars_length: f32 = lengths.iter().sum::<f32>() + gaps.iter().sum::<f32>();
    let add_gap = if count > 0 { ADD_BUTTON_GAP } else { 0.0 };
    let actions_size = slot_size(bars_settings, scale(count));
    let add_size = slot_size(bars_settings, scale(count));
    let search_size = slot_size(bars_settings, scale(count + 1));
    let gear_size = slot_size(bars_settings, scale(count + 2));
    let expanded_length = add_size
        .height
        .max(search_size.height)
        .max(gear_size.height);
    let row_length = if actions_expanded {
        expanded_length
    } else {
        actions_size.height
    };
    let content_length = bars_length + add_gap + row_length;

    let available = span.length - 2.0 * EDGE_PADDING;
    let (mut along, max_scroll) = if content_length <= available {
        (span.along + (span.length - content_length) / 2.0, 0.0)
    } else {
        let max_scroll = content_length - available;
        (
            span.along + EDGE_PADDING - scroll_offset.clamp(0.0, max_scroll),
            max_scroll,
        )
    };

    let mut bars = Vec::with_capacity(count);
    for (i, length) in lengths.iter().enumerate() {
        bars.push(frame.rect(LocalRect {
            along,
            away: EDGE_MARGIN,
            length: *length,
            thickness: bars_settings.width * scale(i),
        }));
        along += length + gaps.get(i).copied().unwrap_or(add_gap);
    }

    let row_start = along;
    // A slot of `size` (Right's width and height) `away` from the edge,
    // centred in the row.
    let slot = |size: Size, away: f32| LocalRect {
        along: row_start + (row_length - size.height) / 2.0,
        away,
        length: size.height,
        thickness: size.width,
    };
    let actions_button = slot(actions_size, EDGE_MARGIN);
    // Where the expanded pills sit, collapsed or not, centred in the
    // expanded row.
    let pill = |size: Size, away: f32| LocalRect {
        along: row_start + (expanded_length - size.height) / 2.0,
        away,
        length: size.height,
        thickness: size.width,
    };
    let pill_add = pill(add_size, EDGE_MARGIN);
    let pill_search = pill(search_size, pill_add.away + pill_add.thickness + ACTION_GAP);
    let pill_gear = pill(
        gear_size,
        pill_search.away + pill_search.thickness + ACTION_GAP,
    );
    // Expanded: New, Search, Settings going away from the edge.
    let add_button = slot(add_size, EDGE_MARGIN);
    let search_button = slot(
        search_size,
        add_button.away + add_button.thickness + ACTION_GAP,
    );
    let settings_button = slot(
        gear_size,
        search_button.away + search_button.thickness + ACTION_GAP,
    );
    let row_far = if actions_expanded {
        settings_button.away + settings_button.thickness
    } else {
        actions_button.away + actions_button.thickness
    };
    // Expanded chips can reach past the strip band once they magnify; the
    // hit area must cover the whole row (plus pad) or hover collapses.
    let hit_thickness = if actions_expanded {
        (row_far + ACTIONS_HOVER_PAD).max(span.thickness)
    } else {
        span.thickness
    };
    let hit = LocalRect {
        along: row_start - add_gap / 2.0,
        away: 0.0,
        length: row_length + add_gap / 2.0 + EDGE_PADDING,
        thickness: hit_thickness,
    };
    let actions_hit_area = frame.rect(hit);
    // The share of the hit area from `near` to `far` away from the edge.
    let slot_hit = |button: LocalRect, near: f32, far: f32| {
        frame
            .rect(LocalRect {
                away: near,
                thickness: far - near,
                ..hit
            })
            .intersection(&actions_hit_area)
            .unwrap_or(frame.rect(button))
    };
    let (add_hit_area, search_hit_area, settings_hit_area) = if actions_expanded {
        let mid_add_search = (add_button.away + add_button.thickness + search_button.away) / 2.0;
        let mid_search_gear =
            (search_button.away + search_button.thickness + settings_button.away) / 2.0;
        (
            slot_hit(add_button, 0.0, mid_add_search),
            slot_hit(search_button, mid_add_search, mid_search_gear),
            slot_hit(
                settings_button,
                mid_search_gear,
                row_far.max(span.thickness),
            ),
        )
    } else {
        // Collapsed: children share the Actions geometry for panel anchors.
        (actions_hit_area, actions_hit_area, actions_hit_area)
    };
    let actions_button_slot = actions_button;
    let (add_button, search_button, settings_button) = if actions_expanded {
        (add_button, search_button, settings_button)
    } else {
        (actions_button, actions_button, actions_button)
    };

    StripLayout {
        bars,
        actions_button: frame.rect(if actions_expanded {
            LocalRect {
                along: row_start,
                away: EDGE_MARGIN,
                length: row_length,
                thickness: row_far - EDGE_MARGIN,
            }
        } else {
            actions_button
        }),
        actions_hit_area,
        actions_expanded,
        actions_progress: if actions_expanded { 1.0 } else { 0.0 },
        actions_slot: frame.rect(actions_button_slot),
        pill_slots: [pill_add, pill_search, pill_gear].map(|r| frame.rect(r)),
        add_button: frame.rect(add_button),
        add_hit_area,
        search_button: frame.rect(search_button),
        search_hit_area,
        settings_button: frame.rect(settings_button),
        settings_hit_area,
        max_scroll,
        edge,
    }
}

pub struct BarStrip<'a> {
    pub notes: &'a [Note],
    /// One bar per entry; bar indices below are entry indices.
    pub entries: &'a [Entry],
    /// Task progress per entry (`done, total`), summed over the stack.
    pub progress: &'a [Option<(usize, usize)>],
    pub magnification: &'a MagnificationState,
    pub drag: &'a Option<DragState>,
    pub scroll_offset: f32,
    /// Bar being peeked and the peek's progress (0..=1).
    pub peek: Option<(usize, f32)>,
    /// How far the peek's body is scrolled, in pixels.
    pub peek_scroll: f32,
    pub bars: &'a BarSettings,
    /// Share of the widget height the bars may use (centered).
    pub height_fraction: f32,
    /// Paper tint of an open note, which the hover peek imitates.
    pub paper_tint: f32,
    /// Width of a note without a size of its own; the peek is as wide as the
    /// note it opens.
    pub default_note_width: f32,
    pub theme: theme::Theme,
    /// The bar holding the open (or opening) note and the note's morph
    /// progress.
    pub open: Option<(usize, f32)>,
    /// Bar of a deleted note and how far it has collapsed.
    pub collapse: Option<(usize, f32)>,
    /// Bars to draw faded, by index: entries none of whose notes match the
    /// search.
    /// Empty when nothing is dimmed.
    pub dimmed: Vec<bool>,
    /// How strongly each bar's fired reminder pulses (0..=1), by index.
    /// Empty when nothing pulses.
    pub pulse: Vec<f32>,
    /// How far each bar jumps away from the screen edge (px), by index.
    /// Empty when nothing jumps.
    pub jump: Vec<f32>,
    /// Bars with an uncleared fired reminder. While the strip is slid away
    /// these stay drawn and clickable at the edge; others stay hidden.
    pub alert: Vec<bool>,
    /// How far the `+` slot is shaken sideways, after an empty clipboard.
    pub add_shake: f32,
    /// Shows "Clipboard is empty" beside the `+` slot.
    pub clipboard_hint: bool,
    /// Shows the undo toast after a delete beside the `+` slot.
    pub toast: bool,
    /// A panel (search, settings or export) is open: the slots show no
    /// tooltips.
    pub panel_open: bool,
    /// The Actions slot is expanded into New / Search / Settings.
    pub actions_expanded: bool,
    /// How far the row has unfolded, eased (see `StripLayout::actions_progress`).
    pub actions_progress: f32,
    /// How far toward its screen edge everything is drawn while auto-hide
    /// slides the strip away (0 = in place, `HIDE_SHIFT` = off screen), an
    /// away offset that `hide_translation` turns into a direction. Above 0
    /// the strip takes no clicks or hovers.
    pub x_offset: f32,
    /// The screen edge the strip docks to.
    pub edge: Edge,
}

impl<'a> BarStrip<'a> {
    /// Whether the strip is (partly) slid away and so ignores the mouse.
    fn slid(&self) -> bool {
        self.x_offset > 0.0
    }

    /// Widget thickness away from the edge: the strip band, or the expanded
    /// Actions row when it sticks further into the screen (so hover/click
    /// still land on Search and Settings).
    fn widget_thickness(&self) -> f32 {
        if self.actions_expanded {
            actions_row_extent(self.bars, self.entries.len(), |i| {
                self.magnification.scale(i)
            })
        } else {
            STRIP_WIDTH
        }
    }

    fn is_alert(&self, i: usize) -> bool {
        self.alert.get(i).copied().unwrap_or(false)
    }

    fn any_alert(&self) -> bool {
        self.alert.iter().any(|&a| a)
    }

    /// Hide offset for bar `i`: alerting bars stay on-screen while the strip
    /// is otherwise slid away.
    fn bar_slide(&self, i: usize) -> f32 {
        if self.is_alert(i) {
            0.0
        } else {
            self.x_offset
        }
    }

    /// The note bar `i` shows: its entry's top.
    fn note(&self, i: usize) -> Option<&'a Note> {
        self.notes.get(self.entries.get(i)?.top)
    }

    fn layout_in(&self, bounds: Rectangle) -> StripLayout {
        let mut layout = compute_layout(
            self.entries.len(),
            |i| self.magnification.scale(i),
            band(bounds, self.height_fraction, self.edge),
            self.scroll_offset,
            self.bars,
            self.actions_expanded,
            self.collapse,
            self.edge,
        );
        layout.actions_progress = self.actions_progress;
        layout
    }

    /// The cursor at `pos`'s coordinate along the edge.
    fn along(&self, bounds: Rectangle, pos: Point) -> f32 {
        self.edge.to_local(pos, bounds.size()).along
    }

    /// The note bar `i` (at `bar`) peeks, and the peek's width and text.
    fn peek_parts(&self, i: usize, bar: Rectangle) -> Option<(&'a Note, f32, PeekText)> {
        let note = self.note(i)?;
        let width = note_peek_width(note, self.default_note_width, bar, self.edge);
        let text = entry_peek_text(self.notes, self.entries.get(i)?, width);
        Some((note, width, text))
    }

    /// Bar whose open peek is under `pos`, if any.
    fn peek_hit(&self, bounds: Rectangle, pos: Point) -> Option<usize> {
        let (i, _) = self.peek?;
        let bar = *self.layout_in(bounds).bars.get(i)?;
        let (_, width, text) = self.peek_parts(i, bar)?;
        peek_target(bar, bounds, &text, width, self.edge)
            .contains(pos)
            .then_some(i)
    }

    /// The peek body's new scroll for a wheel `dy` at `pos`: only over an
    /// open peek with more lines than it shows.
    fn peek_wheel(&self, bounds: Rectangle, pos: Point, dy: f32) -> Option<f32> {
        let i = self.peek_hit(bounds, pos)?;
        let bar = *self.layout_in(bounds).bars.get(i)?;
        let (_, _, text) = self.peek_parts(i, bar)?;
        (max_body_scroll(&text) > 0.0).then(|| scroll_body(&text, self.peek_scroll, dy))
    }

    /// The hover update for a cursor at `pos`: magnify around it inside the
    /// strip, end the hover outside. None over the open peek, so the bars
    /// (and the peek centered on its bar) hold still while it's in use.
    /// While slid away, only alerting bars keep hover (for peek / settle).
    fn hover_message(&self, bounds: Rectangle, pos: Point) -> Option<Message> {
        if self.peek_hit(bounds, pos).is_some() {
            return None;
        }
        if self.slid() {
            let strip = self.layout_in(bounds);
            let over_alert = strip
                .bars
                .iter()
                .enumerate()
                .any(|(i, bar)| self.is_alert(i) && bar.contains(pos));
            return if over_alert {
                Some(Message::StripHover(Some(self.along(bounds, pos))))
            } else {
                Some(Message::StripHover(None))
            };
        }
        if bounds.contains(pos) {
            Some(Message::StripHover(Some(self.along(bounds, pos))))
        } else {
            Some(Message::StripHover(None))
        }
    }

    /// What a left press at `pos` does: `None` when the strip doesn't take
    /// it, `Some(None)` when it takes it without a message. While slid away,
    /// only alerting bars (and their peek) take presses.
    fn left_press(
        &self,
        bounds: Rectangle,
        pos: Point,
        modifiers: keyboard::Modifiers,
    ) -> Option<Option<Message>> {
        if self.slid() {
            if let Some(action) = self.peek_press(bounds, pos) {
                if self.peek.is_some_and(|(i, _)| self.is_alert(i)) {
                    return Some(action);
                }
                return None;
            }
            let strip = self.layout_in(bounds);
            if let Some(i) = strip
                .bars
                .iter()
                .position(|bar| bar.contains(pos))
                .filter(|&i| self.is_alert(i))
            {
                // Open directly: no drag while the strip is slid away.
                return Some(Some(Message::BarClicked(i)));
            }
            return None;
        }
        if let Some(message) = self.toast_press(bounds, pos) {
            return Some(Some(message));
        }
        // Clicking the open peek opens its note, or deletes it.
        if let Some(action) = self.peek_press(bounds, pos) {
            return Some(action);
        }
        let strip = self.layout_in(bounds);
        if let Some(i) = strip.bars.iter().position(|bar| bar.contains(pos)) {
            return Some(Some(Message::DragStart(i, self.along(bounds, pos))));
        }
        strip.press_message(pos, modifiers).map(Some)
    }

    /// What a press at `pos` on the open peek does: its trash button and a
    /// stack's rows and Unstack button once it is fully open, otherwise
    /// opening the note.
    fn peek_press(&self, bounds: Rectangle, pos: Point) -> Option<Option<Message>> {
        let i = self.peek_hit(bounds, pos)?;
        let (_, progress) = self.peek?;
        let bar = *self.layout_in(bounds).bars.get(i)?;
        let (_, width, text) = self.peek_parts(i, bar)?;
        let parts = peek_layout(bar, bounds, 1.0, &text, width, self.edge);
        let row = parts.stack_rows.iter().position(|r| r.contains(pos));
        let open = progress >= 0.99;
        Some(if open && parts.trash.contains(pos) {
            Some(Message::PeekDelete(i))
        } else if open && parts.unstack.is_some_and(|u| u.contains(pos)) {
            Some(Message::Unstack(i))
        } else if let Some(row) = row.filter(|_| open) {
            Some(Message::OpenStackMember(text.stack[row].0))
        } else if open && parts.stack_more.is_some_and(|m| m.contains(pos)) {
            None
        } else {
            Some(Message::BarClicked(i))
        })
    }

    /// The hint chip to show at `now`, if any (see `hint_kind`).
    fn hint(
        &self,
        strip: &StripLayout,
        slot_hover: Option<(Slot, Instant)>,
        now: Instant,
    ) -> Option<Hint> {
        match self.hint_kind() {
            HintKind::None => None,
            HintKind::Stack => drag_hint(&strip.bars, self.drag.as_ref(), self.edge),
            HintKind::Clipboard => Some(Hint {
                text: CLIPBOARD_EMPTY,
                accent: None,
                at: HintAt::Actions,
            }),
            HintKind::Toast => Some(TOAST),
            HintKind::Tooltip => {
                let text = slot_tooltip(slot_hover, now)?;
                let at = match slot_hover?.0 {
                    Slot::Actions => HintAt::Actions,
                    Slot::Add => HintAt::Add,
                    Slot::Search => HintAt::Search,
                    Slot::Settings => HintAt::Settings,
                };
                Some(Hint {
                    text,
                    accent: None,
                    at,
                })
            }
        }
    }

    fn hint_kind(&self) -> HintKind {
        hint_kind(
            self.peek,
            self.drag.as_ref(),
            self.clipboard_hint,
            self.toast,
            self.panel_open,
        )
    }

    /// What a press at `pos` on the undo toast does, while it shows.
    fn toast_press(&self, bounds: Rectangle, pos: Point) -> Option<Message> {
        let strip = self.layout_in(bounds);
        (self.hint_kind() == HintKind::Toast && toast_rect(&strip, bounds).contains(pos))
            .then_some(Message::UndoDelete)
    }

    /// Notes in `state` the frame drawn at `at` and the chip showing then.
    fn note_frame(&self, state: &mut StripState, strip: &StripLayout, at: Instant) {
        let hint = self.hint(strip, state.slot_hover, at);
        state.frame = Some(at);
        state.show_chip(hint, at);
    }

    /// The chip to draw on the latest frame and its opacity, while it is
    /// still the one to show.
    fn frame_chip(&self, state: &StripState, strip: &StripLayout) -> Option<(Hint, f32)> {
        let now = state.frame?;
        let current = self.hint(strip, state.slot_hover, now);
        let (hint, since) = state.chip.filter(|(h, _)| Some(*h) == current)?;
        Some((hint, chip_alpha(since, now)))
    }

    /// The gap the dragged bar drops into, kept within its pin group.
    fn insertion_index(&self, drag: &DragState, bars: &[Rectangle]) -> usize {
        let centers: Vec<f32> = bars
            .iter()
            .map(|bar| along_center(*bar, self.edge))
            .collect();
        let pinned: Vec<bool> = (0..bars.len())
            .map(|i| self.note(i).is_some_and(|note| note.pinned))
            .collect();
        strip_model::insertion_slot(&centers, drag.current_y, &pinned, drag.bar_index)
    }

    /// What a wheel `delta` at `pos` does: the message to publish, and
    /// whether the strip captures the event. A peek with more lines than it
    /// shows scrolls its body by the vertical delta, and never the strip
    /// behind it; otherwise the strip scrolls along its edge. On Top both
    /// wheel axes scroll along it.
    fn wheel(
        &self,
        bounds: Rectangle,
        pos: Point,
        delta: mouse::ScrollDelta,
    ) -> (Option<Message>, bool) {
        let (dx, dy) = match delta {
            mouse::ScrollDelta::Lines { x, y } => (x * 30.0, y * 30.0),
            mouse::ScrollDelta::Pixels { x, y } => (x, y),
        };
        if let Some(scroll) = self.peek_wheel(bounds, pos, dy) {
            let message = (scroll != self.peek_scroll).then_some(Message::PeekScroll(scroll));
            (message, true)
        } else if bounds.contains(pos) {
            let along = match self.edge {
                Edge::Right | Edge::Left => dy,
                Edge::Top => dx + dy,
            };
            (Some(Message::StripScroll(along)), false)
        } else {
            (None, false)
        }
    }

    /// Draws the strip in place, with `cursor` for its hover looks.
    fn draw_strip(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) {
        let strip = self.layout_in(bounds);
        let edge = self.edge;
        let frame = Frame { edge, bounds };
        let bars = &strip.bars;
        let dragging_index = self.drag.as_ref().map(|d| d.bar_index);
        let drag_active = self.drag.as_ref().is_some_and(drag_moved);

        for (i, bar_rect) in bars.iter().enumerate() {
            if let Some((peek_index, progress)) = self.peek {
                if peek_index == i && progress > 0.0 {
                    if let Some((note, width, text)) = self.peek_parts(i, *bar_rect) {
                        let peek_bar = *bar_rect + hide_translation(edge, self.bar_slide(i));
                        draw_peek(
                            renderer,
                            note,
                            &text,
                            peek_bar,
                            bounds,
                            &self.theme,
                            CORNER_RADIUS,
                            progress,
                            self.paper_tint,
                            cursor.position(),
                            width,
                            self.peek_scroll,
                            edge,
                        );
                    }
                    continue;
                }
            }
            // A stack whose top is being deleted still draws it: the collapse
            // lasts 0.18 s and clicks already reach the next note.
            if let Some(note) = self.note(i) {
                let dim = if self.dimmed.get(i).copied().unwrap_or(false) {
                    DIM_ALPHA
                } else {
                    1.0
                };
                let alpha = if drag_active && Some(i) == dragging_index {
                    0.3
                } else {
                    note.color.rgba[3]
                } * dim;
                let progress = self.progress.get(i).copied().flatten();
                let fill_alpha = alpha;
                let alpha = alpha * progress_alpha(progress);
                let open = self.open.filter(|(o, _)| *o == i);
                let jump = self.jump.get(i).copied().unwrap_or(0.0);
                let rect = bar_rect_for(open.map_or(0.0, |(_, p)| p), *bar_rect, edge)
                    + hide_translation(edge, self.bar_slide(i))
                    + jump_translation(edge, jump);
                let corner = CORNER_RADIUS;
                let pulse = self.pulse.get(i).copied().unwrap_or(0.0);
                if pulse > 0.0 {
                    let [r, g, b, _] = note.color.rgba;
                    renderer::Renderer::fill_quad(
                        renderer,
                        renderer::Quad {
                            bounds: pulse_halo(rect, pulse),
                            border: iced::Border {
                                radius: (corner + PULSE_REACH * pulse).into(),
                                ..Default::default()
                            },
                            shadow: Default::default(),
                            snap: true,
                        },
                        Color::from_rgba(r, g, b, 0.6 * pulse * dim),
                    );
                }
                if open.is_some() {
                    let [_, ambient] = self.theme.shadows(1.0);
                    let [r, g, b, _] = note.color.rgba;
                    renderer::Renderer::fill_quad(
                        renderer,
                        renderer::Quad {
                            bounds: rect,
                            border: iced::Border {
                                radius: corner.into(),
                                ..Default::default()
                            },
                            shadow: ambient,
                            snap: true,
                        },
                        Color::from_rgba(r, g, b, alpha),
                    );
                }
                renderer::Renderer::fill_quad(
                    renderer,
                    renderer::Quad {
                        bounds: rect,
                        border: iced::Border {
                            radius: corner.into(),
                            ..Default::default()
                        },
                        shadow: Default::default(),
                        snap: true,
                    },
                    self.theme.bar_gradient(note.color, alpha),
                );
                // Open tasks leave the bar faint; done ones fill it from its
                // end along the edge. All done → fill covers the whole bar.
                if let Some((done, total)) = progress.filter(|(d, t)| *t > 0 && *d > 0) {
                    let fill = progress_fill(rect, done, total, edge);
                    let fill_length = along_span(fill, edge).1;
                    if fill_length > 0.0 {
                        let [r, g, b, _] = note.color.rgba;
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad {
                                bounds: fill,
                                border: iced::Border {
                                    radius: corner.min(fill_length / 2.0).into(),
                                    ..Default::default()
                                },
                                shadow: Default::default(),
                                snap: true,
                            },
                            Color::from_rgba(r, g, b, fill_alpha),
                        );
                    }
                }
                renderer::Renderer::fill_quad(
                    renderer,
                    renderer::Quad {
                        // A collapsing bar takes its highlight with it.
                        bounds: start_cap(rect, 1.0, edge),
                        border: Default::default(),
                        shadow: Default::default(),
                        snap: true,
                    },
                    self.theme.highlight().scale_alpha(dim),
                );
                if note.pinned {
                    let [r, g, b, _] = note.color.rgba;
                    renderer::Renderer::fill_quad(
                        renderer,
                        renderer::Quad {
                            bounds: start_cap(rect, NOTCH_HEIGHT, edge),
                            border: Default::default(),
                            shadow: Default::default(),
                            snap: true,
                        },
                        notch_color(Color::from_rgb(r, g, b), alpha),
                    );
                }
                if self.entries.get(i).is_some_and(|e| !e.members.is_empty()) {
                    for line in stack_edges(rect, edge) {
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad {
                                bounds: line,
                                border: iced::border::rounded(STACK_EDGE_WIDTH / 2.0),
                                shadow: Default::default(),
                                snap: true,
                            },
                            self.theme.ink(0.55 * dim),
                        );
                    }
                }
            }
        }

        // Slots, drag ghost and chips stay off-screen while the strip is
        // slid; only alerting bars (above) remain visible.
        if self.slid() {
            return;
        }

        // Actions slot: collapsed brand mark, or New / Search / Settings in a
        // row, sliding out of it as the row unfolds.
        let idle = self.drag.is_none();
        let pressed = tree.state.downcast_ref::<StripState>().pressed_slot;
        let unfold = strip.actions_progress.clamp(0.0, 1.0);
        let settled = strip.actions_settled();
        if unfold < 1.0 {
            let actions = sink_slot(
                strip.actions_slot + Vector::new(self.add_shake, 0.0),
                pressed == Some(Slot::Actions),
            );
            let hovered = idle && !strip.actions_expanded && cursor.is_over(strip.actions_hit_area);
            let alpha = 1.0 - unfold;
            draw_slot(renderer, actions, hovered, alpha, &self.theme);
            let color = slot_style(false, &self.theme).icon.scale_alpha(alpha);
            for quad in actions_glyph(actions) {
                renderer::Renderer::fill_quad(
                    renderer,
                    renderer::Quad {
                        bounds: quad,
                        border: iced::Border {
                            radius: 1.0.into(),
                            ..Default::default()
                        },
                        shadow: Default::default(),
                        snap: true,
                    },
                    color,
                );
            }
        }
        if unfold > 0.0 {
            let hit_areas = [
                strip.add_hit_area,
                strip.search_hit_area,
                strip.settings_hit_area,
            ];
            let slots = [Slot::Add, Slot::Search, Slot::Settings];
            for ((rect, hit), slot) in strip.pill_rects().into_iter().zip(hit_areas).zip(slots) {
                // The shake is a horizontal "no" on every edge.
                let shake = if slot == Slot::Add {
                    Vector::new(self.add_shake, 0.0)
                } else {
                    Vector::ZERO
                };
                let rect = sink_slot(rect + shake, settled && pressed == Some(slot));
                let hovered = idle && settled && cursor.is_over(hit);
                draw_slot(renderer, rect, hovered, unfold, &self.theme);
                let color = slot_style(false, &self.theme).icon.scale_alpha(unfold);
                draw_pill_icon(renderer, slot, rect, color);
            }
        }

        if let Some(drag) = &self.drag {
            if drag_active {
                if let Some(note) = self.note(drag.bar_index) {
                    // The ghost follows the cursor along the edge, flush
                    // with the screen edge.
                    let scale = self.magnification.scale(drag.bar_index);
                    let length = self.bars.height * scale;
                    let ghost = frame.rect(LocalRect {
                        along: drag.current_y - length / 2.0,
                        away: 0.0,
                        length,
                        thickness: self.bars.width * scale,
                    });
                    renderer::Renderer::fill_quad(
                        renderer,
                        renderer::Quad {
                            bounds: ghost,
                            border: iced::Border {
                                radius: CORNER_RADIUS.into(),
                                ..Default::default()
                            },
                            shadow: Default::default(),
                            snap: true,
                        },
                        Color::from_rgba(
                            note.color.rgba[0],
                            note.color.rgba[1],
                            note.color.rgba[2],
                            0.8,
                        ),
                    );

                    // Over a bar's middle the drop stacks: that bar is ringed.
                    if let Some(onto) = stack_target(bars, drag.bar_index, drag.current_y, edge) {
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad {
                                bounds: bars[onto].expand(2.0),
                                border: iced::Border {
                                    radius: (CORNER_RADIUS + 2.0).into(),
                                    width: 1.5,
                                    color: self.theme.ink(0.8),
                                },
                                shadow: Default::default(),
                                snap: true,
                            },
                            Color::TRANSPARENT,
                        );
                    } else {
                        let target = self.insertion_index(drag, bars);
                        let indicator = if target < bars.len() {
                            along_span(bars[target], edge).0 - self.bars.gap / 2.0
                        } else {
                            bars.last().map_or(frame.span().along, |b| {
                                let (start, length) = along_span(*b, edge);
                                start + length + self.bars.gap / 2.0
                            })
                        };
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad {
                                bounds: frame.rect(LocalRect {
                                    along: indicator - 1.0,
                                    away: 0.0,
                                    length: 2.0,
                                    thickness: 20.0,
                                }),
                                border: Default::default(),
                                shadow: Default::default(),
                                snap: true,
                            },
                            self.theme.ink(0.8),
                        );
                    }
                }
            }
        }

        // The hint chip, once `update` has noted it, fading in from then.
        let state = tree.state.downcast_ref::<StripState>();
        if let Some((hint, alpha)) = self.frame_chip(state, &strip) {
            if let Some(anchor) = hint_anchor(&strip, hint.at) {
                draw_chip(
                    renderer,
                    &self.theme,
                    bounds,
                    anchor,
                    hint.text,
                    hint.accent,
                    alpha,
                    edge,
                );
            }
        }
    }
}

impl<'a> advanced::Widget<Message, Theme, iced::Renderer> for BarStrip<'a> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<StripState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(StripState::default())
    }

    fn size(&self) -> Size<Length> {
        let thick = self.widget_thickness();
        match self.edge {
            Edge::Right | Edge::Left => Size::new(Length::Fixed(thick), Length::Fill),
            Edge::Top => Size::new(Length::Fill, Length::Fixed(thick)),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let thick = self.widget_thickness();
        let size = match self.edge {
            Edge::Right | Edge::Left => limits
                .width(Length::Fixed(thick))
                .height(Length::Fill)
                .resolve(thick, f32::INFINITY, Size::new(thick, 0.0)),
            Edge::Top => limits
                .width(Length::Fill)
                .height(Length::Fixed(thick))
                .resolve(f32::INFINITY, thick, Size::new(0.0, thick)),
        };
        layout::Node::new(size)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        if self.slid() && !self.any_alert() {
            // Fully hidden: slide everything off-screen together.
            renderer::Renderer::with_translation(
                renderer,
                hide_translation(self.edge, self.x_offset),
                |renderer| {
                    self.draw_strip(tree, renderer, layout.bounds(), mouse::Cursor::Unavailable);
                },
            );
            return;
        }
        // In place, or alert-only: per-bar slide keeps alerting bars visible.
        self.draw_strip(tree, renderer, layout.bounds(), cursor);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<StripState>();
        state.observe(event);
        // A frame's own instant, so hover, tooltip timing and the chip go
        // by one clock.
        let now = match event {
            Event::Window(iced::window::Event::RedrawRequested(at)) => *at,
            _ => Instant::now(),
        };
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let slot = self.layout_in(bounds).slot_at(*position);
                state.hover_slot(slot.filter(|_| !self.slid()), now);
            }
            Event::Mouse(mouse::Event::CursorLeft) => state.hover_slot(None, now),
            Event::Mouse(mouse::Event::ButtonPressed(_))
            | Event::Keyboard(keyboard::Event::KeyPressed { .. }) => {
                let strip = self.layout_in(bounds);
                state.press(cursor.position().and_then(|pos| strip.slot_at(pos)));
            }
            Event::Window(iced::window::Event::RedrawRequested(_)) => {
                self.note_frame(state, &self.layout_in(bounds), now);
                if state.chip_fading(now) {
                    shell.request_redraw();
                }
            }
            _ => {}
        }
        if let Some(due) = state.tooltip_due(now) {
            shell.request_redraw_at(due);
        }

        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if !self.slid() {
                    let over = self.layout_in(bounds).over_actions(*position);
                    if over != self.actions_expanded {
                        shell.publish(Message::ActionsHovered(over));
                    }
                } else if self.actions_expanded {
                    shell.publish(Message::ActionsHovered(false));
                }
                if let Some(message) = self.hover_message(bounds, *position) {
                    shell.publish(message);
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                if self.actions_expanded {
                    shell.publish(Message::ActionsHovered(false));
                }
                shell.publish(Message::StripHover(None));
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let modifiers = tree.state.downcast_ref::<StripState>().modifiers;
                let action = cursor
                    .position()
                    .and_then(|pos| self.left_press(bounds, pos, modifiers));
                if let Some(action) = action {
                    if let Some(message) = action {
                        shell.publish(message);
                    }
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                tree.state.downcast_mut::<StripState>().pressed_slot = None;
                if self.drag.is_some() {
                    shell.publish(Message::DragEnd);
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if !self.slid() => {
                let Some(pos) = cursor.position() else {
                    return;
                };
                let (message, capture) = self.wheel(bounds, pos, *delta);
                if let Some(message) = message {
                    shell.publish(message);
                }
                if capture {
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.drag.as_ref().is_some_and(drag_moved) {
            return mouse::Interaction::Grabbing;
        }
        if self.slid() {
            if let Some(pos) = cursor.position() {
                let strip = self.layout_in(layout.bounds());
                let over_alert = self
                    .peek_hit(layout.bounds(), pos)
                    .is_some_and(|i| self.is_alert(i))
                    || strip
                        .bars
                        .iter()
                        .enumerate()
                        .any(|(i, bar)| self.is_alert(i) && bar.contains(pos));
                if over_alert {
                    return mouse::Interaction::Pointer;
                }
            }
            return mouse::Interaction::None;
        }
        if let Some(pos) = cursor.position() {
            let strip = self.layout_in(layout.bounds());
            if self.peek_hit(layout.bounds(), pos).is_some()
                || self.toast_press(layout.bounds(), pos).is_some()
                || strip.bars.iter().any(|bar| bar.contains(pos))
                || strip.slot_message(pos).is_some()
            {
                return mouse::Interaction::Pointer;
            }
        }
        mouse::Interaction::None
    }
}

impl<'a> From<BarStrip<'a>> for Element<'a, Message> {
    fn from(strip: BarStrip<'a>) -> Self {
        Self::new(strip)
    }
}

/// What the chip for `at` sits beside.
fn hint_anchor(strip: &StripLayout, at: HintAt) -> Option<Rectangle> {
    match at {
        HintAt::Bar(i) => strip.bars.get(i).copied(),
        HintAt::Actions => Some(strip.actions_button),
        HintAt::Add => Some(strip.add_button),
        HintAt::Search => Some(strip.search_button),
        HintAt::Settings => Some(strip.settings_button),
    }
}

/// Brand mark inside the collapsed Actions slot: a sticky with a strip bar.
fn actions_glyph(slot: Rectangle) -> Vec<Rectangle> {
    let w = slot.width * 0.55;
    let h = slot.height * 0.55;
    let x = slot.x + (slot.width - w) / 2.0 - slot.width * 0.04;
    let y = slot.y + (slot.height - h) / 2.0;
    let band = (h * 0.18).max(1.5);
    let bar_w = (slot.width * 0.12).max(1.5);
    let stroke = (slot.width.min(slot.height) * 0.08).max(1.0);
    vec![
        // Sticky outline: top, bottom, left, right (before the bar).
        Rectangle::new(Point::new(x, y), Size::new(w, stroke)),
        Rectangle::new(Point::new(x, y + h - stroke), Size::new(w, stroke)),
        Rectangle::new(Point::new(x, y), Size::new(stroke, h)),
        Rectangle::new(Point::new(x + w - stroke, y), Size::new(stroke, h)),
        // Adhesive band.
        Rectangle::new(
            Point::new(x + stroke, y + stroke),
            Size::new(w - 2.0 * stroke, band),
        ),
        // Strip bar to the right of the sticky.
        Rectangle::new(
            Point::new(x + w + stroke * 0.5, y + h * 0.15),
            Size::new(bar_w, h * 0.7),
        ),
    ]
}

/// The Lucide icon an expanded action pill shows; the collapsed Actions
/// pill draws its brand mark instead.
fn pill_icon(slot: Slot) -> Option<Icon> {
    match slot {
        Slot::Add => Some(Icon::Plus),
        Slot::Search => Some(Icon::Search),
        Slot::Settings => Some(Icon::Settings),
        Slot::Actions => None,
    }
}

/// The icon font size that fits a pill: 60% of its shorter side.
fn pill_icon_size(slot: Rectangle) -> f32 {
    (slot.width.min(slot.height) * 0.6).max(1.0)
}

/// Draws `slot`'s Lucide icon centered in the pill `rect`.
fn draw_pill_icon(renderer: &mut iced::Renderer, slot: Slot, rect: Rectangle, color: Color) {
    let Some(icon) = pill_icon(slot) else {
        return;
    };
    renderer.fill_text(
        text::Text {
            content: icon.codepoint().to_string(),
            bounds: rect.size(),
            size: Pixels(pill_icon_size(rect)),
            line_height: text::LineHeight::Relative(1.0),
            font: ICON_FONT,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        },
        rect.center(),
        color,
        rect,
    );
}

/// Where the fully open peek of the note on `bar` sits: the area that
/// keeps it open while hovered and opens the note when clicked.
pub fn peek_target(
    bar: Rectangle,
    bounds: Rectangle,
    text: &PeekText,
    width: f32,
    edge: Edge,
) -> Rectangle {
    peek_layout(bar, bounds, 1.0, text, width, edge).rect
}

/// Nudges an action pill 1 px down while pressed.
fn sink_slot(rect: Rectangle, pressed: bool) -> Rectangle {
    if pressed {
        Rectangle {
            y: rect.y + 1.0,
            ..rect
        }
    } else {
        rect
    }
}

/// How an action pill is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SlotStyle {
    fill: Color,
    border: Color,
    icon: Color,
}

/// Action pills are solid cards, like the peek and the chips: the window is
/// transparent, so a tint would vanish against a dark or busy desktop.
fn slot_style(hovered: bool, theme: &theme::Theme) -> SlotStyle {
    let card = theme.card();
    SlotStyle {
        fill: if hovered {
            theme::mix(card, theme.ink(1.0), 0.08)
        } else {
            card
        },
        border: theme.ink(0.18),
        icon: theme.ink(SLOT_ICON_INK),
    }
}

/// An action pill: a solid card with a hairline border and a soft shadow,
/// slightly darker on hover.
/// `alpha` fades it in and out while the Actions row unfolds.
fn draw_slot(
    renderer: &mut iced::Renderer,
    rect: Rectangle,
    hovered: bool,
    alpha: f32,
    theme: &theme::Theme,
) {
    let style = slot_style(hovered, theme);
    let [contact, ambient] = theme.shadows(alpha);
    for shadow in [ambient, contact] {
        renderer::Renderer::fill_quad(
            renderer,
            renderer::Quad {
                bounds: rect,
                border: iced::Border {
                    radius: GLASS_SLOT_RADIUS.into(),
                    width: 1.0,
                    color: style.border.scale_alpha(alpha),
                },
                shadow,
                snap: true,
            },
            style.fill.scale_alpha(alpha),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peek::peek_text;

    fn bounds(height: f32) -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, height))
    }

    fn layout(count: usize, scale: impl Fn(usize) -> f32, height: f32, scroll: f32) -> StripLayout {
        compute_layout(
            count,
            scale,
            bounds(height),
            scroll,
            &BarSettings::default(),
            false,
            None,
            Edge::Right,
        )
    }

    fn layout_expanded(
        count: usize,
        scale: impl Fn(usize) -> f32,
        height: f32,
        scroll: f32,
    ) -> StripLayout {
        compute_layout(
            count,
            scale,
            bounds(height),
            scroll,
            &BarSettings::default(),
            true,
            None,
            Edge::Right,
        )
    }

    fn stack_center(l: &StripLayout) -> f32 {
        let top = l.bars.first().map_or(l.actions_button.y, |b| b.y);
        let bottom = l.actions_button.y + l.actions_button.height;
        (top + bottom) / 2.0
    }

    #[test]
    fn progress_fill_grows_from_bottom() {
        let r = Rectangle::new(Point::new(10.0, 100.0), Size::new(8.0, 40.0));
        assert_eq!(progress_fill(r, 0, 4, Edge::Right).height, 0.0);
        let half = progress_fill(r, 2, 4, Edge::Right);
        assert_eq!((half.height, half.y + half.height), (20.0, 140.0));
        assert_eq!((half.x, half.width), (r.x, r.width));
        assert_eq!(progress_fill(r, 4, 4, Edge::Right), r);
    }

    #[test]
    fn jump_translation_moves_into_the_screen() {
        assert_eq!(jump_translation(Edge::Right, 5.0), Vector::new(-5.0, 0.0));
        assert_eq!(jump_translation(Edge::Left, 5.0), Vector::new(5.0, 0.0));
        assert_eq!(jump_translation(Edge::Top, 5.0), Vector::new(0.0, 5.0));
    }

    #[test]
    fn pulse_halo_grows_with_the_pulse() {
        let r = Rectangle::new(Point::new(10.0, 10.0), Size::new(6.0, 40.0));
        assert_eq!(pulse_halo(r, 0.0), r);
        let full = pulse_halo(r, 1.0);
        assert_eq!(full.width, r.width + 2.0 * PULSE_REACH);
        assert_eq!(full.center(), r.center());
    }

    #[test]
    fn notch_is_darker_than_its_bar() {
        let bar = Color::from_rgb(1.0, 0.85, 0.24);
        let notch = notch_color(bar, 0.3);
        let luma = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        assert!(luma(notch) < luma(bar));
        assert_eq!(notch.a, 0.3);
    }

    #[test]
    fn checklist_bar_stays_faint_under_fill() {
        assert_eq!(progress_alpha(Some((3, 3))), 0.45);
        assert_eq!(progress_alpha(Some((1, 3))), 0.45);
        assert_eq!(progress_alpha(None), 1.0);
        assert_eq!(
            progress_fill(
                Rectangle::new(Point::ORIGIN, Size::new(10.0, 40.0)),
                5,
                5,
                Edge::Right,
            )
            .height,
            40.0
        );
    }

    #[test]
    fn stack_edges_sit_left_of_the_bar() {
        let r = Rectangle::new(Point::new(40.0, 100.0), Size::new(8.0, 40.0));
        let [a, b] = stack_edges(r, Edge::Right);
        // Separate lines with a visible gap between them and the bar.
        assert!(a.x + a.width < r.x && b.x + b.width < a.x);
        assert!(a.width >= 2.0 && b.width >= 2.0);
        assert!(b.height < a.height && a.height < r.height);
    }

    #[test]
    fn open_bar_is_wider_and_keeps_its_right_edge() {
        let r = Rectangle::new(Point::new(40.0, 100.0), Size::new(6.0, 30.0));
        let open = bar_rect_for(1.0, r, Edge::Right);
        assert_eq!(open.width, r.width * 1.5);
        assert_eq!(open.x + open.width, r.x + r.width);
        assert_eq!((open.y, open.height), (r.y, r.height));
        assert_eq!(bar_rect_for(0.0, r, Edge::Right), r);
        // It widens with the note's morph instead of jumping.
        let half = bar_rect_for(0.5, r, Edge::Right);
        assert_eq!(half.width, r.width * 1.25);
        assert_eq!(half.x + half.width, r.x + r.width);
    }

    #[test]
    fn stack_is_vertically_centered_for_any_count() {
        for count in [0, 1, 3, 8] {
            let l = layout(count, |_| 1.0, 900.0, 0.0);
            assert!((stack_center(&l) - 450.0).abs() < 0.01, "count {count}");
            assert_eq!(l.max_scroll, 0.0);
        }
    }

    #[test]
    fn collapsing_bar_shrinks_with_progress() {
        let d = BarSettings::default();
        let with = |collapse| {
            compute_layout(
                3,
                |_| 1.0,
                bounds(900.0),
                0.0,
                &d,
                false,
                collapse,
                Edge::Right,
            )
        };
        let full = with(None);
        let half = with(Some((1, 0.5)));
        assert!((half.bars[1].height - d.height / 2.0).abs() < 0.01);
        let gone = with(Some((1, 1.0)));
        assert_eq!(gone.bars[1].height, 0.0);
        let extent = |l: &StripLayout| l.actions_button.y + l.actions_button.height - l.bars[0].y;
        assert!((extent(&full) - extent(&gone) - (d.height + d.gap)).abs() < 0.01);
    }

    #[test]
    fn bars_are_spaced_by_gap() {
        let l = layout(3, |_| 1.0, 900.0, 0.0);
        let gap = l.bars[1].y - (l.bars[0].y + l.bars[0].height);
        assert!((gap - BarSettings::default().gap).abs() < 0.01);
    }

    #[test]
    fn bar_settings_drive_sizes() {
        let bars = BarSettings {
            width: 10.0,
            height: 50.0,
            gap: 20.0,
        };
        let l = compute_layout(
            3,
            |_| 1.0,
            bounds(900.0),
            0.0,
            &bars,
            false,
            None,
            Edge::Right,
        );
        assert_eq!(l.bars[0].size(), Size::new(10.0, 50.0));
        let gap = l.bars[1].y - (l.bars[0].y + l.bars[0].height);
        assert!((gap - 20.0).abs() < 0.01);
    }

    #[test]
    fn overflowing_stack_scrolls_and_clamps() {
        let l = layout(40, |_| 1.0, 400.0, 0.0);
        assert!(l.max_scroll > 0.0);
        assert!((l.bars[0].y - EDGE_PADDING).abs() < 0.01);
        let scrolled = layout(40, |_| 1.0, 400.0, 1.0e6);
        let last = scrolled.actions_button.y + scrolled.actions_button.height;
        assert!((last - (400.0 - EDGE_PADDING)).abs() < 0.01);
    }

    #[test]
    fn bars_keep_margin_from_screen_edge() {
        let l = layout(3, |_| 5.0, 900.0, 0.0);
        for bar in l.bars.iter().chain([&l.actions_button]) {
            assert!((STRIP_WIDTH - (bar.x + bar.width) - EDGE_MARGIN).abs() < 0.01);
        }
        let open = layout_expanded(3, |_| 1.0, 900.0, 0.0);
        assert!(open.add_button.x + open.add_button.width <= STRIP_WIDTH - EDGE_MARGIN + 0.01);
        assert!(open.settings_button.x >= 0.0);
    }

    #[test]
    fn actions_slot_is_a_squat_chip_at_rest() {
        let d = BarSettings::default();
        let l = layout(3, |_| 1.0, 900.0, 0.0);
        assert!((l.actions_button.width - d.width * SLOT_AWAY_SCALE).abs() < 0.01);
        assert!((l.actions_button.height - d.height * SLOT_ALONG_SCALE).abs() < 0.01);
        assert!(
            l.actions_button.width > l.bars[0].width,
            "wider than a note bar"
        );
        assert!(
            l.actions_button.height < l.bars[0].height,
            "shorter along the edge than a note bar"
        );
        assert!(!l.actions_expanded);
    }

    #[test]
    fn magnified_actions_slot_widens_but_stays_compact() {
        let d = BarSettings::default();
        let l = layout(3, |i| if i == 3 { 5.0 } else { 1.0 }, 900.0, 0.0);
        assert!((l.actions_button.width - d.width * SLOT_AWAY_SCALE * 5.0).abs() < 0.01);
        assert!(
            (l.actions_button.height - d.height * SLOT_ALONG_SCALE * ADD_MAX_HEIGHT_SCALE).abs()
                < 0.01
        );
    }

    #[test]
    fn actions_hit_area_spans_strip_width() {
        let l = layout(3, |_| 1.0, 900.0, 0.0);
        assert_eq!(l.actions_hit_area.width, STRIP_WIDTH);
        assert!(l
            .actions_hit_area
            .contains(Point::new(2.0, l.actions_button.y + 2.0)));
        assert!(l.over_actions(l.actions_button.center()));
        assert!(l.slot_message(l.actions_button.center()).is_none());
    }

    #[test]
    fn band_is_centered_share_of_bounds() {
        let b = band(
            Rectangle::new(Point::new(5.0, 100.0), Size::new(64.0, 1000.0)),
            0.5,
            Edge::Right,
        );
        assert_eq!(
            b,
            Rectangle::new(Point::new(5.0, 350.0), Size::new(64.0, 500.0))
        );
        let full = bounds(800.0);
        assert_eq!(band(full, 1.0, Edge::Right), full);
    }

    #[test]
    fn action_pills_use_lucide_icons_from_the_subset() {
        assert_eq!(pill_icon(Slot::Add), Some(Icon::Plus));
        assert_eq!(pill_icon(Slot::Search), Some(Icon::Search));
        assert_eq!(pill_icon(Slot::Settings), Some(Icon::Settings));
        assert_eq!(pill_icon(Slot::Actions), None);
        let face = ttf_parser::Face::parse(crate::icons::FONTS[5], 0).expect("icon font parses");
        for slot in [Slot::Add, Slot::Search, Slot::Settings] {
            let icon = pill_icon(slot).unwrap();
            assert!(face.glyph_index(icon.codepoint()).is_some(), "{icon:?}");
        }
    }

    #[test]
    fn pill_icon_fits_its_slot() {
        let slot = Rectangle::new(Point::new(10.0, 20.0), Size::new(24.0, 36.0));
        let size = pill_icon_size(slot);
        assert!(size > 0.0 && size <= slot.width.min(slot.height));
        // Grows with the pill.
        let big = Rectangle::new(Point::ORIGIN, Size::new(48.0, 72.0));
        assert!(pill_icon_size(big) > size);
    }

    #[test]
    fn drop_zone_edges() {
        let bar = Rectangle::new(Point::new(40.0, 100.0), Size::new(8.0, 40.0));
        assert_eq!(drop_zone(bar, 90.0, Edge::Right), DropZone::Before);
        assert_eq!(drop_zone(bar, 109.0, Edge::Right), DropZone::Before);
        assert_eq!(drop_zone(bar, 110.0, Edge::Right), DropZone::Onto);
        assert_eq!(drop_zone(bar, 120.0, Edge::Right), DropZone::Onto);
        assert_eq!(drop_zone(bar, 130.0, Edge::Right), DropZone::Onto);
        assert_eq!(drop_zone(bar, 131.0, Edge::Right), DropZone::After);
        assert_eq!(drop_zone(bar, 150.0, Edge::Right), DropZone::After);
        let bars = [bar, Rectangle { y: 150.0, ..bar }];
        assert_eq!(stack_target(&bars, 1, 120.0, Edge::Right), Some(0));
        assert_eq!(
            stack_target(&bars, 0, 120.0, Edge::Right),
            None,
            "not onto itself"
        );
        assert_eq!(
            stack_target(&bars, 1, 145.0, Edge::Right),
            None,
            "between bars"
        );
    }

    #[test]
    fn stack_target_shows_stack_chip() {
        let bar = Rectangle::new(Point::new(40.0, 100.0), Size::new(8.0, 40.0));
        let bars = [bar, Rectangle { y: 150.0, ..bar }];
        let drag = |current_y| DragState {
            bar_index: 1,
            origin_y: 170.0,
            current_y,
        };
        assert_eq!(
            drag_hint(&bars, Some(&drag(120.0)), Edge::Right),
            Some(Hint {
                text: "Stack",
                accent: None,
                at: HintAt::Bar(0)
            })
        );
        assert_eq!(
            drag_hint(&bars, Some(&drag(145.0)), Edge::Right),
            None,
            "between bars"
        );
        assert_eq!(drag_hint(&bars, None, Edge::Right), None);
        // A press that hasn't moved yet is no drag.
        let still = DragState {
            bar_index: 0,
            origin_y: 120.0,
            current_y: 122.0,
        };
        assert_eq!(drag_hint(&bars, Some(&still), Edge::Right), None);
    }

    #[test]
    fn slot_tooltip_after_delay() {
        let at = Instant::now();
        let hover = Some((Slot::Add, at));
        let later = |ms| at + Duration::from_millis(ms);
        assert_eq!(slot_tooltip(hover, later(599)), None);
        let add = slot_tooltip(hover, later(600)).unwrap();
        assert!(add.starts_with("New note · ") && add.ends_with("-click: from clipboard"));
        let search = slot_tooltip(Some((Slot::Search, at)), later(900)).unwrap();
        assert!(search == "Search (Cmd+F)" || search == "Search (Ctrl+F)");
        assert_eq!(slot_tooltip(None, later(900)), None);
    }

    #[test]
    fn slot_tooltip_needs_a_fresh_hover_after_a_press() {
        let at = Instant::now();
        let mut state = StripState::default();
        state.hover_slot(Some(Slot::Add), at);
        assert_eq!(state.slot_hover, Some((Slot::Add, at)));
        // Moving within the slot keeps the timer running.
        state.hover_slot(Some(Slot::Add), at + Duration::from_millis(300));
        assert_eq!(state.slot_hover, Some((Slot::Add, at)));
        state.press(Some(Slot::Add));
        assert_eq!(state.slot_hover, None);
        state.hover_slot(Some(Slot::Add), at + Duration::from_millis(900));
        assert_eq!(state.slot_hover, None, "still the pressed slot");
        let back = at + Duration::from_millis(1000);
        state.hover_slot(None, back);
        state.hover_slot(Some(Slot::Add), back);
        assert_eq!(state.slot_hover, Some((Slot::Add, back)));
        state.hover_slot(Some(Slot::Search), back);
        assert_eq!(state.slot_hover, Some((Slot::Search, back)));
    }

    #[test]
    fn chip_fades_in() {
        let at = Instant::now();
        assert_eq!(chip_alpha(at, at), 0.0);
        assert!((chip_alpha(at, at + Duration::from_millis(60)) - 0.5).abs() < 0.01);
        assert_eq!(chip_alpha(at, at + Duration::from_millis(120)), 1.0);
    }

    #[test]
    fn chip_sits_beside_its_anchor() {
        let anchor = Rectangle::new(Point::new(1040.0, 300.0), Size::new(8.0, 40.0));
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let short = chip_rect(bounds, anchor, 320.0, "Stack", None, Edge::Right);
        assert!(short.x + short.width <= anchor.x);
        assert!((short.center().y - 320.0).abs() < 0.01);
        let long = chip_rect(
            bounds,
            anchor,
            320.0,
            "Clipboard is empty",
            None,
            Edge::Right,
        );
        assert!(long.width > short.width);
        assert!((long.x + long.width - (short.x + short.width)).abs() < 0.01);
        let accented = chip_rect(bounds, anchor, 320.0, "Stack", Some(" more"), Edge::Right);
        assert!(accented.width > short.width);
    }

    /// A strip of `notes` with nothing showing beyond its bars.
    fn plain_strip<'a>(
        notes: &'a [Note],
        entries: &'a [Entry],
        magnification: &'a MagnificationState,
        drag: &'a Option<DragState>,
        bars: &'a BarSettings,
    ) -> BarStrip<'a> {
        BarStrip {
            notes,
            entries,
            progress: &[],
            magnification,
            drag,
            scroll_offset: 0.0,
            peek: None,
            bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            default_note_width: 260.0,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
            dimmed: Vec::new(),
            pulse: Vec::new(),
            jump: Vec::new(),
            alert: Vec::new(),
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            actions_progress: 0.0,
            x_offset: 0.0,
            peek_scroll: 0.0,
            edge: Edge::Right,
        }
    }

    const STRIP_BOUNDS: Rectangle = Rectangle {
        x: 1000.0,
        y: 0.0,
        width: STRIP_WIDTH,
        height: 900.0,
    };

    #[test]
    fn slot_tooltip_suppressed_while_panel_open() {
        let notes = [Note::new(crate::note::PALETTE[0])];
        let entries = crate::strip_model::entries(&notes);
        let (magnification, drag, bars) = (MagnificationState::new(), None, BarSettings::default());
        let mut strip = plain_strip(&notes, &entries, &magnification, &drag, &bars);
        let layout = strip.layout_in(STRIP_BOUNDS);
        let at = Instant::now();
        let hover = Some((Slot::Search, at));
        let later = at + Duration::from_millis(700);
        assert!(strip.hint(&layout, hover, later).is_some());
        strip.panel_open = true;
        assert_eq!(strip.hint(&layout, hover, later), None);
        // The toast is the app's to hide; other chips still show.
        strip.clipboard_hint = true;
        assert!(strip.hint(&layout, hover, later).is_some());
    }

    #[test]
    fn hint_hides_everything_with_the_peek() {
        let notes = [Note::new(crate::note::PALETTE[0])];
        let entries = crate::strip_model::entries(&notes);
        let (magnification, drag, bars) = (MagnificationState::new(), None, BarSettings::default());
        let mut strip = plain_strip(&notes, &entries, &magnification, &drag, &bars);
        let layout = strip.layout_in(STRIP_BOUNDS);
        let at = Instant::now();
        let hover = Some((Slot::Add, at));
        let later = at + Duration::from_millis(900);
        assert!(strip.hint(&layout, hover, later).is_some());
        strip.peek = Some((0, 0.5));
        assert_eq!(strip.hint(&layout, hover, later), None, "not with the peek");
        strip.toast = true;
        assert_eq!(strip.hint(&layout, hover, later), None);
        // A peek that has closed again hides nothing.
        strip.peek = Some((0, 0.0));
        assert_eq!(strip.hint(&layout, hover, later), Some(TOAST));
    }

    #[test]
    fn tooltip_due_at_the_redraw_instant_shows() {
        let notes = [Note::new(crate::note::PALETTE[0])];
        let entries = crate::strip_model::entries(&notes);
        let (magnification, drag, bars) = (MagnificationState::new(), None, BarSettings::default());
        let strip = plain_strip(&notes, &entries, &magnification, &drag, &bars);
        let layout = strip.layout_in(STRIP_BOUNDS);
        let mut state = StripState::default();
        // Hovered now; the frame comes exactly when the tooltip is due.
        let at = Instant::now();
        state.hover_slot(Some(Slot::Add), at);
        let due = at + TOOLTIP_DELAY;
        strip.note_frame(&mut state, &layout, due);
        assert!(state.chip_fading(due));
        let (hint, alpha) = strip
            .frame_chip(&state, &layout)
            .expect("the tooltip shows");
        assert_eq!(hint.text, ADD_TOOLTIP);
        assert_eq!(alpha, 0.0, "fading in from that frame");
        strip.note_frame(&mut state, &layout, due + CHIP_FADE / 2);
        let (_, alpha) = strip.frame_chip(&state, &layout).unwrap();
        assert!((alpha - 0.5).abs() < 0.01);
        assert!(
            state.tooltip_due(due).is_none(),
            "no redraw left to wait for"
        );
    }

    #[test]
    fn chip_stays_inside_bounds() {
        let bounds = Rectangle::new(Point::new(1000.0, 100.0), Size::new(STRIP_WIDTH, 400.0));
        let top = Rectangle::new(Point::new(1040.0, 100.0), Size::new(8.0, 4.0));
        let chip = chip_rect(bounds, top, 101.0, "Stack", None, Edge::Right);
        assert_eq!(chip.y, bounds.y);
        let bottom = Rectangle::new(Point::new(1040.0, 496.0), Size::new(8.0, 4.0));
        let chip = chip_rect(bounds, bottom, 499.0, "Stack", None, Edge::Right);
        assert!((chip.y + chip.height - (bounds.y + bounds.height)).abs() < 0.01);
        // Centred where there is room.
        let chip = chip_rect(bounds, top, 300.0, "Stack", None, Edge::Right);
        assert!((chip.center_y() - 300.0).abs() < 0.01);
    }

    #[test]
    fn toast_stays_until_a_drag_moves() {
        let notes = [
            Note::new(crate::note::PALETTE[0]),
            Note::new(crate::note::PALETTE[1]),
        ];
        let entries = crate::strip_model::entries(&notes);
        let (magnification, bars) = (MagnificationState::new(), BarSettings::default());
        let y = 400.0;
        let still = Some(DragState {
            bar_index: 1,
            origin_y: y,
            current_y: y + 2.0,
        });
        let mut strip = plain_strip(&notes, &entries, &magnification, &still, &bars);
        strip.toast = true;
        let layout = strip.layout_in(STRIP_BOUNDS);
        let toast = toast_rect(&layout, STRIP_BOUNDS);
        assert_eq!(strip.hint(&layout, None, Instant::now()), Some(TOAST));
        assert!(strip.toast_press(STRIP_BOUNDS, toast.center()).is_some());
        let moved = Some(DragState {
            bar_index: 1,
            origin_y: y,
            current_y: y + 40.0,
        });
        strip.drag = &moved;
        assert_ne!(strip.hint(&layout, None, Instant::now()), Some(TOAST));
        assert!(strip.toast_press(STRIP_BOUNDS, toast.center()).is_none());
    }

    #[test]
    fn toast_click_sends_undo() {
        let notes = [crate::note::Note::new(crate::note::PALETTE[0])];
        let entries = crate::strip_model::entries(&notes);
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let mut strip = BarStrip {
            notes: &notes,
            entries: &entries,
            progress: &[],
            magnification: &magnification,
            drag: &drag,
            scroll_offset: 0.0,
            peek: None,
            bars: &bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            default_note_width: 260.0,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
            dimmed: Vec::new(),
            pulse: Vec::new(),
            jump: Vec::new(),
            alert: Vec::new(),
            add_shake: 0.0,
            clipboard_hint: false,
            toast: true,
            panel_open: false,
            actions_expanded: false,
            actions_progress: 0.0,
            x_offset: 0.0,
            peek_scroll: 0.0,
            edge: Edge::Right,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let toast = toast_rect(&strip.layout_in(bounds), bounds);
        // The chip sits left of `+` and reads "Note deleted · Undo".
        let add = strip.layout_in(bounds).add_button;
        assert!(toast.x + toast.width <= add.x);
        assert_eq!(format!("{TOAST_TEXT}{TOAST_ACCENT}"), "Note deleted · Undo");
        // The whole chip undoes, its label as much as its edges.
        for pos in [
            toast.center(),
            Point::new(toast.x + 1.0, toast.y + 1.0),
            Point::new(toast.x + toast.width - 1.0, toast.y + toast.height - 1.0),
        ] {
            assert!(matches!(
                strip.toast_press(bounds, pos),
                Some(Message::UndoDelete)
            ));
        }
        assert!(strip
            .toast_press(bounds, Point::new(toast.x - 2.0, toast.center_y()))
            .is_none());
        // Hidden (and so not clickable) while the peek shows.
        strip.peek = Some((0, 0.5));
        assert!(strip.toast_press(bounds, toast.center()).is_none());
        strip.peek = None;
        strip.toast = false;
        assert!(strip.toast_press(bounds, toast.center()).is_none());
    }

    #[test]
    fn stack_peek_rows_open_their_notes() {
        let notes = [
            crate::note::Note::new(crate::note::PALETTE[0]),
            crate::note::Note::new(crate::note::PALETTE[1]),
        ];
        let mut notes = notes;
        notes[1].stack = Some(notes[0].id);
        let entries = crate::strip_model::entries(&notes);
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let strip = BarStrip {
            notes: &notes,
            entries: &entries,
            progress: &[],
            magnification: &magnification,
            drag: &drag,
            scroll_offset: 0.0,
            peek: Some((0, 1.0)),
            bars: &bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            default_note_width: 260.0,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
            dimmed: Vec::new(),
            pulse: Vec::new(),
            jump: Vec::new(),
            alert: Vec::new(),
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            actions_progress: 0.0,
            x_offset: 0.0,
            peek_scroll: 0.0,
            edge: Edge::Right,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let bar = strip.layout_in(bounds).bars[0];
        let (_, width, text) = strip.peek_parts(0, bar).unwrap();
        let parts = peek_layout(bar, bounds, 1.0, &text, width, Edge::Right);
        let press = |pos| strip.peek_press(bounds, pos);
        let member = notes[1].id;
        assert!(matches!(
            press(parts.stack_rows[1].center()),
            Some(Some(Message::OpenStackMember(id))) if id == member
        ));
        let top = notes[0].id;
        assert!(matches!(
            press(parts.stack_rows[0].center()),
            Some(Some(Message::OpenStackMember(id))) if id == top
        ));
        assert!(matches!(
            press(parts.unstack.unwrap().center()),
            Some(Some(Message::Unstack(0)))
        ));
        assert!(matches!(
            press(parts.title),
            Some(Some(Message::BarClicked(0)))
        ));
    }

    #[test]
    fn long_stack_peek_more_row_does_nothing() {
        let mut notes: Vec<_> = (0..31)
            .map(|_| crate::note::Note::new(crate::note::PALETTE[0]))
            .collect();
        let top = notes[0].id;
        for n in &mut notes[1..] {
            n.stack = Some(top);
        }
        let entries = crate::strip_model::entries(&notes);
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let strip = BarStrip {
            notes: &notes,
            entries: &entries,
            progress: &[],
            magnification: &magnification,
            drag: &drag,
            scroll_offset: 0.0,
            peek: Some((0, 1.0)),
            bars: &bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            default_note_width: 260.0,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
            dimmed: Vec::new(),
            pulse: Vec::new(),
            jump: Vec::new(),
            alert: Vec::new(),
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            actions_progress: 0.0,
            x_offset: 0.0,
            peek_scroll: 0.0,
            edge: Edge::Right,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let bar = strip.layout_in(bounds).bars[0];
        let (_, width, text) = strip.peek_parts(0, bar).unwrap();
        let parts = peek_layout(bar, bounds, 1.0, &text, width, Edge::Right);
        let more = parts.stack_more.unwrap();
        assert!(matches!(
            strip.peek_press(bounds, more.center()),
            Some(None)
        ));
        assert!(matches!(
            strip.peek_press(bounds, parts.unstack.unwrap().center()),
            Some(Some(Message::Unstack(0)))
        ));
    }

    #[test]
    fn peek_target_finds_open_peek() {
        let bar = Rectangle::new(Point::new(48.0, 400.0), Size::new(6.0, 30.0));
        let strip = Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, 900.0));
        let note = crate::note::Note::new(crate::note::PALETTE[0]);
        let rect = peek_target(bar, strip, &peek_text(&note, 260.0), 260.0, Edge::Right);
        assert!(rect.width > STRIP_WIDTH);
        assert!(rect.contains(Point::new(bar.x - 100.0, bar.center().y)));
    }

    #[test]
    fn narrow_window_peek_target_keeps_minimum_width() {
        let notes = [crate::note::Note::new(crate::note::PALETTE[0])];
        let entries = crate::strip_model::entries(&notes);
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let strip = BarStrip {
            notes: &notes,
            entries: &entries,
            progress: &[],
            magnification: &magnification,
            drag: &drag,
            scroll_offset: 0.0,
            peek: Some((0, 1.0)),
            bars: &bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            default_note_width: 500.0,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
            dimmed: Vec::new(),
            pulse: Vec::new(),
            jump: Vec::new(),
            alert: Vec::new(),
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            actions_progress: 0.0,
            x_offset: 0.0,
            peek_scroll: 0.0,
            edge: Edge::Right,
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, 900.0));
        let bar = strip.layout_in(bounds).bars[0];
        let width = note_peek_width(&notes[0], strip.default_note_width, bar, Edge::Right);
        assert_eq!(
            peek_target(
                bar,
                bounds,
                &peek_text(&notes[0], width),
                width,
                Edge::Right
            )
            .width,
            260.0
        );
        let inside = Point::new(bar.x + bar.width - 100.0, bar.center().y);
        assert_eq!(strip.peek_hit(bounds, inside), Some(0));
    }

    #[test]
    fn wheel_on_a_long_peek_scrolls_its_body() {
        let mut long = Note::new(crate::note::PALETTE[0]);
        long.content = "a\nb\nc\nd\ne\nf".into();
        let notes = [long, Note::new(crate::note::PALETTE[1])];
        let entries = crate::strip_model::entries(&notes);
        let (magnification, drag, bars) = (MagnificationState::new(), None, BarSettings::default());
        let mut strip = plain_strip(&notes, &entries, &magnification, &drag, &bars);
        let bar = strip.layout_in(STRIP_BOUNDS).bars[0];
        let on_peek = Point::new(bar.x - 100.0, bar.center().y);
        assert_eq!(strip.peek_wheel(STRIP_BOUNDS, on_peek, -30.0), None);

        strip.peek = Some((0, 1.0));
        assert_eq!(strip.peek_wheel(STRIP_BOUNDS, on_peek, -30.0), Some(30.0));
        strip.peek_scroll = 30.0;
        assert_eq!(strip.peek_wheel(STRIP_BOUNDS, on_peek, 30.0), Some(0.0));
        let (_, _, text) = strip.peek_parts(0, bar).unwrap();
        assert_eq!(
            strip.peek_wheel(STRIP_BOUNDS, on_peek, -10_000.0),
            Some(max_body_scroll(&text))
        );
        let off_peek = Point::new(bar.x - 600.0, bar.center().y);
        assert_eq!(strip.peek_wheel(STRIP_BOUNDS, off_peek, -30.0), None);

        // A note that fits leaves the wheel to the strip.
        strip.peek = Some((1, 1.0));
        let short_bar = strip.layout_in(STRIP_BOUNDS).bars[1];
        let on_short = Point::new(short_bar.x - 100.0, short_bar.center().y);
        assert_eq!(strip.peek_wheel(STRIP_BOUNDS, on_short, -30.0), None);
    }

    #[test]
    fn peek_hit_covers_open_peek_only() {
        let notes = [crate::note::Note::new(crate::note::PALETTE[0])];
        let entries = crate::strip_model::entries(&notes);
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let strip = |peek| BarStrip {
            notes: &notes,
            entries: &entries,
            progress: &[],
            magnification: &magnification,
            drag: &drag,
            scroll_offset: 0.0,
            peek,
            bars: &bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            default_note_width: 260.0,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
            dimmed: Vec::new(),
            pulse: Vec::new(),
            jump: Vec::new(),
            alert: Vec::new(),
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            actions_progress: 0.0,
            x_offset: 0.0,
            peek_scroll: 0.0,
            edge: Edge::Right,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let bar = strip(None).layout_in(bounds).bars[0];
        let on_peek = Point::new(bar.x - 100.0, bar.center().y);
        assert_eq!(strip(Some((0, 1.0))).peek_hit(bounds, on_peek), Some(0));
        assert_eq!(strip(None).peek_hit(bounds, on_peek), None);
        let far = Point::new(bar.x - 600.0, bar.center().y);
        assert_eq!(strip(Some((0, 1.0))).peek_hit(bounds, far), None);
    }

    #[test]
    fn bars_hold_still_while_the_cursor_is_on_the_peek() {
        let notes = [
            crate::note::Note::new(crate::note::PALETTE[0]),
            crate::note::Note::new(crate::note::PALETTE[1]),
        ];
        let entries = crate::strip_model::entries(&notes);
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let strip = |peek| BarStrip {
            notes: &notes,
            entries: &entries,
            progress: &[],
            magnification: &magnification,
            drag: &drag,
            scroll_offset: 0.0,
            peek,
            bars: &bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            default_note_width: 260.0,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
            dimmed: Vec::new(),
            pulse: Vec::new(),
            jump: Vec::new(),
            alert: Vec::new(),
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            actions_progress: 0.0,
            x_offset: 0.0,
            peek_scroll: 0.0,
            edge: Edge::Right,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let bar = strip(None).layout_in(bounds).bars[0];
        let peek = peek_target(
            bar,
            bounds,
            &peek_text(&notes[0], 260.0),
            260.0,
            Edge::Right,
        );
        // The peek's top right corner (where its trash sits) lies inside the
        // strip, above the bar it grew from.
        let on_peek_in_strip = Point::new(peek.x + peek.width - 4.0, peek.y + 4.0);
        assert!(bounds.contains(on_peek_in_strip));
        assert!(strip(Some((0, 1.0)))
            .hover_message(bounds, on_peek_in_strip)
            .is_none());

        // Without a peek the same spot magnifies the bars as usual.
        assert!(matches!(
            strip(None).hover_message(bounds, on_peek_in_strip),
            Some(Message::StripHover(Some(_)))
        ));
        // Off the peek, inside the strip: hover resumes.
        let below = strip(None).layout_in(bounds).bars[1];
        let off_peek = Point::new(below.center().x, peek.y + peek.height + 20.0);
        assert!(matches!(
            strip(Some((0, 1.0))).hover_message(bounds, off_peek),
            Some(Message::StripHover(Some(_)))
        ));
        // Outside the strip the hover ends.
        assert!(matches!(
            strip(Some((0, 1.0))).hover_message(bounds, Point::new(10.0, 10.0)),
            Some(Message::StripHover(None))
        ));
    }

    #[test]
    fn expanded_actions_sit_side_by_side() {
        for count in [0, 1, 5] {
            let l = layout_expanded(count, |_| 1.0, 900.0, 0.0);
            assert!(l.actions_expanded);
            assert!((l.add_button.y - l.search_button.y).abs() < 0.01);
            assert!((l.search_button.y - l.settings_button.y).abs() < 0.01);
            assert!(l.settings_button.x + l.settings_button.width <= l.search_button.x);
            assert!(l.search_button.x + l.search_button.width <= l.add_button.x);
            assert!(matches!(
                l.slot_message(l.add_button.center()),
                Some(Message::AddNote)
            ));
            assert!(matches!(
                l.slot_message(l.search_button.center()),
                Some(Message::ToggleSearch)
            ));
            assert!(matches!(
                l.slot_message(l.settings_button.center()),
                Some(Message::ToggleSettings)
            ));
            assert_eq!(l.settings_anchor(), l.settings_button);
            assert_eq!(l.search_anchor(), l.search_button);
        }
    }

    #[test]
    fn sink_slot_drops_one_pixel_when_pressed() {
        let r = Rectangle::new(Point::new(10.0, 20.0), Size::new(24.0, 16.0));
        assert_eq!(sink_slot(r, false), r);
        assert_eq!(
            sink_slot(r, true),
            Rectangle::new(Point::new(10.0, 21.0), Size::new(24.0, 16.0))
        );
    }

    #[test]
    fn expanded_actions_hit_area_covers_settings() {
        // Regression: Search/Settings sit further into the screen than the
        // strip band once slots magnify. The hover hit area must cover them,
        // or moving toward Settings collapses the row before a click lands.
        let l = layout_expanded(3, |i| if i >= 3 { 2.5 } else { 1.0 }, 900.0, 0.0);
        assert!(
            l.actions_button.width > STRIP_WIDTH,
            "precondition: magnified row overflows the strip ({})",
            l.actions_button.width
        );
        assert!(
            l.over_actions(l.settings_button.center()),
            "{:?}",
            l.settings_button
        );
        assert!(
            l.over_actions(l.search_button.center()),
            "{:?}",
            l.search_button
        );
        assert!(l.over_actions(l.add_button.center()), "{:?}", l.add_button);
        assert!(l.settings_hit_area.contains(l.settings_button.center()));
        assert!(
            l.actions_hit_area.width + 0.01 >= l.actions_button.width,
            "hit {:?} row {:?}",
            l.actions_hit_area,
            l.actions_button
        );
    }

    #[test]
    fn collapsed_actions_anchor_panels() {
        let l = layout(2, |_| 1.0, 900.0, 0.0);
        assert_eq!(l.settings_anchor(), l.actions_button);
        assert_eq!(l.search_anchor(), l.actions_button);
        assert!(l.over_actions(l.actions_button.center()));
        assert!(l.slot_message(l.actions_button.center()).is_none());
    }

    #[test]
    fn magnified_gear_uses_its_own_scale() {
        let d = BarSettings::default();
        let l = layout_expanded(2, |i| if i == 4 { 4.0 } else { 1.0 }, 900.0, 0.0);
        assert!((l.settings_button.width - d.width * SLOT_AWAY_SCALE * 4.0).abs() < 0.01);
        let chip = Size::new(d.width * SLOT_AWAY_SCALE, d.height * SLOT_ALONG_SCALE);
        assert_eq!(l.add_button.size(), chip);
        assert_eq!(l.search_button.size(), chip);
    }

    #[test]
    fn pinned_notch_is_three_px() {
        assert_eq!(NOTCH_HEIGHT, 3.0);
    }

    #[test]
    fn alt_click_on_add_makes_clipboard_note() {
        let l = layout_expanded(2, |_| 1.0, 900.0, 0.0);
        let alt = iced::keyboard::Modifiers::ALT;
        let none = iced::keyboard::Modifiers::empty();
        let add = l.add_button.center();
        assert!(matches!(
            l.press_message(add, alt),
            Some(Message::ClipboardNote)
        ));
        assert!(matches!(l.press_message(add, none), Some(Message::AddNote)));
        assert!(matches!(
            l.press_message(l.search_button.center(), alt),
            Some(Message::ToggleSearch)
        ));
    }

    #[test]
    fn alt_resets_when_the_cursor_leaves_or_focus_goes() {
        let l = layout_expanded(2, |_| 1.0, 900.0, 0.0);
        let add = l.add_button.center();
        let alt = Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::ALT));
        for reset in [
            Event::Mouse(mouse::Event::CursorLeft),
            Event::Window(iced::window::Event::Unfocused),
        ] {
            let mut state = StripState::default();
            state.observe(&alt);
            assert!(matches!(
                l.press_message(add, state.modifiers),
                Some(Message::ClipboardNote)
            ));
            state.observe(&reset);
            assert!(matches!(
                l.press_message(add, state.modifiers),
                Some(Message::AddNote)
            ));
        }
    }

    #[test]
    fn magnified_search_slot_uses_its_own_scale() {
        let d = BarSettings::default();
        let l = layout_expanded(2, |i| if i == 3 { 4.0 } else { 1.0 }, 900.0, 0.0);
        assert!((l.search_button.width - d.width * SLOT_AWAY_SCALE * 4.0).abs() < 0.01);
        assert_eq!(
            l.add_button.size(),
            Size::new(d.width * SLOT_AWAY_SCALE, d.height * SLOT_ALONG_SCALE)
        );
    }

    #[test]
    fn strip_ignores_clicks_while_offset() {
        let notes = [crate::note::Note::new(crate::note::PALETTE[0])];
        let entries = crate::strip_model::entries(&notes);
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let mut strip = plain_strip(&notes, &entries, &magnification, &drag, &bars);
        strip.toast = true;
        let layout = strip.layout_in(STRIP_BOUNDS);
        let bar = layout.bars[0].center();
        let actions = layout.actions_button.center();
        let toast = toast_rect(&layout, STRIP_BOUNDS).center();
        let none = keyboard::Modifiers::default();
        assert!(matches!(
            strip.left_press(STRIP_BOUNDS, bar, none),
            Some(Some(Message::DragStart(0, _)))
        ));
        // Collapsed Actions only expands on hover; presses do nothing.
        assert!(strip.left_press(STRIP_BOUNDS, actions, none).is_none());
        assert!(strip.left_press(STRIP_BOUNDS, toast, none).is_some());

        // Sliding away (or back), nothing takes a click; hover clears so an
        // alerting bar's settle can resume when the cursor leaves it.
        for x_offset in [0.5, HIDE_SHIFT] {
            strip.x_offset = x_offset;
            for pos in [bar, actions, toast] {
                assert!(strip.left_press(STRIP_BOUNDS, pos, none).is_none());
                assert!(matches!(
                    strip.hover_message(STRIP_BOUNDS, pos),
                    Some(Message::StripHover(None))
                ));
            }
        }

        // Fully hidden, every bar and slot, with the open bar's shadow,
        // lies past the right edge.
        let right = STRIP_BOUNDS.x + STRIP_BOUNDS.width;
        let rects = layout
            .bars
            .iter()
            .map(|bar| bar_rect_for(1.0, *bar, Edge::Right))
            .chain([layout.actions_button]);
        for rect in rects {
            assert!(rect.x + HIDE_SHIFT >= right + 24.0, "{rect:?}");
        }
    }

    fn edge_bounds(edge: Edge) -> Rectangle {
        match edge {
            Edge::Right => STRIP_BOUNDS,
            Edge::Left => Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, 900.0)),
            Edge::Top => Rectangle::new(Point::ORIGIN, Size::new(900.0, STRIP_WIDTH)),
        }
    }

    const SCALES: [f32; 6] = [1.0, 2.3, 3.7, 1.6, 2.9, 1.15];

    fn edge_layout(edge: Edge, expanded: bool) -> StripLayout {
        let b = edge_bounds(edge);
        compute_layout(
            3,
            |i| SCALES[i],
            band(b, 0.8, edge),
            0.0,
            &BarSettings::default(),
            expanded,
            None,
            edge,
        )
    }

    /// Every bar and slot rect of `l`, in the edge-local frame of `edge`.
    fn local_rects(l: &StripLayout, edge: Edge) -> Vec<LocalRect> {
        let frame = Frame {
            edge,
            bounds: edge_bounds(edge),
        };
        l.bars
            .iter()
            .chain([
                &l.actions_button,
                &l.actions_hit_area,
                &l.add_button,
                &l.add_hit_area,
                &l.search_button,
                &l.search_hit_area,
                &l.settings_button,
                &l.settings_hit_area,
            ])
            .map(|r| frame.local(*r))
            .collect()
    }

    #[test]
    fn layout_is_edge_equivalent() {
        for expanded in [false, true] {
            let right = local_rects(&edge_layout(Edge::Right, expanded), Edge::Right);
            for edge in [Edge::Left, Edge::Top] {
                let l = edge_layout(edge, expanded);
                assert_eq!(l.edge, edge);
                let other = local_rects(&l, edge);
                assert_eq!(right.len(), other.len());
                for (a, b) in right.iter().zip(&other) {
                    for (x, y) in [
                        (a.along, b.along),
                        (a.away, b.away),
                        (a.length, b.length),
                        (a.thickness, b.thickness),
                    ] {
                        assert!((x - y).abs() < 1e-3, "{edge:?} {a:?} vs {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn right_layout_unchanged() {
        let close = |a: Rectangle, (x, y, w, h): (f32, f32, f32, f32)| {
            assert!(
                (a.x - x).abs() < 1e-4
                    && (a.y - y).abs() < 1e-4
                    && (a.width - w).abs() < 1e-4
                    && (a.height - h).abs() < 1e-4,
                "{a:?} vs {:?}",
                (x, y, w, h)
            );
        };
        // Recorded for squat action chips (1.5× bar width, half bar height).
        let bars = [
            (1044.0, 262.3, 10.0, 42.0),
            (1031.0, 320.3, 23.0, 96.6),
            (1017.0, 432.9, 37.0, 155.40001),
        ];
        let collapsed = edge_layout(Edge::Right, false);
        for (bar, want) in collapsed.bars.iter().zip(bars) {
            close(*bar, want);
        }
        close(collapsed.actions_button, (1030.0, 608.3, 24.0, 29.4));
        close(collapsed.actions_hit_area, (1000.0, 598.3, 64.0, 55.4));
        close(collapsed.add_hit_area, (1000.0, 598.3, 64.0, 55.4));
        assert_eq!(collapsed.max_scroll, 0.0);

        let expanded = edge_layout(Edge::Right, true);
        for (bar, want) in expanded.bars.iter().zip(bars) {
            close(*bar, want);
        }
        close(expanded.actions_button, (961.25, 608.3, 92.75, 29.4));
        close(expanded.actions_hit_area, (953.25, 598.3, 110.75, 55.4));
        close(expanded.add_button, (1030.0, 608.3, 24.0, 29.4));
        close(expanded.add_hit_area, (1028.0, 598.3, 36.0, 55.400024));
        close(expanded.search_button, (982.5, 608.3, 43.5, 29.4));
        close(expanded.search_hit_area, (980.5, 598.3, 47.5, 55.400024));
        close(expanded.settings_button, (961.25, 610.925, 17.25, 24.15));
        close(
            expanded.settings_hit_area,
            (961.25, 598.3, 19.25, 55.400024),
        );

        // Overflowing, scrolled and with a bar collapsing.
        let over = compute_layout(
            12,
            |i| 1.0 + 0.1 * i as f32,
            band(STRIP_BOUNDS, 0.5, Edge::Right),
            50.0,
            &BarSettings::default(),
            false,
            Some((4, 0.5)),
            Edge::Right,
        );
        close(over.bars[0], (1044.0, 191.0, 10.0, 42.0));
        close(over.bars[4], (1040.0, 448.2, 14.0, 29.4));
        close(over.bars[11], (1033.0, 1022.6, 21.0, 88.2));
        close(over.actions_button, (1021.0, 1130.7999, 33.0, 29.4));
        assert!((over.max_scroll - 551.2001).abs() < 1e-3);
    }

    fn three_notes() -> [Note; 3] {
        [
            Note::new(crate::note::PALETTE[0]),
            Note::new(crate::note::PALETTE[1]),
            Note::new(crate::note::PALETTE[2]),
        ]
    }

    #[test]
    fn hit_tests_follow_edge() {
        let notes = three_notes();
        let entries = crate::strip_model::entries(&notes);
        let (magnification, drag, bars) = (MagnificationState::new(), None, BarSettings::default());
        let none = keyboard::Modifiers::default();
        // Where bar 2 and the add slot sit on Right, in the edge-local frame.
        let mut right = plain_strip(&notes, &entries, &magnification, &drag, &bars);
        right.actions_expanded = true;
        right.actions_progress = 1.0;
        let right_frame = Frame {
            edge: Edge::Right,
            bounds: STRIP_BOUNDS,
        };
        let layout = right.layout_in(STRIP_BOUNDS);
        let bar = right_frame.local(layout.bars[2]);
        let add = right_frame.local(layout.add_button);
        for edge in Edge::ALL {
            let mut strip = plain_strip(&notes, &entries, &magnification, &drag, &bars);
            strip.actions_expanded = true;
            strip.actions_progress = 1.0;
            strip.edge = edge;
            let bounds = edge_bounds(edge);
            let frame = Frame { edge, bounds };
            let at = frame.rect(bar).center();
            let along = edge.to_local(at, bounds.size()).along;
            assert!(
                matches!(
                    strip.left_press(bounds, at, none),
                    Some(Some(Message::DragStart(2, a))) if (a - along).abs() < 1e-3
                ),
                "{edge:?}"
            );
            assert!(
                matches!(
                    strip.left_press(bounds, frame.rect(add).center(), none),
                    Some(Some(Message::AddNote))
                ),
                "{edge:?}"
            );
            assert!(
                matches!(
                    strip.hover_message(bounds, at),
                    Some(Message::StripHover(Some(a))) if (a - along).abs() < 1e-3
                ),
                "{edge:?}"
            );
        }
    }

    #[test]
    fn left_bars_hug_the_left_edge() {
        let notes = three_notes();
        let entries = crate::strip_model::entries(&notes);
        let (magnification, bars) = (MagnificationState::new(), BarSettings::default());
        let none = None;
        let mut strip = plain_strip(&notes, &entries, &magnification, &none, &bars);
        strip.edge = Edge::Left;
        let layout = strip.layout_in(edge_bounds(Edge::Left));
        assert_eq!(layout.bars.len(), 3);
        for bar in &layout.bars {
            assert!((bar.x - EDGE_MARGIN).abs() < 1e-3, "{bar:?}");
        }
    }

    #[test]
    fn top_drag_reorders_by_x() {
        let notes = three_notes();
        let entries = crate::strip_model::entries(&notes);
        let (magnification, bars) = (MagnificationState::new(), BarSettings::default());
        let bounds = edge_bounds(Edge::Top);
        let drag = |current_y| {
            Some(DragState {
                bar_index: 0,
                origin_y: 0.0,
                current_y,
            })
        };
        let none = None;
        let mut strip = plain_strip(&notes, &entries, &magnification, &none, &bars);
        strip.edge = Edge::Top;
        let layout = strip.layout_in(bounds);
        let row = &layout.bars;
        // A row along x, all hugging the top.
        assert!(row[0].x + row[0].width < row[1].x && row[1].x + row[1].width < row[2].x);
        assert!(row.iter().all(|b| (b.y - EDGE_MARGIN).abs() < 1e-3));
        let mid = row[1].center_x();
        assert_eq!(stack_target(row, 0, mid, Edge::Top), Some(1));
        assert_eq!(
            drop_zone(row[1], row[1].x + 1.0, Edge::Top),
            DropZone::Before
        );
        assert_eq!(
            drop_zone(row[1], row[1].x + row[1].width - 1.0, Edge::Top),
            DropZone::After
        );
        let over = drag(mid);
        assert_eq!(
            drag_hint(row, over.as_ref(), Edge::Top).map(|h| h.at),
            Some(HintAt::Bar(1))
        );
        // Past bar 2's middle along x, the drop goes after it.
        let past = drag(row[2].center_x() + 1.0).unwrap();
        assert_eq!(strip.insertion_index(&past, row), 3);
        let before = drag(row[1].x - 1.0).unwrap();
        assert_eq!(strip.insertion_index(&before, row), 1);
    }

    #[test]
    fn top_wheel_scrolls_along() {
        let notes = three_notes();
        let entries = crate::strip_model::entries(&notes);
        let (magnification, drag, bars) = (MagnificationState::new(), None, BarSettings::default());
        let scroll = |edge: Edge, delta| {
            let mut strip = plain_strip(&notes, &entries, &magnification, &drag, &bars);
            strip.edge = edge;
            let bounds = edge_bounds(edge);
            match strip.wheel(bounds, bounds.center(), delta) {
                (Some(Message::StripScroll(d)), false) => Some(d),
                _ => None,
            }
        };
        let vertical = mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
        let right = scroll(Edge::Right, vertical);
        assert_eq!(right, Some(-30.0));
        assert_eq!(scroll(Edge::Top, vertical), right);
        assert_eq!(scroll(Edge::Left, vertical), right);
        // On Top a horizontal wheel scrolls along it too.
        let horizontal = mouse::ScrollDelta::Pixels { x: -12.0, y: 0.0 };
        assert_eq!(scroll(Edge::Top, horizontal), Some(-12.0));
        assert_eq!(scroll(Edge::Right, horizontal), Some(0.0));
    }

    #[test]
    fn notch_and_progress_sides() {
        // A bar on Top runs along x.
        let top = Rectangle::new(Point::new(100.0, 10.0), Size::new(42.0, 10.0));
        let notch = start_cap(top, NOTCH_HEIGHT, Edge::Top);
        assert_eq!((notch.x, notch.width), (top.x, NOTCH_HEIGHT));
        assert!(notch.y > top.y && notch.y + notch.height < top.y + top.height);
        let fill = progress_fill(top, 1, 4, Edge::Top);
        assert_eq!(fill.x + fill.width, top.x + top.width);
        assert_eq!((fill.width, fill.y, fill.height), (10.5, top.y, top.height));
        let open = bar_rect_for(1.0, top, Edge::Top);
        assert_eq!((open.y, open.height), (top.y, top.height * 1.5));

        // On Right the notch caps the bar's top.
        let right = Rectangle::new(Point::new(40.0, 100.0), Size::new(10.0, 42.0));
        let notch = start_cap(right, NOTCH_HEIGHT, Edge::Right);
        assert_eq!((notch.y, notch.height), (right.y, NOTCH_HEIGHT));

        // On Left the stack edges sit right of the bar, and an open bar
        // keeps its left edge.
        let left = Rectangle::new(Point::new(10.0, 100.0), Size::new(8.0, 40.0));
        let [a, b] = stack_edges(left, Edge::Left);
        assert!(a.x > left.x + left.width && b.x > a.x + a.width);
        assert!(b.height < a.height && a.height < left.height);
        let open = bar_rect_for(1.0, left, Edge::Left);
        assert_eq!((open.x, open.width), (left.x, left.width * 1.5));
        // On Top they sit below it.
        let [a, b] = stack_edges(top, Edge::Top);
        assert!(a.y > top.y + top.height && b.y > a.y + a.height);
        assert!(b.width < a.width && a.width < top.width);
    }

    #[test]
    fn chips_sit_on_the_away_side() {
        let left = Rectangle::new(Point::new(10.0, 300.0), Size::new(8.0, 40.0));
        let bounds = edge_bounds(Edge::Left);
        let chip = chip_rect(bounds, left, 320.0, "Stack", None, Edge::Left);
        assert!(chip.x >= left.x + left.width);
        assert!((chip.center_y() - 320.0).abs() < 0.01);
        let top = Rectangle::new(Point::new(300.0, 10.0), Size::new(40.0, 8.0));
        let bounds = edge_bounds(Edge::Top);
        let chip = chip_rect(bounds, top, 320.0, "Stack", None, Edge::Top);
        assert!(chip.y >= top.y + top.height);
        assert!((chip.center_x() - 320.0).abs() < 0.01);
    }

    #[test]
    fn hidden_strip_moves_toward_its_edge() {
        assert_eq!(hide_translation(Edge::Right, 5.0), Vector::new(5.0, 0.0));
        assert_eq!(hide_translation(Edge::Left, 5.0), Vector::new(-5.0, 0.0));
        assert_eq!(hide_translation(Edge::Top, 5.0), Vector::new(0.0, -5.0));
    }

    #[test]
    fn top_band_is_centred_along_x() {
        let b = band(
            Rectangle::new(Point::new(100.0, 5.0), Size::new(1000.0, 64.0)),
            0.5,
            Edge::Top,
        );
        assert_eq!(
            b,
            Rectangle::new(Point::new(350.0, 5.0), Size::new(500.0, 64.0))
        );
    }

    #[test]
    fn action_pills_are_solid_and_readable_in_both_modes() {
        for mode in [theme::Mode::Light, theme::Mode::Dark] {
            let theme = theme::Theme::new(mode);
            for hovered in [false, true] {
                let style = slot_style(hovered, &theme);
                // Opaque, so the desktop behind never washes it out.
                assert_eq!(style.fill.a, 1.0, "{mode:?} fill");
                let ratio = theme::contrast(style.icon, style.fill);
                assert!(ratio >= 4.5, "{mode:?} hovered={hovered} icon: {ratio}");
            }
            assert_ne!(
                slot_style(true, &theme).fill,
                slot_style(false, &theme).fill,
                "{mode:?} hover shows"
            );
        }
    }

    #[test]
    fn action_pills_are_visible_at_rest() {
        let mut l = compute_layout(
            3,
            |_| 1.0,
            STRIP_BOUNDS,
            0.0,
            &BarSettings::default(),
            true,
            None,
            Edge::Right,
        );
        // Fully unfolded, the pills sit at full size in their slots.
        assert!(l.actions_settled());
        assert_eq!(
            l.pill_rects(),
            [l.add_button, l.search_button, l.settings_button]
        );
        // Mid-way they are smaller and still between the slot and their spot.
        l.actions_progress = 0.5;
        assert!(!l.actions_settled());
        for (pill, slot) in l.pill_rects().iter().zip(l.pill_slots) {
            assert!(pill.width < slot.width && pill.width > 0.0);
        }
    }
}
