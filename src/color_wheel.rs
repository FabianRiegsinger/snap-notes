//! An HSV color disk: the angle around the center sets the hue, the
//! distance from it the saturation, and the value stays that of the current
//! color. Pressing or dragging on it publishes the color under the cursor.

use crate::note::NoteColor;

use iced::advanced::image::{self, Image};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{tree, Tree};
use iced::advanced::{renderer, Clipboard, Shell, Widget};
use iced::event::Event;
use iced::{mouse, Border, Color, Element, Length, Point, Rectangle, Size, Theme};
use std::cell::RefCell;

/// Hue (degrees, 0..360), saturation and value (0..=1) of `color`.
pub fn to_hsv(color: NoteColor) -> (f32, f32, f32) {
    let [r, g, b, _] = color.rgba;
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let s = if max > 0.0 { d / max } else { 0.0 };
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h.rem_euclid(360.0), s, max)
}

/// The color with hue `h` (degrees), saturation `s` and value `v`.
pub fn from_hsv(h: f32, s: f32, v: f32) -> NoteColor {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    NoteColor::new(r + m, g + m, b + m)
}

/// The color at `point` on a disk of `radius` around `center`, at value
/// `value`. Red is to the right and the hue turns counterclockwise; points
/// beyond the edge take the edge's color.
pub fn hsv_at(center: Point, radius: f32, point: Point, value: f32) -> NoteColor {
    let (dx, dy) = (point.x - center.x, center.y - point.y);
    let s = if radius > 0.0 {
        (dx.hypot(dy) / radius).min(1.0)
    } else {
        0.0
    };
    from_hsv(dy.atan2(dx).to_degrees(), s, value)
}

/// Where `color` sits on a disk of `radius` around `center`.
pub fn point_for(color: NoteColor, center: Point, radius: f32) -> Point {
    let (h, s, _) = to_hsv(color);
    let (sin, cos) = h.to_radians().sin_cos();
    Point::new(center.x + cos * s * radius, center.y - sin * s * radius)
}

/// Side of the disk's texture; it is drawn scaled to the widget.
const TEXTURE: u32 = 256;
/// Diameter of the ring marking the current color.
const MARKER: f32 = 14.0;

thread_local! {
    /// The last disk drawn and the value (in 1/255 steps) it was drawn at:
    /// dragging keeps the value, so the texture is made once per value.
    static DISK: RefCell<Option<(u8, image::Handle)>> = const { RefCell::new(None) };
}

/// The disk's texture at `value`, with an anti-aliased edge.
fn disk(value: f32) -> image::Handle {
    let key = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    DISK.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((cached, handle)) = cache.as_ref() {
            if *cached == key {
                return handle.clone();
            }
        }
        let radius = TEXTURE as f32 / 2.0;
        let center = Point::new(radius, radius);
        let mut pixels = Vec::with_capacity((TEXTURE * TEXTURE * 4) as usize);
        for y in 0..TEXTURE {
            for x in 0..TEXTURE {
                let p = Point::new(x as f32 + 0.5, y as f32 + 0.5);
                let alpha = (radius - p.distance(center)).clamp(0.0, 1.0);
                let [r, g, b, _] = hsv_at(center, radius, p, key as f32 / 255.0).rgba;
                pixels.extend([r, g, b, alpha].map(|c| (c * 255.0).round() as u8));
            }
        }
        let handle = image::Handle::from_rgba(TEXTURE, TEXTURE, pixels);
        *cache = Some((key, handle.clone()));
        handle
    })
}

pub struct ColorWheel<'a, Message> {
    color: NoteColor,
    diameter: f32,
    on_change: Box<dyn Fn(NoteColor) -> Message + 'a>,
}

/// A color disk `diameter` px wide showing `color`; presses and drags on
/// it send `on_change` with the color under the cursor.
pub fn color_wheel<'a, Message>(
    color: NoteColor,
    diameter: f32,
    on_change: impl Fn(NoteColor) -> Message + 'a,
) -> ColorWheel<'a, Message> {
    ColorWheel {
        color,
        diameter,
        on_change: Box::new(on_change),
    }
}

#[derive(Default)]
struct State {
    dragging: bool,
}

