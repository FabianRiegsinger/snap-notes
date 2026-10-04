use crate::animation::MagnificationState;
use crate::app::{DragState, Message};
use crate::note::Note;
use crate::peek::{draw_peek, peek_layout, peek_text};
use crate::settings::BarSettings;
use crate::theme;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::Tree;
use iced::advanced::{self, Clipboard, Shell};
use iced::event::Event;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme};

pub const STRIP_WIDTH: f32 = 64.0;
/// Horizontal space between the screen edge and the bars.
pub const EDGE_MARGIN: f32 = 10.0;
const ADD_BUTTON_GAP: f32 = 20.0;
/// The add button is shaped like a bar and magnifies with them, but its
/// height grows less so it stays a compact "slot" rather than a tall note.
const ADD_MAX_HEIGHT_SCALE: f32 = 1.4;
/// Magnification at which the "+" inside the add bar is fully visible.
const ADD_PLUS_SCALE: f32 = 3.0;
const EDGE_PADDING: f32 = 16.0;
const CORNER_RADIUS: f32 = theme::RADIUS_BAR;
/// The open note's bar is this much wider than a docked one.
const OPEN_BAR_SCALE: f32 = 1.5;

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
    pub add_button: Rectangle,
    /// Generous click target for the add button: the full strip width around it.
    pub add_hit_area: Rectangle,
    /// Settings slot below the add button, drawn and magnified like it.
    /// Only where there is no tray menu to open settings from.
    pub settings_button: Option<Rectangle>,
    pub settings_hit_area: Option<Rectangle>,
    pub max_scroll: f32,
}

impl StripLayout {
    /// Where the settings panel unfolds from: the gear slot, or the add
    /// button where there is none.
    pub fn settings_anchor(&self) -> Rectangle {
        self.settings_button.unwrap_or(self.add_button)
    }

    /// Bottom edge of the lowest hit area.
    pub fn hit_bottom(&self) -> f32 {
        let area = self.settings_hit_area.unwrap_or(self.add_hit_area);
        area.y + area.height
    }
}

/// The strip shows a settings slot only where no tray menu offers Settings.
pub const SETTINGS_SLOT: bool = !cfg!(any(windows, target_os = "macos"));

/// The vertically centered `fraction` of `bounds` the bars are laid out in.
pub fn band(bounds: Rectangle, fraction: f32) -> Rectangle {
    let height = bounds.height * fraction;
    Rectangle::new(
        Point::new(bounds.x, bounds.y + (bounds.height - height) / 2.0),
        Size::new(bounds.width, height),
    )
}

/// Size of a slot (add or settings button) at magnification `scale`: it
/// widens like a bar but its height grows less, so it stays compact.
fn slot_size(bars: &BarSettings, scale: f32) -> Size {
    Size::new(
        bars.width * scale,
        bars.height * scale.min(ADD_MAX_HEIGHT_SCALE),
    )
}

/// Lays out the bars plus the add and settings slots as one stack, centered
/// vertically in `bounds`. Centering on the *current* (magnified) height keeps
/// the hovered bar roughly in place while its neighbours grow. If the stack is
/// taller than the bounds, it is top-aligned and scrolled by `scroll_offset`.
///
/// `scale(count)` is the add button's magnification, `scale(count + 1)` the
/// settings button's (if `settings_slot`).
///
/// `collapse` is a deleted bar's index and how far it has collapsed (0..=1):
/// its height and one gap next to it shrink by that share.
pub fn compute_layout(
    count: usize,
    scale: impl Fn(usize) -> f32,
    bounds: Rectangle,
    scroll_offset: f32,
    bars_settings: &BarSettings,
    settings_slot: bool,
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
    let add_size = slot_size(bars_settings, scale(count));
    let gear_size = slot_size(bars_settings, scale(count + 1));
    let gear_height = if settings_slot {
        gap + gear_size.height
    } else {
        0.0
    };
    let content_height = bars_height + add_gap + add_size.height + gear_height;

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

    let add_button = Rectangle::new(Point::new(right - add_size.width, y), add_size);
    let add_top = y - add_gap / 2.0;
    let gear_y = y + add_size.height + gap;
    // With a gear slot the two hit areas meet halfway between the slots.
    let add_bottom = if settings_slot {
        gear_y - gap / 2.0
    } else {
        y + add_size.height + EDGE_PADDING
    };
    let add_hit_area = Rectangle::new(
        Point::new(bounds.x, add_top),
        Size::new(bounds.width, add_bottom - add_top),
    );
    let (settings_button, settings_hit_area) = if settings_slot {
        let button = Rectangle::new(Point::new(right - gear_size.width, gear_y), gear_size);
        let hit = Rectangle::new(
            Point::new(bounds.x, add_bottom),
            Size::new(
                bounds.width,
                gear_y + gear_size.height + EDGE_PADDING - add_bottom,
            ),
        );
        (Some(button), Some(hit))
    } else {
        (None, None)
    };

    StripLayout {
        bars,
        add_button,
        add_hit_area,
        settings_button,
        settings_hit_area,
        max_scroll,
    }
}

