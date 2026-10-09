//! The screen edge the strip docks to, and the mapping between the
//! edge-local frame and window coordinates.
//!
//! In the edge-local frame `along` runs parallel to the edge from its start
//! (the top for Right and Left, the left for Top) and `away` is the distance
//! from the screen edge into the screen. For Right this is today's layout:
//! along = y, away = window width - x.

// Removed by Task 2/3, once the strip layout uses this module.
#![allow(dead_code)]

use iced::{Point, Rectangle, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edge {
    #[default]
    Right,
    Left,
    Top,
}

/// A point in the edge-local frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Local {
    pub along: f32,
    pub away: f32,
}

/// A rect in the edge-local frame. `along`/`away` is its corner nearest the
/// edge's start and the screen edge; `length` runs along the edge and
/// `thickness` away from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalRect {
    pub along: f32,
    pub away: f32,
    pub length: f32,
    pub thickness: f32,
}

impl Edge {
    pub const ALL: [Edge; 3] = [Edge::Right, Edge::Left, Edge::Top];

    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Right => "right",
            Edge::Left => "left",
            Edge::Top => "top",
        }
    }

    /// The edge named `s`; anything unknown is Right.
    pub fn parse(s: &str) -> Edge {
        match s {
            "left" => Edge::Left,
            "top" => Edge::Top,
            _ => Edge::Right,
        }
    }

    pub fn to_window(self, p: Local, window: Size) -> Point {
        match self {
            Edge::Right => Point::new(window.width - p.away, p.along),
            Edge::Left => Point::new(p.away, p.along),
            Edge::Top => Point::new(p.along, p.away),
        }
    }

    pub fn to_local(self, p: Point, window: Size) -> Local {
        match self {
            Edge::Right => Local {
                along: p.y,
                away: window.width - p.x,
            },
            Edge::Left => Local {
                along: p.y,
                away: p.x,
            },
            Edge::Top => Local {
                along: p.x,
                away: p.y,
            },
        }
    }

    pub fn rect_to_window(self, r: LocalRect, window: Size) -> Rectangle {
        let (x, y, width, height) = match self {
            Edge::Right => (
                window.width - r.away - r.thickness,
                r.along,
                r.thickness,
                r.length,
            ),
            Edge::Left => (r.away, r.along, r.thickness, r.length),
            Edge::Top => (r.along, r.away, r.length, r.thickness),
        };
        Rectangle::new(Point::new(x, y), Size::new(width, height))
    }

    pub fn rect_to_local(self, r: Rectangle, window: Size) -> LocalRect {
        match self {
            Edge::Right => LocalRect {
                along: r.y,
                away: window.width - r.x - r.width,
                length: r.height,
                thickness: r.width,
            },
            Edge::Left => LocalRect {
                along: r.y,
                away: r.x,
                length: r.height,
                thickness: r.width,
            },
            Edge::Top => LocalRect {
                along: r.x,
                away: r.y,
                length: r.width,
                thickness: r.height,
            },
        }
    }

    /// The side of `size` that runs along the edge.
    pub fn edge_length(self, size: Size) -> f32 {
        match self {
            Edge::Right | Edge::Left => size.height,
            Edge::Top => size.width,
        }
    }

    /// `content` on the away side of `anchor` (`gap` apart), centred on it
    /// along the edge, then clamped into `bounds`.
    pub fn place_away(
        self,
        anchor: Rectangle,
        content: Size,
        gap: f32,
        bounds: Rectangle,
    ) -> Rectangle {
        let center = anchor.center();
        let (x, y) = match self {
            Edge::Right => (
                anchor.x - gap - content.width,
                center.y - content.height / 2.0,
            ),
            Edge::Left => (
                anchor.x + anchor.width + gap,
                center.y - content.height / 2.0,
            ),
            Edge::Top => (
                center.x - content.width / 2.0,
                anchor.y + anchor.height + gap,
            ),
        };
        let max_x = (bounds.x + bounds.width - content.width).max(bounds.x);
        let max_y = (bounds.y + bounds.height - content.height).max(bounds.y);
        Rectangle::new(
            Point::new(x.clamp(bounds.x, max_x), y.clamp(bounds.y, max_y)),
            content,
        )
    }

    /// Where a `window` docks on a `monitor`: flush to the edge and centred
    /// along it. Top sits `top_inset` below the monitor's top.
    pub fn dock_origin(self, window: Size, monitor: Size, top_inset: f32) -> Point {
        match self {
            Edge::Right => Point::new(
                monitor.width - window.width,
                (monitor.height - window.height) / 2.0,
            ),
            Edge::Left => Point::new(0.0, (monitor.height - window.height) / 2.0),
            Edge::Top => Point::new((monitor.width - window.width) / 2.0, top_inset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Size = Size::new(800.0, 600.0);

    fn local() -> Local {
        Local {
            along: 100.0,
            away: 20.0,
        }
    }

    #[test]
    fn right_is_identity() {
        let p = Edge::Right.to_window(local(), WINDOW);
        assert_eq!(p, Point::new(780.0, 100.0));
        assert_eq!(Edge::Right.to_local(p, WINDOW), local());
    }

    #[test]
    fn left_mirrors() {
        let p = Edge::Left.to_window(local(), WINDOW);
        assert_eq!(p, Point::new(20.0, 100.0));
        assert_eq!(Edge::Left.to_local(p, WINDOW), local());
    }

    #[test]
    fn top_swaps_axes() {
        let p = Edge::Top.to_window(local(), WINDOW);
        assert_eq!(p, Point::new(100.0, 20.0));
        assert_eq!(Edge::Top.to_local(p, WINDOW), local());
    }

    #[test]
    fn round_trips_every_edge() {
        for edge in Edge::ALL {
            for (along, away) in [(0.0, 0.0), (100.0, 20.0), (333.3, 7.5), (590.0, 790.0)] {
                let p = Local { along, away };
                let back = edge.to_local(edge.to_window(p, WINDOW), WINDOW);
                assert!((back.along - along).abs() < 1e-4, "{edge:?} along");
                assert!((back.away - away).abs() < 1e-4, "{edge:?} away");

                let r = LocalRect {
                    along,
                    away,
                    length: 120.0,
                    thickness: 36.0,
                };
                let back = edge.rect_to_local(edge.rect_to_window(r, WINDOW), WINDOW);
                assert!((back.along - r.along).abs() < 1e-4, "{edge:?} rect along");
                assert!((back.away - r.away).abs() < 1e-4, "{edge:?} rect away");
                assert!(
                    (back.length - r.length).abs() < 1e-4,
                    "{edge:?} rect length"
                );
                assert!(
                    (back.thickness - r.thickness).abs() < 1e-4,
                    "{edge:?} rect thickness"
                );
            }
        }
    }

    #[test]
    fn rects_hug_their_edge() {
        let r = LocalRect {
            along: 50.0,
            away: 0.0,
            length: 200.0,
            thickness: 40.0,
        };
        let right = Edge::Right.rect_to_window(r, WINDOW);
        assert_eq!(
            (right.x + right.width, right.y, right.height),
            (800.0, 50.0, 200.0)
        );
        let left = Edge::Left.rect_to_window(r, WINDOW);
        assert_eq!((left.x, left.width, left.height), (0.0, 40.0, 200.0));
        let top = Edge::Top.rect_to_window(r, WINDOW);
        assert_eq!(
            (top.y, top.x, top.width, top.height),
            (0.0, 50.0, 200.0, 40.0)
        );
    }

    fn screen() -> Rectangle {
        Rectangle::new(Point::ORIGIN, WINDOW)
    }

    #[test]
    fn place_away_sides() {
        let content = Size::new(200.0, 120.0);
        let anchor = Rectangle::new(Point::new(700.0, 100.0), Size::new(40.0, 60.0));
        let r = Edge::Right.place_away(anchor, content, 8.0, screen());
        assert_eq!(r.x + r.width, 692.0);
        assert_eq!(r.y + r.height / 2.0, 130.0);

        let anchor = Rectangle::new(Point::new(60.0, 100.0), Size::new(40.0, 60.0));
        let r = Edge::Left.place_away(anchor, content, 8.0, screen());
        assert_eq!(r.x, 108.0);

        let anchor = Rectangle::new(Point::new(100.0, 30.0), Size::new(60.0, 40.0));
        let r = Edge::Top.place_away(anchor, content, 8.0, screen());
        assert_eq!(r.y, 78.0);
        assert_eq!(r.x + r.width / 2.0, 130.0);
    }

    #[test]
    fn place_away_clamps() {
        let content = Size::new(200.0, 120.0);
        let inside = |r: Rectangle| {
            r.x >= 0.0 && r.y >= 0.0 && r.x + r.width <= 800.0 && r.y + r.height <= 600.0
        };
        let near_start = Rectangle::new(Point::new(700.0, 0.0), Size::new(40.0, 20.0));
        let r = Edge::Right.place_away(near_start, content, 8.0, screen());
        assert!(inside(r) && r.y == 0.0);
        let near_end = Rectangle::new(Point::new(700.0, 590.0), Size::new(40.0, 10.0));
        let r = Edge::Right.place_away(near_end, content, 8.0, screen());
        assert!(inside(r) && r.y + r.height == 600.0);
        let corner = Rectangle::new(Point::new(780.0, 20.0), Size::new(20.0, 20.0));
        let r = Edge::Top.place_away(corner, content, 8.0, screen());
        assert!(inside(r) && r.x + r.width == 800.0);
        // Too big for the bounds: pinned to the bounds' start.
        let r = Edge::Left.place_away(corner, Size::new(900.0, 700.0), 8.0, screen());
        assert_eq!((r.x, r.y), (0.0, 0.0));
    }

    #[test]
    fn dock_origin_each_edge() {
        let monitor = Size::new(1440.0, 900.0);
        let tall = Size::new(64.0, 810.0);
        assert_eq!(
            Edge::Right.dock_origin(tall, monitor, 25.0),
            Point::new(1376.0, 45.0)
        );
        assert_eq!(
            Edge::Left.dock_origin(tall, monitor, 25.0),
            Point::new(0.0, 45.0)
        );
        let wide = Size::new(1296.0, 64.0);
        assert_eq!(
            Edge::Top.dock_origin(wide, monitor, 25.0),
            Point::new(72.0, 25.0)
        );
    }

    #[test]
    fn edge_length_each() {
        let size = Size::new(64.0, 810.0);
        assert_eq!(Edge::Right.edge_length(size), 810.0);
        assert_eq!(Edge::Left.edge_length(size), 810.0);
        assert_eq!(Edge::Top.edge_length(size), 64.0);
    }

    #[test]
    fn parse_round_trips_and_defaults_to_right() {
        for edge in Edge::ALL {
            assert_eq!(Edge::parse(edge.as_str()), edge);
        }
        assert_eq!(Edge::parse("bottom"), Edge::Right);
        assert_eq!(Edge::parse(""), Edge::Right);
        assert_eq!(Edge::default(), Edge::Right);
    }
}
