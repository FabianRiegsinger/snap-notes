//! Wraps the note's color button and, while open, shows the color bubble
//! as an overlay above it: a card with a tail pointing at the button, on
//! its own layer, so nothing in the note moves.

use crate::app::Message;
use crate::theme::{self, RADIUS_SURFACE};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{overlay, renderer, Clipboard, Shell, Widget};
use iced::event::Event;
use iced::widget::Space;
use iced::{keyboard, mouse, window, Border, Color, Element, Length, Point, Rectangle, Size};
use iced::{Theme, Vector};

/// Space between the card and the window's edges.
pub const MARGIN: f32 = 8.0;
/// Height of the tail between the card and the button.
pub const TAIL: f32 = 7.0;
/// Space between the button and the tail's tip.
pub const GAP: f32 = 2.0;
/// Space between the card's edge and its content.
const PADDING: f32 = 12.0;
/// The tail is drawn as thin strips this tall.
const TAIL_STEP: f32 = 0.5;

/// Where the card goes and which way its tail points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub card: Rectangle,
    /// The card sits below the button (no room above): its tail points up.
    pub below: bool,
    /// The tail's center, kept clear of the card's rounded corners.
    pub tail_x: f32,
}

/// Places a card of `size` above `anchor`, centered on it, within a window
/// of `window` size; below it when there's no room above.
pub fn place(anchor: Rectangle, size: Size, window: Size) -> Placement {
    let max_x = (window.width - MARGIN - size.width).max(MARGIN);
    let x = (anchor.center_x() - size.width / 2.0).clamp(MARGIN, max_x);
    let above = anchor.y - GAP - TAIL - size.height;
    let below = above < MARGIN;
    let y = if below {
        anchor.y + anchor.height + GAP + TAIL
    } else {
        above
    };
    let card = Rectangle::new(Point::new(x, y), size);
    Placement {
        card,
        below,
        tail_x: tail_x(anchor, card),
    }
}

/// The tail's center: under the button, but clear of the card's corners.
fn tail_x(anchor: Rectangle, card: Rectangle) -> f32 {
    let inset = RADIUS_SURFACE + TAIL;
    let left = card.x + inset;
    anchor
        .center_x()
        .clamp(left, (card.x + card.width - inset).max(left))
}

/// Whether a press at `press` closes the bubble: anywhere but on the card
/// or on the button (which toggles it itself).
pub fn dismisses(press: Point, card: Rectangle, anchor: Rectangle) -> bool {
    !card.contains(press) && !anchor.contains(press)
}

pub struct ColorBubble<'a> {
    anchor: Element<'a, Message>,
    card: Element<'a, Message>,
    open: bool,
    theme: theme::Theme,
}

/// `anchor` (the color button) with the bubble holding `card` above it
/// while `card` is given. Esc and presses outside send
/// `Message::CloseColorBubble`; where the card lands is reported with
/// `Message::ColorBubbleMoved`.
pub fn color_bubble<'a>(
    anchor: impl Into<Element<'a, Message>>,
    card: Option<Element<'a, Message>>,
    theme: theme::Theme,
) -> ColorBubble<'a> {
    let open = card.is_some();
    ColorBubble {
        anchor: anchor.into(),
        card: card.unwrap_or_else(|| Space::new().into()),
        open,
        theme,
    }
}

#[derive(Default)]
struct State {
    /// The card's bounds as last reported to the app.
    reported: Option<Rectangle>,
}

