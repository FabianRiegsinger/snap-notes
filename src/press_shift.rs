//! Wraps a button's label and draws it 1 px lower while the left mouse
//! button is held down on it, like padding `top + 1, bottom - 1`. It
//! never captures the press, so the button around it still fires.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{overlay, renderer, Clipboard, Renderer as _, Shell, Widget};
use iced::event::Event;
use iced::mouse;
use iced::{Element, Length, Rectangle, Size, Theme, Vector};

/// How far a pressed label sinks.
const SHIFT: f32 = 1.0;

pub struct PressShift<'a, Message> {
    content: Element<'a, Message>,
}

/// `content` should fill the button (give the button no padding and pad the
/// label inside instead), so a press anywhere on the button sinks it.
pub fn press_shift<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> PressShift<'a, Message> {
    PressShift {
        content: content.into(),
    }
}

#[derive(Default)]
struct State {
    pressed: bool,
}

/// The label's vertical offset for a press state.
fn offset(pressed: bool) -> f32 {
    if pressed {
        SHIFT
    } else {
        0.0
    }
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for PressShift<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

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
        let dy = offset(tree.state.downcast_ref::<State>().pressed);
        renderer.with_translation(Vector::new(0.0, dy), |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        });
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
        let state = tree.state.downcast_mut::<State>();
        let pressed = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                cursor.is_over(layout.bounds())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => false,
            _ => state.pressed,
        };
        if pressed != state.pressed {
            state.pressed = pressed;
            shell.request_redraw();
        }
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
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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

impl<'a, Message: 'a> From<PressShift<'a, Message>> for Element<'a, Message> {
    fn from(widget: PressShift<'a, Message>) -> Self {
        Self::new(widget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_label_sinks_one_pixel() {
        assert_eq!(offset(true), 1.0);
        assert_eq!(offset(false), 0.0);
    }
}
