//! Keyboard and pointer input: shortcuts, cursor tracking and polling, mouse
//! buttons, and the click-through (passthrough) area they decide.

use super::*;

impl App {
    pub(super) fn handle_input(&mut self, message: Message) -> Task<Message> {
        match message {
            // Hidden notes don't react to the keyboard.
            Message::Key(_) if !self.visible => {}
            Message::Key(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) => {
                use keyboard::key::Named;
                // Shortcuts go by the key's Latin letter, so they work on
                // any layout.
                let shortcut = crate::command_passthrough::shortcut_char(&key, physical_key);
                match shortcut {
                    Some(',') if modifiers.command() => {
                        return self.update(Message::ToggleSettings)
                    }
                    Some('n') if modifiers.command() => return self.update(Message::AddNote),
                    Some('f') if modifiers.command() => return self.toggle_search(),
                    // With a note open, Cmd/Ctrl+Z is its own undo (below);
                    // in a panel's field it is the field's.
                    Some('z')
                        if modifiers.command()
                            && !modifiers.shift()
                            && self.active_note.is_none()
                            && !self.panel_open() =>
                    {
                        return self.update(Message::UndoDelete)
                    }
                    _ => {}
                }
                match key.as_ref() {
                    // Esc first drops a delete question, and only that.
                    keyboard::Key::Named(Named::Escape) if self.delete_ask.is_some() => {
                        self.delete_ask = None
                    }
                    keyboard::Key::Named(Named::Escape) if self.color_picker_open => {
                        self.color_picker_open = false
                    }
                    keyboard::Key::Named(Named::Escape) if self.reminder_picker.is_some() => {
                        self.reminder_picker = None
                    }
                    keyboard::Key::Named(Named::Escape) if self.search_panel.morph.is_opening() => {
                        self.close_search()
                    }
                    keyboard::Key::Named(Named::Escape) if self.export_panel.is_showing() => {
                        self.close_export()
                    }
                    keyboard::Key::Named(Named::Escape) if self.settings_panel.open => {
                        return self.update(Message::CloseSettings)
                    }
                    keyboard::Key::Named(Named::Escape) if self.editing => {
                        return self.update(Message::EditorBlurred)
                    }
                    keyboard::Key::Named(Named::Escape) => return self.update(Message::ClosePanel),
                    // In edit mode the body editor's own key binding handles
                    // undo/redo; the title field ignores them, so skip it here.
                    // With the color bubble open, its hex field has the keys.
                    keyboard::Key::Character(_)
                        if !self.editing
                            && self.active_note.is_some()
                            && !self.color_picker_open =>
                    {
                        if let Some(step) =
                            shortcut.and_then(|c| crate::note_panel::history_key(c, modifiers))
                        {
                            return crate::note_panel::title_focused().then(move |focused| {
                                if focused {
                                    Task::none()
                                } else {
                                    Task::done(step.clone())
                                }
                            });
                        }
                    }
                    _ => {}
                }
            }
            Message::Key(_) => {}
            Message::CursorMoved(position) => {
                // A real OS file drag sends no cursor moves, so one means
                // the hover ended without an event.
                if self.drag.is_none() {
                    self.file_hover = false;
                }
                self.last_cursor = Some(position);
                self.pointer = Some(position);
                if let Some(offset) = self.note_drag {
                    self.note_drag_pos = Some(position - offset);
                }
                if let Some(offset) = self.settings_drag {
                    self.settings_drag_pos = Some(position - offset);
                }
                let room = self.note_room();
                if let Some(resize) = &mut self.note_resize {
                    resize.rect = resized(resize.start, resize.edges, position - resize.grab, room);
                }
                // Leaving the open peek (not toward the strip) closes it.
                let in_strip = self.near_edge(position, self.strip_depth());
                if self.cursor_y.is_some() && !in_strip && !self.cursor_on_peek() {
                    self.cursor_y = None;
                    self.animating = true;
                    self.update_hover();
                }
                return self.update_passthrough(Some(position));
            }
            Message::CursorLeftWindow => {
                self.last_cursor = None;
                self.pointer = None;
                return self.update_passthrough(None);
            }
            Message::MouseButton(down) => {
                self.mouse_down = down;
                if !down && self.note_drag.is_some() {
                    self.finish_note_drag();
                }
                if !down && self.settings_drag.is_some() {
                    self.finish_settings_drag();
                }
                if !down {
                    self.finish_note_resize();
                }
                // A click elsewhere ends the title's edit.
                if down && self.title_edit.is_some() {
                    return crate::note_panel::title_focused().map(Message::TitleBlurred);
                }
            }
            Message::PollCursor => {
                if let Some(id) = self.window_id {
                    return window::run(id, platform::cursor_in_window).map(Message::CursorPolled);
                }
            }
            Message::CursorPolled(position) => {
                self.pointer = position;
                if self.passthrough && position.is_some_and(|p| self.is_interactive(p)) {
                    return self.update_passthrough(position);
                }
            }
            _ => unreachable!("not an input message"),
        }
        Task::none()
    }