pub struct BarStrip<'a> {
    pub notes: &'a [Note],
    pub magnification: &'a MagnificationState,
    pub drag: &'a Option<DragState>,
    pub scroll_offset: f32,
    /// Bar index being peeked and the peek's progress (0..=1).
    pub peek: Option<(usize, f32)>,
    pub bars: &'a BarSettings,
    /// Share of the widget height the bars may use (centered).
    pub height_fraction: f32,
    /// Paper tint of an open note, which the hover peek imitates.
    pub paper_tint: f32,
    /// The open peek asks whether to delete its note.
    pub peek_confirm: bool,
    pub theme: theme::Theme,
    /// Index of the open (or opening) note's bar and the note's morph
    /// progress.
    pub open: Option<(usize, f32)>,
    /// Bar index of a deleted note and how far its bar has collapsed.
    pub collapse: Option<(usize, f32)>,
}

impl<'a> BarStrip<'a> {
    fn layout_in(&self, bounds: Rectangle) -> StripLayout {
        compute_layout(
            self.notes.len(),
            |i| self.magnification.scale(i),
            band(bounds, self.height_fraction),
            self.scroll_offset,
            self.bars,
            SETTINGS_SLOT,
            self.collapse,
        )
    }

    /// Index of the note whose open peek is under `pos`, if any.
    fn peek_hit(&self, bounds: Rectangle, pos: Point) -> Option<usize> {
        let (i, _) = self.peek?;
        let bar = *self.layout_in(bounds).bars.get(i)?;
        let note = self.notes.get(i)?;
        peek_target(bar, bounds, note, self.peek_confirm)
            .contains(pos)
            .then_some(i)
    }

    /// What a press at `pos` on the open peek does: delete-related clicks
    /// once it is fully open, otherwise opening the note. While the
    /// confirmation shows, other presses on the peek do nothing.
    fn peek_press(&self, bounds: Rectangle, pos: Point) -> Option<Option<Message>> {
        let i = self.peek_hit(bounds, pos)?;
        let (_, progress) = self.peek?;
        let bar = *self.layout_in(bounds).bars.get(i)?;
        let note = self.notes.get(i)?;
        let parts = peek_layout(bar, bounds, 1.0, &peek_text(note), self.peek_confirm);
        let open = progress >= 0.99;
        Some(if self.peek_confirm {
            if parts.delete.contains(pos) {
                Some(Message::PeekDeleteConfirmed)
            } else if parts.cancel.contains(pos) {
                Some(Message::PeekDeleteCancelled)
            } else {
                None
            }
        } else if open && parts.trash.contains(pos) {
            Some(Message::PeekDeleteRequested(i))
        } else {
            Some(Message::BarClicked(i))
        })
    }

    /// How far a slot at magnification `scale` has revealed its glyph (0..=1).
    fn reveal(scale: f32) -> f32 {
        ((scale - 1.0) / (ADD_PLUS_SCALE - 1.0)).clamp(0.0, 1.0)
    }

    fn insertion_index(&self, cursor_y: f32, bars: &[Rectangle]) -> usize {
        for (i, bar) in bars.iter().enumerate() {
            let center = bar.y + bar.height / 2.0;
            if cursor_y < center {
                return i;
            }
        }
        bars.len()
    }
}

impl<'a> advanced::Widget<Message, Theme, iced::Renderer> for BarStrip<'a> {
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
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let strip = self.layout_in(bounds);
        let bars = &strip.bars;
        let dragging_index = self.drag.as_ref().map(|d| d.bar_index);
        let drag_active = self
            .drag
            .as_ref()
            .is_some_and(|d| (d.current_y - d.origin_y).abs() > 5.0);

