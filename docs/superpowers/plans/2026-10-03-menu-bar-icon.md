# Menu Bar Icon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A menu bar / tray icon with a Show/Hide, New Note, Settings… and Quit menu, plus settings to hide the menu bar icon and the Dock/taskbar icon (never both).

**Architecture:** `src/tray.rs` owns the native tray (created on the main thread through `window::run`, stored in a `thread_local!`), and an iced subscription polls its menu events. `platform.rs` gains `set_dock_icon_visible`. `Settings` gains an `AppSettings` group with two bools, and the settings panel shows them as togglers.

**Tech Stack:** Rust 2021, iced 0.14, `tray-icon` 0.26 (macOS/Windows only), objc2-app-kit 0.3, windows-sys 0.59.

**Spec:** `docs/superpowers/specs/2026-10-03-menu-bar-icon-design.md`

## Global Constraints

- Menu IDs: `"toggle"`, `"new"`, `"settings"`, `"quit"`. Labels: "Show Notes" / "Hide Notes", "New Note", "Settings…", "Quit Work Notes".
- `AppSettings` defaults are both `true`. It is stored under `"app"` with fields `show_menu_bar_icon` and `show_dock_icon`.
- Both icons must never be hidden at once, whether they come from the file, a toggle or a reset.
- Tray and Dock code is compiled only for `cfg(any(windows, target_os = "macos"))`. Linux builds and tests must still pass.
- Remove the `tray-item` dependency.
- After every task, `cargo test` passes and `cargo clippy --all-targets -- -D warnings` is clean.

## Review Focus

1. **Tray creation fails** (no menu bar or status area). The app runs, and the Dock toggle stays locked on. (Task 4: `tray_ok` flag gates the toggle; Task 2 test `dock_toggle_locked_without_tray`.)
2. **The settings file sets `"app":{"show_menu_bar_icon":false,"show_dock_icon":false}`.** Loading restores the menu bar icon. (Task 1: `both_hidden_in_file_restores_menu_bar_icon`.)
3. **New Note or Settings… is chosen while the notes are hidden.** The notes become visible first. (Task 4: handled in `TrayMenu` dispatch; Task 3 test `message_for_maps_ids`.)
4. **Hiding the notes while a note is mid-edit.** The note closes and its edits are saved through the existing dirty/debounce path. (Task 4, manual check.)
5. **A toggle is pressed rapidly** (both toggles flipping within one frame). The invariant still holds, because `toggle()` refuses the second flip. (Task 1: `toggle_refuses_to_hide_last_icon`.)

---

### Task 1: `AppSettings` in the model

**Files:** Modify `src/settings.rs`.

**Interfaces:**
- **Produces:**
  - `pub struct AppSettings { pub show_menu_bar_icon: bool, pub show_dock_icon: bool }`, deriving `Debug, Clone, PartialEq, Serialize` with a `Default` of both `true`.
  - `Settings.app: AppSettings`.
  - `SettingsGroup::App` with label "App".
  - `pub enum SettingToggle { MenuBarIcon, DockIcon }`, deriving `Debug, Clone, Copy, PartialEq, Eq`.
  - `Settings::is_on(&self, SettingToggle) -> bool`
  - `Settings::toggle(&mut self, SettingToggle) -> bool`, which returns whether anything changed.
  - `Settings::can_toggle(&self, SettingToggle) -> bool`, which is false when the toggle is on and the other one is off.
- **Behavior:**
  - `from_json` reads `app.show_menu_bar_icon` and `app.show_dock_icon` with `as_bool`. Afterwards, if both are false, `show_menu_bar_icon = true`.
  - `reset(App)` restores the defaults.

- [ ] **Step 1: Write the failing tests:**
  - `app_defaults_show_both`
  - `app_settings_load_from_json`: `{"app":{"show_dock_icon":false}}` gives dock false and menu bar true.
  - `app_settings_ignore_non_bool`: `{"app":{"show_dock_icon":"no"}}` gives dock true.
  - `both_hidden_in_file_restores_menu_bar_icon`
  - `toggle_refuses_to_hide_last_icon`:
    - `toggle(DockIcon)` returns true.
    - `toggle(MenuBarIcon)` then returns false and the value stays true.
    - `can_toggle(MenuBarIcon)` is false.
  - `reset_app_group_shows_both`
  - `json_roundtrip`: already exists, and now covers `app`.
