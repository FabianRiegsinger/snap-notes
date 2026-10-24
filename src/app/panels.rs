//! The settings, search and export panels: one [`Panel`] type for their shared
//! fold and placement, plus each panel's own messages.

use super::*;

/// What the settings, search and export panels share: whether one is
/// showing, its unfold morph, where along the edge it is centered and,
/// for a panel with a grip (settings, search), where it was dragged to.
pub(super) struct Panel {
    /// Showing (kept while it folds back into its strip slot).
    pub(super) open: bool,
    pub(super) morph: Morph,
    /// Slot center along the edge when the panel opened; the panel stays
    /// centered on it.
    pub(super) anchor_along: f32,
    /// Top-left after a drag (cleared on reset); the panel stays there
    /// when it opens again.
    pub(super) pos: Option<Point>,
    /// Grab offset (cursor minus panel top-left) while dragging.
    pub(super) drag: Option<Vector>,
    /// Live top-left while dragging.
    pub(super) drag_pos: Option<Point>,
}

impl Panel {
    pub(super) fn new(speed: f32) -> Self {
        Self {
            open: false,
            morph: Morph::new(speed),
            anchor_along: 0.0,
            pos: None,
            drag: None,
            drag_pos: None,
        }
    }

    /// Starts dragging the panel shown at `top_left` by a press at `cursor`.
    pub(super) fn start_drag(&mut self, cursor: Point, top_left: Point) {
        self.drag = Some(cursor - top_left);
        self.drag_pos = Some(top_left);
    }

    /// Moves a dragged panel with the cursor at `position`.
    pub(super) fn drag_to(&mut self, position: Point) {
        if let Some(offset) = self.drag {
            self.drag_pos = Some(position - offset);
        }
    }

    /// Ends a drag without moving the panel.
    pub(super) fn cancel_drag(&mut self) {
        self.drag = None;
        self.drag_pos = None;
    }

    /// Where a dragged position puts the panel, if it has one.
    pub(super) fn top_left(&self) -> Option<Point> {
        self.drag_pos.or(self.pos)
    }

    /// Docks the panel back beside the strip.
    pub(super) fn reset_position(&mut self) {
        self.pos = None;
        self.cancel_drag();
    }

    /// Open and unfolding (not folding away).
    pub(super) fn is_showing(&self) -> bool {
        self.open && self.morph.is_opening()
    }

    /// Marks a closed panel open, centered on `anchor_along`.
    pub(super) fn start(&mut self, anchor_along: f32) {
        self.anchor_along = anchor_along;
        self.open = true;
    }

    /// Starts folding the panel back (if it is open); returns whether it is.
    pub(super) fn close(&mut self) -> bool {
        if self.open {
            self.morph.close();
        }
        self.open
    }

    /// Marks the panel closed once it has folded away; true on that tick.
    pub(super) fn finish_closing(&mut self) -> bool {
        let closed = self.open && self.morph.is_closed();
        if closed {
            self.open = false;
        }
        closed
    }
}

