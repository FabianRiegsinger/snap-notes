use crate::animation::MagnificationState;
use crate::app::{DragState, Message};
use crate::note::Note;
use crate::peek::draw_peek;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::Tree;
use iced::advanced::{self, Clipboard, Shell};
use iced::alignment;
use iced::event::Event;
use iced::mouse;
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Size, Theme};

pub const BAR_REST_WIDTH: f32 = 6.0;
pub const BAR_REST_HEIGHT: f32 = 30.0;
pub const BAR_GAP: f32 = 12.0;
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
const CORNER_RADIUS: f32 = 3.0;

pub struct StripLayout {
    pub bars: Vec<Rectangle>,
    pub add_button: Rectangle,
    /// Generous click target for the add button: the full strip width around it.
    pub add_hit_area: Rectangle,
    pub max_scroll: f32,
}

/// Lays out the bars plus the add button as one stack, centered vertically in
/// `bounds`. Centering on the *current* (magnified) height keeps the hovered
/// bar roughly in place while its neighbours grow. If the stack is taller than
/// the bounds, it is top-aligned and scrolled by `scroll_offset`.
///
/// `scale(count)` is the add button's magnification.
pub fn compute_layout(
    count: usize,
    scale: impl Fn(usize) -> f32,
    bounds: Rectangle,
    scroll_offset: f32,
) -> StripLayout {
    let heights: Vec<f32> = (0..count).map(|i| BAR_REST_HEIGHT * scale(i)).collect();
    let bars_height: f32 = heights.iter().sum::<f32>() + BAR_GAP * count.saturating_sub(1) as f32;
    let add_gap = if count > 0 { ADD_BUTTON_GAP } else { 0.0 };
    let add_scale = scale(count);
    let add_size = Size::new(
        BAR_REST_WIDTH * add_scale,
        BAR_REST_HEIGHT * add_scale.min(ADD_MAX_HEIGHT_SCALE),
    );
    let content_height = bars_height + add_gap + add_size.height;

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
        let w = BAR_REST_WIDTH * scale(i);
        bars.push(Rectangle::new(Point::new(right - w, y), Size::new(w, *h)));
        y += h + BAR_GAP;
    }
    if count > 0 {
        y += add_gap - BAR_GAP;
    }

    let add_button = Rectangle::new(Point::new(right - add_size.width, y), add_size);
    let add_hit_area = Rectangle::new(
        Point::new(bounds.x, y - add_gap / 2.0),
        Size::new(bounds.width, add_size.height + add_gap / 2.0 + EDGE_PADDING),
    );

    StripLayout {
        bars,
        add_button,
        add_hit_area,
        max_scroll,
    }
}

pub struct BarStrip<'a> {
    notes: &'a [Note],
    magnification: &'a MagnificationState,
    drag: &'a Option<DragState>,
    scroll_offset: f32,
    /// Bar index being peeked and the peek's progress (0..=1).
    peek: Option<(usize, f32)>,
}

impl<'a> BarStrip<'a> {
    pub fn new(
        notes: &'a [Note],
        magnification: &'a MagnificationState,
        drag: &'a Option<DragState>,
        scroll_offset: f32,
        peek: Option<(usize, f32)>,
    ) -> Self {
        Self {
            notes,
            magnification,
            drag,
            scroll_offset,
            peek,
        }
    }