- [ ] **Step 2:** `cargo test settings::` fails (compile errors for the missing items).
- [ ] **Step 3:** Implement the changes.
- [ ] **Step 4:** `cargo test` and clippy are clean.
- [ ] **Step 5:** Commit `feat: app visibility settings with at-least-one-icon rule`.

### Task 2: App section in the settings panel

**Files:** Modify `src/settings_panel.rs`, `src/app.rs` (messages).

**Interfaces:**
- **Consumes:** `SettingToggle`, `Settings::{is_on, can_toggle, toggle}` and `SettingsGroup::App` (Task 1).
- **Produces:**
  - `Message::SettingToggled(SettingToggle)`
  - `SettingsView` gains `tray_ok: bool`.
  - `pub fn toggle_enabled(settings: &Settings, t: SettingToggle, tray_ok: bool) -> bool` in `settings_panel.rs`. It returns `can_toggle(t)`, except that `DockIcon` is disabled while it is on and `!tray_ok`.
  - `fn toggle_label(t: SettingToggle) -> &'static str` with platform-specific labels:
    - macOS: "Show menu bar icon", "Show Dock icon"
    - Windows: "Show tray icon", "Show taskbar button"
- **App behavior:**
  - On `SettingToggled`, the app calls `settings_mut().toggle(t)`. Applying the change natively happens in Task 4; for now the message only updates the setting.
  - `ResetGroup(App)` goes through the existing reset path.
- **Panel layout:** the App section comes first, above Bars, and only under `cfg(any(windows, target_os = "macos"))`. It shows a group header with Reset, then one `toggler(settings.is_on(t)).label(toggle_label(t)).text_size(12)` per toggle. `.on_toggle(move |_| Message::SettingToggled(t))` is set only when `toggle_enabled` is true.

- [ ] **Step 1: Write the failing tests** in `settings_panel.rs` `mod tests`:
  - `dock_toggle_locked_without_tray`: with defaults and `tray_ok = false`, `toggle_enabled(DockIcon)` is false and `toggle_enabled(MenuBarIcon)` is true.
  - `last_icon_toggle_disabled`: with dock off, `toggle_enabled(MenuBarIcon, true)` is false.
- [ ] **Step 2:** The tests fail.
- [ ] **Step 3:** Implement the changes. `App` passes `tray_ok: false` until Task 4.
- [ ] **Step 4:** `cargo test` and clippy are clean.
- [ ] **Step 5:** Commit `feat: App section with icon visibility toggles`.

### Task 3: Tray module

**Files:**
- Create: `src/tray.rs`
- Modify: `Cargo.toml`, `src/main.rs`

**Interfaces:**
- **Produces**, all compiled on every platform unless noted:
  - `pub const MENU_IDS: [&str; 4]`
  - `pub fn message_for(id: &str) -> Option<Message>`, which maps `toggle` → `ToggleVisibility`, `new` → `AddNote`, `settings` → `ToggleSettings` and `quit` → `Quit`.
  - `pub fn icon_rgba(size: u32) -> Vec<u8>`: a procedural monochrome icon. It draws a rounded note outline plus 3 short horizontal "text" bars in opaque black on a transparent background, at `size`×`size`.
- **Produces**, macOS/Windows only:
  - `pub fn create(window: &dyn iced::window::Window, notes_shown: bool, visible: bool) -> bool`, which returns success.
  - `pub fn set_visible(visible: bool)`
  - `pub fn set_notes_shown(shown: bool)`
  - `pub fn poll() -> Vec<String>`, which drains `MenuEvent::receiver().try_recv()` IDs.
- **Produces**, other platforms: stubs where `create` returns false and `poll` returns an empty `Vec`.
- **Implementation notes:**
  - `thread_local! { static TRAY: RefCell<Option<(TrayIcon, MenuItem)>> }`
  - Build the icon with `TrayIconBuilder::new().with_menu(Box::new(menu)).with_icon(Icon::from_rgba(icon_rgba(36), 36, 36)?).with_icon_as_template(true).with_tooltip("Work Notes")`.
- **Cargo:**
  - Add `tray-icon = "0.26"` under `[target.'cfg(any(windows, target_os = "macos"))'.dependencies]`. If 0.26 conflicts with the existing objc2 0.6, use the newest version that builds.
  - Remove `tray-item`.

