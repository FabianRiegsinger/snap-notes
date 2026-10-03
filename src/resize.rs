//! Resizing the open note by dragging its edges: the geometry, and a
//! wrapper widget that turns a press on the note's border into a resize.

use crate::app::Message;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{overlay, renderer, Clipboard, Shell, Widget};
use iced::event::Event;
use iced::mouse::{self, Interaction};
use iced::{Element, Length, Point, Rectangle, Size, Theme, Vector};

/// Width of the grab zone inside each edge.
pub const GRAB: f32 = 6.0;
/// Widest a note gets: the window is sized for it where it can't cover the
/// whole screen.
pub const MAX_NOTE_WIDTH: f32 = 900.0;
/// Smallest size a note can be resized to.
pub const MIN_SIZE: Size = Size::new(240.0, 200.0);

/// Which edges a resize moves; two set at once is a corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// The edges whose grab zone (inside `rect`) contains `pos`.
pub fn edges_at(rect: Rectangle, pos: Point) -> Option<Edges> {
    if !rect.contains(pos) {
        return None;
    }
    let edges = Edges {
        left: pos.x < rect.x + GRAB,
        right: pos.x >= rect.x + rect.width - GRAB,
        top: pos.y < rect.y + GRAB,
        bottom: pos.y >= rect.y + rect.height - GRAB,
    };
    (edges != Edges::default()).then_some(edges)
}

/// `start` with its `edges` moved by `delta`, no smaller than [`MIN_SIZE`]
/// and inside `bounds`. Edges that don't move stay where they are.
pub fn resized(start: Rectangle, edges: Edges, delta: Vector, bounds: Rectangle) -> Rectangle {
    let (mut left, mut top) = (start.x, start.y);
    let (mut right, mut bottom) = (start.x + start.width, start.y + start.height);
    let (max_right, max_bottom) = (bounds.x + bounds.width, bounds.y + bounds.height);
    if edges.left {
        left = (left + delta.x).clamp(bounds.x, (right - MIN_SIZE.width).max(bounds.x));
    }
    if edges.right {
        right = (right + delta.x).clamp((left + MIN_SIZE.width).min(max_right), max_right);
    }
    if edges.top {
        top = (top + delta.y).clamp(bounds.y, (bottom - MIN_SIZE.height).max(bounds.y));
    }
    if edges.bottom {
        bottom = (bottom + delta.y).clamp((top + MIN_SIZE.height).min(max_bottom), max_bottom);
    }
    Rectangle::new(Point::new(left, top), Size::new(right - left, bottom - top))
}

/// The resize cursor for `edges`.
pub fn interaction(edges: Edges) -> Interaction {
    match (edges.left || edges.right, edges.top || edges.bottom) {
        (true, false) => Interaction::ResizingHorizontally,
        (false, true) => Interaction::ResizingVertically,
        // "\" corners (top-left, bottom-right) vs "/" corners.
        _ if edges.left == edges.top => Interaction::ResizingDiagonallyDown,
        _ => Interaction::ResizingDiagonallyUp,
    }
}

/// Wraps the open note: a press on its border starts a resize instead of
/// reaching the note's content, and the border shows resize cursors.
pub struct ResizeFrame<'a> {
    content: Element<'a, Message>,
}

pub fn resize_frame<'a>(content: impl Into<Element<'a, Message>>) -> ResizeFrame<'a> {
    ResizeFrame {
        content: content.into(),
    }
}

impl<'a> Widget<Message, Theme, iced::Renderer> for ResizeFrame<'a> {
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            let edges = cursor
                .position()
                .and_then(|pos| edges_at(layout.bounds(), pos));
            if let Some(edges) = edges {
                shell.publish(Message::ResizeStart(edges));
                shell.capture_event();
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
        if let Some(edges) = cursor
            .position()
            .and_then(|pos| edges_at(layout.bounds(), pos))
        {
            return interaction(edges);
        }
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

impl<'a> From<ResizeFrame<'a>> for Element<'a, Message> {
    fn from(widget: ResizeFrame<'a>) -> Self {
        Self::new(widget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Rectangle, Size, Vector};

    fn note() -> Rectangle {
        Rectangle::new(Point::new(100.0, 100.0), Size::new(320.0, 320.0))
    }

    fn screen() -> Rectangle {
        Rectangle::new(Point::new(24.0, 24.0), Size::new(1000.0, 800.0))
    }

    fn edges(left: bool, right: bool, top: bool, bottom: bool) -> Edges {
        Edges {
            left,
            right,
            top,
            bottom,
        }
    }

    #[test]
    fn edges_are_found_in_the_inner_border() {
        let r = note();
        assert_eq!(
            edges_at(r, Point::new(102.0, 250.0)),
            Some(edges(true, false, false, false))
        );
        assert_eq!(
            edges_at(r, Point::new(418.0, 250.0)),
            Some(edges(false, true, false, false))
        );
        assert_eq!(
            edges_at(r, Point::new(250.0, 418.0)),
            Some(edges(false, false, false, true))
        );
        assert_eq!(
            edges_at(r, Point::new(102.0, 102.0)),
            Some(edges(true, false, true, false))
        );
        assert_eq!(edges_at(r, Point::new(250.0, 250.0)), None);
        assert_eq!(edges_at(r, Point::new(90.0, 250.0)), None);
    }

    #[test]
    fn right_edge_grows_width_only() {
        let r = resized(
            note(),
            edges(false, true, false, false),
            Vector::new(50.0, 30.0),
            screen(),
        );
        assert_eq!(
            r,
            Rectangle::new(Point::new(100.0, 100.0), Size::new(370.0, 320.0))
        );
    }

    #[test]
    fn left_and_top_edges_move_the_note() {
        let r = resized(
            note(),
            edges(true, false, true, false),
            Vector::new(-40.0, -20.0),
            screen(),
        );
        assert_eq!(
            r,
            Rectangle::new(Point::new(60.0, 80.0), Size::new(360.0, 340.0))
        );
    }

    #[test]
    fn size_stays_within_minimum_and_screen() {
        let small = resized(
            note(),
            edges(false, true, false, true),
            Vector::new(-1000.0, -1000.0),
            screen(),
        );
        assert_eq!(small.size(), MIN_SIZE);
        let big = resized(
            note(),
            edges(true, true, false, true),
            Vector::new(5000.0, 5000.0),
            screen(),
        );
        assert_eq!(big.x + big.width, screen().x + screen().width);
        assert_eq!(big.y + big.height, screen().y + screen().height);
        let left = resized(
            note(),
            edges(true, false, false, false),
            Vector::new(-5000.0, 0.0),
            screen(),
        );
        assert_eq!(left.x, screen().x);
        assert_eq!(left.x + left.width, 420.0);
    }

    #[test]
    fn cursor_matches_the_edge() {
        use iced::mouse::Interaction;
        assert_eq!(
            interaction(edges(true, false, false, false)),
            Interaction::ResizingHorizontally
        );
        assert_eq!(
            interaction(edges(false, false, false, true)),
            Interaction::ResizingVertically
        );
        assert_eq!(
            interaction(edges(true, false, true, false)),
            Interaction::ResizingDiagonallyDown
        );
        assert_eq!(
            interaction(edges(false, true, false, true)),
            Interaction::ResizingDiagonallyDown
        );
        assert_eq!(
            interaction(edges(false, true, true, false)),
            Interaction::ResizingDiagonallyUp
        );
    }
}