    fn layout_in(&self, bounds: Rectangle) -> StripLayout {
        compute_layout(
            self.notes.len(),
            |i| self.magnification.scale(i),
            bounds,
            self.scroll_offset,
        )
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
                        draw_peek(renderer, note, *bar_rect, CORNER_RADIUS, progress);
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
                let color = Color::from_rgba(
                    note.color.rgba[0],
                    note.color.rgba[1],
                    note.color.rgba[2],
                    alpha,
                );
                renderer::Renderer::fill_quad(
                    renderer,
                    renderer::Quad {
                        bounds: *bar_rect,
                        border: iced::Border {
                            radius: CORNER_RADIUS.into(),
                            ..Default::default()
                        },
                        shadow: Default::default(),
                        snap: true,
                    },
                    color,
                );

                let scale = self.magnification.scale(i);
                let preview_source = if note.title.is_empty() {
                    &note.content
                } else {
                    &note.title
                };
                if scale > 2.0 && !preview_source.is_empty() {
                    let preview: String = preview_source.chars().take(40).collect();
                    let text_size = 10.0 * (scale / 5.0).min(1.0);
                    let text_bounds = Rectangle {
                        x: bar_rect.x + 2.0,
                        y: bar_rect.y + 2.0,
                        width: bar_rect.width - 4.0,
                        height: bar_rect.height - 4.0,
                    };
                    renderer::Renderer::fill_quad(
                        renderer,
                        renderer::Quad {
                            bounds: text_bounds,
                            border: Default::default(),
                            shadow: Default::default(),
                            snap: true,
                        },
                        Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                    );
                    use iced::advanced::text::Renderer as TextRenderer;
                    TextRenderer::fill_text(
                        renderer,
                        iced::advanced::Text {
                            content: preview,
                            bounds: Size::new(text_bounds.width, text_bounds.height),
                            size: Pixels(text_size),
                            line_height: iced::widget::text::LineHeight::default(),
                            font: iced::Font::default(),
                            align_x: alignment::Horizontal::Left.into(),
                            align_y: alignment::Vertical::Top,
                            shaping: iced::widget::text::Shaping::Basic,
                            wrapping: iced::widget::text::Wrapping::None,
                        },
                        Point::new(text_bounds.x, text_bounds.y),
                        Color::WHITE,
                        *bar_rect,
                    );
                }
            }
        }

        // Add button: an "empty slot" bar. At rest it is a hollow outline
        // the size of a note bar; magnified it fills in and shows a "+".
        let btn = strip.add_button;
        let hovered = cursor.is_over(strip.add_hit_area) && self.drag.is_none();
        let reveal = ((self.magnification.scale(self.notes.len()) - 1.0) / (ADD_PLUS_SCALE - 1.0))
            .clamp(0.0, 1.0);
        let fill_alpha = if hovered { 0.9 } else { 0.15 + 0.5 * reveal };
        let shade = if hovered { 0.3 } else { 0.45 };
        renderer::Renderer::fill_quad(
            renderer,
            renderer::Quad {
                bounds: btn,
                border: iced::Border {
                    radius: CORNER_RADIUS.into(),
                    width: 1.5 * (1.0 - reveal),
                    color: Color::from_rgba(0.45, 0.46, 0.5, 0.75),
                },
                shadow: Default::default(),
                snap: true,
            },
            Color::from_rgba(shade, shade + 0.01, shade + 0.05, fill_alpha),
        );
        if reveal > 0.0 {
            let thickness = 2.0;
            let len = (btn.width.min(btn.height) * 0.5).max(thickness);
            let cx = btn.x + btn.width / 2.0;
            let cy = btn.y + btn.height / 2.0;
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
                    Color::from_rgba(1.0, 1.0, 1.0, 0.95 * reveal),
                );
            }
        }

        if let Some(drag) = &self.drag {
            if drag_active {
                if let Some(note) = self.notes.get(drag.bar_index) {
                    let scale = self.magnification.scale(drag.bar_index);
                    let w = BAR_REST_WIDTH * scale;
                    let h = BAR_REST_HEIGHT * scale;
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
                        bars[target].y - BAR_GAP / 2.0
                    } else {
                        bars.last()
                            .map_or(bounds.y, |b| b.y + b.height + BAR_GAP / 2.0)
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
                        Color::from_rgba(1.0, 1.0, 1.0, 0.8),
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
            if strip.bars.iter().any(|bar| bar.contains(pos)) || strip.add_hit_area.contains(pos) {
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

pub fn bar_strip<'a>(
    notes: &'a [Note],
    magnification: &'a MagnificationState,
    drag: &'a Option<DragState>,
    scroll_offset: f32,
    peek: Option<(usize, f32)>,
) -> Element<'a, Message> {
    BarStrip::new(notes, magnification, drag, scroll_offset, peek).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(height: f32) -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, height))
    }

    fn stack_center(l: &StripLayout) -> f32 {
        let top = l.bars.first().map_or(l.add_button.y, |b| b.y);
        let bottom = l.add_button.y + l.add_button.height;
        (top + bottom) / 2.0
    }

    #[test]
    fn stack_is_vertically_centered_for_any_count() {
        for count in [0, 1, 3, 8] {
            let l = compute_layout(count, |_| 1.0, bounds(900.0), 0.0);
            assert!((stack_center(&l) - 450.0).abs() < 0.01, "count {count}");
            assert_eq!(l.max_scroll, 0.0);
        }
    }

    #[test]
    fn bars_are_spaced_by_gap() {
        let l = compute_layout(3, |_| 1.0, bounds(900.0), 0.0);
        let gap = l.bars[1].y - (l.bars[0].y + l.bars[0].height);
        assert!((gap - BAR_GAP).abs() < 0.01);
    }

    #[test]
    fn overflowing_stack_scrolls_and_clamps() {
        let l = compute_layout(40, |_| 1.0, bounds(400.0), 0.0);
        assert!(l.max_scroll > 0.0);
        assert!((l.bars[0].y - EDGE_PADDING).abs() < 0.01);
        let scrolled = compute_layout(40, |_| 1.0, bounds(400.0), 1.0e6);
        let last = scrolled.add_button.y + scrolled.add_button.height;
        assert!((last - (400.0 - EDGE_PADDING)).abs() < 0.01);
    }

    #[test]
    fn bars_keep_margin_from_screen_edge() {
        let l = compute_layout(3, |_| 5.0, bounds(900.0), 0.0);
        for bar in l.bars.iter().chain([&l.add_button]) {
            assert!((STRIP_WIDTH - (bar.x + bar.width) - EDGE_MARGIN).abs() < 0.01);
        }
    }

    #[test]
    fn add_button_is_bar_shaped_at_rest() {
        let l = compute_layout(3, |_| 1.0, bounds(900.0), 0.0);
        assert_eq!(l.add_button.size(), l.bars[0].size());
    }

    #[test]
    fn magnified_add_button_widens_but_stays_compact() {
        let l = compute_layout(3, |i| if i == 3 { 5.0 } else { 1.0 }, bounds(900.0), 0.0);
        assert!((l.add_button.width - BAR_REST_WIDTH * 5.0).abs() < 0.01);
        assert!((l.add_button.height - BAR_REST_HEIGHT * ADD_MAX_HEIGHT_SCALE).abs() < 0.01);
    }

    #[test]
    fn add_hit_area_spans_strip_width() {
        let l = compute_layout(3, |_| 1.0, bounds(900.0), 0.0);
        assert_eq!(l.add_hit_area.width, STRIP_WIDTH);
        assert!(l
            .add_hit_area
            .contains(Point::new(2.0, l.add_button.y + 2.0)));
    }
}
