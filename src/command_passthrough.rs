//! Wraps a text input and keeps command shortcuts (Cmd+F, Cmd+N, Cmd+,)
//! away from it. iced's `text_input` types the key's letter even with
//! Cmd held and then captures the press, so the app's keyboard listener
//! never sees it. The wrapper doesn't forward such a press: it stays
//! uncaptured and reaches the app. Copy, cut, paste, select-all and undo
//! stay with the field.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{overlay, renderer, Clipboard, Shell, Widget};
use iced::event::Event;
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse;
use iced::{Element, Length, Rectangle, Size, Theme, Vector};

/// Whether a press of `key` with `modifiers` skips the field and goes to
/// the app: a command shortcut on a character the field doesn't handle.
/// With Alt held it types (Ctrl+Alt is AltGr on Windows layouts).
pub fn passes_through(key: &Key, modifiers: Modifiers) -> bool {
    if !modifiers.command() || modifiers.alt() {
        return false;
    }
    match key.as_ref() {
        Key::Character(c) => !matches!(c.to_lowercase().as_str(), "c" | "x" | "v" | "a" | "z"),
        _ => false,
    }
}

pub struct CommandPassthrough<'a, Message> {
    content: Element<'a, Message>,
}

pub fn command_passthrough<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> CommandPassthrough<'a, Message> {
    CommandPassthrough {
        content: content.into(),
    }
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for CommandPassthrough<'a, Message> {
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
        if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            if passes_through(key, *modifiers) {
                return;
            }
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

impl<'a, Message: 'a> From<CommandPassthrough<'a, Message>> for Element<'a, Message> {
    fn from(widget: CommandPassthrough<'a, Message>) -> Self {
        Self::new(widget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn command_shortcuts_skip_the_field() {
        let cmd = Modifiers::COMMAND;
        for c in ["f", "n", ","] {
            assert!(passes_through(&key(c), cmd), "{c}");
        }
        for c in ["c", "x", "v", "a", "z"] {
            assert!(!passes_through(&key(c), cmd), "{c}");
        }
        assert!(!passes_through(&key("f"), Modifiers::empty()));
        assert!(!passes_through(&key("f"), cmd | Modifiers::ALT));
    }
}
