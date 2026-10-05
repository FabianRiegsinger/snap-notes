//! Wraps a text input and keeps command shortcuts (Cmd+F, Cmd+N, Cmd+,)
//! away from it. iced's `text_input` types the key's letter even with
//! Cmd held and then captures the press, so the app's keyboard listener
//! never sees it. The wrapper doesn't forward such a press: it stays
//! uncaptured and reaches the app. Copy, cut, paste and select-all stay
//! with the field; undo and redo, which it lacks, go to the app.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{overlay, renderer, Clipboard, Shell, Widget};
use iced::event::Event;
use iced::keyboard::key::{Code, Physical};
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse;
use iced::{Element, Length, Rectangle, Size, Theme, Vector};

/// The shortcut letter of a press, lowercased: the key's Latin letter or
/// digit on any layout (as `text_input` finds it), or `,` for the comma
/// key, which `to_latin` doesn't map. `None` when there is none, e.g. a
/// non-Latin character on an unknown physical key.
pub fn shortcut_char(key: &Key, physical: Physical) -> Option<char> {
    key.to_latin(physical)
        .or_else(|| (physical == Physical::Code(Code::Comma)).then_some(','))
        .map(|c| c.to_ascii_lowercase())
}

/// Whether a press of `key` (on `physical`) with `modifiers` skips the
/// field and goes to the app: a command shortcut other than the field's
/// copy, cut, paste and select-all. Those are found by the key's Latin
/// letter, as `text_input` does, so they stay with the field on any
/// layout. Undo and redo pass through: the field keeps no history. Off
/// macOS a press with Alt held types, since Ctrl+Alt is AltGr on Windows
/// layouts; on macOS Cmd+Opt+letter stays a shortcut.
pub fn passes_through(key: &Key, physical: Physical, modifiers: Modifiers) -> bool {
    if !modifiers.command() || (modifiers.alt() && !cfg!(target_os = "macos")) {
        return false;
    }
    match shortcut_char(key, physical) {
        Some(c) => !matches!(c, 'c' | 'x' | 'v' | 'a'),
        None => matches!(key, Key::Character(_)),
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
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) = event
        {
            if passes_through(key, *physical_key, *modifiers) {
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
    use iced::keyboard::key::{Code, NativeCode};

    fn key(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn unknown() -> Physical {
        Physical::Unidentified(NativeCode::Unidentified)
    }

    #[test]
    fn command_shortcuts_skip_the_field() {
        let cmd = Modifiers::COMMAND;
        for c in ["f", "n", ",", "z"] {
            assert!(passes_through(&key(c), unknown(), cmd), "{c}");
        }
        // Undo and redo go to the app: the field has no history.
        assert!(passes_through(&key("Z"), unknown(), cmd | Modifiers::SHIFT));
        for c in ["c", "x", "v", "a", "C", "V"] {
            assert!(!passes_through(&key(c), unknown(), cmd), "{c}");
        }
        assert!(!passes_through(&key("f"), unknown(), Modifiers::empty()));
        // Ctrl+Alt is AltGr off macOS and types; on macOS Cmd+Opt+letter
        // stays a shortcut.
        assert_eq!(
            passes_through(&key("f"), unknown(), cmd | Modifiers::ALT),
            cfg!(target_os = "macos")
        );
        assert!(!passes_through(&key("c"), unknown(), cmd | Modifiers::ALT));
    }

    #[test]
    fn non_latin_layout_keeps_clipboard_shortcuts() {
        let cmd = Modifiers::COMMAND;
        // Russian layout: the key labelled C types "с", V types "м".
        assert!(!passes_through(&key("с"), Physical::Code(Code::KeyC), cmd));
        assert!(!passes_through(&key("м"), Physical::Code(Code::KeyV), cmd));
        assert!(!passes_through(&key("ф"), Physical::Code(Code::KeyA), cmd));
        assert!(!passes_through(&key("ч"), Physical::Code(Code::KeyX), cmd));
        // Other letters still reach the app.
        assert!(passes_through(&key("а"), Physical::Code(Code::KeyF), cmd));
        // Russian layout: the comma key types "б".
        assert!(passes_through(&key("б"), Physical::Code(Code::Comma), cmd));
    }

    #[test]
    fn shortcut_char_maps_any_layout() {
        assert_eq!(shortcut_char(&key("f"), unknown()), Some('f'));
        assert_eq!(shortcut_char(&key("F"), unknown()), Some('f'));
        assert_eq!(
            shortcut_char(&key("а"), Physical::Code(Code::KeyF)),
            Some('f')
        );
        assert_eq!(
            shortcut_char(&key("я"), Physical::Code(Code::KeyZ)),
            Some('z')
        );
        assert_eq!(
            shortcut_char(&key("б"), Physical::Code(Code::Comma)),
            Some(',')
        );
        assert_eq!(shortcut_char(&key("б"), unknown()), None);
        assert_eq!(
            shortcut_char(&Key::Named(keyboard::key::Named::Escape), unknown()),
            None
        );
    }
}
