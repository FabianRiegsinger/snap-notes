//! The window: docking it to the edge, switching edges, showing and hiding the
//! app, the tray and the global hotkey.

use super::*;

impl App {
    pub(super) fn handle_docking(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ThemeChanged(mode) => {
                self.theme = theme::Theme::new(mode);
            }
            Message::EdgeChosen(edge) => return self.change_edge(edge),
            Message::ToggleVisibility => {
                self.visible = !self.visible;
                let mut tasks = Vec::new();
                if !self.visible {
                    self.close_search();
                    self.close_export();
                    tasks.push(self.update(Message::CloseSettings));
                    tasks.push(self.update(Message::ClosePanel));
                    self.hide_peek();
                }
                self.animating |= self.pulse_running();
                if let Some(id) = self.window_id {
                    let shown = self.visible;
                    tasks.push(window::run(id, move |w| tray::set_notes_shown(w, shown)).discard());
                }
                tasks.push(self.update_passthrough(self.last_cursor));
                return Task::batch(tasks);
            }
            Message::HotkeyPressed => {
                self.force_reveal = true;
                let mut tasks = Vec::new();
                if !self.visible {
                    tasks.push(self.update(Message::ToggleVisibility));
                }
                // Opening the new note closes settings, search and export.
                tasks.push(self.update(Message::AddNote));
                self.pending_focus = Some(PendingFocus::Body);
                // The hotkey is pressed in another app; bring the window to
                // the front (on macOS this also activates the app).
                if let Some(id) = self.window_id {
                    tasks.push(window::gain_focus(id));
                }
                return Task::batch(tasks);
            }
            Message::TrayReady(ok) => {
                self.tray_ok = ok;
                // Without a tray icon the Dock icon is the only way back in.
                // Forced for this session only; the saved choice stays.
                self.tray_failed = !ok;
                if !ok {
                    return self.apply_app_visibility();
                }
            }
            Message::TrayMenu(id) => {
                let Some(message) = tray::message_for(&id) else {
                    return Task::none();
                };
                let mut tasks = Vec::new();
                // Adding a note or opening a panel needs the notes on screen.
                let settings_showing = self.settings_panel.is_showing();
                let search_showing = self.search_panel.is_showing();
                let export_showing = self.export_panel.is_showing();
                let showing = match message {
                    Message::ToggleSettings => settings_showing,
                    Message::ToggleSearch => search_showing,
                    Message::ToggleExport => export_showing,
                    _ => false,
                };
                if showing {
                    // The menu items open their panel; they never close it.
                    return Task::none();
                }
                let needs_notes = matches!(
                    message,
                    Message::AddNote
                        | Message::ClipboardNote
                        | Message::ToggleSettings
                        | Message::ToggleSearch
                        | Message::ToggleExport
                );
                if needs_notes && !self.visible {
                    tasks.push(self.update(Message::ToggleVisibility));
                }
                self.force_reveal |= needs_notes;
                tasks.push(self.update(message));
                return Task::batch(tasks);
            }
            Message::Quit => {
                let _ = self.finish_collapse();
                let _ = self.store.save();
                let _ = self.settings.save();
                std::process::exit(0);
            }
            Message::WindowReady(id, monitor, top_inset) => {
                self.window_id = Some(id);
                self.monitor = monitor;
                self.top_inset = top_inset;
                let shadow = window::run(id, platform::disable_native_shadow).discard();
                let dock = self.dock_window();
                let show_icon = self.settings.settings().app.show_menu_bar_icon;
                let tray = window::run(id, move |w| tray::create(w, true, show_icon))
                    .map(Message::TrayReady);
                self.sync_hotkey();
                return Task::batch([
                    shadow,
                    dock,
                    self.update_passthrough(None),
                    tray,
                    self.apply_app_visibility(),
                ]);
            }
            Message::WindowResized(size) => {
                self.window_size = size;
            }
            Message::WindowUnfocused
                if self.keep_open_until.is_some_and(|t| Instant::now() < t) => {}
            Message::WindowUnfocused => {
                // Clicking another app (through the passthrough area) closes
                // the note and the panels.
                self.close_search();
                // The save dialog takes the focus as long as it is open.
                if !self.export_dialog_open {
                    self.close_export();
                }
                let close_settings = self.update(Message::CloseSettings);
                return Task::batch([close_settings, self.update(Message::ClosePanel)]);
            }
            _ => unreachable!("not a docking message"),
        }
        Task::none()
    }

    /// Window thickness away from the edge (the width on Right and Left,
    /// the height on Top): room for an open note when `wide` (see
    /// `needs_wide_window`), else the edge sliver when `sliver`, else just
    /// the strip. With passthrough the window is always wide, so opening and
    /// closing never resize it (a resize is a separate move + resize and
    /// flickers for a frame).
    pub(super) fn docked_thickness(settings: &Settings, wide: bool, sliver: bool) -> f32 {
        if wide {
            settings.open_width()
        } else if sliver {
            EDGE_SLIVER
        } else {
            STRIP_WIDTH
        }
    }

    /// Whether the window shrinks to the edge sliver: without
    /// `passthrough`, while auto-hide has the strip fully hidden and
    /// nothing needs the wide window. A reveal widens it before sliding.
    pub(super) fn wants_sliver(&self, passthrough: bool) -> bool {
        !passthrough && self.auto_hide.is_hidden() && !self.needs_wide_window(passthrough)
    }

    /// The window is currently the edge sliver.
    pub(super) fn is_sliver(&self) -> bool {
        self.edge().thickness(self.window_size) <= EDGE_SLIVER
    }

    /// Whether the window needs room on the strip's away side: always with
    /// `passthrough`, otherwise for an open note or panel, a chip that
    /// stays up on its own, such as the undo toast (hover chips don't
    /// widen it), or an alerting bar that must stay visible while the
    /// strip is otherwise auto-hidden.
    pub(super) fn needs_wide_window(&self, passthrough: bool) -> bool {
        passthrough
            || self.active_note.is_some()
            || self.settings_panel.open
            || self.search_panel.open
            || self.export.is_some()
            || self.clipboard_empty_at.is_some()
            || self.last_deleted.is_some()
            || !self.pulsing.is_empty()
            || self.actions_open
            || !self.actions_morph.is_closed()
    }

    /// The docked window's thickness away from the edge (see
    /// `docked_thickness`), with or without `passthrough`.
    pub(super) fn window_thickness(&self, passthrough: bool) -> f32 {
        Self::docked_thickness(
            self.settings.settings(),
            self.needs_wide_window(passthrough),
            self.wants_sliver(passthrough),
        )
    }

    /// How much of the edge's length the docked window spans without
    /// `passthrough`: all of it as the edge sliver, so the cursor can reveal
    /// the strip anywhere along the edge, else the configured fraction.
    pub(super) fn dock_fraction(&self, passthrough: bool) -> f32 {
        if self.wants_sliver(passthrough) {
            1.0
        } else {
            self.settings.settings().window.height_fraction
        }
    }

    /// Docks the window to the chosen screen edge, centred along it. The
    /// strip is centred inside, so it sits at the middle of that screen
    /// border. Does nothing if the window is already there.
    pub(super) fn dock_window(&mut self) -> Task<Message> {
        let (Some(id), Some(monitor)) = (self.window_id, self.monitor) else {
            return Task::none();
        };
        // With passthrough the window covers the whole screen, so an open note
        // can be dragged anywhere and the strip sits at the exact center of
        // its edge.
        let edge = self.edge();
        let frame = dock_frame(
            edge,
            monitor,
            self.top_inset,
            SUPPORTS_PASSTHROUGH,
            self.window_thickness(SUPPORTS_PASSTHROUGH),
            self.dock_fraction(SUPPORTS_PASSTHROUGH),
        );
        let (origin, size) = (frame.position(), frame.size());
        if size == self.window_size && self.window_origin == Some(origin) {
            return Task::none();
        }
        // The side on the screen edge stays put, so a cursor that doesn't
        // move keeps its screen spot; e.g. on the strip after the sliver
        // widens to reveal. A shrink can leave it outside, without a
        // `CursorLeft`.
        if !SUPPORTS_PASSTHROUGH {
            self.pointer = match self.window_origin {
                Some(old) => pointer_after_dock(
                    self.pointer,
                    edge,
                    Rectangle::new(old, self.window_size),
                    frame,
                ),
                None => pointer_after_resize(
                    self.pointer,
                    edge,
                    edge.thickness(self.window_size),
                    edge.thickness(size),
                ),
            };
        }
        self.window_size = size;
        self.window_origin = Some(origin);
        Task::batch([window::move_to(id, origin), window::resize(id, size)])
    }

    /// The screen edge the strip docks to.
    pub(super) fn edge(&self) -> Edge {
        self.settings.settings().window.edge
    }

    pub(super) fn dock_icon_shown(&self) -> bool {
        self.settings.settings().app.show_dock_icon || self.tray_failed
    }

    /// Registers or unregisters the global hotkey to match the setting.
    /// Only the macOS/Windows settings show the toggle; elsewhere it is
    /// always on. Waits for the window, so the event loop is running.
    pub(super) fn sync_hotkey(&mut self) {
        if self.window_id.is_none() {
            return;
        }
        let settings = self.settings.settings();
        let wanted = (!cfg!(any(windows, target_os = "macos"))
            || settings.is_on(SettingToggle::GlobalHotkey))
            && !self.hotkey_recording;
        let combo = settings.app.global_hotkey_combo.clone();
        if !wanted {
            self.hotkey = None;
            self.hotkey_error = None;
        } else if self.hotkey.as_ref().is_none_or(|h| h.combo != combo) {
            // Let go of the old combo before taking the new one.
            self.hotkey = None;
            self.apply_registration(hotkey::register(&combo));
        }
    }

    /// Makes a recorded `combo` the global hotkey.
    pub(super) fn adopt_hotkey(&mut self, combo: hotkey::Combo) {
        if self.window_id.is_none() {
            self.hotkey_recording = false;
            self.hotkey_hint = None;
            self.settings.settings_mut().app.global_hotkey_combo = combo;
            return;
        }
        self.adopt_hotkey_with(combo, hotkey::register);
    }

    /// [`App::adopt_hotkey`] registering through `register`. A combo that
    /// can't be registered isn't kept: the previous one is taken back and
    /// the red line names the one that failed.
    pub(super) fn adopt_hotkey_with(
        &mut self,
        combo: hotkey::Combo,
        register: impl FnOnce(&hotkey::Combo) -> Result<hotkey::Manager, String>,
    ) {
        self.hotkey_recording = false;
        self.hotkey_hint = None;
        let wanted = !cfg!(any(windows, target_os = "macos"))
            || self.settings.settings().is_on(SettingToggle::GlobalHotkey);
        if !wanted {
            self.settings.settings_mut().app.global_hotkey_combo = combo;
            return;
        }
        // Let go of the old combo before taking the new one.
        self.hotkey = None;
        match register(&combo) {
            Ok(manager) => {
                self.settings.settings_mut().app.global_hotkey_combo = combo;
                self.apply_registration(Ok(manager));
            }
            Err(reason) => {
                let failed = format!("{} ({reason})", combo.label(cfg!(target_os = "macos")));
                self.sync_hotkey();
                self.hotkey_error = Some(failed);
            }
        }
    }

    /// Keeps the registered hotkey, or the reason it failed.
    pub(super) fn apply_registration(&mut self, result: Result<hotkey::Manager, String>) {
        match result {
            Ok(manager) => {
                self.hotkey = Some(manager);
                self.hotkey_error = None;
            }
            Err(reason) => {
                self.hotkey = None;
                self.hotkey_error = Some(reason);
            }
        }
    }

    /// Shows or hides the menu bar icon and the Dock icon to match settings.
    pub(super) fn apply_app_visibility(&self) -> Task<Message> {
        let Some(id) = self.window_id else {
            return Task::none();
        };
        let menu_bar = self.settings.settings().app.show_menu_bar_icon;
        let dock = self.dock_icon_shown();
        Task::batch([
            window::run(id, move |w| tray::set_visible(w, menu_bar)).discard(),
            window::run(id, move |w| platform::set_dock_icon_visible(w, dock)).discard(),
        ])
    }

    /// Docks the strip to `edge` right away (E10).
    pub(super) fn change_edge(&mut self, edge: Edge) -> Task<Message> {
        let old = self.edge();
        if edge == old {
            return Task::none();
        }
        self.settings.settings_mut().window.edge = edge;
        self.edge_changed(old)
    }

    /// Follows the edge setting having changed from `old` (E10): the peek,
    /// the hover and any chip close, a drag is cancelled, an open note or
    /// panel stays and re-anchors on its bar or slot, and the window
    /// re-docks.
    pub(super) fn edge_changed(&mut self, old: Edge) -> Task<Message> {
        let edge = self.edge();
        debug_assert_ne!(old, edge, "the edge didn't change");
        self.hide_peek();
        self.cursor_y = None;
        self.drag = None;
        self.clipboard_empty_at = None;
        self.set_actions_open(false);
        let (old_origin, pointer, last_cursor) =
            (self.window_origin, self.pointer, self.last_cursor);
        let dock = self.apply_settings();
        // Positions seen before are in the old window's frame: the cursor
        // keeps its screen spot, if that is still in the window.
        let shift = old_origin
            .zip(self.window_origin)
            .map(|(old, new)| old - new);
        let window = Rectangle::new(Point::ORIGIN, self.window_size);
        let moved = |p: Option<Point>| {
            shift
                .and_then(|shift| Some(p? + shift))
                .filter(|p| window.contains(*p))
        };
        self.pointer = moved(pointer);
        self.last_cursor = moved(last_cursor);
        // Re-anchor in the docked window.
        self.follow_open_bar();
        let strip = self.strip_layout();
        for (panel, slot) in [
            (&mut self.settings_panel, strip.settings_anchor()),
            (&mut self.search_panel, strip.search_anchor()),
            (&mut self.export_panel, strip.settings_anchor()),
        ] {
            panel.anchor_along = along_center(slot, edge);
        }
        Task::batch([dock, self.update_passthrough(self.pointer)])
    }
}
