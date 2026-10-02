use crate::animation::MagnificationState;
use crate::app::Message;
use crate::note::Note;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::Tree;
use iced::advanced::{self, Clipboard, Shell};
use iced::event::Event;
use iced::mouse;
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme};

const BAR_REST_WIDTH: f32 = 6.0;
const BAR_REST_HEIGHT: f32 = 30.0;
const BAR_GAP: f32 = 4.0;
const ADD_BUTTON_SIZE: f32 = 20.0;
const CORNER_RADIUS: f32 = 3.0;

pub struct BarStrip<'a> {
    notes: &'a [Note],
    magnification: &'a MagnificationState,
    show_add_button: bool,
}

impl<'a> BarStrip<'a> {
    pub fn new(
        notes: &'a [Note],
        magnification: &'a MagnificationState,
        show_add_button: bool,
    ) -> Self {
        Self {
            notes,
            magnification,
            show_add_button,
        }
    }

    fn bar_bounds(&self, layout_bounds: Rectangle) -> Vec<Rectangle> {
        let mut bars = Vec::new();
        let mut y = layout_bounds.y;
        for (i, _note) in self.notes.iter().enumerate() {
            let scale = self.magnification.scale(i);
            let w = BAR_REST_WIDTH * scale;
            let h = BAR_REST_HEIGHT * scale;
            let x = layout_bounds.x + layout_bounds.width - w;
            bars.push(Rectangle::new(Point::new(x, y), Size::new(w, h)));
            y += h + BAR_GAP;
        }
        bars
    }

    fn add_button_bounds(&self, layout_bounds: Rectangle) -> Rectangle {
        let bars = self.bar_bounds(layout_bounds);
        let y = bars
            .last()
            .map(|b| b.y + b.height + BAR_GAP * 2.0)
            .unwrap_or(layout_bounds.y);
        let x = layout_bounds.x + layout_bounds.width - ADD_BUTTON_SIZE;
        Rectangle::new(Point::new(x, y), Size::new(ADD_BUTTON_SIZE, ADD_BUTTON_SIZE))
    }
}

impl<'a> advanced::Widget<Message, Theme, iced::Renderer> for BarStrip<'a> {
    fn size(&self) -> Size<Length> {
        let max_scale = (0..self.notes.len())
            .map(|i| self.magnification.scale(i))
            .fold(1.0f32, f32::max);
        let width = (BAR_REST_WIDTH * max_scale).max(ADD_BUTTON_SIZE + 4.0);
        Size::new(Length::Fixed(width + 30.0), Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max_scale = (0..self.notes.len())
            .map(|i| self.magnification.scale(i))
            .fold(1.0f32, f32::max);
        let width = (BAR_REST_WIDTH * max_scale).max(ADD_BUTTON_SIZE + 4.0) + 30.0;
        let limits = limits.width(Length::Fixed(width)).height(Length::Fill);
        let size = limits.resolve(width, f32::INFINITY, Size::new(width, 0.0));
        layout::Node::new(size)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let bars = self.bar_bounds(bounds);

        for (i, bar_rect) in bars.iter().enumerate() {
            if let Some(note) = self.notes.get(i) {
                let color = Color::from_rgba(
                    note.color.rgba[0],
                    note.color.rgba[1],
                    note.color.rgba[2],
                    note.color.rgba[3],
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
                if scale > 2.0 && !note.content.is_empty() {
                    let preview: String = note.content.chars().take(40).collect();
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
                    let _ = (preview, text_size);
                }
            }
        }

        if self.show_add_button {
            let btn = self.add_button_bounds(bounds);
            renderer::Renderer::fill_quad(
                renderer,
                renderer::Quad {
                    bounds: btn,
                    border: iced::Border {
                        radius: (ADD_BUTTON_SIZE / 2.0).into(),
                        width: 1.5,
                        color: Color::from_rgba(0.6, 0.6, 0.6, 0.8),
                    },
                    shadow: Default::default(),
                    snap: true,
                },
                Color::from_rgba(0.3, 0.3, 0.3, 0.5),
            );
            let plus_h = 2.0;
            let plus_len = ADD_BUTTON_SIZE * 0.4;
            let cx = btn.x + btn.width / 2.0;
            let cy = btn.y + btn.height / 2.0;
            renderer::Renderer::fill_quad(
                renderer,
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(cx - plus_len / 2.0, cy - plus_h / 2.0),
                        Size::new(plus_len, plus_h),
                    ),
                    border: Default::default(),
                    shadow: Default::default(),
                    snap: true,
                },
                Color::WHITE,
            );
            renderer::Renderer::fill_quad(
                renderer,
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(cx - plus_h / 2.0, cy - plus_len / 2.0),
                        Size::new(plus_h, plus_len),
                    ),
                    border: Default::default(),
                    shadow: Default::default(),
                    snap: true,
                },
                Color::WHITE,
            );
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
                    let bars = self.bar_bounds(bounds);
                    for (i, bar_rect) in bars.iter().enumerate() {
                        if bar_rect.contains(pos) {
                            shell.publish(Message::BarClicked(i));
                            return;
                        }
                    }
                    if self.show_add_button {
                        let btn = self.add_button_bounds(bounds);
                        if btn.contains(pos) {
                            shell.publish(Message::AddNote);
                        }
                    }
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
        if let Some(pos) = cursor.position() {
            let bounds = layout.bounds();
            let bars = self.bar_bounds(bounds);
            for bar_rect in &bars {
                if bar_rect.contains(pos) {
                    return mouse::Interaction::Pointer;
                }
            }
            if self.show_add_button {
                let btn = self.add_button_bounds(bounds);
                if btn.contains(pos) {
                    return mouse::Interaction::Pointer;
                }
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
    show_add_button: bool,
) -> Element<'a, Message> {
    BarStrip::new(notes, magnification, show_add_button).into()
}
