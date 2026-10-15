//! The open note: opening and closing it, editing its title and body, colors,
//! images and paste, undo, moving and resizing it, and deleting notes.

use super::*;

impl App {
    pub(super) fn handle_notes(&mut self, message: Message) -> Task<Message> {
        // Working on the note anywhere else drops the header's delete question.
        if matches!(
            message,
            Message::AddNote
                | Message::NoteEdited(..)
                | Message::BodyClicked(..)
                | Message::BodyPressed(..)
                | Message::ToggleTask(..)
                | Message::TitleEdited(..)
                | Message::NoteDragStart
                | Message::ResizeStart(..)
                | Message::ClosePanel
                | Message::CloseFront
                | Message::CopyNote
                | Message::TogglePin
                | Message::ToggleColorPicker
                | Message::ToggleTextColorPicker
                | Message::FormatApplied(..)
                | Message::PickImage
        ) {
            self.drop_header_ask();
        }
        match message {
            Message::AddNote => {
                self.force_reveal = true;
                return self.create_note(None, String::new());
            }
            Message::NoteHovered(hovered) => {
                self.note_hovered = hovered;
            }
            Message::NoteDragStart => {
                if let (Some(cursor), Some(frame)) = (self.last_cursor, self.note_frame()) {
                    let top_left = frame.rect.position();
                    self.note_drag = Some(cursor - top_left);
                    self.note_drag_pos = Some(top_left);
                }
            }
            Message::ResizeStart(edges) => {
                if let (Some(grab), Some(_)) = (self.last_cursor, self.active_note) {
                    let start = self.note_target_rect();
                    self.note_resize = Some(NoteResize {
                        edges,
                        start,
                        grab,
                        rect: start,
                    });
                }
            }
            Message::NoteResetPosition => {
                self.placed = None;
                if let Some(id) = self.active_note {
                    if let Some(note) = self.store.note_mut(id) {
                        note.position = None;
                    }
                    self.store.mark_dirty();
                }
            }
            Message::TitleEdited(typed) => {
                if let Some(id) = self.active_note {
                    if self.title_edit.as_ref().is_none_or(|e| e.id != id) {
                        let note = self.store.notes().iter().find(|n| n.id == id);
                        self.title_edit = Some(TitleEdit {
                            id,
                            draft: String::new(),
                            hidden: note
                                .and_then(|n| reminder::tag_text(&n.title))
                                .map(str::to_string),
                            anchor: note.and_then(|n| n.reminder_set_at),
                        });
                    }
                    let Some(edit) = self.title_edit.as_mut() else {
                        return Task::none();
                    };
                    edit.draft = typed;
                    // The tag hidden when the edit began stays, unless the
                    // text has a valid tag of its own, which the header label
                    // shows right away.
                    let hidden = edit.hidden.as_deref();
                    let title = reminder::edit_visible(hidden.unwrap_or(""), &edit.draft);
                    if let Some(note) = self.store.note_mut(id) {
                        reminder::retitle(note, title, chrono::Local::now());
                        // Back to the hidden tag: it counts from when it was set.
                        if hidden.is_some() && reminder::tag_text(&note.title) == hidden {
                            note.reminder_set_at = edit.anchor;
                        }
                        note.updated_at = chrono::Utc::now();
                    }
                    self.store.mark_dirty();
                }
            }
            Message::TitleCommitted => self.commit_title(),
            Message::TitleBlurred(focused) => {
                if !focused {
                    self.commit_title();
                }
            }
            Message::NoteEdited(
                text_editor::Action::Click(_)
                | text_editor::Action::SelectWord
                | text_editor::Action::SelectLine,
            ) if self
                .rendered_click
                .as_ref()
                .is_some_and(|c| c.at.elapsed() <= MULTI_CLICK) =>
            {
                self.extend_rendered_click();
            }
            Message::NoteEdited(action) => {
                self.rendered_click = None;
                if let Some(content) = &mut self.editor_content {
                    let is_edit = action.is_edit();
                    // Enter on a list item continues or ends the list.
                    let list = match &action {
                        text_editor::Action::Edit(
                            text_editor::Edit::Enter | text_editor::Edit::Insert('\n'),
                        ) if content.cursor().selection.is_none() => {
                            let at = content.cursor().position;
                            crate::list_continue::continue_list_at(
                                &content.text(),
                                at.line,
                                at.column,
                            )
                        }
                        _ => None,
                    };
                    if is_edit {
                        let typing = matches!(
                            action,
                            text_editor::Action::Edit(text_editor::Edit::Insert(c))
                                if !c.is_whitespace()
                        );
                        let before = (content.text(), content.cursor());
                        self.history.record(before, typing, Instant::now());
                    } else {
                        self.history.break_step();
                    }
                    match list {
                        Some(edit) => {
                            for _ in 0..edit.delete {
                                content.perform(text_editor::Action::Edit(
                                    text_editor::Edit::Backspace,
                                ));
                            }
                            if !edit.insert.is_empty() {
                                content.perform(text_editor::Action::Edit(
                                    text_editor::Edit::Paste(std::sync::Arc::new(edit.insert)),
                                ));
                            }
                        }
                        None => content.perform(action),
                    }
                    if is_edit {
                        if let Some(id) = self.active_note {
                            let text = content.text();
                            if let Some(note) = self.store.note_mut(id) {
                                note.content = text.trim_end_matches('\n').to_string();
                                note.updated_at = chrono::Utc::now();
                            }
                            self.store.mark_dirty();
                        }
                        // Follow the caret only once writing at the end runs
                        // past the bottom of the note.
                        if content.cursor().position.line + 1 >= content.line_count() {
                            return crate::note_panel::reveal_last_line();
                        }
                    }
                }
            }
            Message::BodyPressed(offset) => {
                self.commit_title();
                if !self.editing && self.editor_content.is_some() {
                    self.restart_mode_fade();
                }
                if let Some(content) = &mut self.editor_content {
                    self.editing = true;
                    let (line, column) = rich::position_of(&content.text(), offset);
                    content.move_to(text_editor::Cursor {
                        position: text_editor::Position { line, column },
                        selection: None,
                    });
                    self.history.break_step();
                    self.rendered_click = Some(RenderedClick {
                        at: Instant::now(),
                        offset,
                        count: 1,
                    });
                    return focus_body();
                }
            }
            Message::BodyClicked(line) => {
                self.commit_title();
                if !self.editing && self.editor_content.is_some() {
                    self.restart_mode_fade();
                }
                if let Some(content) = &mut self.editor_content {
                    self.editing = true;
                    match line {
                        Some(line) => content.move_to(text_editor::Cursor {
                            position: text_editor::Position {
                                line: line.min(content.line_count().saturating_sub(1)),
                                column: 0,
                            },
                            selection: None,
                        }),
                        None => content
                            .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd)),
                    }
                    return focus_body();
                }
            }
            Message::EditorBlurred => {
                if self.editing {
                    self.editing = false;
                    self.text_color_picker_open = false;
                    self.reminder_picker = None;
                    self.reparse();
                    self.restart_mode_fade();
                }
            }
            Message::ToggleTask(line) => {
                // Only real task items: `toggle_task` would also flip a
                // look-alike line inside a code block.
                let is_task = self.doc.blocks.iter().any(|b| {
                    b.source_line == line
                        && matches!(b.kind, BlockKind::ListItem { task: Some(_), .. })
                });
                if !is_task {
                    return Task::none();
                }
                let Some(note) = self.active_note.and_then(|id| self.store.note_mut(id)) else {
                    return Task::none();
                };
                if let Some(content) = rich::toggle_task(&note.content, line) {
                    if let Some(editor) = &self.editor_content {
                        let before = (editor.text(), editor.cursor());
                        self.history.record(before, false, Instant::now());
                    }
                    self.editor_content = Some(text_editor::Content::with_text(&content));
                    note.content = content;
                    note.updated_at = chrono::Utc::now();
                    self.store.mark_dirty();
                    self.reparse();
                }
            }
            Message::LinkClicked(url) => {
                if let Some(url) = rich_view::openable(&url) {
                    platform::open_url(url);
                }
            }
            Message::FocusNote(id) if self.behind.iter().any(|open| open.id == id) => {
                return self.open_note_id(id);
            }
            Message::FocusNote(_) => {}
            Message::ClosePanel => {
                self.commit_title();
                // Closes every open note, not just the front one.
                self.front_only = false;
                if self.active_note.is_some() {
                    self.morph.close();
                    self.animating = true;
                    self.color_picker_open = false;
                    self.text_color_picker_open = false;
                    self.reminder_picker = None;
                }
            }
            Message::CloseFront => return self.close_front(),
            Message::RequestDelete(id) => {
                if self.active_note == Some(id) && self.pending_delete.is_none() {
                    // The question replaces the bubble and the picker, so
                    // Esc drops just the question.
                    self.color_picker_open = false;
                    self.text_color_picker_open = false;
                    self.reminder_picker = None;
                    self.delete_ask = Some(DeleteAsk::Header(id));
                }
            }
            Message::ConfirmDelete => match self.delete_ask.take() {
                Some(DeleteAsk::Header(id)) => self.delete_open(id),
                Some(DeleteAsk::Peek(id)) => return self.delete_peeked(id),
                None => {}
            },
            Message::CancelDelete => self.delete_ask = None,
            Message::CopyNote => {
                let note = self
                    .active_note
                    .and_then(|id| self.store.notes().iter().find(|n| n.id == id));
                if let Some(note) = note {
                    let text = crate::note::copy_text(note);
                    self.copies += 1;
                    let copied = (note.id, self.copies);
                    self.copied_at = Some(copied);
                    return Task::batch([
                        iced::clipboard::write(text),
                        delayed(COPIED_FOR, Message::CopiedExpired(copied.0, copied.1)),
                    ]);
                }
            }
            Message::TogglePin => {
                let note = self
                    .active_note
                    .and_then(|id| self.store.notes().iter().find(|n| n.id == id));
                if let Some(note) = note {
                    // A member pins through its stack's top.
                    let top = note.stack.unwrap_or(note.id);
                    let pinned = self.top_pinned(note);
                    self.store.set_pinned(top, !pinned);
                    self.store.mark_dirty();
                    self.follow_open_bar();
                }
            }
            Message::CopiedExpired(id, at) => {
                if self.copied_at == Some((id, at)) {
                    self.copied_at = None;
                }
            }
            Message::ClipboardEmptyExpired(at) => {
                if self.clipboard_empty_at == Some(at) {
                    self.clipboard_empty_at = None;
                    return self.dock_window();
                }
            }
            // A drag holds a bar index the restored note would shift.
            Message::UndoDelete if self.drag.is_some() => {}
            Message::UndoDelete => {
                if let Some(last) = self.last_deleted.take() {
                    let id = last.deleted.note.id;
                    self.store.restore(last.deleted);
                    if last.pulsing {
                        self.begin_alert(id);
                        self.animating |= self.pulse_running();
                    }
                    self.store.mark_dirty();
                    let _ = self.store.save();
                    self.store.did_save();
                    return self.dock_window();
                }
            }
            Message::UndoExpired(at) => {
                if self.last_deleted.as_ref().is_some_and(|last| last.at == at) {
                    self.last_deleted = None;
                    return self.dock_window();
                }
            }
            Message::SaveTick => {
                if self.store.should_save() {
                    let _ = self.store.save();
                    self.store.did_save();
                }
                if self.settings.should_save() {
                    let _ = self.settings.save();
                    self.settings.did_save();
                }
            }
            Message::ToggleColorPicker => {
                self.color_picker_open = !self.color_picker_open;
                self.text_color_picker_open = false;
                self.reminder_picker = None;
                if self.color_picker_open {
                    self.color_bubble_rect = None;
                    let notes = self.store.notes();
                    if let Some(note) = notes.iter().find(|n| Some(n.id) == self.active_note) {
                        self.color_hex = note.color.to_hex();
                    }
                }
            }
            Message::ToggleTextColorPicker => {
                self.text_color_picker_open = !self.text_color_picker_open;
                self.color_picker_open = false;
                self.reminder_picker = None;
            }
            Message::FormatApplied(format) => return self.apply_format(format),
            Message::ImageDropped(path) if self.note_open() => {
                self.file_hover = false;
                return self.import_image(&path);
            }
            Message::ImageDropped(path) => {
                self.file_hover = false;
                let entry = self.last_cursor.and_then(|at| {
                    self.strip_layout()
                        .bars
                        .iter()
                        .position(|bar| bar.contains(at))
                });
                return self.update(Message::StripFileDropped(entry, path));
            }
            Message::ClipboardNote => return self.clipboard_note(read_clipboard()),
            Message::ImagePicked(id, Some(path))
                if self.active_note == Some(id) && self.morph.is_opening() =>
            {
                return self.import_image(&path)
            }
            Message::ImagePicked(..) => {}
            Message::PasteRequested if self.editing => {
                return self.apply_paste(read_clipboard());
            }
            Message::PasteRequested => {}
            Message::ClipboardText(id, text)
                if self.editing && self.active_note == Some(id) && self.morph.is_opening() =>
            {
                return self.apply_paste(ClipboardContent::Text(text));
            }
            Message::ClipboardText(..) => {}
            Message::Undo => return self.step_history(false),
            Message::Redo => return self.step_history(true),
            Message::PickImage => {
                let Some(id) = self.active_note else {
                    return Task::none();
                };
                // The dialog steals focus, which must not fold the note.
                self.keep_open_until = Some(Instant::now() + FOCUS_GRACE);
                let dialog = rfd::AsyncFileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
                    .pick_file();
                return Task::perform(dialog, move |file| {
                    Message::ImagePicked(id, file.map(|f| f.path().to_path_buf()))
                });
            }
            Message::ColorChosen(color) => {
                self.recolor_open_note(color);
                self.color_hex = color.to_hex();
                self.color_picker_open = false;
            }
            Message::ColorAdjusted(color) => {
                self.recolor_open_note(color);
                self.color_hex = color.to_hex();
            }
            Message::ColorHexEdited(text) => {
                // Anything but a whole color only changes the field.
                if let Some(color) = crate::color_picker::parse_hex_field(&text) {
                    self.recolor_open_note(color);
                }
                self.color_hex = text;
            }
            Message::ColorHexSubmitted | Message::CloseColorBubble => {
                self.color_picker_open = false;
                self.reminder_picker = None;
            }
            Message::ColorBubbleMoved(rect) => {
                self.color_bubble_rect = Some(rect);
            }
            _ => unreachable!("not a notes message"),
        }
        Task::none()
    }

    /// Deletes the open note `id`: it folds back into its bar first, then
    /// its bar collapses away. Only the latest delete can be undone.
    pub(super) fn delete_open(&mut self, id: Uuid) {
        if self.active_note == Some(id) {
            self.last_deleted = None;
            self.pending_delete = Some(id);
            self.morph.close();
            self.front_only = true;
            self.animating = true;
        }
    }

    /// Drops the header's delete question, if it asks one.
    pub(super) fn drop_header_ask(&mut self) {
        if matches!(self.delete_ask, Some(DeleteAsk::Header(_))) {
            self.delete_ask = None;
        }
    }

    pub(super) fn open_note(&mut self, index: usize) -> Task<Message> {
        let Some(id) = self.store.notes().get(index).map(|note| note.id) else {
            return Task::none();
        };
        // A note being deleted folds away and can't reopen.
        if self.pending_delete == Some(id) {
            return Task::none();
        }
        // Opening another note mid-fold still deletes the folding one.
        let collapse = match self.pending_delete.take() {
            Some(deleted) => self.start_collapse(deleted),
            None => Task::none(),
        };
        Task::batch([collapse, self.open_note_id(id)])
    }

    /// Opens the note `id`, or closes it when it is the open one.
    pub(super) fn open_note_id(&mut self, id: Uuid) -> Task<Message> {
        self.drop_header_ask();
        // A deleted note can't be opened while its bar collapses.
        if self.dying(id) {
            return Task::none();
        }

        if self.active_note == Some(id) && self.morph.is_opening() {
            return self.close_front();
        }

        let switching = self.active_note.is_some_and(|active| active != id);
        if switching {
            self.commit_title();
            // A drag or resize of the old note ends with it.
            if self.note_drag.is_some() {
                self.finish_note_drag();
            }
            self.finish_note_resize();
            if self.morph.is_opening() {
                // Opening another note leaves this one open behind it.
                self.send_behind();
            } else if !self.front_only {
                // Everything is folding away; the notes behind stay closed.
                self.behind.clear();
            }
        }
        self.front_only = false;
        // Starting the collapse may have removed a note, moving this one.
        let Some(index) = self.store.notes().iter().position(|n| n.id == id) else {
            return Task::none();
        };
        let note = &self.store.notes()[index];
        let fresh = text_editor::Content::with_text(&note.content);
        let empty = note.content.trim().is_empty();
        self.pulsing.remove(&id);
        if self.jump_settle.is_some_and(|(sid, _, _)| sid == id) {
            self.jump_settle = None;
        }
        self.rendered_click = None;
        self.hide_peek();
        // A note and the panels never show at the same time.
        self.settings_panel.close();
        self.close_search();
        self.close_export();
        self.active_note = Some(id);
        self.color_picker_open = false;
        self.text_color_picker_open = false;
        self.reminder_picker = None;
        let behind = self.behind.iter().position(|open| open.id == id);
        if let Some(open) = behind.map(|i| self.behind.remove(i)) {
            // Back to the front where it was, as it was.
            self.editor_content = Some(open.content);
            self.history = open.history;
            self.editing = false;
            self.placed = Some(open.rect.position());
            self.reparse();
            self.morph.finish_open();
            return self.dock_window();
        }
        self.editor_content = Some(fresh);
        self.history.clear();
        self.editing = empty;
        self.reparse();
        if let Some(bar) = self
            .note_entry(index)
            .and_then(|entry| self.strip_layout().bars.get(entry).copied())
        {
            self.anchor_along = along_center(bar, self.edge());
        }
        self.placed = None;
        self.placed = self.cascade();
        if switching {
            self.morph.restart();
        } else {
            self.morph.open();
        }
        self.animating = true;
        self.dock_window()
    }

    /// Folds the active note away alone; the frontmost note behind it
    /// becomes active once it has (see `finish_close`).
    pub(super) fn close_front(&mut self) -> Task<Message> {
        let task = self.update(Message::ClosePanel);
        self.front_only = true;
        task
    }

    /// Keeps the active note open behind, rendered where it is now, with
    /// its editor and undo for when it comes back to the front.
    fn send_behind(&mut self) {
        let Some(id) = self.active_note else {
            return;
        };
        let Some(content) = self.editor_content.take() else {
            return;
        };
        // The rendered view catches up with what was typed.
        self.reparse();
        let rect = self.note_target_rect();
        self.behind.retain(|open| open.id != id);
        self.behind.push(OpenNote {
            id,
            rect,
            content,
            doc: std::mem::replace(&mut self.doc, Doc { blocks: Vec::new() }),
            broken_images: std::mem::take(&mut self.broken_images),
            history: std::mem::take(&mut self.history),
        });
    }

    /// Where the active note goes when it would hide a note open behind:
    /// stepped away from the edge until its corner is clear. `None` when it
    /// already is.
    fn cascade(&self) -> Option<Point> {
        const STEP: f32 = 32.0;
        let taken = |p: Point| {
            self.behind
                .iter()
                .any(|open| open.rect.position().distance(p) < STEP)
        };
        let mut at = self.note_target_rect().position();
        if !taken(at) {
            return None;
        }
        let step = match self.edge() {
            Edge::Right => Vector::new(-STEP, STEP),
            Edge::Left | Edge::Top => Vector::new(STEP, STEP),
        };
        for _ in 0..self.behind.len() {
            at += step;
            if !taken(at) {
                break;
            }
        }
        Some(at)
    }

    /// Where a note open behind shows: in place, or folding into its bar
    /// along with the active note while everything closes.
    pub(super) fn behind_frame(&self, open: &OpenNote) -> MorphFrame {
        let progress = if self.morph.is_opening() || self.front_only {
            1.0
        } else {
            self.morph.progress()
        };
        let source = self
            .id_entry(open.id)
            .and_then(|entry| self.strip_layout().bars.get(entry).copied())
            .map_or(open.rect, |bar| self.slid(bar));
        morph_frame(source, open.rect, progress)
    }

    /// Gives the open note `color`; it is saved after the usual debounce.
    pub(super) fn recolor_open_note(&mut self, color: NoteColor) {
        let Some(note) = self.active_note.and_then(|id| self.store.note_mut(id)) else {
            return;
        };
        note.color = color;
        note.updated_at = chrono::Utc::now();
        self.store.mark_dirty();
    }

    /// Wraps the editor selection in `format`'s markup, writes the result
    /// back to the note and leaves the wrapped text selected.
    pub(super) fn apply_format(&mut self, format: rich::Format) -> Task<Message> {
        self.text_color_picker_open = false;
        self.reminder_picker = None;
        let (Some(id), true, Some(content)) =
            (self.active_note, self.editing, &mut self.editor_content)
        else {
            return Task::none();
        };
        let cursor = content.cursor();
        // The untrimmed text, so markup typed on a fresh last line lands there.
        let text = content.text();
        self.history
            .record((text.clone(), cursor), false, Instant::now());
        let offset = |p: text_editor::Position| rich::offset_of(&text, p.line, p.column);
        let head = offset(cursor.position);
        let anchor = cursor.selection.map_or(head, offset);
        let selected = content.selection();
        let (from, to) = rich::selection_range(&text, head, anchor, selected.as_deref());
        let (wrapped, start, end) = rich::wrap_selection(&text, from, to, format);
        let position = |o| {
            let (line, column) = rich::position_of(&wrapped, o);
            text_editor::Position { line, column }
        };
        let mut rebuilt = text_editor::Content::with_text(&wrapped);
        rebuilt.move_to(text_editor::Cursor {
            position: position(end),
            selection: (start != end).then(|| position(start)),
        });
        *content = rebuilt;
        if let Some(note) = self.store.note_mut(id) {
            note.content = wrapped.trim_end_matches('\n').to_string();
            note.updated_at = chrono::Utc::now();
        }
        self.store.mark_dirty();
        focus_body()
    }

    /// Adds a note, sets its title (tags in it get their reminder anchor)
    /// and body, and opens it: rendered, unless the body is empty.
    pub(super) fn create_note(&mut self, title: Option<String>, content: String) -> Task<Message> {
        let id = self.store.add_note(&self.settings.settings().palette);
        if let Some(note) = self.store.note_mut(id) {
            if let Some(title) = title {
                reminder::retitle(note, title, chrono::Local::now());
            }
            note.content = content;
        }
        self.store.mark_dirty();
        let last = self.store.notes().len() - 1;
        self.scroll_offset = self.strip_layout().max_scroll;
        self.open_note(last)
    }

    /// A note is open and not closing.
    pub(super) fn note_open(&self) -> bool {
        self.active_note.is_some() && self.morph.is_opening()
    }

    /// Adds `text` below the note's body, after a blank line unless the
    /// body is empty. Blank text adds nothing.
    pub(super) fn append_to_note(&mut self, id: Uuid, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let Some(note) = self.store.note_mut(id) else {
            return;
        };
        let body = note.content.trim_end_matches('\n');
        note.content = if body.trim().is_empty() {
            text.to_string()
        } else {
            format!("{body}\n\n{text}")
        };
        note.updated_at = chrono::Utc::now();
        let content = note.content.clone();
        self.store.mark_dirty();
        if self.active_note == Some(id) {
            self.editor_content = Some(text_editor::Content::with_text(&content));
            self.reparse();
        }
        // A note open behind shows, and later edits, the new text too.
        if let Some(i) = self.behind.iter().position(|open| open.id == id) {
            let (doc, broken) = self.parse_note(&content);
            let open = &mut self.behind[i];
            open.content = text_editor::Content::with_text(&content);
            open.doc = doc;
            open.broken_images = broken;
            open.history.clear();
        }
    }

    /// A new note with the clipboard's text or image; nothing for an empty
    /// clipboard.
    pub(super) fn clipboard_note(&mut self, content: ClipboardContent) -> Task<Message> {
        let body = match content {
            ClipboardContent::Text(text) => text.trim_end_matches(['\r', '\n']).to_string(),
            ClipboardContent::Image {
                rgba,
                width,
                height,
            } => match images::import_png(&self.data_dir, &rgba, width, height) {
                Ok(rel) => format!("![]({rel})"),
                Err(e) => {
                    eprintln!("clipboard image failed: {e}");
                    return Task::none();
                }
            },
            ClipboardContent::Empty => {
                let at = Instant::now();
                self.clipboard_empty_at = Some(at);
                self.animating = true;
                // Without passthrough the window widens to show the chip.
                return Task::batch([
                    self.dock_window(),
                    delayed(CLIPBOARD_HINT_FOR, Message::ClipboardEmptyExpired(at)),
                ]);
            }
        };
        self.create_note(None, body)
    }

    /// Seconds into the `+` slot's shake at `now`, while it lasts.
    pub(super) fn shake_secs(&self, now: Instant) -> Option<f32> {
        let t = now
            .saturating_duration_since(self.clipboard_empty_at?)
            .as_secs_f32();
        (t < SHAKE_SECS).then_some(t)
    }

    /// Copies an image file into the open note's folder and references it;
    /// does nothing without an open note and logs files that don't import.
    pub(super) fn import_image(&mut self, path: &std::path::Path) -> Task<Message> {
        if self.active_note.is_none() {
            return Task::none();
        }
        match images::import(&self.data_dir, path) {
            Ok(rel) => self.insert_image(&rel),
            Err(e) => {
                eprintln!("image import failed: {e}");
                Task::none()
            }
        }
    }

    /// Pastes what the clipboard held: text through the editor's own edit
    /// path, or an image as a new file. Encodes synchronously, which is fine
    /// for screenshots. When arboard found nothing (or failed, as on Wayland
    /// without XWayland), iced's own clipboard supplies the text.
    pub(super) fn apply_paste(&mut self, content: ClipboardContent) -> Task<Message> {
        match content {
            ClipboardContent::Text(text) => self.update(Message::NoteEdited(
                text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text))),
            )),
            ClipboardContent::Image {
                rgba,
                width,
                height,
            } => match images::import_png(&self.data_dir, &rgba, width, height) {
                Ok(rel) => self.insert_image(&rel),
                Err(e) => {
                    eprintln!("image paste failed: {e}");
                    Task::none()
                }
            },
            ClipboardContent::Empty => {
                // The read finishes later, so the result names its note.
                let Some(id) = self.active_note else {
                    return Task::none();
                };
                iced::clipboard::read().and_then(move |text| {
                    if text.is_empty() {
                        return Task::none();
                    }
                    Task::done(Message::ClipboardText(id, text))
                })
            }
        }
    }

    /// Counts one more quick click after a press on the rendered note and
    /// selects the word (second click) or line (third and later) around the
    /// press. The editor's own idea of the click spot is ignored: it lays
    /// the raw text out differently from the rendered view.
    pub(super) fn extend_rendered_click(&mut self) {
        let (Some(click), Some(content)) = (&mut self.rendered_click, &mut self.editor_content)
        else {
            return;
        };
        click.count = click.count.saturating_add(1);
        click.at = Instant::now();
        let text = content.text();
        let (start, end) = if click.count == 2 {
            rich::word_at(&text, click.offset)
        } else {
            rich::line_bounds(&text, click.offset)
        };
        let position = |offset| {
            let (line, column) = rich::position_of(&text, offset);
            text_editor::Position { line, column }
        };
        content.move_to(text_editor::Cursor {
            position: position(end),
            selection: (start != end).then(|| position(start)),
        });
        self.history.break_step();
    }

    /// Undoes (or with `forward`, redoes) one step of the open note's body.
    pub(super) fn step_history(&mut self, forward: bool) -> Task<Message> {
        let (Some(id), Some(content)) = (self.active_note, &self.editor_content) else {
            return Task::none();
        };
        let current = (content.text(), content.cursor());
        let target = if forward {
            self.history.redo(current)
        } else {
            self.history.undo(current)
        };
        let Some((text, cursor)) = target else {
            return Task::none();
        };
        let mut restored = text_editor::Content::with_text(&text);
        restored.move_to(cursor);
        self.editor_content = Some(restored);
        if let Some(note) = self.store.note_mut(id) {
            note.content = text.trim_end_matches('\n').to_string();
            note.updated_at = chrono::Utc::now();
        }
        self.store.mark_dirty();
        if self.editing {
            focus_body()
        } else {
            self.reparse();
            Task::none()
        }
    }

    /// Puts `![](rel)` on its own line at the cursor (editing) or at the end
    /// of the open note (rendered).
    pub(super) fn insert_image(&mut self, rel: &str) -> Task<Message> {
        let Some(id) = self.active_note else {
            return Task::none();
        };
        let block = format!("![]({rel})");
        if let Some(editor) = &self.editor_content {
            let before = (editor.text(), editor.cursor());
            self.history.record(before, false, Instant::now());
        }
        let (text, end) = match (&self.editor_content, self.editing) {
            (Some(content), true) => {
                // The untrimmed text, so a block typed below blank lines stays there.
                let text = content.text();
                let at = content.cursor().position;
                rich::insert_block(&text, rich::offset_of(&text, at.line, at.column), &block)
            }
            _ => {
                let Some(note) = self.store.notes().iter().find(|n| n.id == id) else {
                    return Task::none();
                };
                rich::insert_block(&note.content, note.content.len(), &block)
            }
        };
        let mut rebuilt = text_editor::Content::with_text(&text);
        let (line, column) = rich::position_of(&text, end);
        rebuilt.move_to(text_editor::Cursor {
            position: text_editor::Position { line, column },
            selection: None,
        });
        self.editor_content = Some(rebuilt);
        if let Some(note) = self.store.note_mut(id) {
            note.content = text.trim_end_matches('\n').to_string();
            note.updated_at = chrono::Utc::now();
        }
        self.store.mark_dirty();
        self.reparse();
        if self.editing {
            focus_body()
        } else {
            Task::none()
        }
    }

    /// Re-parses the open note for the rendered view and checks its images
    /// once, so the view itself never touches the disk.
    pub(super) fn reparse(&mut self) {
        let Some(note) = self
            .active_note
            .and_then(|id| self.store.notes().iter().find(|n| n.id == id))
        else {
            return;
        };
        (self.doc, self.broken_images) = self.parse_note(&note.content);
    }

    /// `content` parsed for the rendered view, and its image references
    /// that are missing or can't be decoded.
    fn parse_note(&self, content: &str) -> (Doc, HashSet<String>) {
        let doc = rich::parse(content, &self.settings.settings().palette);
        let broken = doc
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Image { path, .. } if !images::is_usable(&self.data_dir, path) => {
                    Some(path.clone())
                }
                _ => None,
            })
            .collect();
        (doc, broken)
    }

    pub(super) fn finish_close(&mut self) -> Task<Message> {
        self.drop_header_ask();
        if self.note_drag.is_some() {
            self.finish_note_drag();
        }
        self.commit_title();
        let next = if std::mem::take(&mut self.front_only) {
            self.behind.pop()
        } else {
            self.behind.clear();
            None
        };
        self.placed = None;
        self.active_note = None;
        self.note_hovered = false;
        self.editor_content = None;
        self.editing = false;
        self.doc = Doc { blocks: Vec::new() };
        self.broken_images.clear();
        self.history.clear();
        self.rendered_click = None;
        self.color_picker_open = false;
        self.text_color_picker_open = false;
        self.reminder_picker = None;
        self.note_resize = None;
        let collapse = match self.pending_delete.take() {
            Some(id) => self.start_collapse(id),
            None => Task::none(),
        };
        // The frontmost note behind comes forward, as it was.
        let front = match next {
            Some(open) => {
                let id = open.id;
                self.behind.push(open);
                self.open_note_id(id)
            }
            None => Task::none(),
        };
        Task::batch([collapse, front, self.dock_window()])
    }

    /// Starts shrinking the deleted note's bar away. A collapse still
    /// running for another note finishes at once (see `finish_collapse`
    /// for the task it returns).
    pub(super) fn start_collapse(&mut self, id: Uuid) -> Task<Message> {
        let finished = self.finish_collapse();
        // Only the latest delete can be undone, so not one before this.
        self.last_deleted = None;
        self.behind.retain(|open| open.id != id);
        let mut morph = Morph::with_secs(COLLAPSE_SECS, self.settings.settings().motion.speed);
        morph.restart();
        self.collapsing = Some((id, morph));
        if self.peek_note == Some(id) || self.hover_bar.is_some_and(|(hovered, _)| hovered == id) {
            self.hide_peek();
        }
        self.animating = true;
        finished
    }

    /// Removes the collapsing note (if any) for good and shows the undo
    /// toast for it, replacing any earlier one. The task ends the toast
    /// after `UNDO_FOR` and docks the window, which widens for it.
    pub(super) fn finish_collapse(&mut self) -> Task<Message> {
        let index = self.collapse().map(|(entry, _)| entry);
        let Some((id, _)) = self.collapsing.take() else {
            return Task::none();
        };
        // A bar dragged below the removed one moves up a slot; a drag on
        // the removed bar itself is dropped.
        if let (Some(drag), Some(index)) = (&mut self.drag, index) {
            if drag.bar_index == index {
                self.drag = None;
            } else if drag.bar_index > index {
                drag.bar_index -= 1;
            }
        }
        let deleted = self.store.delete_note(id);
        let pulsing = self.pulsing.remove(&id);
        let _ = self.store.save();
        self.store.did_save();
        self.scroll_offset = self.scroll_offset.min(self.strip_layout().max_scroll);
        let Some(deleted) = deleted else {
            return Task::none();
        };
        let at = Instant::now();
        self.last_deleted = Some(LastDelete {
            deleted,
            pulsing,
            at,
        });
        Task::batch([
            self.dock_window(),
            delayed(UNDO_FOR, Message::UndoExpired(at)),
        ])
    }

    /// The collapsing bar and how far it has shrunk (eased). A stack's bar
    /// stays: its other notes remain.
    pub(super) fn collapse(&self) -> Option<(usize, f32)> {
        let (id, morph) = self.collapsing.as_ref()?;
        let entry = self.id_entry(*id)?;
        if !self.entries()[entry].members.is_empty() {
            return None;
        }
        Some((entry, ease_out_cubic(morph.progress())))
    }

    pub(super) fn is_collapsing(&self, index: usize) -> bool {
        self.collapse().is_some_and(|(c, _)| c == index)
    }

    /// Stores where the note was dropped, so it opens there next time.
    /// Saves the resized note's size, and its position since resizing
    /// from the left or top edge moves it.
    pub(super) fn finish_note_resize(&mut self) {
        let Some(resize) = self.note_resize.take() else {
            return;
        };
        let rect = resize.rect;
        self.placed = None;
        if let Some(note) = self.active_note.and_then(|id| self.store.note_mut(id)) {
            note.size = Some([rect.width, rect.height]);
            note.position = Some([rect.x, rect.y]);
            note.updated_at = chrono::Utc::now();
            self.store.mark_dirty();
        }
    }

    /// The area a note may occupy: the window minus its margins.
    pub(super) fn note_room(&self) -> Rectangle {
        Rectangle::new(
            Point::new(NOTE_MARGIN, NOTE_MARGIN),
            Size::new(
                self.window_size.width - 2.0 * NOTE_MARGIN,
                self.window_size.height - 2.0 * NOTE_MARGIN,
            ),
        )
    }

    pub(super) fn finish_note_drag(&mut self) {
        let rect = self.note_target_rect();
        self.note_drag = None;
        self.note_drag_pos = None;
        self.placed = None;
        if let Some(id) = self.active_note {
            if let Some(note) = self.store.note_mut(id) {
                note.position = Some([rect.x, rect.y]);
                note.updated_at = chrono::Utc::now();
            }
            self.store.mark_dirty();
        }
    }

    pub(super) fn note_frame(&self) -> Option<MorphFrame> {
        let id = self.active_note?;
        let entry = self.id_entry(id)?;
        // A note folding while the strip is away heads for its hidden bar.
        let source = self.slid(*self.strip_layout().bars.get(entry)?);
        Some(morph_frame(
            source,
            self.note_target_rect(),
            self.morph.progress(),
        ))
    }

    /// Ends the title field's edit: the field shows the title without its
    /// valid tag again, so a tag typed in becomes the reminder. A tag other
    /// than the one before the edit counts from now, the commit.
    pub(super) fn commit_title(&mut self) {
        let Some(edit) = self.title_edit.take() else {
            return;
        };
        if let Some(note) = self.store.note_mut(edit.id) {
            let tag = reminder::tag_text(&note.title);
            if tag.is_some() && tag != edit.hidden.as_deref() {
                note.reminder_set_at = Some(chrono::Utc::now());
                self.store.mark_dirty();
            }
        }
    }

    /// What the open note's title field shows: the text as typed while it is
    /// being edited, otherwise the title without its reminder tag.
    pub(super) fn title_field_text(&self) -> Option<String> {
        let id = self.active_note?;
        if let Some(edit) = self.title_edit.as_ref().filter(|e| e.id == id) {
            return Some(edit.draft.clone());
        }
        let note = self.store.notes().iter().find(|n| n.id == id)?;
        Some(crate::note_panel::title_field_text(note))
    }

    pub(super) fn note_target_rect(&self) -> Rectangle {
        if let Some(resize) = &self.note_resize {
            return resize.rect;
        }
        let note = self
            .active_note
            .and_then(|id| self.store.notes().iter().find(|n| n.id == id));
        let default = self.settings.settings().notes.size;
        let [width, height] = note.and_then(|n| n.size).unwrap_or([default, default]);
        let room = self.note_room();
        let width = width.min(room.width.max(MIN_SIZE.width));
        let height = height.min(room.height.max(MIN_SIZE.height));
        let saved = self
            .note_drag_pos
            .or(self.placed)
            .or_else(|| note?.position.map(|[x, y]| Point::new(x, y)));
        if let Some(top_left) = saved {
            let max_x = (self.window_size.width - width - NOTE_MARGIN).max(NOTE_MARGIN);
            let max_y = (self.window_size.height - height - NOTE_MARGIN).max(NOTE_MARGIN);
            return Rectangle::new(
                Point::new(
                    top_left.x.clamp(NOTE_MARGIN, max_x),
                    top_left.y.clamp(NOTE_MARGIN, max_y),
                ),
                Size::new(width, height),
            );
        }

        // Below a Top strip, the note keeps to the room left under it.
        let height = match self.edge() {
            Edge::Right | Edge::Left => height,
            Edge::Top => height.min(self.panel_room().max(MIN_SIZE.height)),
        };
        self.beside_strip(Size::new(width, height), self.anchor_along)
    }
}