impl Widget<Message, Theme, iced::Renderer> for ColorBubble<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        self.anchor.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.anchor.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.anchor
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
        self.anchor.as_widget().draw(
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
        vec![Tree::new(&self.anchor), Tree::new(&self.card)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.anchor, &self.card]);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.anchor
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
        if !self.open {
            // Report again when it next opens.
            tree.state.downcast_mut::<State>().reported = None;
        }
        self.anchor.as_widget_mut().update(
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
        self.anchor.as_widget().mouse_interaction(
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
        let Tree {
            state, children, ..
        } = tree;
        let [anchor_tree, card_tree] = &mut children[..] else {
            return None;
        };
        let inner = self.anchor.as_widget_mut().overlay(
            anchor_tree,
            layout,
            renderer,
            viewport,
            translation,
        );
        let bubble = self.open.then(|| {
            overlay::Element::new(Box::new(Bubble {
                card: &mut self.card,
                tree: card_tree,
                reported: &mut state.downcast_mut::<State>().reported,
                anchor: layout.bounds() + translation,
                theme: self.theme,
            }))
        });
        match (inner, bubble) {
            (Some(inner), Some(bubble)) => {
                Some(overlay::Group::with_children(vec![inner, bubble]).overlay())
            }
            (inner, bubble) => inner.or(bubble),
        }
    }
}

impl<'a> From<ColorBubble<'a>> for Element<'a, Message> {
    fn from(widget: ColorBubble<'a>) -> Self {
        Self::new(widget)
    }
}

/// The open bubble. Its layout spans the window (so its layer doesn't clip
/// the shadow or the tail) and holds the card, which holds the content.
struct Bubble<'a, 'b> {
    card: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    reported: &'b mut Option<Rectangle>,
    /// The color button's bounds on screen.
    anchor: Rectangle,
    theme: theme::Theme,
}

impl Bubble<'_, '_> {
    /// Draws a triangle from its `tip` to a base `half` px either side at
    /// `base_y`, as horizontal strips (quads have no slanted edges).
    fn fill_triangle(
        renderer: &mut iced::Renderer,
        tip: Point,
        base_y: f32,
        half: f32,
        color: Color,
    ) {
        use iced::advanced::Renderer as _;

        let height = (base_y - tip.y).abs();
        if height <= 0.0 {
            return;
        }
        let down = base_y > tip.y;
        let mut near = 0.0;
        while near < height {
            let far = (near + TAIL_STEP).min(height);
            let width = half * (near + far) / height;
            let y = if down { tip.y + near } else { tip.y - far };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(tip.x - width / 2.0, y),
                        Size::new(width, far - near),
                    ),
                    ..renderer::Quad::default()
                },
                color,
            );
            near = far;
        }
    }
}

