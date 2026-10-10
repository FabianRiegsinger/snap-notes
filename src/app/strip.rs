//! The strip: its bars and peeks, dragging, reordering and stacking bars, drops
//! onto it, the Actions row, the undo toast and auto-hide.

use super::*;

impl App {
    pub(super) fn handle_strip(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StripHover(None) if self.cursor_on_peek() => {
                // Moving from the bar onto its open peek keeps it open.
            }
            Message::StripHover(y) => {
                if self.cursor_y != y {
                    self.cursor_y = y;
                    self.animating = true;
                }
                self.update_hover();
                if let Some(cursor_y) = y {
                    if self.drag.is_some() {
                        return self.update(Message::DragMove(cursor_y));
                    }
                }
            }
            Message::BarClicked(entry) => {
                if let Some(index) = self.clicked_note(entry) {
                    return self.open_note(index);
                }
            }
            Message::ActionsHovered(open) => {
                // Keep Actions collapsed while a panel covers that column.
                let open = open && !self.panel_open();
                let was = self.actions_open;
                self.set_actions_open(open);
                // Without passthrough the window widens now; it narrows once
                // the row has folded back (see `Tick`).
                if open && !was && !SUPPORTS_PASSTHROUGH {
                    return self.dock_window();
                }
            }
            Message::PeekDelete(entry) => {
                let id = self.entry_note(entry).map(|i| self.store.notes()[i].id);
                if let Some(id) = id.filter(|id| self.peek_note == Some(*id)) {
                    self.hide_peek();
                    if self.active_note == Some(id) {
                        // The note is open too: fold it away like its own trash button does.
                        return self.update(Message::DeleteNote(id));
                    }
                    return self.start_collapse(id);
                }
            }
            Message::StripFileDropped(entry, path) => return self.drop_on_strip(entry, &path),
            Message::FileHovered => {
                self.file_hover = true;
                self.last_cursor = None;
            }
            Message::FileHoverEnded => self.file_hover = false,
            Message::PeekTick(now) => {
                self.update_hover();
                if let Some((id, since)) = self.hover_bar {
                    if !self.peek.is_opening()
                        && now.duration_since(since).as_secs_f32()
                            >= self.settings.settings().hover.peek_delay_secs
                    {
                        self.peek_note = Some(id);
                        self.peek_scroll = None;
                        self.peek.open();
                        self.animating = true;
                    }
                }
            }
            Message::DragStart(index, _) if self.is_collapsing(index) => {}
            Message::DragStart(index, origin_y) => {
                self.hide_peek();
                // Origin is the press position, so a click without movement
                // opens the note even when pressed far from the bar's center.
                self.drag = Some(DragState {
                    bar_index: index,
                    origin_y,
                    current_y: origin_y,
                });
            }
            Message::DragMove(y) => {
                if let Some(drag) = &mut self.drag {
                    drag.current_y = y;
                }
            }
            Message::DragEnd => {
                if let Some(drag) = self.drag.take() {
                    let moved = (drag.current_y - drag.origin_y).abs() > 5.0;
                    let onto = moved
                        .then(|| {
                            stack_target(
                                &self.strip_layout().bars,
                                drag.bar_index,
                                drag.current_y,
                                self.edge(),
                            )
                        })
                        .flatten();
                    if let Some(target) = onto {
                        return self.update(Message::StackOnto {
                            dragged: drag.bar_index,
                            target,
                        });
                    } else if moved {
                        let pinned: Vec<bool> = self
                            .entries()
                            .iter()
                            .map(|e| self.store.notes()[e.top].pinned)
                            .collect();
                        let target = strip_model::insertion_slot(
                            &self.bar_centers(),
                            drag.current_y,
                            &pinned,
                            drag.bar_index,
                        );
                        if target != drag.bar_index && target != drag.bar_index + 1 {
                            let to = if target > drag.bar_index {
                                target - 1
                            } else {
                                target
                            };
                            if let Some((from, to)) = self.reorder_notes(drag.bar_index, to) {
                                self.store.reorder(from, to);
                                self.store.mark_dirty();
                            }
                        }
                    } else if let Some(index) = self.clicked_note(drag.bar_index) {
                        return self.open_note(index);
                    }
                }
            }
            Message::StackOnto { dragged, target } => {
                let entries = self.entries();
                let (Some(from), Some(onto)) = (entries.get(dragged), entries.get(target)) else {
                    return Task::none();
                };
                let notes = self.store.notes();
                let (id, onto) = (notes[from.top].id, notes[onto.top].id);
                if !self.dying(id) && !self.dying(onto) && self.store.stack(id, onto) {
                    self.store.mark_dirty();
                    self.follow_open_bar();
                }
            }
            Message::Unstack(entry) => {
                let top = self
                    .entries()
                    .get(entry)
                    .map(|e| self.store.notes()[e.top].id);
                if let Some(top) = top.filter(|top| !self.dying(*top)) {
                    self.hide_peek();
                    self.store.unstack(top);
                    self.store.mark_dirty();
                    self.follow_open_bar();
                }
            }
            Message::OpenStackMember(id) => {
                if let Some(index) = self.store.notes().iter().position(|n| n.id == id) {
                    return self.open_note(index);
                }
            }
            Message::PeekScroll(scroll) => {
                self.peek_scroll = self.peek_note.map(|id| (id, scroll));
            }
            Message::StripScroll(delta) => {
                let max = self.strip_layout().max_scroll;
                self.scroll_offset = (self.scroll_offset - delta).clamp(0.0, max);
            }
            // `update` steps auto-hide after every message; this one only
            // lets the next wake-up be scheduled.
            Message::AutoHideWake(deadline) => {
                if self.auto_hide_wake == Some(deadline) {
                    self.auto_hide_wake = None;
                }
            }
            _ => unreachable!("not a strip message"),
        }
        Task::none()
    }

    /// A file dropped with no note open: appended to the note the bar at
    /// `entry` stands for, or else a new note titled with the file's stem.
    pub(super) fn drop_on_strip(
        &mut self,
        entry: Option<usize>,
        path: &std::path::Path,
    ) -> Task<Message> {
        let text = self.dropped_text(path);
        let top = entry
            .and_then(|entry| self.entry_note(entry))
            .map(|top| self.store.notes()[top].id)
            .filter(|&id| !self.dying(id));
        match top {
            Some(id) => {
                self.append_to_note(id, &text);
                Task::none()
            }
            None => {
                let title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                self.create_note(title, text)
            }
        }
    }

    /// What a dropped file adds to a note: a small text file's text, an
    /// image's reference, or else the file's path.
    pub(super) fn dropped_text(&self, path: &std::path::Path) -> String {
        if let Some(text) = read_text_file(path) {
            return text;
        }
        match images::import(&self.data_dir, path) {
            Ok(rel) => format!("![]({rel})"),
            Err(_) => path.display().to_string(),
        }
    }

    /// Tracks which bar the cursor rests on. Leaving it collapses the peek;
    /// moving to another bar while a peek is showing peeks it right away.
    pub(super) fn update_hover(&mut self) {
        let hovered = match (self.cursor_y, self.active_note, &self.drag) {
            (Some(y), None, None)
                if !self.settings_panel.open
                    && !self.search_panel.open
                    && self.export.is_none() =>
            {
                let strip = self.strip_layout();
                strip
                    .bars
                    .iter()
                    .position(|bar| {
                        let (start, length) = along_span(*bar, strip.edge);
                        (start..=start + length).contains(&y)
                    })
                    .filter(|i| !self.is_collapsing(*i))
                    .and_then(|i| self.entry_note(i))
                    .map(|i| self.store.notes()[i].id)
            }
            _ => None,
        };
        if hovered == self.hover_bar.map(|(id, _)| id) {
            return;
        }
        self.hover_bar = hovered.map(|id| (id, Instant::now()));
        self.sync_jump_settle(hovered);
        match hovered {
            Some(id) if self.peek.is_opening() => {
                self.peek_note = Some(id);
                self.peek.restart();
            }
            _ => self.peek.close(),
        }
        self.animating = true;
    }

    pub(super) fn cursor_on_peek(&self) -> bool {
        match (self.last_cursor, self.peek_rect()) {
            (Some(cursor), Some(rect)) => rect.contains(cursor),
            _ => false,
        }
    }

    /// The open peek's full area, while a peek is open or opening.
    pub(super) fn peek_rect(&self) -> Option<Rectangle> {
        if !self.peek.is_opening() {
            return None;
        }
        let id = self.peek_note?;
        let entry = self.id_entry(id)?;
        let bar = *self.strip_layout().bars.get(entry)?;
        let strip = self.strip_bounds();
        let entries = self.entries();
        let entry = entries.get(entry)?;
        let notes = self.store.notes();
        let edge = self.edge();
        let width = note_peek_width(
            &notes[entry.top],
            self.settings.settings().notes.size,
            bar,
            edge,
        );
        Some(peek_target(
            bar,
            strip,
            &entry_peek_text(notes, entry, width),
            width,
            edge,
        ))
    }

    pub(super) fn hide_peek(&mut self) {
        self.hover_bar = None;
        self.jump_settle = None;
        self.peek_note = None;
        self.peek = Morph::peek(self.settings.settings().motion.speed);
    }

    /// The peeked bar and how far its peek is open, as the strip shows it.
    pub(super) fn strip_peek(&self) -> Option<(usize, f32)> {
        self.peek_note
            .and_then(|id| self.id_entry(id))
            .map(|entry| (entry, self.peek.progress()))
    }

    /// How far the open peek's body is scrolled: 0 for a note not scrolled
    /// since it was last peeked.
    pub(super) fn peek_body_scroll(&self) -> f32 {
        self.peek_scroll
            .filter(|(id, _)| self.peek_note == Some(*id))
            .map_or(0.0, |(_, scroll)| scroll)
    }

    /// Whether `id`'s copy button still shows its check.
    pub(super) fn copied_for(&self, id: Uuid) -> bool {
        self.copied_at.is_some_and(|(copied, _)| copied == id)
    }

    /// The undo toast is up and no panel covers it.
    pub(super) fn toast_visible(&self) -> bool {
        self.last_deleted.is_some() && !self.panel_open()
    }

    /// The undo toast's area, while the strip draws it: not under a panel,
    /// nor while the peek, a drag or the empty-clipboard chip hides it.
    pub(super) fn toast_rect(&self) -> Option<Rectangle> {
        let kind = crate::bar_strip::hint_kind(
            self.strip_peek(),
            self.drag.as_ref(),
            self.clipboard_empty_at.is_some(),
            self.toast_visible(),
            self.panel_open(),
        );
        (kind == crate::bar_strip::HintKind::Toast)
            .then(|| crate::bar_strip::toast_rect(&self.strip_layout(), self.strip_bounds()))
    }

    /// Where the strip widget sits in the window: the strip band, or wider
    /// while Actions is expanded so Search/Settings stay hittable.
    pub(super) fn strip_bounds(&self) -> Rectangle {
        let edge = self.edge();
        let thickness = crate::bar_strip::strip_thickness(
            &self.settings.settings().bars,
            self.entries().len(),
            |i| self.magnification.scale(i),
            self.actions_open,
            self.actions_progress(),
        );
        edge.rect_to_window(
            LocalRect {
                along: 0.0,
                away: 0.0,
                length: edge.edge_length(self.window_size),
                thickness,
            },
            self.window_size,
        )
    }

    /// Expands or collapses the Actions row, animated from where it is.
    pub(super) fn set_actions_open(&mut self, open: bool) {
        self.actions_open = open;
        if open {
            self.actions_morph.open();
        } else {
            self.actions_morph.close();
        }
        self.animating = true;
    }

    /// How far the Actions row has unfolded, eased: out on the way open and,
    /// time-reversed, in on the way back, so a reversal never jumps.
    pub(super) fn actions_progress(&self) -> f32 {
        ease_out_cubic(self.actions_morph.progress())
    }

    pub(super) fn strip_layout(&self) -> StripLayout {
        let bounds = self.strip_bounds();
        let edge = self.edge();
        compute_layout(
            LayoutInput {
                count: self.entries().len(),
                bounds: band(bounds, self.strip_fraction(), edge),
                scroll_offset: self.scroll_offset,
                bars: &self.settings.settings().bars,
                actions_expanded: self.actions_open,
                actions_progress: self.actions_progress(),
                collapse: self.collapse(),
                edge,
            },
            |i| self.magnification.scale(i),
        )
    }

    /// Share of the window's length along the edge the strip uses. Without
    /// passthrough the window itself is already sized to the configured
    /// fraction.
    pub(super) fn strip_fraction(&self) -> f32 {
        if SUPPORTS_PASSTHROUGH {
            self.settings.settings().window.height_fraction
        } else {
            1.0
        }
    }

    /// Bar centers plus the Actions slot(s), which magnify along with them.
    pub(super) fn magnification_centers(&self) -> Vec<f32> {
        let strip = self.strip_layout();
        let edge = strip.edge;
        let mut centers: Vec<f32> = strip.bars.iter().map(|r| along_center(*r, edge)).collect();
        if strip.actions_expanded {
            centers.extend(
                [strip.add_button, strip.search_button, strip.settings_button]
                    .map(|r| along_center(r, edge)),
            );
        } else {
            centers.push(along_center(strip.actions_button, edge));
        }
        centers
    }

    /// The strip's entries, one per bar: the cached ones while the store
    /// hasn't changed since [`App::sync_entries`].
    pub(super) fn entries(&self) -> Cow<'_, [Entry]> {
        if self.entries_synced == self.store.revision() {
            Cow::Borrowed(&self.strip_entries)
        } else {
            Cow::Owned(strip_model::entries(self.store.notes()))
        }
    }

    pub(super) fn sync_entries(&mut self) {
        if self.entries_synced != self.store.revision() {
            self.strip_entries = strip_model::entries(self.store.notes());
            self.strip_progress = strip_model::progress(self.store.notes(), &self.strip_entries);
            self.entries_synced = self.store.revision();
        }
    }

    /// The note bar `entry` stands for (the stack's top), by note index.
    /// While a stack's top is being deleted, its bar stands for the next
    /// note, which takes its place.
    pub(super) fn entry_note(&self, entry: usize) -> Option<usize> {
        let entries = self.entries();
        let entry = entries.get(entry)?;
        match entry.members.first() {
            Some(&next) if self.dying(self.store.notes()[entry.top].id) => Some(next),
            _ => Some(entry.top),
        }
    }

    /// The note a click on the bar at `entry` opens: the first of its notes
    /// (top, then members) whose reminder pulses, else the note it stands
    /// for. Clicking the open note's bar still closes it.
    pub(super) fn clicked_note(&self, entry: usize) -> Option<usize> {
        let index = self.entry_note(entry)?;
        let notes = self.store.notes();
        if self.active_note == Some(notes[index].id) && self.morph.is_opening() {
            return Some(index);
        }
        let pulsing = self.entries().get(entry)?.notes().find(|&i| {
            let id = notes[i].id;
            self.pulsing.contains(&id) && !self.dying(id)
        });
        Some(pulsing.unwrap_or(index))
    }

    /// Whether the note is being deleted while its bar collapses.
    pub(super) fn dying(&self, id: Uuid) -> bool {
        self.collapsing.as_ref().is_some_and(|(c, _)| *c == id)
    }

    /// The open note's bar may have moved; its panel follows.
    pub(super) fn follow_open_bar(&mut self) {
        self.sync_entries();
        if let Some(bar) = self
            .active_note
            .and_then(|id| self.id_entry(id))
            .and_then(|entry| self.strip_layout().bars.get(entry).copied())
        {
            self.anchor_along = along_center(bar, self.edge());
        }
    }

    /// The bar holding the note at `index`: a stacked note's is its stack's.
    pub(super) fn note_entry(&self, index: usize) -> Option<usize> {
        self.entries().iter().position(|e| e.holds(index))
    }

    /// Whether the note's stack (or the note itself) is pinned: a pinned top
    /// pins its whole stack.
    pub(super) fn top_pinned(&self, note: &crate::note::Note) -> bool {
        match note.stack {
            Some(top) => self.store.notes().iter().any(|n| n.id == top && n.pinned),
            None => note.pinned,
        }
    }

    pub(super) fn id_entry(&self, id: Uuid) -> Option<usize> {
        let index = self.store.notes().iter().position(|n| n.id == id)?;
        self.note_entry(index)
    }

    /// Maps moving bar `from` to slot `to` of the remaining bars onto the
    /// store: the top's note index and the note index its group lands at.
    pub(super) fn reorder_notes(&self, from: usize, to: usize) -> Option<(usize, usize)> {
        let entries = self.entries();
        let top = entries.get(from)?.top;
        let to = entries
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != from)
            .take(to)
            .map(|(_, e)| 1 + e.members.len())
            .sum();
        Some((top, to))
    }

    pub(super) fn bar_centers(&self) -> Vec<f32> {
        let strip = self.strip_layout();
        strip
            .bars
            .iter()
            .map(|bar| along_center(*bar, strip.edge))
            .collect()
    }

    /// What auto-hide goes by now. An open note doesn't block hiding.
    pub(super) fn auto_hide_inputs(&self) -> autohide::Inputs {
        let at = |test: fn(&Self, Point) -> bool| self.pointer.is_some_and(|p| test(self, p));
        autohide::Inputs {
            enabled: self.settings.settings().window.auto_hide && self.visible,
            in_edge: at(Self::in_edge),
            in_use_area: at(Self::in_use_area),
            blocked: self.panel_open()
                || self.drag.is_some()
                || self.file_hover
                || self.last_deleted.is_some()
                || self.clipboard_empty_at.is_some()
                || self.export_dialog_open,
            force_reveal: self.force_reveal,
        }
    }

    /// Steps auto-hide at `now` by `dt` seconds. Returns a wake-up for a
    /// newly pending dwell or grace deadline, and the passthrough update
    /// once the strip starts or stops being in place. Keeps frames running
    /// while it slides.
    pub(super) fn drive_auto_hide(&mut self, now: Instant, dt: f32) -> Task<Message> {
        let was_in_place = self.strip_away_offset() == 0.0;
        let was = self.auto_hide.phase();
        let inputs = self.auto_hide_inputs();
        self.force_reveal = false;
        let speed = self.settings.settings().motion.speed;
        if self.auto_hide.step(inputs, now, dt, speed) {
            self.animating = true;
        }
        let mut tasks = Vec::new();
        let phase = self.auto_hide.phase();
        if phase != was {
            if phase == autohide::Phase::Hiding {
                // The strip may not see the cursor go once it slides (no
                // passthrough, no polling); drop the hover so the bars
                // don't magnify around a stale spot when they come back.
                self.cursor_y = None;
                self.update_hover();
                self.animating = true;
            }
            // Without passthrough: shrink to the sliver once Hidden, widen
            // before a reveal slides in.
            tasks.push(self.dock_window());
        }
        if let Some(deadline) = self.auto_hide.next_deadline() {
            if self.auto_hide_wake != Some(deadline) {
                self.auto_hide_wake = Some(deadline);
                tasks.push(delayed(
                    deadline.saturating_duration_since(now),
                    Message::AutoHideWake(deadline),
                ));
            }
        }
        if was_in_place != (self.strip_away_offset() == 0.0) {
            tasks.push(self.update_passthrough(self.pointer));
        }
        Task::batch(tasks)
    }

    /// How far toward its screen edge the strip is drawn while auto-hide
    /// slides it away (an away offset; see `hide_translation`).
    pub(super) fn strip_away_offset(&self) -> f32 {
        self.auto_hide.offset() * HIDE_SHIFT
    }

    /// Where the strip's `rect` is drawn while auto-hide slides it away.
    pub(super) fn slid(&self, rect: Rectangle) -> Rectangle {
        rect + hide_translation(self.edge(), self.strip_away_offset())
    }
}