impl<Message> ColorWheel<'_, Message> {
    fn radius(&self) -> f32 {
        self.diameter / 2.0
    }

    /// Sends the color under `cursor`, unless it is the current one.
    fn pick(&self, center: Point, cursor: Point, shell: &mut Shell<'_, Message>) {
        let (_, _, value) = to_hsv(self.color);
        let color = hsv_at(center, self.radius(), cursor, value);
        if color != self.color {
            shell.publish((self.on_change)(color));
        }
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer> for ColorWheel<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.diameter), Length::Fixed(self.diameter))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.diameter, self.diameter)
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
        use iced::advanced::image::Renderer as _;
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let (_, _, value) = to_hsv(self.color);
        renderer.draw_image(Image::new(disk(value)), bounds, bounds);

        // A white ring with a dark rim shows on any part of the disk. Its
        // own layer: within one, images draw over quads.
        let at = point_for(self.color, bounds.center(), self.radius());
        let [r, g, b, _] = self.color.rgba;
        renderer.with_layer(bounds.expand(MARKER), |renderer| {
            for (size, width, rim) in [
                (MARKER + 2.0, 1.0, Color::from_rgba(0.0, 0.0, 0.0, 0.45)),
                (MARKER, 2.0, Color::WHITE),
            ] {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(at.x - size / 2.0, at.y - size / 2.0),
                            Size::new(size, size),
                        ),
                        border: Border {
                            radius: (size / 2.0).into(),
                            width,
                            color: rim,
                        },
                        ..renderer::Quad::default()
                    },
                    Color::from_rgb(r, g, b),
                );
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let center = layout.bounds().center();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                // Only on the disk, not its bounding square's corners.
                if let Some(p) = cursor
                    .position()
                    .filter(|p| p.distance(center) <= self.radius() + 2.0)
                {
                    state.dragging = true;
                    self.pick(center, p, shell);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                self.pick(center, *position, shell);
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                shell.capture_event();
            }
            // A release outside the window may never arrive.
            Event::Mouse(mouse::Event::CursorLeft) => state.dragging = false,
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let over = cursor
            .position()
            .is_some_and(|p| p.distance(layout.bounds().center()) <= self.radius());
        if over || tree.state.downcast_ref::<State>().dragging {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a, Message: 'a> From<ColorWheel<'a, Message>> for Element<'a, Message> {
    fn from(wheel: ColorWheel<'a, Message>) -> Self {
        Self::new(wheel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    const CENTER: Point = Point::new(50.0, 50.0);
    const RADIUS: f32 = 50.0;

    #[test]
    fn hsv_roundtrips() {
        for color in PALETTE {
            let (h, s, v) = to_hsv(color);
            assert_eq!(from_hsv(h, s, v), color, "{}", color.to_hex());
        }
        let (h, s, v) = to_hsv(NoteColor::new(1.0, 0.0, 0.0));
        assert!(h.abs() < 1e-4 && (s - 1.0).abs() < 1e-4 && (v - 1.0).abs() < 1e-4);
    }

    #[test]
    fn point_for_then_hsv_at_gives_the_color_back() {
        for color in PALETTE {
            let (_, _, v) = to_hsv(color);
            let p = point_for(color, CENTER, RADIUS);
            assert_eq!(hsv_at(CENTER, RADIUS, p, v), color, "{}", color.to_hex());
        }
    }

    #[test]
    fn hue_goes_round_and_saturation_out() {
        // Red to the right at the edge, cyan to the left.
        let red = hsv_at(CENTER, RADIUS, Point::new(100.0, 50.0), 1.0);
        assert_eq!(red.to_hex(), "#FF0000");
        let cyan = hsv_at(CENTER, RADIUS, Point::new(0.0, 50.0), 1.0);
        assert_eq!(cyan.to_hex(), "#00FFFF");
        let (_, s, _) = to_hsv(hsv_at(CENTER, RADIUS, Point::new(75.0, 50.0), 1.0));
        assert!((s - 0.5).abs() < 0.01, "s {s}");
    }

    #[test]
    fn center_has_no_saturation() {
        let gray = hsv_at(CENTER, RADIUS, CENTER, 0.6);
        let (_, s, v) = to_hsv(gray);
        assert_eq!(s, 0.0);
        assert!((v - 0.6).abs() < 1e-4);
        assert_eq!(point_for(gray, CENTER, RADIUS), CENTER);
    }

    #[test]
    fn points_beyond_the_edge_clamp_to_it() {
        let far = hsv_at(CENTER, RADIUS, Point::new(400.0, 50.0), 1.0);
        let edge = hsv_at(CENTER, RADIUS, Point::new(100.0, 50.0), 1.0);
        assert_eq!(far, edge);
        let (_, s, _) = to_hsv(hsv_at(CENTER, RADIUS, Point::new(-300.0, -300.0), 0.8));
        assert!((s - 1.0).abs() < 1e-4);
    }

    #[test]
    fn value_is_kept() {
        let c = hsv_at(CENTER, RADIUS, Point::new(80.0, 30.0), 0.4);
        let (_, _, v) = to_hsv(c);
        assert!((v - 0.4).abs() < 1e-4);
    }
}
