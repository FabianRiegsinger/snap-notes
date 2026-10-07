// No console window next to the app on Windows.
#![windows_subsystem = "windows"]

mod animation;
mod app;
mod autohide;
mod bar_strip;
mod color_bubble;
mod color_picker;
mod color_wheel;
mod command_passthrough;
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
use iced::window;
use iced::{Color, Point, Size, Theme};

fn main() -> iced::Result {
    platform::prepare_graphics();
    platform::enforce_single_instance();

    let mut app = iced::application(App::boot, App::update, App::view);
    for font in icons::FONTS {
        app = app.font(font);
    }
    app.default_font(icons::BODY_FONT)
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

fn dock_right(window: Size, monitor: Size) -> Point {
    Point::new(
        monitor.width - window.width,
        ((monitor.height - window.height) / 2.0).max(0.0),
    )
}
