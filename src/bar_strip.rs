use crate::animation::MagnificationState;
use crate::app::{DragState, Message};
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
/// The add button is shaped like a bar and magnifies with them, but its
/// height grows less so it stays a compact "slot" rather than a tall note.
const ADD_MAX_HEIGHT_SCALE: f32 = 1.4;
/// Magnification at which the "+" inside the add bar is fully visible.
const ADD_PLUS_SCALE: f32 = 3.0;
const EDGE_PADDING: f32 = 16.0;
const CORNER_RADIUS: f32 = theme::RADIUS_BAR;
/// The open note's bar is this much wider than a docked one.
const OPEN_BAR_SCALE: f32 = 1.5;
/// Opacity factor for bars of notes that don't match the search.
const DIM_ALPHA: f32 = 0.3;

/// The bottom `done` of `total` share of `rect`, the checklist progress.
pub fn progress_fill(rect: Rectangle, done: usize, total: usize) -> Rectangle {
    let share = if total == 0 {
        0.0
    } else {
        (done as f32 / total as f32).clamp(0.0, 1.0)
    };
    let height = rect.height * share;
    Rectangle {
        y: rect.y + rect.height - height,
        height,
        ..rect
    }
}

/// Factor on a bar's alpha: a checklist bar is faint under its progress
/// fill, and an all-done one dims entirely.
fn progress_alpha(progress: Option<(usize, usize)>) -> f32 {
    match progress {
        Some((done, total)) if total > 0 && done >= total => 0.5,
        Some(_) => 0.45,
        None => 1.0,
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

/// Width of each "card edge" line left of a stack's bar.
const STACK_EDGE_WIDTH: f32 = 2.0;
/// Horizontal step from one stack edge to the next.
const STACK_EDGE_STEP: f32 = 4.0;

/// The two "card edge" lines left of a stack's bar, each further out and
/// shorter than the last.
fn stack_edges(rect: Rectangle) -> [Rectangle; 2] {
    [1, 2].map(|n| {
        let inset = 3.0 * n as f32;
        Rectangle::new(
            Point::new(rect.x - STACK_EDGE_STEP * n as f32, rect.y + inset),
            Size::new(STACK_EDGE_WIDTH, (rect.height - 2.0 * inset).max(0.0)),
        )
    })
}

/// Where a dragged bar would land when dropped at a height over `bar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Before,
    /// The middle half: stacks the dragged note under this bar's.
    Onto,
    After,
}

/// The zone of `bar` a drag released at `y` falls in; above or below the
/// bar counts as its edge.
pub fn drop_zone(bar: Rectangle, y: f32) -> DropZone {
    let quarter = bar.height / 4.0;
    if y < bar.y + quarter {
        DropZone::Before
    } else if y > bar.y + bar.height - quarter {
        DropZone::After
    } else {
        DropZone::Onto
    }
}

/// The bar a drag of bar `dragged` released at `y` stacks onto, if any.
pub fn stack_target(bars: &[Rectangle], dragged: usize, y: f32) -> Option<usize> {
    bars.iter()
        .enumerate()
        .position(|(i, bar)| i != dragged && drop_zone(*bar, y) == DropZone::Onto)
}

/// The bar's rectangle: the open note's bar grows to the left from its right
/// edge as the note unfolds (`progress` 0 is docked, 1 fully open).
pub fn bar_rect_for(progress: f32, rect: Rectangle) -> Rectangle {
    let width = rect.width * (1.0 + (OPEN_BAR_SCALE - 1.0) * progress);
    Rectangle {
        x: rect.x + rect.width - width,
        width,
        ..rect
    }
}

pub struct StripLayout {
    pub bars: Vec<Rectangle>,
    /// Collapsed Actions control, or the bounding box of the expanded row.
    pub actions_button: Rectangle,
    pub actions_hit_area: Rectangle,
    /// Whether the three action children are shown side by side.
    pub actions_expanded: bool,
    pub add_button: Rectangle,
    pub add_hit_area: Rectangle,
    pub search_button: Rectangle,
    pub search_hit_area: Rectangle,
    pub settings_button: Rectangle,
    pub settings_hit_area: Rectangle,
    pub max_scroll: f32,
}

impl StripLayout {
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

    /// Bottom edge of the lowest hit area.
    pub fn hit_bottom(&self) -> f32 {
        self.actions_hit_area.y + self.actions_hit_area.height
    }

    /// What a press at `pos` on one of the slots below the bars does.
    pub fn slot_message(&self, pos: Point) -> Option<Message> {
        if !self.actions_expanded {
            return self
                .actions_hit_area
                .contains(pos)
                .then_some(Message::ToggleActions);
        }
        if self.add_hit_area.contains(pos) {
            Some(Message::AddNote)
        } else if self.search_hit_area.contains(pos) {
            Some(Message::ToggleSearch)
        } else if self.settings_hit_area.contains(pos) {
            Some(Message::ToggleSettings)
        } else if self.actions_hit_area.contains(pos) {
            Some(Message::CloseActions)
        } else {
            None
        }
    }

    /// The slot with a tooltip under `pos`, if any.
    pub fn slot_at(&self, pos: Point) -> Option<Slot> {
        if !self.actions_expanded {
            return self.actions_hit_area.contains(pos).then_some(Slot::Actions);
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

/// A slot below the bars that explains itself after a hover.
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

/// Whether a pressed bar has moved far enough to count as dragged.
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
fn drag_hint(bars: &[Rectangle], drag: Option<&DragState>) -> Option<Hint> {
    let drag = drag.filter(|d| drag_moved(d))?;
    stack_target(bars, drag.bar_index, drag.current_y).map(|onto| Hint {
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

/// A chip around a label of size `label`: left of `anchor`, where the
/// peek grows, and vertically centred on `y`, kept inside `bounds` like
/// the peek.
fn chip_frame(bounds: Rectangle, anchor: Rectangle, y: f32, label: Size) -> Rectangle {
    let size = Size::new(
        label.width + 2.0 * CHIP_PADDING.width,
        label.height + 2.0 * CHIP_PADDING.height,
    );
    let max_y = (bounds.y + bounds.height - size.height).max(bounds.y);
    Rectangle::new(
        Point::new(
            anchor.x - CHIP_GAP - size.width,
            (y - size.height / 2.0).clamp(bounds.y, max_y),
        ),
        size,
    )
}

/// Where the chip labelled `text` (and `accent`) beside `anchor` at `y`
/// is drawn in `bounds`, for hit-testing without a renderer.
pub fn chip_rect(
    bounds: Rectangle,
    anchor: Rectangle,
    y: f32,
    text: &str,
    accent: Option<&str>,
) -> Rectangle {
    let label = chip_paragraph(text, accent, Color::TRANSPARENT).min_bounds();
    chip_frame(bounds, anchor, y, label)
}

/// Draws a hint chip beside `anchor` (a bar or slot) in `bounds`,
/// vertically centred on it: a small card like the peek's paper with
/// `text` at 0.75 ink and `accent` after it at full ink, all at `alpha`.
/// Returns its rect.
pub fn draw_chip(
    renderer: &mut iced::Renderer,
    theme: &theme::Theme,
    bounds: Rectangle,
    anchor: Rectangle,
    text: &str,
    accent: Option<&str>,
    alpha: f32,
) -> Rectangle {
    let paragraph = chip_paragraph(text, accent, theme.ink(alpha));
    let rect = chip_frame(bounds, anchor, anchor.center_y(), paragraph.min_bounds());
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
/// its hit area.
pub fn toast_rect(strip: &StripLayout, bounds: Rectangle) -> Rectangle {
    let anchor = if strip.actions_expanded {
        strip.add_button
    } else {
        strip.actions_button
    };
    chip_rect(bounds, anchor, anchor.center_y(), TOAST.text, TOAST.accent)
}

/// The vertically centered `fraction` of `bounds` the bars are laid out in.
pub fn band(bounds: Rectangle, fraction: f32) -> Rectangle {
    let height = bounds.height * fraction;
    Rectangle::new(
        Point::new(bounds.x, bounds.y + (bounds.height - height) / 2.0),
        Size::new(bounds.width, height),
    )
}

/// Size of a slot (add, search or settings button) at magnification `scale`: it
/// widens like a bar but its height grows less, so it stays compact.
fn slot_size(bars: &BarSettings, scale: f32) -> Size {
    Size::new(
        bars.width * scale,
        bars.height * scale.min(ADD_MAX_HEIGHT_SCALE),
    )
}

/// Lays out the bars plus the Actions slot (collapsed or expanded into New /
/// Search / Settings side by side), centered vertically in `bounds`.
/// Centering on the *current* (magnified) height keeps the hovered bar
/// roughly in place while its neighbours grow. If the stack is taller than
/// the bounds, it is top-aligned and scrolled by `scroll_offset`.
///
/// When collapsed, `scale(count)` magnifies Actions. When expanded,
/// `scale(count)` / `count+1` / `count+2` magnify New / Search / Settings.
///
/// `collapse` is a deleted bar's index and how far it has collapsed (0..=1):
/// its height and one gap next to it shrink by that share.
pub fn compute_layout(
    count: usize,
    scale: impl Fn(usize) -> f32,
    bounds: Rectangle,
    scroll_offset: f32,
    bars_settings: &BarSettings,
    actions_expanded: bool,
    collapse: Option<(usize, f32)>,
) -> StripLayout {
    let gap = bars_settings.gap;
    let shrink = |i: usize| match collapse {
        Some((c, t)) if c == i => 1.0 - t.clamp(0.0, 1.0),
        _ => 1.0,
    };
    let heights: Vec<f32> = (0..count)
        .map(|i| bars_settings.height * scale(i) * shrink(i))
        .collect();
    // Gap `i` sits below bar `i`; the last bar collapses into the gap above.
    let gaps: Vec<f32> = (0..count.saturating_sub(1))
        .map(|i| {
            let owner = match collapse {
                Some((c, _)) if c + 1 == count => i + 1,
                _ => i,
            };
            gap * shrink(owner)
        })
        .collect();
    let bars_height: f32 = heights.iter().sum::<f32>() + gaps.iter().sum::<f32>();
    let add_gap = if count > 0 { ADD_BUTTON_GAP } else { 0.0 };
    let actions_size = slot_size(bars_settings, scale(count));
    let add_size = slot_size(bars_settings, scale(count));
    let search_size = slot_size(bars_settings, scale(count + 1));
    let gear_size = slot_size(bars_settings, scale(count + 2));
    let row_height = if actions_expanded {
        add_size
            .height
            .max(search_size.height)
            .max(gear_size.height)
    } else {
        actions_size.height
    };
    let content_height = bars_height + add_gap + row_height;

    let available = bounds.height - 2.0 * EDGE_PADDING;
    let (mut y, max_scroll) = if content_height <= available {
        (bounds.y + (bounds.height - content_height) / 2.0, 0.0)
    } else {
        let max_scroll = content_height - available;
        (
            bounds.y + EDGE_PADDING - scroll_offset.clamp(0.0, max_scroll),
            max_scroll,
        )
    };

    let right = bounds.x + bounds.width - EDGE_MARGIN;
    let mut bars = Vec::with_capacity(count);
    for (i, h) in heights.iter().enumerate() {
        let w = bars_settings.width * scale(i);
        bars.push(Rectangle::new(Point::new(right - w, y), Size::new(w, *h)));
        y += h + gaps.get(i).copied().unwrap_or(add_gap);
    }

    let row_top = y;
    let actions_button = Rectangle::new(
        Point::new(
            right - actions_size.width,
            row_top + (row_height - actions_size.height) / 2.0,
        ),
        actions_size,
    );
    // Expanded: New, Search, Settings from right to left.
    let add_button = Rectangle::new(
        Point::new(
            right - add_size.width,
            row_top + (row_height - add_size.height) / 2.0,
        ),
        add_size,
    );
    let search_button = Rectangle::new(
        Point::new(
            add_button.x - ACTION_GAP - search_size.width,
            row_top + (row_height - search_size.height) / 2.0,
        ),
        search_size,
    );
    let settings_button = Rectangle::new(
        Point::new(
            search_button.x - ACTION_GAP - gear_size.width,
            row_top + (row_height - gear_size.height) / 2.0,
        ),
        gear_size,
    );
    let row_left = if actions_expanded {
        settings_button.x
    } else {
        actions_button.x
    };
    let actions_hit_area = Rectangle::new(
        Point::new(bounds.x, row_top - add_gap / 2.0),
        Size::new(bounds.width, row_height + add_gap / 2.0 + EDGE_PADDING),
    );
    let slot_hit = |button: Rectangle, left: f32, right_edge: f32| {
        Rectangle::new(
            Point::new(left, actions_hit_area.y),
            Size::new(right_edge - left, actions_hit_area.height),
        )
        .intersection(&actions_hit_area)
        .unwrap_or(button)
    };
    let (add_hit_area, search_hit_area, settings_hit_area) = if actions_expanded {
        let mid_add_search = (search_button.x + search_button.width + add_button.x) / 2.0;
        let mid_search_gear = (settings_button.x + settings_button.width + search_button.x) / 2.0;
        (
            slot_hit(add_button, mid_add_search, bounds.x + bounds.width),
            slot_hit(search_button, mid_search_gear, mid_add_search),
            slot_hit(settings_button, row_left.min(bounds.x), mid_search_gear),
        )
    } else {
        // Collapsed: children share the Actions geometry for panel anchors.
        (actions_hit_area, actions_hit_area, actions_hit_area)
    };
    let (add_button, search_button, settings_button) = if actions_expanded {
        (add_button, search_button, settings_button)
    } else {
        (actions_button, actions_button, actions_button)
    };

    StripLayout {
        bars,
        actions_button: if actions_expanded {
            Rectangle::new(
                Point::new(row_left, row_top),
                Size::new(right - row_left, row_height),
            )
        } else {
            actions_button
        },
        actions_hit_area,
        actions_expanded,
        add_button,
        add_hit_area,
        search_button,
        search_hit_area,
        settings_button,
        settings_hit_area,
        max_scroll,
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
    /// How far right everything is drawn while auto-hide slides the strip
    /// away (0 = in place, `HIDE_SHIFT` = off screen). Above 0 the strip
    /// takes no clicks or hovers.
    pub x_offset: f32,
}

impl<'a> BarStrip<'a> {
    /// Whether the strip is (partly) slid away and so ignores the mouse.
    fn slid(&self) -> bool {
        self.x_offset > 0.0
    }

    /// The note bar `i` shows: its entry's top.
    fn note(&self, i: usize) -> Option<&'a Note> {
        self.notes.get(self.entries.get(i)?.top)
    }

    fn layout_in(&self, bounds: Rectangle) -> StripLayout {
        compute_layout(
            self.entries.len(),
            |i| self.magnification.scale(i),
            band(bounds, self.height_fraction),
            self.scroll_offset,
            self.bars,
            self.actions_expanded,
            self.collapse,
        )
    }

    /// The note bar `i` (at `bar`) peeks, and the peek's width and text.
    fn peek_parts(&self, i: usize, bar: Rectangle) -> Option<(&'a Note, f32, PeekText)> {
        let note = self.note(i)?;
        let width = note_peek_width(note, self.default_note_width, bar);
        let text = entry_peek_text(self.notes, self.entries.get(i)?, width);
        Some((note, width, text))
    }

    /// Bar whose open peek is under `pos`, if any.
    fn peek_hit(&self, bounds: Rectangle, pos: Point) -> Option<usize> {
        let (i, _) = self.peek?;
        let bar = *self.layout_in(bounds).bars.get(i)?;
        let (_, width, text) = self.peek_parts(i, bar)?;
        peek_target(bar, bounds, &text, width)
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
    fn hover_message(&self, bounds: Rectangle, pos: Point) -> Option<Message> {
        if self.slid() || self.peek_hit(bounds, pos).is_some() {
            None
        } else if bounds.contains(pos) {
            Some(Message::StripHover(Some(pos.y)))
        } else {
            Some(Message::StripHover(None))
        }
    }

    /// What a left press at `pos` does: `None` when the strip doesn't take
    /// it, `Some(None)` when it takes it without a message. Nothing while
    /// slid away.
    fn left_press(
        &self,
        bounds: Rectangle,
        pos: Point,
        modifiers: keyboard::Modifiers,
    ) -> Option<Option<Message>> {
        if self.slid() {
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
            return Some(Some(Message::DragStart(i, pos.y)));
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
        let parts = peek_layout(bar, bounds, 1.0, &text, width);
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
            HintKind::Stack => drag_hint(&strip.bars, self.drag.as_ref()),
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

    /// How far a slot at magnification `scale` has revealed its glyph (0..=1).
    fn reveal(scale: f32) -> f32 {
        ((scale - 1.0) / (ADD_PLUS_SCALE - 1.0)).clamp(0.0, 1.0)
    }

    /// The gap the dragged bar drops into, kept within its pin group.
    fn insertion_index(&self, drag: &DragState, bars: &[Rectangle]) -> usize {
        let centers: Vec<f32> = bars.iter().map(|bar| bar.y + bar.height / 2.0).collect();
        let pinned: Vec<bool> = (0..bars.len())
            .map(|i| self.note(i).is_some_and(|note| note.pinned))
            .collect();
        strip_model::insertion_slot(&centers, drag.current_y, &pinned, drag.bar_index)
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
        let bars = &strip.bars;
        let dragging_index = self.drag.as_ref().map(|d| d.bar_index);
        let drag_active = self.drag.as_ref().is_some_and(drag_moved);

        for (i, bar_rect) in bars.iter().enumerate() {
            if let Some((peek_index, progress)) = self.peek {
                if peek_index == i && progress > 0.0 {
                    if let Some((note, width, text)) = self.peek_parts(i, *bar_rect) {
                        draw_peek(
                            renderer,
                            note,
                            &text,
                            *bar_rect,
                            bounds,
                            &self.theme,
                            CORNER_RADIUS,
                            progress,
                            self.paper_tint,
                            cursor.position(),
                            width,
                            self.peek_scroll,
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
                let rect = bar_rect_for(open.map_or(0.0, |(_, p)| p), *bar_rect);
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
                // Open tasks leave the bar faint; done ones fill it from the
                // bottom. An all-done bar just dims.
                if let Some((done, total)) = progress.filter(|(d, t)| d < t) {
                    let fill = progress_fill(rect, done, total);
                    if fill.height > 0.0 {
                        let [r, g, b, _] = note.color.rgba;
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad {
                                bounds: fill,
                                border: iced::Border {
                                    radius: corner.min(fill.height / 2.0).into(),
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
                        bounds: Rectangle::new(
                            Point::new(rect.x + corner, rect.y),
                            Size::new(
                                (rect.width - 2.0 * corner).max(0.0),
                                // A collapsing bar takes its highlight with it.
                                rect.height.min(1.0),
                            ),
                        ),
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
                            bounds: Rectangle::new(
                                Point::new(rect.x + corner, rect.y),
                                Size::new(
                                    (rect.width - 2.0 * corner).max(0.0),
                                    rect.height.min(NOTCH_HEIGHT),
                                ),
                            ),
                            border: Default::default(),
                            shadow: Default::default(),
                            snap: true,
                        },
                        notch_color(Color::from_rgb(r, g, b), alpha),
                    );
                }
                if self.entries.get(i).is_some_and(|e| !e.members.is_empty()) {
                    for edge in stack_edges(rect) {
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad {
                                bounds: edge,
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

        // Actions slot: collapsed brand mark, or New / Search / Settings in a row.
        let idle = self.drag.is_none();
        let count = self.entries.len();
        if strip.actions_expanded {
            let add_reveal = Self::reveal(self.magnification.scale(count));
            let add = strip.add_button + Vector::new(self.add_shake, 0.0);
            draw_slot(
                renderer,
                add,
                idle && cursor.is_over(strip.add_hit_area),
                add_reveal,
                &self.theme,
            );
            if add_reveal > 0.0 {
                draw_plus(
                    renderer,
                    add,
                    self.theme.card().scale_alpha(0.95 * add_reveal),
                );
            }

            let search = strip.search_button;
            let search_reveal = Self::reveal(self.magnification.scale(count + 1));
            draw_slot(
                renderer,
                search,
                idle && cursor.is_over(strip.search_hit_area),
                search_reveal,
                &self.theme,
            );
            if search_reveal > 0.0 {
                draw_search(
                    renderer,
                    search,
                    self.theme.card().scale_alpha(0.95 * search_reveal),
                );
            }

            let gear = strip.settings_button;
            let gear_reveal = Self::reveal(self.magnification.scale(count + 2));
            draw_slot(
                renderer,
                gear,
                idle && cursor.is_over(strip.settings_hit_area),
                gear_reveal,
                &self.theme,
            );
            if gear_reveal > 0.0 {
                let color = self.theme.card().scale_alpha(0.95 * gear_reveal);
                for quad in settings_glyph(gear) {
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
        } else {
            let reveal = Self::reveal(self.magnification.scale(count));
            let actions = strip.actions_button + Vector::new(self.add_shake, 0.0);
            draw_slot(
                renderer,
                actions,
                idle && cursor.is_over(strip.actions_hit_area),
                reveal,
                &self.theme,
            );
            if reveal > 0.0 {
                let color = self.theme.card().scale_alpha(0.95 * reveal);
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
        }

        if let Some(drag) = &self.drag {
            if drag_active {
                if let Some(note) = self.note(drag.bar_index) {
                    let scale = self.magnification.scale(drag.bar_index);
                    let w = self.bars.width * scale;
                    let h = self.bars.height * scale;
                    let x = bounds.x + bounds.width - w;
                    let ghost_y = drag.current_y - h / 2.0;
                    renderer::Renderer::fill_quad(
                        renderer,
                        renderer::Quad {
                            bounds: Rectangle::new(Point::new(x, ghost_y), Size::new(w, h)),
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
                    if let Some(onto) = stack_target(bars, drag.bar_index, drag.current_y) {
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
                        let indicator_y = if target < bars.len() {
                            bars[target].y - self.bars.gap / 2.0
                        } else {
                            bars.last()
                                .map_or(bounds.y, |b| b.y + b.height + self.bars.gap / 2.0)
                        };
                        renderer::Renderer::fill_quad(
                            renderer,
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(bounds.x + bounds.width - 20.0, indicator_y - 1.0),
                                    Size::new(20.0, 2.0),
                                ),
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
        Size::new(Length::Fixed(STRIP_WIDTH), Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits
            .width(Length::Fixed(STRIP_WIDTH))
            .height(Length::Fill);
        let size = limits.resolve(STRIP_WIDTH, f32::INFINITY, Size::new(STRIP_WIDTH, 0.0));
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
        if !self.slid() {
            self.draw_strip(tree, renderer, layout.bounds(), cursor);
            return;
        }
        // Slid away: everything moves right together, and nothing shows a
        // hover.
        renderer::Renderer::with_translation(
            renderer,
            Vector::new(self.x_offset, 0.0),
            |renderer| {
                self.draw_strip(tree, renderer, layout.bounds(), mouse::Cursor::Unavailable);
            },
        );
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
                if let Some(message) = self.hover_message(bounds, *position) {
                    shell.publish(message);
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
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
                if self.drag.is_some() {
                    shell.publish(Message::DragEnd);
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if !self.slid() => {
                let Some(pos) = cursor.position() else {
                    return;
                };
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 30.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                // A peek with more lines than it shows scrolls them, and
                // never the strip behind it.
                if let Some(scroll) = self.peek_wheel(bounds, pos, dy) {
                    if scroll != self.peek_scroll {
                        shell.publish(Message::PeekScroll(scroll));
                    }
                    shell.capture_event();
                } else if bounds.contains(pos) {
                    shell.publish(Message::StripScroll(dy));
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

fn draw_plus(renderer: &mut iced::Renderer, slot: Rectangle, color: Color) {
    let thickness = 2.0;
    let len = (slot.width.min(slot.height) * 0.5).max(thickness);
    let cx = slot.x + slot.width / 2.0;
    let cy = slot.y + slot.height / 2.0;
    for size in [Size::new(len, thickness), Size::new(thickness, len)] {
        renderer::Renderer::fill_quad(
            renderer,
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(cx - size.width / 2.0, cy - size.height / 2.0),
                    size,
                ),
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

fn draw_search(renderer: &mut iced::Renderer, slot: Rectangle, color: Color) {
    let glyph = search_glyph(slot);
    renderer::Renderer::fill_quad(
        renderer,
        renderer::Quad {
            bounds: glyph.ring,
            border: iced::Border {
                radius: (glyph.ring.width / 2.0).into(),
                width: glyph.stroke,
                color,
            },
            shadow: Default::default(),
            snap: true,
        },
        Color::TRANSPARENT,
    );
    renderer::Renderer::fill_quad(
        renderer,
        renderer::Quad {
            bounds: glyph.handle,
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

/// A "sliders" settings icon inside `slot`: three tracks, each with a knob
/// at a different position. Drawn from quads so it needs no icon font.
fn settings_glyph(slot: Rectangle) -> Vec<Rectangle> {
    let track = 1.5_f32.min(slot.height * 0.05);
    let knob = (slot.width.min(slot.height) * 0.22).max(track);
    let width = slot.width * 0.6;
    let left = slot.x + (slot.width - width) / 2.0;
    let mut quads = Vec::with_capacity(6);
    for (row, at) in [(0.3, 0.3), (0.5, 0.7), (0.7, 0.45)] {
        let cy = slot.y + slot.height * row;
        quads.push(Rectangle::new(
            Point::new(left, cy - track / 2.0),
            Size::new(width, track),
        ));
        let kx = (left + width * at - knob / 2.0).clamp(left, left + width - knob);
        quads.push(Rectangle::new(
            Point::new(kx, cy - knob / 2.0),
            Size::new(knob, knob),
        ));
    }
    quads
}

/// A magnifier inside a slot: a ring and a short handle at its lower right.
struct SearchGlyph {
    ring: Rectangle,
    /// The ring's line width.
    stroke: f32,
    handle: Rectangle,
}

/// The magnifier for `slot`, drawn from quads so it needs no icon font.
/// Quads can't rotate, so the handle is an axis-aligned square set
/// diagonally off the ring.
fn search_glyph(slot: Rectangle) -> SearchGlyph {
    let size = slot.width.min(slot.height) * 0.6;
    let stroke = (size * 0.12).clamp(1.0, 2.0);
    let radius = size * 0.36;
    let left = slot.x + (slot.width - size) / 2.0;
    let top = slot.y + (slot.height - size) / 2.0;
    // The handle overlaps the ring where its diagonal leaves it.
    let handle = (stroke * 2.0).min(size * 0.3);
    let at = radius * (1.0 + std::f32::consts::FRAC_1_SQRT_2) - handle * 0.15;
    SearchGlyph {
        ring: Rectangle::new(Point::new(left, top), Size::new(2.0 * radius, 2.0 * radius)),
        stroke,
        handle: Rectangle::new(Point::new(left + at, top + at), Size::new(handle, handle)),
    }
}

/// Where the fully open peek of the note on `bar` sits: the area that
/// keeps it open while hovered and opens the note when clicked.
pub fn peek_target(bar: Rectangle, bounds: Rectangle, text: &PeekText, width: f32) -> Rectangle {
    peek_layout(bar, bounds, 1.0, text, width).rect
}

/// Draws an add/settings slot: a hollow outline that fills in as `reveal`
/// grows, darker while hovered.
fn draw_slot(
    renderer: &mut iced::Renderer,
    rect: Rectangle,
    hovered: bool,
    reveal: f32,
    theme: &theme::Theme,
) {
    let fill_alpha = if hovered { 0.8 } else { 0.1 + 0.4 * reveal };
    renderer::Renderer::fill_quad(
        renderer,
        renderer::Quad {
            bounds: rect,
            border: iced::Border {
                radius: CORNER_RADIUS.into(),
                width: 1.5 * (1.0 - reveal),
                color: theme.ink(0.4),
            },
            shadow: Default::default(),
            snap: true,
        },
        theme.ink(fill_alpha),
    );
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
        assert_eq!(progress_fill(r, 0, 4).height, 0.0);
        let half = progress_fill(r, 2, 4);
        assert_eq!((half.height, half.y + half.height), (20.0, 140.0));
        assert_eq!((half.x, half.width), (r.x, r.width));
        assert_eq!(progress_fill(r, 4, 4), r);
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
    fn all_done_bar_dims() {
        assert_eq!(progress_alpha(Some((3, 3))), 0.5);
        assert_eq!(progress_alpha(Some((1, 3))), 0.45);
        assert_eq!(progress_alpha(None), 1.0);
    }

    #[test]
    fn stack_edges_sit_left_of_the_bar() {
        let r = Rectangle::new(Point::new(40.0, 100.0), Size::new(8.0, 40.0));
        let [a, b] = stack_edges(r);
        // Separate lines with a visible gap between them and the bar.
        assert!(a.x + a.width < r.x && b.x + b.width < a.x);
        assert!(a.width >= 2.0 && b.width >= 2.0);
        assert!(b.height < a.height && a.height < r.height);
    }

    #[test]
    fn open_bar_is_wider_and_keeps_its_right_edge() {
        let r = Rectangle::new(Point::new(40.0, 100.0), Size::new(6.0, 30.0));
        let open = bar_rect_for(1.0, r);
        assert_eq!(open.width, r.width * 1.5);
        assert_eq!(open.x + open.width, r.x + r.width);
        assert_eq!((open.y, open.height), (r.y, r.height));
        assert_eq!(bar_rect_for(0.0, r), r);
        // It widens with the note's morph instead of jumping.
        let half = bar_rect_for(0.5, r);
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
        let with = |collapse| compute_layout(3, |_| 1.0, bounds(900.0), 0.0, &d, false, collapse);
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
        let l = compute_layout(3, |_| 1.0, bounds(900.0), 0.0, &bars, false, None);
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
    fn actions_slot_is_bar_shaped_at_rest() {
        let l = layout(3, |_| 1.0, 900.0, 0.0);
        assert_eq!(l.actions_button.size(), l.bars[0].size());
        assert!(!l.actions_expanded);
    }

    #[test]
    fn magnified_actions_slot_widens_but_stays_compact() {
        let d = BarSettings::default();
        let l = layout(3, |i| if i == 3 { 5.0 } else { 1.0 }, 900.0, 0.0);
        assert!((l.actions_button.width - d.width * 5.0).abs() < 0.01);
        assert!((l.actions_button.height - d.height * ADD_MAX_HEIGHT_SCALE).abs() < 0.01);
    }

    #[test]
    fn actions_hit_area_spans_strip_width() {
        let l = layout(3, |_| 1.0, 900.0, 0.0);
        assert_eq!(l.actions_hit_area.width, STRIP_WIDTH);
        assert!(l
            .actions_hit_area
            .contains(Point::new(2.0, l.actions_button.y + 2.0)));
        assert!(matches!(
            l.slot_message(l.actions_button.center()),
            Some(Message::ToggleActions)
        ));
    }

    #[test]
    fn band_is_centered_share_of_bounds() {
        let b = band(
            Rectangle::new(Point::new(5.0, 100.0), Size::new(64.0, 1000.0)),
            0.5,
        );
        assert_eq!(
            b,
            Rectangle::new(Point::new(5.0, 350.0), Size::new(64.0, 500.0))
        );
        let full = bounds(800.0);
        assert_eq!(band(full, 1.0), full);
    }

    #[test]
    fn settings_glyph_fits_its_slot() {
        let slot = Rectangle::new(Point::new(10.0, 20.0), Size::new(24.0, 36.0));
        let quads = settings_glyph(slot);
        assert!(!quads.is_empty());
        for q in &quads {
            assert!(q.width > 0.0 && q.height > 0.0);
            assert!(slot.contains(q.position()));
            assert!(slot.contains(Point::new(q.x + q.width, q.y + q.height)));
        }
    }

    #[test]
    fn drop_zone_edges() {
        let bar = Rectangle::new(Point::new(40.0, 100.0), Size::new(8.0, 40.0));
        assert_eq!(drop_zone(bar, 90.0), DropZone::Before);
        assert_eq!(drop_zone(bar, 109.0), DropZone::Before);
        assert_eq!(drop_zone(bar, 110.0), DropZone::Onto);
        assert_eq!(drop_zone(bar, 120.0), DropZone::Onto);
        assert_eq!(drop_zone(bar, 130.0), DropZone::Onto);
        assert_eq!(drop_zone(bar, 131.0), DropZone::After);
        assert_eq!(drop_zone(bar, 150.0), DropZone::After);
        let bars = [bar, Rectangle { y: 150.0, ..bar }];
        assert_eq!(stack_target(&bars, 1, 120.0), Some(0));
        assert_eq!(stack_target(&bars, 0, 120.0), None, "not onto itself");
        assert_eq!(stack_target(&bars, 1, 145.0), None, "between bars");
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
            drag_hint(&bars, Some(&drag(120.0))),
            Some(Hint {
                text: "Stack",
                accent: None,
                at: HintAt::Bar(0)
            })
        );
        assert_eq!(drag_hint(&bars, Some(&drag(145.0))), None, "between bars");
        assert_eq!(drag_hint(&bars, None), None);
        // A press that hasn't moved yet is no drag.
        let still = DragState {
            bar_index: 0,
            origin_y: 120.0,
            current_y: 122.0,
        };
        assert_eq!(drag_hint(&bars, Some(&still)), None);
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
        let short = chip_rect(bounds, anchor, 320.0, "Stack", None);
        assert!(short.x + short.width <= anchor.x);
        assert!((short.center().y - 320.0).abs() < 0.01);
        let long = chip_rect(bounds, anchor, 320.0, "Clipboard is empty", None);
        assert!(long.width > short.width);
        assert!((long.x + long.width - (short.x + short.width)).abs() < 0.01);
        let accented = chip_rect(bounds, anchor, 320.0, "Stack", Some(" more"));
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
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            x_offset: 0.0,
            peek_scroll: 0.0,
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
        let chip = chip_rect(bounds, top, 101.0, "Stack", None);
        assert_eq!(chip.y, bounds.y);
        let bottom = Rectangle::new(Point::new(1040.0, 496.0), Size::new(8.0, 4.0));
        let chip = chip_rect(bounds, bottom, 499.0, "Stack", None);
        assert!((chip.y + chip.height - (bounds.y + bounds.height)).abs() < 0.01);
        // Centred where there is room.
        let chip = chip_rect(bounds, top, 300.0, "Stack", None);
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
            add_shake: 0.0,
            clipboard_hint: false,
            toast: true,
            panel_open: false,
            actions_expanded: false,
            x_offset: 0.0,
            peek_scroll: 0.0,
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
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            x_offset: 0.0,
            peek_scroll: 0.0,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let bar = strip.layout_in(bounds).bars[0];
        let (_, width, text) = strip.peek_parts(0, bar).unwrap();
        let parts = peek_layout(bar, bounds, 1.0, &text, width);
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
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            x_offset: 0.0,
            peek_scroll: 0.0,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let bar = strip.layout_in(bounds).bars[0];
        let (_, width, text) = strip.peek_parts(0, bar).unwrap();
        let parts = peek_layout(bar, bounds, 1.0, &text, width);
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
        let rect = peek_target(bar, strip, &peek_text(&note, 260.0), 260.0);
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
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            x_offset: 0.0,
            peek_scroll: 0.0,
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, 900.0));
        let bar = strip.layout_in(bounds).bars[0];
        let width = note_peek_width(&notes[0], strip.default_note_width, bar);
        assert_eq!(
            peek_target(bar, bounds, &peek_text(&notes[0], width), width).width,
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
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            x_offset: 0.0,
            peek_scroll: 0.0,
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
            add_shake: 0.0,
            clipboard_hint: false,
            toast: false,
            panel_open: false,
            actions_expanded: false,
            x_offset: 0.0,
            peek_scroll: 0.0,
        };
        let bounds = Rectangle::new(Point::new(1000.0, 0.0), Size::new(STRIP_WIDTH, 900.0));
        let bar = strip(None).layout_in(bounds).bars[0];
        let peek = peek_target(bar, bounds, &peek_text(&notes[0], 260.0), 260.0);
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
    fn collapsed_actions_anchor_panels() {
        let l = layout(2, |_| 1.0, 900.0, 0.0);
        assert_eq!(l.settings_anchor(), l.actions_button);
        assert_eq!(l.search_anchor(), l.actions_button);
        assert!(matches!(
            l.slot_message(l.actions_button.center()),
            Some(Message::ToggleActions)
        ));
    }

    #[test]
    fn magnified_gear_uses_its_own_scale() {
        let d = BarSettings::default();
        let l = layout_expanded(2, |i| if i == 4 { 4.0 } else { 1.0 }, 900.0, 0.0);
        assert!((l.settings_button.width - d.width * 4.0).abs() < 0.01);
        assert_eq!(l.add_button.size(), l.bars[0].size());
        assert_eq!(l.search_button.size(), l.bars[0].size());
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
        assert!((l.search_button.width - d.width * 4.0).abs() < 0.01);
        assert_eq!(l.add_button.size(), l.bars[0].size());
    }

    #[test]
    fn search_glyph_fits_its_slot() {
        let slot = Rectangle::new(Point::new(10.0, 20.0), Size::new(24.0, 36.0));
        let glyph = search_glyph(slot);
        for q in [glyph.ring, glyph.handle] {
            assert!(q.width > 0.0 && q.height > 0.0);
            assert!(slot.contains(q.position()));
            assert!(slot.contains(Point::new(q.x + q.width, q.y + q.height)));
        }
        assert_eq!(glyph.ring.width, glyph.ring.height);
        // The handle sits at the ring's lower right.
        assert!(glyph.handle.center().x > glyph.ring.center().x);
        assert!(glyph.handle.center().y > glyph.ring.center().y);
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
        let actions = layout.actions_hit_area.center();
        let toast = toast_rect(&layout, STRIP_BOUNDS).center();
        let none = keyboard::Modifiers::default();
        assert!(matches!(
            strip.left_press(STRIP_BOUNDS, bar, none),
            Some(Some(Message::DragStart(0, _)))
        ));
        assert!(strip.left_press(STRIP_BOUNDS, actions, none).is_some());
        assert!(strip.left_press(STRIP_BOUNDS, toast, none).is_some());

        // Sliding away (or back), nothing takes a click or a hover.
        for x_offset in [0.5, HIDE_SHIFT] {
            strip.x_offset = x_offset;
            for pos in [bar, actions, toast] {
                assert!(strip.left_press(STRIP_BOUNDS, pos, none).is_none());
                assert!(strip.hover_message(STRIP_BOUNDS, pos).is_none());
            }
        }

        // Fully hidden, every bar and slot, with the open bar's shadow,
        // lies past the right edge.
        let right = STRIP_BOUNDS.x + STRIP_BOUNDS.width;
        let rects = layout
            .bars
            .iter()
            .map(|bar| bar_rect_for(1.0, *bar))
            .chain([layout.actions_button]);
        for rect in rects {
            assert!(rect.x + HIDE_SHIFT >= right + 24.0, "{rect:?}");
        }
    }
}
