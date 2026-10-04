//! Wraps one line of rendered rich text and reports which character a left
//! press lands on. iced's `rich_text` only reports presses on links, so this
//! lays out the same spans again (only on a press) and hit-tests them.

use crate::rich_view::LINE_HEIGHT;

use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self, Paragraph as _, Renderer as _};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{overlay, renderer, Clipboard, Shell, Widget};
use iced::event::Event;
use iced::widget::text::Span;
use iced::{alignment, mouse};
use iced::{Element, Length, Rectangle, Size, Theme, Vector};

pub struct HitText<'a, Message> {
    content: Element<'a, Message>,
    /// The spans `content` draws, laid out the same way for hit-testing.
    spans: Vec<Span<'a, String>>,
    /// Maps the byte offset of the hit within this line to a message.
    on_press: Box<dyn Fn(usize) -> Message + 'a>,
}

/// `content` must be a `rich_text` of `spans` with default size, font,
/// alignment and wrapping and `rich_view::LINE_HEIGHT`, at the full
/// available width.
pub fn hit_text<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    spans: Vec<Span<'a, String>>,
    on_press: impl Fn(usize) -> Message + 'a,
) -> HitText<'a, Message> {
    HitText {
        content: content.into(),
        spans,
        on_press: Box::new(on_press),
    }
}

impl<'a, Message> HitText<'a, Message> {
    fn hit(&self, renderer: &iced::Renderer, bounds: Rectangle, at: iced::Point) -> Option<usize> {
        let paragraph = <iced::Renderer as text::Renderer>::Paragraph::with_spans(text::Text {
            content: &self.spans[..],
            bounds: Size::new(bounds.width, f32::INFINITY),
            size: renderer.default_size(),
            line_height: LINE_HEIGHT,
            font: renderer.default_font(),
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::default(),
        });
        paragraph
            .hit_test(at - Vector::new(bounds.x, bounds.y))
            .map(|hit| hit.cursor())
    }
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for HitText<'a, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Links go first: the rich text captures presses on them.
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.is_event_captured()
            || !matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
        {
            return;
        }
        let bounds = layout.bounds();
        if let Some(at) = cursor.position_over(bounds) {
            if let Some(offset) = self.hit(renderer, bounds, at) {
                shell.publish((self.on_press)(offset));
                shell.capture_event();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        match self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        ) {
            mouse::Interaction::None if cursor.is_over(layout.bounds()) => mouse::Interaction::Text,
            interaction => interaction,
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a> From<HitText<'a, Message>> for Element<'a, Message> {
    fn from(widget: HitText<'a, Message>) -> Self {
        Self::new(widget)
    }
}
