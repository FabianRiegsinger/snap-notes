// No console window next to the app on Windows.
#![windows_subsystem = "windows"]

mod animation;
mod app;
mod autohide;
mod bar_strip;
mod brand;
mod color_bubble;
mod color_picker;
mod color_wheel;
mod command_passthrough;
mod edge;
mod export;
mod export_panel;
mod history;
mod hit_text;
mod hotkey;
mod icons;
mod images;
mod note;
mod note_panel;
mod notify;
mod pass_wheel;
mod peek;
mod platform;
mod press_shift;
mod press_through;
mod reminder;
mod resize;
mod rich;
mod rich_highlight;
mod rich_view;
mod search;
mod search_panel;
mod settings;
mod settings_panel;
mod store;
mod strip_model;
mod theme;
mod toolbar;
mod tray;

use app::App;
use edge::Edge;
use iced::window;
use iced::{Color, Point, Size, Theme};

fn main() -> iced::Result {
    platform::prepare_graphics();
    platform::enforce_single_instance();

    let mut app = iced::application(App::boot, App::update, App::view);
    for font in icons::FONTS {
        app = app.font(font);
    }
    let icon = window::icon::from_rgba(brand::icon_rgba(32), 32, 32).expect("brand icon");
    // Placed on the saved edge from the start; the app re-docks it once the
    // monitor is known.
    let edge = App::saved_edge();
    let docked: fn(Size, Size) -> Point = match edge {
        Edge::Right => |window, monitor| dock(Edge::Right, window, monitor),
        Edge::Left => |window, monitor| dock(Edge::Left, window, monitor),
        Edge::Top => |window, monitor| dock(Edge::Top, window, monitor),
    };
    app.default_font(icons::BODY_FONT)
        .subscription(App::subscription)
        .title("Snap Notes")
        .window(window::Settings {
            size: edge.size(600.0, settings::Settings::default().open_width()),
            position: window::Position::SpecificWith(docked),
            transparent: true,
            decorations: false,
            resizable: false,
            level: window::Level::AlwaysOnTop,
            icon: Some(icon),
            ..window::Settings::default()
        })
        .theme(theme)
        .style(style)
        .run()
}

fn theme(app: &App) -> Theme {
    match app.theme_mode() {
        theme::Mode::Light => Theme::Light,
        theme::Mode::Dark => Theme::Dark,
    }
}

fn style(_app: &App, theme: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    }
}

/// Where a `window` docked to `edge` sits on a `monitor`: flush to the edge
/// and centred along it, but never starting before the monitor.
fn dock(edge: Edge, window: Size, monitor: Size) -> Point {
    let origin = edge.dock_origin(window, monitor, 0.0);
    match edge {
        Edge::Right | Edge::Left => Point::new(origin.x, origin.y.max(0.0)),
        Edge::Top => Point::new(origin.x.max(0.0), origin.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_hugs_each_edge() {
        let monitor = Size::new(1440.0, 900.0);
        let window = Size::new(400.0, 600.0);
        assert_eq!(
            dock(Edge::Right, window, monitor),
            Point::new(1040.0, 150.0)
        );
        assert_eq!(dock(Edge::Left, window, monitor), Point::new(0.0, 150.0));
        let wide = Size::new(600.0, 400.0);
        assert_eq!(dock(Edge::Top, wide, monitor), Point::new(420.0, 0.0));
        // Taller than the monitor: starts at its top, as before.
        let tall = Size::new(400.0, 1000.0);
        assert_eq!(dock(Edge::Right, tall, monitor), Point::new(1040.0, 0.0));
    }
}
