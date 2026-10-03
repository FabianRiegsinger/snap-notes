# Menu Bar Icon and Dock Visibility — Design Spec

## Overview

Add a menu bar icon (macOS) / tray icon (Windows) with a dropdown menu, and settings to hide the menu bar icon and the Dock icon (macOS) / taskbar button (Windows). At least one of the two always stays visible so the app remains reachable.

## Decisions

- **Click behavior:** clicking the icon opens a dropdown menu. There is no separate left-click action.
- **Platforms:** macOS and Windows. On Linux there is no tray icon and the new settings group is hidden.
- **Visibility rule:** both icons are optional, but never both hidden at once.
- **Library:** the `tray-icon` crate (0.26, re-exports `muda` for menus). It replaces the unused Windows-only `tray-item` dependency.

## Menu

| Item | Message | Notes |
|---|---|---|
| Show Notes / Hide Notes | `ToggleVisibility` | Label reflects the current state |
| New Note | `AddNote` | Also shows the notes if they are hidden |
| Settings… | `ToggleSettings` | Also shows the notes if they are hidden; opens the panel |
| Quit Work Notes | `Quit` | Saves notes and settings first (existing behavior) |

The menu item IDs are fixed strings (`"toggle"`, `"new"`, `"settings"`, `"quit"`). `tray::message_for(id: &str) -> Option<Message>` maps an ID to its message.

## Show/Hide Notes

`App.visible` already exists but nothing reads it yet. While it is `false`:

- `view` renders an empty transparent layer.
- Mouse passthrough is forced on, and `is_interactive` returns `false`.
- Any open note or settings panel is closed when hiding.
- Keyboard shortcuts are ignored.

## Tray Module (`src/tray.rs`, macOS and Windows only)

- **Creation:** the tray is created on the main thread inside the `window::run` callback on `WindowReady` (the same pattern as `platform::disable_native_shadow`). This guarantees the native application object exists.
- **Storage:** the `TrayIcon` handle and the toggle `MenuItem` live in a `thread_local!` because they are not `Send`. `tray::set_visible(bool)` adds or removes the icon, and `tray::set_notes_shown(bool)` updates the toggle label. Both run through `window::run` so they stay on the main thread.
- **Events:** `muda::MenuEvent::receiver()` is polled by an iced subscription every 100 ms. The poll runs only while the tray exists. Each event becomes `Message::TrayMenu(String)`, which is mapped with `message_for`.
- **Icon:** a 36×36 monochrome icon (a rounded note outline with three text lines), drawn in code by `tray::icon_rgba`, so the app needs no image asset or PNG decoder. On macOS it is marked as a template icon (`with_icon_as_template(true)`) so the system tints it for light and dark menu bars. Windows uses the same image.
- **Failure:** if tray creation fails, the error is ignored and the app keeps running without a tray. The settings still keep the Dock icon on in that case (see Settings).

## Dock / Taskbar Visibility (`src/platform.rs`)

`pub fn set_dock_icon_visible(window: &dyn iced::window::Window, visible: bool)`:

- **macOS:** calls `NSApplication::sharedApplication(mtm).setActivationPolicy(Regular | Accessory)`. Switching to `Accessory` removes the Dock icon and the app menu. Afterwards the window is re-raised, because changing the policy can hide it.
- **Windows:** toggles `WS_EX_TOOLWINDOW` on the window's extended style with `GetWindowLongPtrW`/`SetWindowLongPtrW`, then calls `SetWindowPos` with `SWP_FRAMECHANGED`. The window is hidden and re-shown around the change so the taskbar picks it up.
- **Other platforms:** does nothing.

This is applied on `WindowReady` and whenever the setting changes.

## Settings

New group `AppSettings`, stored under the `"app"` key in `settings.json`:

```rust
pub struct AppSettings {
    pub show_menu_bar_icon: bool, // default true
    pub show_dock_icon: bool,     // default true
}
```

- **Loading:** lenient, like the other groups. A missing or non-bool value falls back to the default. If both values are `false`, `show_menu_bar_icon` is forced to `true`.
- **Toggling:** `SettingToggle` has two variants, `MenuBarIcon` and `DockIcon`. `Settings::toggle(t)` flips the value unless that would leave both hidden, in which case it does nothing and returns `false`.
- **Message:** `Message::SettingToggled(SettingToggle)` calls `toggle`, then applies the change through the tray or platform calls.
- **Reset:** `SettingsGroup::App` resets to both shown.
- **Panel:** a new "App" section with an iced `toggler` per setting. The toggle that would hide the last visible icon is shown disabled. Labels:
  - macOS: "Show menu bar icon", "Show Dock icon"
  - Windows: "Show tray icon", "Show taskbar button"
  - The section is hidden on Linux.
- **Tray failure:** if the tray failed to start, the Dock toggle is disabled while the Dock icon is shown, so the app can't become unreachable.

## Testing

- `settings.rs`:
  - The defaults are both `true`.
  - `from_json` reads the `"app"` key, ignores non-bool values, and turns both-false into menu-bar-on.
  - `toggle` refuses to hide the last visible icon.
  - `reset(App)` restores both.
- `tray.rs`: `message_for` maps every menu ID to its message and returns `None` for unknown IDs.
- `app.rs`: no new unit tests. Native behavior is checked by hand.
- Manual check on macOS:
  - The icon appears in the menu bar and every menu item works.
  - Hiding the Dock icon removes it and the notes stay usable.
  - Turning off one icon disables the other's toggle.
  - Both settings persist across restart.

## Out of Scope

- Linux tray support.
- A left-click action that differs from opening the menu.
- Launch at login.