- [ ] **Step 1: Write the failing tests:**
  - `message_for_maps_ids`: each of the four IDs maps to the expected variant (use `matches!`), and `message_for("x")` is `None`.
  - `icon_has_opaque_and_transparent_pixels`: `icon_rgba(36)` has length 36·36·4, contains at least one alpha 255 and at least one alpha 0, and every opaque pixel is black (rgb 0, 0, 0).
- [ ] **Step 2:** The tests fail.
- [ ] **Step 3:** Implement the module and the Cargo changes.
- [ ] **Step 4:** `cargo test` and clippy are clean. `cargo build` succeeds on macOS.
- [ ] **Step 5:** Commit `feat: tray module with menu and procedural icon`.

### Task 4: Wire tray, visibility and Dock control into the app

**Files:** Modify `src/app.rs`, `src/platform.rs`, `Cargo.toml` (objc2-app-kit features `NSApplication`, `NSRunningApplication`; windows-sys already has `Win32_UI_WindowsAndMessaging`).

**Interfaces:**
- **Consumes:** Tasks 1–3.
- **Produces:**
  - `platform::set_dock_icon_visible(window: &dyn iced::window::Window, visible: bool)`:
    - macOS: `NSApplication::sharedApplication(MainThreadMarker::new()?)`, then `.setActivationPolicy(Regular | Accessory)`, then `activate()`.
    - Windows: toggle `WS_EX_TOOLWINDOW` in `GWL_EXSTYLE`, call `ShowWindow(SW_HIDE)`, then `SetWindowPos(.., SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER)`, then `ShowWindow(SW_SHOWNOACTIVATE)`.
    - Other platforms: no-op.
  - `App.tray_ok: bool`
  - `Message::TrayReady(bool)`
  - `Message::TrayMenu(String)`
- **App behavior:**
  - **`WindowReady`:** add `window::run(id, |w| tray::create(w, true, show_menu_bar_icon)).map(Message::TrayReady)` and `window::run(id, |w| platform::set_dock_icon_visible(w, show_dock_icon))`. Capture the bools before the closures.
  - **`TrayReady(ok)`:** sets `tray_ok`. If `!ok` and the Dock icon is hidden, force `show_dock_icon = true`, mark settings dirty and apply.
  - **Tray subscription:** `iced::time::every(100ms)` that maps to `Message::TrayPoll` while `tray_ok`. `TrayPoll` drains `tray::poll()` and returns `Task::batch` of `update(TrayMenu(id))`.
  - **`TrayMenu(id)`:** maps through `message_for`. For `AddNote` and `ToggleSettings`, it first sets `visible = true` (and updates passthrough), then forwards the message.
  - **`ToggleVisibility`:**
    - Flips `visible`. When hiding, it sends `ClosePanel` and `CloseSettings` and calls `hide_peek`.
    - It calls `tray::set_notes_shown(visible)` through `window::run`, and `update_passthrough(None)`.
  - **When hidden:**
    - `view` returns `Space::new().width(Fill).height(Fill)`.
    - `is_interactive` returns false.
    - `Key` events are ignored.
  - **`SettingToggled(t)`:** after toggling, run the native apply for that toggle through `window::run`: `tray::set_visible` for `MenuBarIcon`, `set_dock_icon_visible` for `DockIcon`.
  - **`ResetGroup(App)`:** applies both.
  - `settings_panel` receives `tray_ok: self.tray_ok`.

- [ ] **Step 1:** Implement the changes. There are no new unit tests here; the logic is covered by the tests in Tasks 1–3. Mark Step 2 as the gate.
- [ ] **Step 2:** `cargo test` and clippy are clean, and `cargo run` starts with the icon in the menu bar.
- [ ] **Step 3:** Commit `feat: menu bar icon, show/hide notes and Dock visibility`.

### Task 5: Verification

- [ ] **Step 1:** `cargo fmt --check`, clippy and `cargo test` all pass.
- [ ] **Step 2:** Run `cargo run` and check the result (the user does the GUI checks if the agent can't):
  - every menu item works
  - Hide leaves nothing on screen and no click blocking, and Show restores the notes
  - the Dock toggle removes and restores the Dock icon
  - the toggle that would hide the last icon is disabled
  - the settings persist across restart
- [ ] **Step 3:** Commit any fmt fixes.
