mod animation;
mod app;
mod bar_strip;
mod color_picker;
mod images;
mod note;
mod note_panel;
mod pass_wheel;
mod peek;
mod platform;
mod press_through;
mod resize;
mod rich;
mod rich_view;
mod settings;
mod settings_panel;
mod store;
mod tray;

use app::App;
use iced::window;
use iced::{Color, Point, Size, Theme};

fn main() -> iced::Result {
    platform::enforce_single_instance();

    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .title("Snap Notes")
        .transparent(true)
        .decorations(false)
        .level(window::Level::AlwaysOnTop)
        .window_size((settings::Settings::default().open_width(), 600.0))
        .position(window::Position::SpecificWith(dock_right))
        .resizable(false)
        .theme(theme)
        .style(style)
        .run()
}

fn theme(_app: &App) -> Theme {
    Theme::Dark
}

fn style(_app: &App, theme: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    }
}

fn dock_right(window: Size, monitor: Size) -> Point {
    Point::new(
        monitor.width - window.width,
        ((monitor.height - window.height) / 2.0).max(0.0),
    )
}