        for (i, bar_rect) in bars.iter().enumerate() {
            if let Some((peek_index, progress)) = self.peek {
                if peek_index == i && progress > 0.0 {
                    if let Some(note) = self.notes.get(i) {
                        draw_peek(
                            renderer,
                            note,
                            *bar_rect,
                            bounds,
                            &self.theme,
                            CORNER_RADIUS,
                            progress,
                            self.paper_tint,
                            self.peek_confirm,
                            cursor.position(),
                        );
                    }
                    continue;
                }
            }
            if let Some(note) = self.notes.get(i) {
                let alpha = if drag_active && Some(i) == dragging_index {
                    0.3
                } else {
                    note.color.rgba[3]
                };
                let open = self.open.filter(|(o, _)| *o == i);
                let rect = bar_rect_for(open.map_or(0.0, |(_, p)| p), *bar_rect);
                let corner = CORNER_RADIUS;
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
                    self.theme.highlight(),
                );
            }
        }

        // Add and settings buttons: "empty slot" bars. At rest they are
        // hollow outlines the size of a note bar; magnified they fill in and
        // show their glyph.
        let idle = self.drag.is_none();
        let count = self.notes.len();
        let add_reveal = Self::reveal(self.magnification.scale(count));
        let add = strip.add_button;
        draw_slot(
            renderer,
            add,
            idle && cursor.is_over(strip.add_hit_area),
            add_reveal,
            &self.theme,
        );
        if add_reveal > 0.0 {
            let thickness = 2.0;
            let len = (add.width.min(add.height) * 0.5).max(thickness);
            let cx = add.x + add.width / 2.0;
            let cy = add.y + add.height / 2.0;
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
                    self.theme.card().scale_alpha(0.95 * add_reveal),
                );
            }
        }

        let gear_reveal = Self::reveal(self.magnification.scale(count + 1));
        if let (Some(gear), Some(gear_hit)) = (strip.settings_button, strip.settings_hit_area) {
            draw_slot(
                renderer,
                gear,
                idle && cursor.is_over(gear_hit),
                gear_reveal,
                &self.theme,
            );
            if gear_reveal > 0.0 {
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
                        self.theme.card().scale_alpha(0.95 * gear_reveal),
                    );
                }
            }
        }

        if let Some(drag) = &self.drag {
            if drag_active {
                if let Some(note) = self.notes.get(drag.bar_index) {
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

                    let target = self.insertion_index(drag.current_y, bars);
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

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if bounds.contains(*position) {
                    shell.publish(Message::StripHover(Some(position.y)));
                } else {
                    shell.publish(Message::StripHover(None));
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                shell.publish(Message::StripHover(None));
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position() {
                    let strip = self.layout_in(bounds);
                    // Clicking the open peek opens its note, or deletes it.
                    if let Some(action) = self.peek_press(bounds, pos) {
                        if let Some(message) = action {
                            shell.publish(message);
                        }
                        shell.capture_event();
                        return;
                    }
                    for (i, bar_rect) in strip.bars.iter().enumerate() {
                        if bar_rect.contains(pos) {
                            shell.publish(Message::DragStart(i, pos.y));
                            shell.capture_event();
                            return;
                        }
                    }
                    if strip.add_hit_area.contains(pos) {
                        shell.publish(Message::AddNote);
                        shell.capture_event();
                    } else if strip.settings_hit_area.is_some_and(|hit| hit.contains(pos)) {
                        shell.publish(Message::ToggleSettings);
                        shell.capture_event();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if self.drag.is_some() {
                    shell.publish(Message::DragEnd);
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if bounds.contains(cursor.position().unwrap_or_default()) =>
            {
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 30.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                shell.publish(Message::StripScroll(dy));
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
        if self
            .drag
            .as_ref()
            .is_some_and(|d| (d.current_y - d.origin_y).abs() > 5.0)
        {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position() {
            let strip = self.layout_in(layout.bounds());
            if self.peek_hit(layout.bounds(), pos).is_some()
                || strip.bars.iter().any(|bar| bar.contains(pos))
                || strip.add_hit_area.contains(pos)
                || strip.settings_hit_area.is_some_and(|hit| hit.contains(pos))
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

/// Where the fully open peek of the note on `bar` sits: the area that
/// keeps it open while hovered and opens the note when clicked.
pub fn peek_target(bar: Rectangle, bounds: Rectangle, note: &Note, confirming: bool) -> Rectangle {
    peek_layout(bar, bounds, 1.0, &peek_text(note), confirming).rect
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
            true,
            None,
        )
    }

    fn stack_center(l: &StripLayout) -> f32 {
        let top = l.bars.first().map_or(l.add_button.y, |b| b.y);
        let bottom = l.settings_button.unwrap().y + l.settings_button.unwrap().height;
        (top + bottom) / 2.0
    }

    fn center(r: &Rectangle) -> Point {
        Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
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
        let with = |collapse| compute_layout(3, |_| 1.0, bounds(900.0), 0.0, &d, true, collapse);
        let full = with(None);
        let half = with(Some((1, 0.5)));
        assert!((half.bars[1].height - d.height / 2.0).abs() < 0.01);
        let gone = with(Some((1, 1.0)));
        assert_eq!(gone.bars[1].height, 0.0);
        let extent = |l: &StripLayout| {
            let gear = l.settings_button.unwrap();
            gear.y + gear.height - l.bars[0].y
        };
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
        let l = compute_layout(3, |_| 1.0, bounds(900.0), 0.0, &bars, true, None);
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
        let last = scrolled.settings_button.unwrap().y + scrolled.settings_button.unwrap().height;
        assert!((last - (400.0 - EDGE_PADDING)).abs() < 0.01);
    }

    #[test]
    fn bars_keep_margin_from_screen_edge() {
        let l = layout(3, |_| 5.0, 900.0, 0.0);
        for bar in l
            .bars
            .iter()
            .chain([&l.add_button, &l.settings_button.unwrap()])
        {
            assert!((STRIP_WIDTH - (bar.x + bar.width) - EDGE_MARGIN).abs() < 0.01);
        }
    }

    #[test]
    fn add_button_is_bar_shaped_at_rest() {
        let l = layout(3, |_| 1.0, 900.0, 0.0);
        assert_eq!(l.add_button.size(), l.bars[0].size());
    }

    #[test]
    fn magnified_add_button_widens_but_stays_compact() {
        let d = BarSettings::default();
        let l = layout(3, |i| if i == 3 { 5.0 } else { 1.0 }, 900.0, 0.0);
        assert!((l.add_button.width - d.width * 5.0).abs() < 0.01);
        assert!((l.add_button.height - d.height * ADD_MAX_HEIGHT_SCALE).abs() < 0.01);
    }

    #[test]
    fn add_hit_area_spans_strip_width() {
        let l = layout(3, |_| 1.0, 900.0, 0.0);
        assert_eq!(l.add_hit_area.width, STRIP_WIDTH);
        assert!(l
            .add_hit_area
            .contains(Point::new(2.0, l.add_button.y + 2.0)));
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
    fn peek_target_finds_open_peek() {
        let bar = Rectangle::new(Point::new(48.0, 400.0), Size::new(6.0, 30.0));
        let strip = Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, 900.0));
        let note = crate::note::Note::new(crate::note::PALETTE[0]);
        let rect = peek_target(bar, strip, &note, false);
        assert!(rect.width > STRIP_WIDTH);
        assert!(rect.contains(Point::new(bar.x - 100.0, bar.center().y)));
    }

    #[test]
    fn peek_hit_covers_open_peek_only() {
        let notes = [crate::note::Note::new(crate::note::PALETTE[0])];
        let magnification = MagnificationState::new();
        let drag = None;
        let bars = BarSettings::default();
        let strip = |peek| BarStrip {
            notes: &notes,
            magnification: &magnification,
            drag: &drag,
            scroll_offset: 0.0,
            peek,
            bars: &bars,
            height_fraction: 1.0,
            paper_tint: 0.0,
            peek_confirm: false,
            theme: theme::Theme::default(),
            open: None,
            collapse: None,
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
    fn no_settings_slot_where_the_tray_opens_settings() {
        for count in [0, 3] {
            let l = compute_layout(
                count,
                |_| 1.0,
                bounds(900.0),
                0.0,
                &BarSettings::default(),
                false,
                None,
            );
            assert!(l.settings_button.is_none() && l.settings_hit_area.is_none());
            let top = l.bars.first().map_or(l.add_button.y, |b| b.y);
            let bottom = l.add_button.y + l.add_button.height;
            assert!(((top + bottom) / 2.0 - 450.0).abs() < 0.01, "count {count}");
            assert!(l.add_hit_area.contains(l.add_button.center()));
            assert_eq!(l.settings_anchor(), l.add_button);
        }
    }

    #[test]
    fn gear_slot_below_add_button_for_any_count() {
        for count in [0, 1, 5] {
            let l = layout(count, |_| 1.0, 900.0, 0.0);
            let gear = center(&l.settings_button.unwrap());
            assert!(l.settings_button.unwrap().y >= l.add_button.y + l.add_button.height);
            assert!(l.settings_hit_area.unwrap().contains(gear), "count {count}");
            assert!(!l.add_hit_area.contains(gear), "count {count}");
            assert!(l.add_hit_area.contains(center(&l.add_button)));
        }
    }

    #[test]
    fn magnified_gear_uses_its_own_scale() {
        let d = BarSettings::default();
        let l = layout(2, |i| if i == 3 { 4.0 } else { 1.0 }, 900.0, 0.0);
        assert!((l.settings_button.unwrap().width - d.width * 4.0).abs() < 0.01);
        assert_eq!(l.add_button.size(), l.bars[0].size());
    }
}