    /// Whether the window should catch the mouse at `position`: over the bar
    /// stack, the open note, the settings or the undo toast. Everywhere else
    /// clicks go to the apps behind.
    /// While auto-hide has the strip (partly) slid away, only the edge
    /// zone, alerting bars (and their peek), and an open note or panel
    /// count.
    pub(super) fn is_interactive(&self, position: Point) -> bool {
        if !self.visible {
            return false;
        }
        let over_panel = |frame: Option<MorphFrame>| {
            frame.is_some_and(|frame| frame.rect.expand(8.0).contains(position))
        };
        let strip_used = if self.strip_away_offset() > 0.0 {
            self.in_edge(position) || self.over_alerting_ui(position)
        } else {
            self.in_use_area(position)
        };
        // The color bubble, and the reminder calendar that shares its
        // overlay, can reach beyond the note.
        let over_bubble = (self.color_picker_open || self.reminder_picker.is_some())
            && self
                .color_bubble_rect
                .is_some_and(|rect| rect.expand(8.0).contains(position));
        strip_used
            || over_bubble
            || over_panel(self.note_frame())
            || over_panel(self.settings_frame())
            || over_panel(self.search_frame())
            || over_panel(self.export_frame())
    }

    /// `position`'s coordinate along the edge.
    pub(super) fn along(&self, position: Point) -> f32 {
        self.edge().to_local(position, self.window_size).along
    }

    /// Whether `position` is at most `depth` from the window's border on
    /// the strip's edge (or beyond it). On Top a negative y, in the menu bar
    /// above the window, counts too: the menu bar is part of the edge zone.
    pub(super) fn near_edge(&self, position: Point, depth: f32) -> bool {
        match self.edge() {
            Edge::Right => position.x >= self.window_size.width - depth,
            Edge::Left => position.x <= depth,
            Edge::Top => position.y <= depth,
        }
    }

    /// Whether `along` (a coordinate along the edge) is within the strip's
    /// band.
    pub(super) fn in_band(&self, along: f32) -> bool {
        let (start, end) = strip_band(&self.strip_layout(), self.settings.settings().bars.gap);
        (start..=end).contains(&along)
    }

    /// How far into the window the strip (and expanded Actions row) reaches.
    pub(super) fn strip_depth(&self) -> f32 {
        if self.actions_open {
            crate::bar_strip::actions_row_extent(
                &self.settings.settings().bars,
                self.entries().len(),
                |i| self.magnification.scale(i),
            )
        } else {
            STRIP_WIDTH
        }
    }

    /// Whether `position` is over the strip's column (its row on Top)
    /// within its band. While Actions is expanded this includes the pills
    /// that stick further into the screen than [`STRIP_WIDTH`].
    pub(super) fn over_strip(&self, position: Point) -> bool {
        self.near_edge(position, self.strip_depth()) && self.in_band(self.along(position))
    }

    /// Whether `position` is in the edge zone: the window's outermost
    /// `EDGE_ZONE` px on the strip's edge, anywhere along it (the window
    /// spans the whole edge with passthrough, and as the edge sliver).
    ///
    /// In the edge sliver that is anywhere inside the window.
    pub(super) fn in_edge(&self, position: Point) -> bool {
        if self.is_sliver() {
            let edge = self.edge();
            let across = match edge {
                Edge::Right | Edge::Left => position.x,
                Edge::Top => position.y,
            };
            return (0.0..=edge.thickness(self.window_size)).contains(&across);
        }
        self.near_edge(position, EDGE_ZONE)
    }

    /// Whether `position` is where the strip is in use: its column within
    /// the band, the open peek or the undo toast.
    pub(super) fn in_use_area(&self, position: Point) -> bool {
        self.over_strip(position)
            || self.peek_rect().is_some_and(|rect| rect.contains(position))
            || self
                .toast_rect()
                .is_some_and(|rect| rect.contains(position))
    }

    /// Whether `position` is over a bar (or its peek) that is alerting
    /// while the strip is otherwise slid away.
    pub(super) fn over_alerting_ui(&self, position: Point) -> bool {
        if self.pulsing.is_empty() {
            return false;
        }
        if self.peek_rect().is_some_and(|rect| rect.contains(position)) {
            return self.peek_note.is_some_and(|id| self.pulsing.contains(&id));
        }
        let alerts = self.bar_alert();
        self.strip_layout()
            .bars
            .iter()
            .enumerate()
            .any(|(i, bar)| alerts.get(i).copied().unwrap_or(false) && bar.contains(position))
    }

    pub(super) fn update_passthrough(&mut self, cursor: Option<Point>) -> Task<Message> {
        let Some(id) = self.window_id else {
            return Task::none();
        };
        let want = SUPPORTS_PASSTHROUGH
            && !self.mouse_down
            && self.drag.is_none()
            && cursor.is_none_or(|p| !self.is_interactive(p));
        if want == self.passthrough {
            return Task::none();
        }
        self.passthrough = want;
        if want {
            // No more cursor events will arrive, so drop the hover state now.
            self.cursor_y = None;
            self.update_hover();
            self.note_hovered = false;
            self.animating = true;
            window::enable_mouse_passthrough(id)
        } else {
            window::disable_mouse_passthrough(id)
        }
    }
}
