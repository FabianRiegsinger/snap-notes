//! Menu bar (macOS) / tray (Windows) icon with the app menu. The native
//! icon must live on the main thread, so it is created and changed through
//! `window::run` callbacks and kept in a thread local.

use crate::app::Message;

/// Ids of the menu items, in menu order.
#[cfg(any(windows, target_os = "macos", test))]
pub const MENU_IDS: [&str; 4] = ["toggle", "new", "settings", "quit"];

/// The app message a menu item id stands for.
pub fn message_for(id: &str) -> Option<Message> {
    match id {
        "toggle" => Some(Message::ToggleVisibility),
        "new" => Some(Message::AddNote),
        "settings" => Some(Message::ToggleSettings),
        "quit" => Some(Message::Quit),
        _ => None,
    }
}

/// A monochrome note glyph as RGBA: a sheet outline with three text lines in
/// `color` on transparent.
#[cfg(any(windows, target_os = "macos", test))]
pub fn icon_rgba(size: u32, color: [u8; 3]) -> Vec<u8> {
    let s = size as f32 / 36.0;
    let inside = |x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32| {
        x >= x0 * s && x < x1 * s && y >= y0 * s && y < y1 * s
    };
    let mut rgba = vec![0; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let sheet = inside(fx, fy, 7.0, 4.0, 29.0, 32.0);
            let hollow = inside(fx, fy, 10.0, 7.0, 26.0, 29.0);
            // Clip the sheet's corners for a slightly rounded look.
            let corner = [(7.0, 4.0), (28.0, 4.0), (7.0, 31.0), (28.0, 31.0)]
                .iter()
                .any(|&(cx, cy)| inside(fx, fy, cx, cy, cx + 1.0, cy + 1.0));
            let line = [12.0, 17.0, 22.0]
                .iter()
                .any(|&ly| inside(fx, fy, 13.0, ly, 23.0, ly + 2.0));
            if (sheet && !hollow && !corner) || line {
                let i = ((y * size + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }
    rgba
}

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use super::{icon_rgba, MENU_IDS};
    use std::cell::RefCell;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

    const ICON_SIZE: u32 = 36;
    /// macOS recolors the template icon itself; the Windows taskbar is dark
    /// by default and shows the icon as drawn.
    #[cfg(target_os = "macos")]
    const ICON_COLOR: [u8; 3] = [0, 0, 0];
    #[cfg(windows)]
    const ICON_COLOR: [u8; 3] = [255, 255, 255];

    thread_local! {
        /// The tray icon and its Show/Hide item, whose label follows the notes.
        static TRAY: RefCell<Option<(TrayIcon, MenuItem)>> = const { RefCell::new(None) };
    }

    fn toggle_label(notes_shown: bool) -> &'static str {
        if notes_shown {
            "Hide Notes"
        } else {
            "Show Notes"
        }
    }

    /// Creates the icon (hidden unless `visible`). Returns whether it worked.
    pub fn create(_window: &dyn iced::window::Window, notes_shown: bool, visible: bool) -> bool {
        let build = || -> Option<(TrayIcon, MenuItem)> {
            let [toggle, new, settings, quit] = MENU_IDS;
            let toggle_item = MenuItem::with_id(toggle, toggle_label(notes_shown), true, None);
            let menu = Menu::new();
            menu.append_items(&[
                &toggle_item,
                &MenuItem::with_id(new, "New Note", true, None),
                &MenuItem::with_id(settings, "Settings…", true, None),
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id(quit, "Quit Work Notes", true, None),
            ])
            .ok()?;
            let icon =
                Icon::from_rgba(icon_rgba(ICON_SIZE, ICON_COLOR), ICON_SIZE, ICON_SIZE).ok()?;
            let builder = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Work Notes");
            #[cfg(target_os = "macos")]
            let builder = builder.with_icon_templated(icon);
            #[cfg(windows)]
            let builder = builder.with_icon(icon);
            let tray = builder.build().ok()?;
            // Clicks and hovers on the icon itself are unused. Without a
            // handler they queue up in an unbounded channel forever.
            TrayIconEvent::set_event_handler(Some(|_| {}));
            tray.set_visible(visible).ok()?;
            Some((tray, toggle_item))
        };
        let Some(tray) = build() else {
            return false;
        };
        TRAY.with(|t| *t.borrow_mut() = Some(tray));
        true
    }

    pub fn set_visible(_window: &dyn iced::window::Window, visible: bool) {
        TRAY.with(|t| {
            if let Some((tray, _)) = &*t.borrow() {
                let _ = tray.set_visible(visible);
            }
        });
    }

    pub fn set_notes_shown(_window: &dyn iced::window::Window, shown: bool) {
        TRAY.with(|t| {
            if let Some((_, item)) = &*t.borrow() {
                item.set_text(toggle_label(shown));
            }
        });
    }

    /// Ids of clicked menu items, pushed as they happen so the app can
    /// sleep while idle instead of polling.
    pub fn menu_events() -> iced::Subscription<String> {
        iced::Subscription::run(menu_event_stream)
    }

    fn menu_event_stream() -> impl iced::futures::Stream<Item = String> {
        iced::stream::channel(16, async |sender| {
            let sender = std::sync::Mutex::new(sender);
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Ok(mut sender) = sender.lock() {
                    let _ = sender.try_send(event.id.as_ref().to_string());
                }
            }));
            // The handler feeds the channel; keep the stream alive.
            std::future::pending::<()>().await;
        })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod native {
    pub fn create(_window: &dyn iced::window::Window, _notes_shown: bool, _visible: bool) -> bool {
        false
    }

    pub fn set_visible(_window: &dyn iced::window::Window, _visible: bool) {}

    pub fn set_notes_shown(_window: &dyn iced::window::Window, _shown: bool) {}

    pub fn menu_events() -> iced::Subscription<String> {
        iced::Subscription::none()
    }
}

pub use native::{create, menu_events, set_notes_shown, set_visible};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_for_maps_ids() {
        assert!(matches!(
            message_for("toggle"),
            Some(Message::ToggleVisibility)
        ));
        assert!(matches!(message_for("new"), Some(Message::AddNote)));
        assert!(matches!(
            message_for("settings"),
            Some(Message::ToggleSettings)
        ));
        assert!(matches!(message_for("quit"), Some(Message::Quit)));
        assert!(message_for("x").is_none());
        for id in MENU_IDS {
            assert!(message_for(id).is_some(), "{id}");
        }
    }

    #[test]
    fn icon_is_drawn_in_the_given_color() {
        let rgba = icon_rgba(36, [255, 255, 255]);
        assert!(rgba
            .chunks(4)
            .filter(|p| p[3] > 0)
            .all(|p| p[..3] == [255, 255, 255]));
    }

    #[test]
    fn icon_has_opaque_and_transparent_pixels() {
        let rgba = icon_rgba(36, [0, 0, 0]);
        assert_eq!(rgba.len(), 36 * 36 * 4);
        let pixels: Vec<&[u8]> = rgba.chunks(4).collect();
        assert!(pixels.iter().any(|p| p[3] == 255));
        assert!(pixels.iter().any(|p| p[3] == 0));
        assert!(pixels
            .iter()
            .filter(|p| p[3] > 0)
            .all(|p| p[..3] == [0, 0, 0]));
    }
}
