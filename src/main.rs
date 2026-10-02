mod animation;
mod app;
mod bar_strip;
mod color_picker;
mod note;
mod note_panel;
mod platform;
mod store;

use app::App;
use iced::window;
use iced::Theme;

fn main() -> iced::Result {
    platform::enforce_single_instance();

    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .title("Work Notes")
        .transparent(true)
        .decorations(false)
        .level(window::Level::AlwaysOnTop)
        .window_size((80, 600))
        .resizable(false)
        .theme(theme)
        .run()
}

fn theme(_app: &App) -> Theme {
    Theme::Dark
}