impl overlay::Overlay<Message, Theme, iced::Renderer> for Bubble<'_, '_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds).shrink(iced::Padding::new(PADDING));
        let content = self
            .card
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = content
            .size()
            .expand(Size::new(2.0 * PADDING, 2.0 * PADDING));
        let placement = place(self.anchor, size, bounds);
        let card =
            layout::Node::with_children(size, vec![content.move_to(Point::new(PADDING, PADDING))])
                .move_to(placement.card.position());
        layout::Node::with_children(bounds, vec![card])
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        use iced::advanced::Renderer as _;

        let Some(card_layout) = layout.children().next() else {
            return;
        };
        let card = card_layout.bounds();
        let t = self.theme;
        let border = t.ink(0.15);
        let [contact, ambient] = t.shadows(1.0);
        for shadow in [ambient, contact] {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: card,
                    border: Border {
                        radius: RADIUS_SURFACE.into(),
                        width: 1.0,
                        color: border,
                    },
                    shadow,
                    snap: true,
                },
                t.card(),
            );
        }

        // The tail: a bordered triangle, then the card's color over it and
        // over the card's border where the two meet.
        let below = card.y >= self.anchor.y + self.anchor.height;
        let (edge, toward) = if below {
            (card.y, -1.0)
        } else {
            (card.y + card.height, 1.0)
        };
        let x = tail_x(self.anchor, card);
        let outline = 1.4;
        Self::fill_triangle(
            renderer,
            Point::new(x, edge + toward * (TAIL + outline)),
            edge,
            2.0 * (TAIL + outline),
            border,
        );
        Self::fill_triangle(
            renderer,
            Point::new(x, edge + toward * TAIL),
            edge - toward,
            2.0 * (TAIL + 1.0),
            t.card(),
        );

        if let Some(content) = card_layout.children().next() {
            self.card
                .as_widget()
                .draw(self.tree, renderer, theme, style, content, cursor, &card);
        }
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if let Some(content) = layout.children().next().and_then(|c| c.children().next()) {
            self.card
                .as_widget_mut()
                .operate(self.tree, content, renderer, operation);
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let Some(card_layout) = layout.children().next() else {
            return;
        };
        let card = card_layout.bounds();
        match event {
            Event::Window(window::Event::RedrawRequested(_)) if *self.reported != Some(card) => {
                *self.reported = Some(card);
                shell.publish(Message::ColorBubbleMoved(card));
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                // Captured, so the note doesn't close too.
                shell.publish(Message::CloseColorBubble);
                shell.capture_event();
                return;
            }
            Event::Mouse(mouse::Event::ButtonPressed(_))
                if cursor
                    .position()
                    .is_some_and(|p| dismisses(p, card, self.anchor)) =>
            {
                // Not captured: the press still reaches what it is on.
                shell.publish(Message::CloseColorBubble);
            }
            _ => {}
        }
        if let Some(content) = card_layout.children().next() {
            self.card.as_widget_mut().update(
                self.tree, event, content, cursor, renderer, clipboard, shell, &card,
            );
        }
        // Presses on the card stay on it.
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) && cursor.is_over(card) {
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let Some(card_layout) = layout.children().next() else {
            return mouse::Interaction::None;
        };
        let card = card_layout.bounds();
        if !cursor.is_over(card) {
            return mouse::Interaction::None;
        }
        // Anything but `None` keeps the cursor off the note underneath.
        card_layout
            .children()
            .next()
            .map(|content| {
                self.card
                    .as_widget()
                    .mouse_interaction(self.tree, content, cursor, &card, renderer)
            })
            .filter(|i| *i != mouse::Interaction::None)
            .unwrap_or(mouse::Interaction::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::RADIUS_SURFACE;

    const WINDOW: Size = Size::new(800.0, 600.0);
    const CARD: Size = Size::new(220.0, 150.0);

    fn button(x: f32, y: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(22.0, 22.0))
    }

    #[test]
    fn card_sits_above_the_button_centered() {
        let anchor = button(300.0, 300.0);
        let p = place(anchor, CARD, WINDOW);
        assert!(!p.below);
        assert_eq!(p.card.size(), CARD);
        assert_eq!(p.card.y + p.card.height + TAIL + GAP, anchor.y);
        assert_eq!(p.card.center_x(), anchor.center_x());
        assert_eq!(p.tail_x, anchor.center_x());
    }

    #[test]
    fn card_is_clamped_into_the_window() {
        let right = place(button(780.0, 300.0), CARD, WINDOW);
        assert_eq!(right.card.x + right.card.width, WINDOW.width - MARGIN);
        let left = place(button(0.0, 300.0), CARD, WINDOW);
        assert_eq!(left.card.x, MARGIN);
        // The tail still points at the button, clear of the corners.
        assert!(left.tail_x >= left.card.x + RADIUS_SURFACE);
        assert!(right.tail_x <= right.card.x + right.card.width - RADIUS_SURFACE);
        assert!(right.tail_x > right.card.center_x());
    }

    #[test]
    fn card_flips_below_without_room_above() {
        let anchor = button(300.0, 40.0);
        let p = place(anchor, CARD, WINDOW);
        assert!(p.below);
        assert_eq!(p.card.y, anchor.y + anchor.height + TAIL + GAP);
        // Just enough room above keeps it there.
        let fits = button(300.0, MARGIN + CARD.height + TAIL + GAP);
        assert!(!place(fits, CARD, WINDOW).below);
    }

    #[test]
    fn presses_outside_card_and_button_dismiss() {
        let anchor = button(300.0, 300.0);
        let card = place(anchor, CARD, WINDOW).card;
        assert!(dismisses(Point::new(10.0, 10.0), card, anchor));
        assert!(dismisses(Point::new(300.0, 500.0), card, anchor));
        assert!(!dismisses(card.center(), card, anchor));
        assert!(!dismisses(anchor.center(), card, anchor));
    }
}