impl App {
    pub(super) fn handle_panels(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleSettings => {
                if self.settings_panel.is_showing() {
                    return self.update(Message::CloseSettings);
                }
                self.force_reveal = true;
                self.set_actions_open(false);
                let close_note = self.update(Message::ClosePanel);
                self.close_search();
                self.close_export();
                self.hide_peek();
                // Still folding away: unfold again from where it is.
                if !self.settings_panel.open {
                    let gear = self.strip_layout().settings_anchor();
                    self.settings_panel.start(along_center(gear, self.edge()));
                    self.settings_tab = SettingsTab::default();
                    self.palette_slot = None;
                }
                self.settings_panel.morph.open();
                self.animating = true;
                return Task::batch([close_note, self.dock_window()]);
            }
            Message::CloseSettings => {
                if self.settings_panel.close() {
                    self.settings_panel.cancel_drag();
                    self.animating = true;
                }
            }
            Message::ToggleSearch => return self.toggle_search(),
            Message::SearchChanged(query) => self.search_query = query,
            Message::SearchResultPicked(id) => return self.pick_result(id),
            Message::SearchSubmitted => {
                if let Some(id) = self.search_hits().first().map(|hit| hit.note_id) {
                    return self.pick_result(id);
                }
            }
            Message::CloseSearch => self.close_search(),
            Message::ToggleExport => return self.toggle_export(),
            Message::ExportToggleNote(id) => {
                if let Some(state) = &mut self.export {
                    state.status = None;
                    if !state.selected.remove(&id) {
                        state.selected.insert(id);
                    }
                }
            }
            Message::ExportAll(all) => {
                if let Some(state) = &mut self.export {
                    state.status = None;
                    state.selected = if all {
                        self.store.notes().iter().map(|n| n.id).collect()
                    } else {
                        HashSet::new()
                    };
                }
            }
            Message::ExportFormatChosen(format) => {
                if let Some(state) = &mut self.export {
                    state.status = None;
                    state.format = format;
                }
            }
            Message::ExportRequested => return self.request_export(),
            Message::ExportPicked(job) => {
                self.export_dialog_open = false;
                return self.run_export(job);
            }
            Message::ExportCancelled(_) => self.export_dialog_open = false,
            Message::ExportStatusExpired(generation, at) => {
                let still_shown = self
                    .export_opened_by(generation)
                    .is_some_and(|state| state.succeeded_at() == Some(at));
                if still_shown {
                    self.close_export();
                }
            }
            Message::SettingChanged(key, value) => {
                self.settings.settings_mut().set(key, value);
                return self.apply_settings();
            }
            // Only kept while typing: "2" on the way to "2.5" mustn't
            // clamp the value. Enter, a click or closing Settings applies it.
            Message::RangeEdited(key, end, typed) => {
                self.range_drafts.insert((key, end), typed);
                if self.hotkey_recording {
                    self.stop_recording();
                }
            }
            Message::HotkeyRecord => {
                if std::mem::take(&mut self.hotkey_press_stopped) {
                    // The press on this button already ended the recording.
                } else if self.hotkey_recording {
                    self.stop_recording();
                } else if self.settings_panel.is_showing() {
                    self.hotkey_recording = true;
                    self.hotkey_hint = None;
                    // The old hotkey is let go while recording, so its own
                    // keys can be recorded too.
                    self.sync_hotkey();
                }
            }
            Message::RangeSubmitted(key, end) => {
                if let Some(typed) = self.range_drafts.remove(&(key, end)) {
                    if self.apply_range_draft(key, end, &typed) {
                        return self.apply_settings();
                    }
                }
            }
            Message::ReminderAlertChosen(style) => {
                self.settings.settings_mut().motion.reminder_alert = style;
                self.jump_settle = None;
                self.animating |= self.pulse_running();
                return self.apply_settings();
            }
            Message::NoteKeysChosen(keys) => {
                self.settings.settings_mut().window.note_keys = keys;
                self.sync_note_hotkeys();
            }
            Message::SettingToggled(toggle) => {
                self.settings.settings_mut().toggle(toggle);
                if toggle == SettingToggle::DockIcon {
                    self.keep_open_until = Some(Instant::now() + FOCUS_GRACE);
                }
                if toggle == SettingToggle::AutoHide {
                    // `update` steps auto-hide right after this.
                    return Task::none();
                }
                if toggle == SettingToggle::StartAtLogin {
                    self.sync_start_at_login();
                    return Task::none();
                }
                self.sync_hotkey();
                return self.apply_app_visibility();
            }
            Message::ResetGroup(SettingsGroup::Data) => {}
            Message::ResetGroup(group) => {
                let edge = self.edge();
                if group == SettingsGroup::Palette {
                    let changes = self.settings.settings_mut().reset_palette();
                    if self.store.recolor_many(&changes) {
                        self.store.mark_dirty();
                    }
                    self.reparse();
                } else {
                    self.settings.settings_mut().reset(group);
                    self.range_drafts.retain(|(key, _), _| key.group() != group);
                }
                if group == SettingsGroup::App {
                    self.hotkey_recording = false;
                    self.hotkey_hint = None;
                    self.keep_open_until = Some(Instant::now() + FOCUS_GRACE);
                    self.sync_hotkey();
                    return self.apply_app_visibility();
                }
                self.sync_note_hotkeys();
                if group == SettingsGroup::Window {
                    self.sync_start_at_login();
                }
                if self.edge() != edge {
                    return self.edge_changed(edge);
                }
                return self.apply_settings();
            }
            Message::SettingsTabSelected(tab) => {
                self.settings_tab = tab;
                self.palette_slot = None;
            }
            Message::PaletteSlotSelected(slot) => {
                self.palette_slot = slot;
            }
            Message::PaletteColorChosen(color) => {
                if let Some(slot) = self.palette_slot {
                    if let Some(old) = self
                        .settings
                        .settings_mut()
                        .replace_palette_color(slot, color)
                    {
                        if self.store.recolor(old, color) {
                            self.store.mark_dirty();
                        }
                        self.reparse();
                    }
                }
            }
            Message::SettingsDragStart => {
                if let (Some(cursor), Some(frame)) = (self.last_cursor, self.settings_frame()) {
                    self.settings_panel
                        .start_drag(cursor, frame.rect.position());
                }
            }
            Message::SettingsResetPosition => self.settings_panel.reset_position(),
            Message::SearchDragStart => {
                if let (Some(cursor), Some(frame)) = (self.last_cursor, self.search_frame()) {
                    self.search_panel.start_drag(cursor, frame.rect.position());
                }
            }
            Message::SearchResetPosition => self.search_panel.reset_position(),
            _ => unreachable!("not a panels message"),
        }
        Task::none()
    }

    /// Applies a typed range bound if it is usable; returns whether it was.
    fn apply_range_draft(&mut self, key: SettingKey, end: RangeEnd, typed: &str) -> bool {
        key.parse_bound(typed).is_some_and(|v| {
            self.settings
                .settings()
                .check_range_end(key, end, v)
                .is_some()
                && self.settings.settings_mut().set_range_end(key, end, v)
        })
    }

    /// Applies every typed range bound (unusable ones are dropped), as when
    /// the fields lose focus.
    pub(super) fn commit_range_drafts(&mut self) -> Task<Message> {
        let mut applied = false;
        for ((key, end), typed) in std::mem::take(&mut self.range_drafts) {
            applied |= self.apply_range_draft(key, end, &typed);
        }
        if applied {
            self.apply_settings()
        } else {
            Task::none()
        }
    }

    /// Stops recording the global hotkey and takes the saved one back.
    pub(super) fn stop_recording(&mut self) {
        self.hotkey_recording = false;
        self.hotkey_hint = None;
        self.sync_hotkey();
    }

    /// Settings closed or hidden, by whatever path: typed range bounds
    /// apply and hotkey recording stops.
    pub(super) fn settle_settings_session(&mut self) -> Task<Message> {
        if self.settings_panel.is_showing() && self.visible {
            return Task::none();
        }
        if self.hotkey_recording {
            self.stop_recording();
        }
        if self.range_drafts.is_empty() {
            return Task::none();
        }
        self.commit_range_drafts()
    }

    /// Ends the settings' and the search's drags where they are. Returns
    /// the search field's focus after a search drag.
    pub(super) fn finish_panel_drags(&mut self) -> Task<Message> {
        if self.settings_panel.drag.is_some() {
            let rect = self.panel_target(&self.settings_panel);
            self.settings_panel.cancel_drag();
            self.settings_panel.pos = Some(rect.position());
        }
        if self.search_panel.drag.is_some() {
            let rect = self.panel_target(&self.search_panel);
            self.search_panel.cancel_drag();
            self.search_panel.pos = Some(rect.position());
            return focus_field();
        }
        Task::none()
    }

    /// The settings panel's current frame, morphing out of the gear slot.
    pub(super) fn settings_frame(&self) -> Option<MorphFrame> {
        let source = self.strip_layout().settings_anchor();
        self.panel_frame(source, &self.settings_panel)
    }

    /// Where `panel` sits: a dragged position kept inside the window, or
    /// beside the strip.
    pub(super) fn panel_target(&self, panel: &Panel) -> Rectangle {
        let room = self.panel_room();
        let height = room.min(PANEL_MAX_HEIGHT);
        let size = Size::new(PANEL_WIDTH, height);
        if let Some(top_left) = panel.top_left() {
            let max_x = (self.window_size.width - size.width - NOTE_MARGIN).max(NOTE_MARGIN);
            let max_y = (self.window_size.height - size.height - NOTE_MARGIN).max(NOTE_MARGIN);
            return Rectangle::new(
                Point::new(
                    top_left.x.clamp(NOTE_MARGIN, max_x),
                    top_left.y.clamp(NOTE_MARGIN, max_y),
                ),
                size,
            );
        }
        self.beside_strip(size, panel.anchor_along)
    }

    /// The search panel's current frame, morphing out of the search slot.
    pub(super) fn search_frame(&self) -> Option<MorphFrame> {
        let source = self.strip_layout().search_anchor();
        self.panel_frame(source, &self.search_panel)
    }

    /// The export panel's current frame, morphing out of the gear slot like
    /// the settings it replaces.
    pub(super) fn export_frame(&self) -> Option<MorphFrame> {
        let source = self.strip_layout().settings_anchor();
        self.panel_frame(source, &self.export_panel)
    }

    /// An open panel's frame on its way from the strip slot `source` to its
    /// place (see [`Self::panel_target`]).
    pub(super) fn panel_frame(&self, source: Rectangle, panel: &Panel) -> Option<MorphFrame> {
        if !panel.open {
            return None;
        }
        let target = self.panel_target(panel);
        Some(morph_frame(
            self.slid(source),
            target,
            panel.morph.progress(),
        ))
    }

    /// Height available to a panel, leaving `NOTE_MARGIN` at the window
    /// edges. Below a Top strip it is what is left under [`Self::panel_away`].
    pub(super) fn panel_room(&self) -> f32 {
        match self.edge() {
            Edge::Right | Edge::Left => self.window_size.height - 2.0 * NOTE_MARGIN,
            Edge::Top => self.window_size.height - self.panel_away() - NOTE_MARGIN,
        }
    }

    /// Distance from the screen edge to a panel: clears the strip band and
    /// an expanded Actions row so Settings/Search don't cover those pills.
    pub(super) fn panel_away(&self) -> f32 {
        crate::bar_strip::actions_row_extent(
            &self.settings.settings().bars,
            self.entries().len(),
            |_| 1.0,
        )
        .max(STRIP_WIDTH)
            + NOTE_GAP
    }

    /// A box of `size` on the strip's away side, clear of the Actions row,
    /// centered along the edge on `anchor` where it fits within the
    /// margins, else in the middle.
    pub(super) fn beside_strip(&self, size: Size, anchor: f32) -> Rectangle {
        let edge = self.edge();
        let length = edge.edge_length(size);
        let extent = edge.edge_length(self.window_size);
        let min_center = NOTE_MARGIN + length / 2.0;
        let max_center = extent - NOTE_MARGIN - length / 2.0;
        let center = if max_center > min_center {
            anchor.clamp(min_center, max_center)
        } else {
            extent / 2.0
        };
        edge.rect_to_window(
            LocalRect {
                along: center - length / 2.0,
                away: self.panel_away(),
                length,
                thickness: edge.thickness(size),
            },
            self.window_size,
        )
    }

    /// Opens the search panel, or closes it while it is showing. It closes
    /// the note and the settings, and starts with an empty query.
    pub(super) fn toggle_search(&mut self) -> Task<Message> {
        self.force_reveal = true;
        if self.search_panel.is_showing() {
            self.close_search();
            return Task::none();
        }
        let close_note = self.update(Message::ClosePanel);
        let close_settings = self.update(Message::CloseSettings);
        self.close_export();
        self.hide_peek();
        // Still folding away: unfold again from where it is, query kept.
        if !self.search_panel.open {
            let slot = self.strip_layout().search_anchor();
            self.search_panel.start(along_center(slot, self.edge()));
            self.search_query.clear();
        }
        self.search_panel.morph.open();
        self.pending_focus = Some(PendingFocus::SearchField);
        self.animating = true;
        Task::batch([
            close_note,
            close_settings,
            self.dock_window(),
            focus_field(),
        ])
    }

    /// Opens the export panel with every note selected, or closes it while
    /// it is showing. It closes the note, the search and the settings.
    pub(super) fn toggle_export(&mut self) -> Task<Message> {
        if self.export_panel.is_showing() {
            self.close_export();
            return Task::none();
        }
        let close_note = self.update(Message::ClosePanel);
        let close_settings = self.update(Message::CloseSettings);
        self.close_search();
        self.hide_peek();
        match &mut self.export {
            // Still folding away: unfold again from where it is, choices kept.
            Some(state) => state.status = None,
            None => {
                let gear = self.strip_layout().settings_anchor();
                self.export_panel.start(along_center(gear, self.edge()));
                self.export_generation += 1;
                self.export = Some(ExportState::new(self.store.notes(), self.export_generation));
            }
        }
        self.export_panel.morph.open();
        self.animating = true;
        Task::batch([close_note, close_settings, self.dock_window()])
    }

    /// Folds the export panel back into the gear (if it is showing).
    pub(super) fn close_export(&mut self) {
        if self.export_panel.close() {
            self.animating = true;
        }
    }

    /// Opens the save dialog for the selected notes, unless one is open
    /// already (from this panel or an earlier one) or none of them exists
    /// any more. The dialog's task carries the notes and the format, so the
    /// export happens even if the panel folds meanwhile.
    pub(super) fn request_export(&mut self) -> Task<Message> {
        if !self.export_enabled() {
            return Task::none();
        }
        let Some(state) = &mut self.export else {
            return Task::none();
        };
        state.status = None;
        self.export_dialog_open = true;
        let notes = state.selected_in(self.store.notes());
        let format = state.format;
        let generation = state.generation;
        let name = export::suggested_name(chrono::Local::now().date_naive(), format);
        let kind = match format {
            ExportFormat::Markdown => "Markdown",
            ExportFormat::Text => "Text",
        };
        // The dialog steals focus, which must not fold the panel.
        self.keep_open_until = Some(Instant::now() + FOCUS_GRACE);
        // Built when the task runs, not here: some backends start showing
        // the dialog as soon as it is created.
        let dialog = async move {
            rfd::AsyncFileDialog::new()
                .set_file_name(name)
                .add_filter(kind, &[format.extension()])
                .save_file()
                .await
        };
        Task::perform(dialog, move |file| match file {
            Some(file) => Message::ExportPicked(ExportJob {
                generation,
                path: file.path().to_path_buf(),
                notes,
                format,
            }),
            None => Message::ExportCancelled(generation),
        })
    }

    /// Whether the export panel's Export button can start a dialog: no
    /// save dialog is open and a selected note still exists.
    pub(super) fn export_enabled(&self) -> bool {
        !self.export_dialog_open
            && self
                .export
                .as_ref()
                .is_some_and(|state| state.can_export(self.store.notes()))
    }

    /// The export panel, if it is the opening `generation` belongs to.
    pub(super) fn export_opened_by(&mut self, generation: u64) -> Option<&mut ExportState> {
        self.export
            .as_mut()
            .filter(|state| state.generation == generation)
    }

    /// Writes the job's notes that still exist, in its order, and shows how
    /// that went in the panel that started it, or logs it once that panel
    /// is gone. A success shown there closes the panel `EXPORTED_FOR` later.
    pub(super) fn run_export(&mut self, job: ExportJob) -> Task<Message> {
        let notes: Vec<_> = job
            .notes
            .iter()
            .filter_map(|id| self.store.notes().iter().find(|note| note.id == *id))
            .collect();
        if notes.is_empty() {
            return Task::none();
        }
        let status = match export::write(&job.path, &export::render(&notes, job.format)) {
            Ok(()) => ExportStatus::Exported(notes.len()),
            Err(error) => ExportStatus::Failed(error.to_string()),
        };
        let Some(state) = self.export_opened_by(job.generation) else {
            eprintln!("export to {}: {}", job.path.display(), status.message());
            return Task::none();
        };
        let at = Instant::now();
        let succeeded = matches!(status, ExportStatus::Exported(_));
        state.status = Some((status, at));
        if succeeded {
            delayed(
                EXPORTED_FOR,
                Message::ExportStatusExpired(job.generation, at),
            )
        } else {
            Task::none()
        }
    }

    /// Folds the search panel back into its slot (if it is showing).
    pub(super) fn close_search(&mut self) {
        if self.search_panel.close() {
            self.search_panel.cancel_drag();
            self.animating = true;
        }
        if self.pending_focus == Some(PendingFocus::SearchField) {
            self.pending_focus = None;
        }
    }

    /// Opens the picked note in edit mode with its first body match
    /// selected, or the cursor at the start of the body for a title-only
    /// match, and closes the search.
    pub(super) fn pick_result(&mut self, id: Uuid) -> Task<Message> {
        let Some(index) = self.store.notes().iter().position(|n| n.id == id) else {
            return Task::none();
        };
        let matched = self
            .search_results
            .iter()
            .find(|hit| hit.note_id == id)
            .and_then(|hit| hit.body_match.clone());
        self.close_search();
        // `open_note` would fold a note that is already open.
        let open = if self.active_note == Some(id) && self.morph.is_opening() {
            Task::none()
        } else {
            self.open_note(index)
        };
        let (true, Some(content)) = (self.active_note == Some(id), &mut self.editor_content) else {
            return open;
        };
        let text = content.text();
        let position = |offset| {
            let (line, column) = rich::position_of(&text, offset);
            text_editor::Position { line, column }
        };
        let (head, anchor) = match matched {
            Some(range) => (range.end, Some(range.start)),
            None => (0, None),
        };
        content.move_to(text_editor::Cursor {
            position: position(head),
            selection: anchor.map(position),
        });
        self.editing = true;
        self.history.break_step();
        self.rendered_click = None;
        self.pending_focus = Some(PendingFocus::Body);
        Task::batch([open, focus_body()])
    }

    /// Recomputes the search results while the panel is open and the
    /// query or the notes changed since they were computed.
    pub(super) fn sync_search(&mut self) {
        if !self.search_panel.open {
            return;
        }
        let key = (self.store.revision(), self.search_query.clone());
        if self.search_synced.as_ref() == Some(&key) {
            return;
        }
        let result = search::search(self.store.notes(), &self.search_query);
        self.search_results = result.hits;
        self.search_matches = result.matches;
        self.search_synced = Some(key);
    }

    /// Search results for the current query, for the panel.
    pub(super) fn search_hits(&self) -> &[Hit] {
        &self.search_results
    }

    /// Bars to dim, by index: while the search is open with a query, the
    /// entries none of whose notes match it. Empty otherwise.
    pub(super) fn dimmed_bars(&self) -> Vec<bool> {
        if !self.search_panel.open || self.search_query.trim().is_empty() {
            return Vec::new();
        }
        let notes = self.store.notes();
        self.entries()
            .iter()
            .map(|entry| {
                !entry
                    .notes()
                    .any(|i| self.search_matches.contains(&notes[i].id))
            })
            .collect()
    }

    /// Search, settings or export is open.
    pub(super) fn panel_open(&self) -> bool {
        self.search_panel.open || self.settings_panel.open || self.export.is_some()
    }

    /// Applies changed settings to running state: animation speeds, scroll
    /// bounds and the docked window size.
    pub(super) fn apply_settings(&mut self) -> Task<Message> {
        let s = self.settings.settings();
        let speed = s.motion.speed;
        self.morph.set_speed(speed);
        self.actions_morph.set_speed(speed);
        self.peek.set_speed(speed);
        self.settings_panel.morph.set_speed(speed);
        self.search_panel.morph.set_speed(speed);
        self.export_panel.morph.set_speed(speed);
        self.mode_fade.set_speed(speed);
        if let Some((_, collapse)) = &mut self.collapsing {
            collapse.set_speed(speed);
        }
        self.scroll_offset = self.scroll_offset.min(self.strip_layout().max_scroll);
        self.animating = true;
        self.dock_window()
    }
}
