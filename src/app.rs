use crate::animation::{lerp, morph_frame, AnimationState, MagnificationState, Morph, MorphFrame};
use crate::bar_strip::{band, bar_strip, compute_layout, StripLayout, STRIP_WIDTH};
use crate::note::NoteColor;
use crate::note_panel::{post_it, PostIt};
use crate::platform::{self, SUPPORTS_PASSTHROUGH};
use crate::settings::{SettingKey, SettingToggle, Settings, SettingsGroup, SettingsStore};
use crate::settings_panel::{settings_panel, SettingsView, PANEL_MAX_HEIGHT, PANEL_WIDTH};
use crate::store::NoteStore;

use iced::widget::{container, mouse_area, opaque, pin, stack, text_editor, Space};
use iced::{
    event, keyboard, mouse, window, Element, Fill, Point, Rectangle, Size, Subscription, Task,
    Vector,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(crate) const NOTE_GAP: f32 = 18.0;
pub(crate) const NOTE_MARGIN: f32 = 24.0;
/// While mouse passthrough is on, how often the OS cursor is polled to notice
/// it entering the strip or the note again.
const CURSOR_POLL: Duration = Duration::from_millis(50);
const PEEK_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub enum Message {
    StripHover(Option<f32>),
    BarClicked(usize),
    AddNote,
    ToggleSettings,
    CloseSettings,
    SettingChanged(SettingKey, f32),
    SettingToggled(SettingToggle),
    ResetGroup(SettingsGroup),
    /// Palette slot whose preset grid is open (`None` closes it).
    PaletteSlotSelected(Option<usize>),
    PaletteColorChosen(NoteColor),
    NoteEdited(text_editor::Action),
    TitleEdited(String),
    NoteHovered(bool),
    NoteDragStart,
    NoteResetPosition,
    ClosePanel,
    DeleteRequested,
    ConfirmDelete(bool),
    ToggleColorPicker,
    ColorChosen(NoteColor),
    ExpandNote,
    ShrinkNote,
    DragStart(usize, f32),
    DragMove(f32),
    DragEnd,
    StripScroll(f32),
    Tick(Instant),
    SaveTick,
    ToggleVisibility,
    Quit,
    WindowReady(window::Id, Option<Size>),
    WindowResized(Size),
    Key(keyboard::Event),
    CursorMoved(Point),
    CursorLeftWindow,
    MouseButton(bool),
    WindowUnfocused,
    PollCursor,
    PeekTick(Instant),
    CursorPolled(Option<Point>),
}

pub struct App {
    store: NoteStore,
    settings: SettingsStore,
    magnification: MagnificationState,
    cursor_y: Option<f32>,
    active_note: Option<Uuid>,
    editor_content: Option<text_editor::Content>,
    morph: Morph,
    anchor_y: f32,
    pending_delete: Option<Uuid>,
    expanded: bool,
    expand_animation: AnimationState,
    color_picker_open: bool,
    note_hovered: bool,
    drag: Option<DragState>,
    scroll_offset: f32,
    confirm_delete: Option<Uuid>,
    animating: bool,
    last_tick: Option<Instant>,
    visible: bool,
    window_id: Option<window::Id>,
    monitor: Option<Size>,
    window_size: Size,
    /// Clicks fall through the window to the apps behind it.
    passthrough: bool,
    mouse_down: bool,
    last_cursor: Option<Point>,
    /// Grab offset (cursor minus note top-left) while the open note is dragged.
    note_drag: Option<Vector>,
    note_drag_pos: Option<Point>,
    /// Bar under the cursor and since when, for the hover peek delay.
    hover_bar: Option<(Uuid, Instant)>,
    /// Note whose bar is widened into a peek (kept while it collapses).
    peek_note: Option<Uuid>,
    peek: Morph,
    /// The settings panel is showing (kept while it folds back into the gear).
    settings_open: bool,
    settings_morph: Morph,
    /// Gear center when the panel opened; the panel stays centered on it.
    settings_anchor_y: f32,
    palette_slot: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DragState {
    pub bar_index: usize,
    pub origin_y: f32,
    pub current_y: f32,
}

/// Notes and settings live next to the executable (portable app).
fn data_dir() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let settings = SettingsStore::load(data_dir().join("settings.json"));
        let s = settings.settings();
        let mut store = NoteStore::load(data_dir().join("notes.json"));
        if store.notes().is_empty() {
            store.seed_templates(&s.palette);
            let _ = store.save();
        }
        let morph = Morph::new(s.motion.speed);
        let peek = Morph::peek(s.motion.speed);
        let settings_morph = Morph::new(s.motion.speed);
        let expand_animation = AnimationState::new(0.0, s.motion.stiffness);
        let window_size = Size::new(Self::docked_width(s, false), 600.0);
        (
            Self {
                store,
                settings,
                magnification: MagnificationState::new(),
                cursor_y: None,
                active_note: None,
                editor_content: None,
                morph,
                anchor_y: 0.0,
                pending_delete: None,
                expanded: false,
                expand_animation,
                color_picker_open: false,
                note_hovered: false,
                drag: None,
                scroll_offset: 0.0,
                confirm_delete: None,
                animating: false,
                last_tick: None,
                visible: true,
                window_id: None,
                monitor: None,
                window_size,
                passthrough: false,
                mouse_down: false,
                last_cursor: None,
                note_drag: None,
                note_drag_pos: None,
                hover_bar: None,
                peek_note: None,
                peek,
                settings_open: false,
                settings_morph,
                settings_anchor_y: 0.0,
                palette_slot: None,
            },
            window::oldest().then(|id| match id {
                Some(id) => window::monitor_size(id).map(move |m| Message::WindowReady(id, m)),
                None => Task::none(),
            }),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
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
            Message::BarClicked(index) => return self.open_note(index),
            Message::ToggleSettings => {
                if self.settings_open && self.settings_morph.is_opening() {
                    return self.update(Message::CloseSettings);
                }
                let close_note = self.update(Message::ClosePanel);
                self.hide_peek();
                let gear = self.strip_layout().settings_button;
                self.settings_anchor_y = gear.y + gear.height / 2.0;
                self.settings_open = true;
                self.palette_slot = None;
                self.settings_morph.open();
                self.animating = true;
                return Task::batch([close_note, self.dock_window()]);
            }
            Message::CloseSettings => {
                if self.settings_open {
                    self.settings_morph.close();
                    self.animating = true;
                }
            }
            Message::SettingChanged(key, value) => {
                self.settings.settings_mut().set(key, value);
                return self.apply_settings();
            }
            Message::SettingToggled(toggle) => {
                self.settings.settings_mut().toggle(toggle);
            }
            Message::ResetGroup(group) => {
                if group == SettingsGroup::Palette {
                    let changes = self.settings.settings_mut().reset_palette();
                    if self.store.recolor_many(&changes) {
                        self.store.mark_dirty();
                    }
                } else {
                    self.settings.settings_mut().reset(group);
                }
                return self.apply_settings();
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
                    }
                }
            }
            Message::AddNote => {
                self.store.add_note(&self.settings.settings().palette);
                self.store.mark_dirty();
                let last = self.store.notes().len() - 1;
                self.scroll_offset = self.strip_layout().max_scroll;
                return self.open_note(last);
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
            Message::NoteResetPosition => {
                if let Some(id) = self.active_note {
                    if let Some(note) = self.store.note_mut(id) {
                        note.position = None;
                    }
                    self.store.mark_dirty();
                }
            }
            Message::TitleEdited(title) => {
                if let Some(id) = self.active_note {
                    if let Some(note) = self.store.note_mut(id) {
                        note.title = title;
                        note.updated_at = chrono::Utc::now();
                    }
                    self.store.mark_dirty();
                }
            }
            Message::NoteEdited(action) => {
                if let Some(content) = &mut self.editor_content {
                    let is_edit = action.is_edit();
                    content.perform(action);
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
            Message::ClosePanel => {
                if self.active_note.is_some() {
                    self.morph.close();
                    self.animating = true;
                    self.color_picker_open = false;
                    self.confirm_delete = None;
                }
            }
            Message::DeleteRequested => {
                if let Some(id) = self.active_note {
                    self.confirm_delete = Some(id);
                }
            }
            Message::ConfirmDelete(confirmed) => {
                if confirmed {
                    if let Some(id) = self.confirm_delete.take() {
                        // Fold the note back into its bar first, then delete it.
                        self.pending_delete = Some(id);
                        self.morph.close();
                        self.animating = true;
                    }
                } else {
                    self.confirm_delete = None;
                }
            }
            Message::Tick(now) => {
                let dt = self
                    .last_tick
                    .map_or(1.0 / 60.0, |t| now.duration_since(t).as_secs_f32())
                    .clamp(0.0, 1.0 / 30.0);
                self.last_tick = Some(now);

                let centers = self.magnification_centers();
                let mag_active = self.magnification.update(
                    self.cursor_y,
                    &centers,
                    dt,
                    &self.settings.settings().hover,
                );
                let morph_active =
                    self.morph.tick(dt) | self.peek.tick(dt) | self.settings_morph.tick(dt);
                if self.peek.is_closed() {
                    self.peek_note = None;
                }
                let settings_closed = self.settings_open && self.settings_morph.is_closed();
                if settings_closed {
                    self.settings_open = false;
                    self.palette_slot = None;
                }
                let expand_active = self.expand_animation.tick(dt);
                self.animating = mag_active || morph_active || expand_active;
                if !self.animating {
                    self.last_tick = None;
                }

                if self.active_note.is_some() && self.morph.is_closed() {
                    return self.finish_close();
                }
                if settings_closed {
                    // Without passthrough the window shrinks back to the strip.
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
            }
            Message::ColorChosen(color) => {
                if let Some(id) = self.active_note {
                    if let Some(note) = self.store.note_mut(id) {
                        note.color = color;
                        note.updated_at = chrono::Utc::now();
                    }
                    self.store.mark_dirty();
                    self.color_picker_open = false;
                }
            }
            Message::ExpandNote => {
                self.expanded = true;
                self.expand_animation.set_target(1.0);
                self.animating = true;
            }
            Message::ShrinkNote => {
                self.expanded = false;
                self.expand_animation.set_target(0.0);
                self.animating = true;
            }
            Message::PeekTick(now) => {
                self.update_hover();
                if let Some((id, since)) = self.hover_bar {
                    if !self.peek.is_opening()
                        && now.duration_since(since).as_secs_f32()
                            >= self.settings.settings().hover.peek_delay_secs
                    {
                        self.peek_note = Some(id);
                        self.peek.open();
                        self.animating = true;
                    }
                }
            }
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
                    if moved {
                        let bars = self.bar_centers();
                        let mut target = bars.len();
                        for (i, center) in bars.iter().enumerate() {
                            if drag.current_y < *center {
                                target = i;
                                break;
                            }
                        }
                        if target != drag.bar_index && target != drag.bar_index + 1 {
                            let to = if target > drag.bar_index {
                                target - 1
                            } else {
                                target
                            };
                            self.store.reorder(drag.bar_index, to);
                            self.store.mark_dirty();
                        }
                    } else {
                        return self.open_note(drag.bar_index);
                    }
                }
            }
            Message::StripScroll(delta) => {
                let max = self.strip_layout().max_scroll;
                self.scroll_offset = (self.scroll_offset - delta).clamp(0.0, max);
            }
            Message::ToggleVisibility => {
                self.visible = !self.visible;
            }
            Message::Quit => {
                let _ = self.store.save();
                let _ = self.settings.save();
                std::process::exit(0);
            }
            Message::WindowReady(id, monitor) => {
                self.window_id = Some(id);
                self.monitor = monitor;
                let shadow = window::run(id, platform::disable_native_shadow).discard();
                let dock = self.dock_window();
                return Task::batch([shadow, dock, self.update_passthrough(None)]);
            }
            Message::WindowResized(size) => {
                self.window_size = size;
            }
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                use keyboard::key::Named;
                match key.as_ref() {
                    keyboard::Key::Named(Named::Escape) if self.settings_open => {
                        return self.update(Message::CloseSettings)
                    }
                    keyboard::Key::Named(Named::Escape) => return self.update(Message::ClosePanel),
                    keyboard::Key::Character(",") if modifiers.command() => {
                        return self.update(Message::ToggleSettings)
                    }
                    keyboard::Key::Character("n") if modifiers.command() => {
                        return self.update(Message::AddNote)
                    }
                    _ => {}
                }
            }
            Message::Key(_) => {}
            Message::CursorMoved(position) => {
                self.last_cursor = Some(position);
                if let Some(offset) = self.note_drag {
                    self.note_drag_pos = Some(position - offset);
                }
                return self.update_passthrough(Some(position));
            }
            Message::CursorLeftWindow => {
                self.last_cursor = None;
                return self.update_passthrough(None);
            }
            Message::MouseButton(down) => {
                self.mouse_down = down;
                if !down && self.note_drag.is_some() {
                    self.finish_note_drag();
                }
            }
            Message::WindowUnfocused => {
                // Clicking another app (through the passthrough area) closes
                // the note and the settings.
                let close_settings = self.update(Message::CloseSettings);
                return Task::batch([close_settings, self.update(Message::ClosePanel)]);
            }
            Message::PollCursor => {
                if let Some(id) = self.window_id {
                    return window::run(id, platform::cursor_in_window).map(Message::CursorPolled);
                }
            }
            Message::CursorPolled(position) => {
                if self.passthrough && position.is_some_and(|p| self.is_interactive(p)) {
                    return self.update_passthrough(position);
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let strip = container(bar_strip(
            self.store.notes(),
            &self.magnification,
            &self.drag,
            self.scroll_offset,
            self.peek_note
                .and_then(|id| self.store.notes().iter().position(|n| n.id == id))
                .map(|index| (index, self.peek.progress())),
            &self.settings.settings().bars,
            self.strip_fraction(),
        ))
        .width(Fill)
        .height(Fill)
        .align_x(iced::Alignment::End);

        let mut layers: Vec<Element<'_, Message>> = Vec::new();

        if let (Some(id), Some(content)) = (self.active_note, &self.editor_content) {
            if let Some(index) = self.store.notes().iter().position(|n| n.id == id) {
                let note = &self.store.notes()[index];
                let frame = self.note_frame().expect("active note has a frame");
                let rect = frame.rect;

                if rect.width >= 1.0 && rect.height >= 1.0 {
                    // Clicking anywhere outside the note closes it.
                    layers.push(
                        mouse_area(Space::new().width(Fill).height(Fill))
                            .on_press(Message::ClosePanel)
                            .into(),
                    );
                    let note_view = post_it(PostIt {
                        note,
                        palette: &self.settings.settings().palette,
                        paper_tint: self.settings.settings().notes.paper_tint,
                        idle_control_alpha: self.settings.settings().notes.idle_control_alpha,
                        content,
                        size: rect.size(),
                        morph_progress: self.morph.progress(),
                        content_alpha: frame.content_alpha,
                        confirm_delete: self.confirm_delete.is_some(),
                        expanded: self.expanded,
                        color_picker_open: self.color_picker_open,
                        hovered: self.note_hovered,
                        dragging: self.note_drag.is_some(),
                    });
                    let note_view = mouse_area(note_view)
                        .on_enter(Message::NoteHovered(true))
                        .on_exit(Message::NoteHovered(false));
                    layers.push(pin(opaque(note_view)).x(rect.x).y(rect.y).into());
                }
            }
        }

        if let Some(frame) = self.settings_frame() {
            let rect = frame.rect;
            if rect.width >= 1.0 && rect.height >= 1.0 {
                layers.push(
                    mouse_area(Space::new().width(Fill).height(Fill))
                        .on_press(Message::CloseSettings)
                        .into(),
                );
                let panel = settings_panel(SettingsView {
                    settings: self.settings.settings(),
                    size: rect.size(),
                    morph_progress: self.settings_morph.progress(),
                    content_alpha: frame.content_alpha,
                    selected_slot: self.palette_slot,
                    tray_ok: false,
                });
                layers.push(pin(opaque(panel)).x(rect.x).y(rect.y).into());
            }
        }

        layers.push(strip.into());
        stack(layers).width(Fill).height(Fill).into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let save = iced::time::every(Duration::from_secs(1)).map(|_| Message::SaveTick);
        let resized = window::resize_events().map(|(_id, size)| Message::WindowResized(size));
        let keys = keyboard::listen().map(Message::Key);
        let pointer = event::listen_with(|event, _status, _id| match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::CursorLeftWindow),
            iced::Event::Mouse(mouse::Event::ButtonPressed(_)) => Some(Message::MouseButton(true)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                Some(Message::MouseButton(false))
            }
            iced::Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused),
            _ => None,
        });

        let mut subs = vec![save, resized, keys, pointer];
        if self.animating {
            // Frame-synced ticks keep the morph in step with the display.
            subs.push(window::frames().map(Message::Tick));
        }
        if self.passthrough {
            subs.push(iced::time::every(CURSOR_POLL).map(|_| Message::PollCursor));
        }
        if self.hover_bar.is_some() && !self.peek.is_opening() {
            subs.push(iced::time::every(PEEK_POLL).map(Message::PeekTick));
        }
        Subscription::batch(subs)
    }

    fn open_note(&mut self, index: usize) -> Task<Message> {
        let Some(note) = self.store.notes().get(index) else {
            return Task::none();
        };
        let id = note.id;

        if self.active_note == Some(id) && self.morph.is_opening() {
            return self.update(Message::ClosePanel);
        }

        let switching = self.active_note.is_some_and(|active| active != id);
        self.editor_content = Some(text_editor::Content::with_text(&note.content));
        self.hide_peek();
        // A note and the settings never show at the same time.
        if self.settings_open {
            self.settings_morph.close();
        }
        self.active_note = Some(id);
        self.pending_delete = None;
        self.color_picker_open = false;
        self.confirm_delete = None;
        if let Some(bar) = self.strip_layout().bars.get(index) {
            self.anchor_y = bar.y + bar.height / 2.0;
        }
        if switching {
            self.morph.restart();
        } else {
            self.morph.open();
        }
        self.animating = true;
        self.dock_window()
    }

    fn finish_close(&mut self) -> Task<Message> {
        if self.note_drag.is_some() {
            self.finish_note_drag();
        }
        self.active_note = None;
        self.note_hovered = false;
        self.editor_content = None;
        self.color_picker_open = false;
        self.confirm_delete = None;
        self.expanded = false;
        self.expand_animation = AnimationState::new(0.0, self.settings.settings().motion.stiffness);
        if let Some(id) = self.pending_delete.take() {
            self.store.delete_note(id);
            let _ = self.store.save();
            self.store.did_save();
            self.scroll_offset = self.scroll_offset.min(self.strip_layout().max_scroll);
        }
        self.dock_window()
    }

    /// Window width for the current state. With passthrough the window always
    /// has room for an open note, so opening and closing never resize it (a
    /// resize is a separate move + resize and flickers for a frame).
    fn docked_width(settings: &Settings, note_open: bool) -> f32 {
        if SUPPORTS_PASSTHROUGH || note_open {
            settings.open_width()
        } else {
            STRIP_WIDTH
        }
    }

    /// Docks the window to the right screen edge, vertically centered. The
    /// strip is centered inside, so it sits at the middle of the right screen
    /// border. Does nothing if the window is already there.
    fn dock_window(&mut self) -> Task<Message> {
        let (Some(id), Some(monitor)) = (self.window_id, self.monitor) else {
            return Task::none();
        };
        // With passthrough the window covers the whole screen, so an open note
        // can be dragged anywhere and the strip sits at the exact vertical
        // center of the right edge.
        let size = if SUPPORTS_PASSTHROUGH {
            monitor
        } else {
            Size::new(
                Self::docked_width(
                    self.settings.settings(),
                    self.active_note.is_some() || self.settings_open,
                ),
                (monitor.height * self.settings.settings().window.height_fraction).round(),
            )
        };
        if size == self.window_size {
            return Task::none();
        }
        let y = ((monitor.height - size.height) / 2.0).round();
        self.window_size = size;
        Task::batch([
            window::move_to(id, Point::new(monitor.width - size.width, y)),
            window::resize(id, size),
        ])
    }

    /// Whether the window should catch the mouse at `position`: over the bar
    /// stack, the open note or the settings. Everywhere else clicks go to the apps behind.
    fn is_interactive(&self, position: Point) -> bool {
        let strip = self.strip_layout();
        let top = strip.bars.first().map_or(strip.add_hit_area.y, |b| b.y)
            - self.settings.settings().bars.gap;
        let bottom = strip.settings_hit_area.y + strip.settings_hit_area.height;
        let over_strip = position.x >= self.window_size.width - STRIP_WIDTH
            && (top..=bottom).contains(&position.y);
        let over_panel = |frame: Option<MorphFrame>| {
            frame.is_some_and(|frame| frame.rect.expand(8.0).contains(position))
        };
        over_strip || over_panel(self.note_frame()) || over_panel(self.settings_frame())
    }

    fn update_passthrough(&mut self, cursor: Option<Point>) -> Task<Message> {
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

    /// Stores where the note was dropped, so it opens there next time.
    fn finish_note_drag(&mut self) {
        let rect = self.note_target_rect();
        self.note_drag = None;
        self.note_drag_pos = None;
        if let Some(id) = self.active_note {
            if let Some(note) = self.store.note_mut(id) {
                note.position = Some([rect.x, rect.y]);
                note.updated_at = chrono::Utc::now();
            }
            self.store.mark_dirty();
        }
    }

    /// Tracks which bar the cursor rests on. Leaving it collapses the peek;
    /// moving to another bar while a peek is showing peeks it right away.
    fn update_hover(&mut self) {
        let hovered = match (self.cursor_y, self.active_note, &self.drag) {
            (Some(y), None, None) if !self.settings_open => {
                let strip = self.strip_layout();
                strip
                    .bars
                    .iter()
                    .position(|bar| (bar.y..=bar.y + bar.height).contains(&y))
                    .map(|i| self.store.notes()[i].id)
            }
            _ => None,
        };
        if hovered == self.hover_bar.map(|(id, _)| id) {
            return;
        }
        self.hover_bar = hovered.map(|id| (id, Instant::now()));
        match hovered {
            Some(id) if self.peek.is_opening() => {
                self.peek_note = Some(id);
                self.peek.restart();
            }
            _ => self.peek.close(),
        }
        self.animating = true;
    }

    fn hide_peek(&mut self) {
        self.hover_bar = None;
        self.peek_note = None;
        self.peek = Morph::peek(self.settings.settings().motion.speed);
    }

    /// The settings panel's current frame, morphing out of the gear slot.
    fn settings_frame(&self) -> Option<MorphFrame> {
        if !self.settings_open {
            return None;
        }
        let source = self.strip_layout().settings_button;
        let height = (self.window_size.height - 2.0 * NOTE_MARGIN).min(PANEL_MAX_HEIGHT);
        let right = self.window_size.width - STRIP_WIDTH - NOTE_GAP;
        let min_center = NOTE_MARGIN + height / 2.0;
        let max_center = self.window_size.height - NOTE_MARGIN - height / 2.0;
        let center_y = if max_center > min_center {
            self.settings_anchor_y.clamp(min_center, max_center)
        } else {
            self.window_size.height / 2.0
        };
        let target = Rectangle::new(
            Point::new(right - PANEL_WIDTH, center_y - height / 2.0),
            Size::new(PANEL_WIDTH, height),
        );
        Some(morph_frame(source, target, self.settings_morph.progress()))
    }

    fn note_frame(&self) -> Option<MorphFrame> {
        let id = self.active_note?;
        let index = self.store.notes().iter().position(|n| n.id == id)?;
        let source = *self.strip_layout().bars.get(index)?;
        Some(morph_frame(
            source,
            self.note_target_rect(),
            self.morph.progress(),
        ))
    }

    fn strip_layout(&self) -> StripLayout {
        let bounds = Rectangle::new(
            Point::new(self.window_size.width - STRIP_WIDTH, 0.0),
            Size::new(STRIP_WIDTH, self.window_size.height),
        );
        compute_layout(
            self.store.notes().len(),
            |i| self.magnification.scale(i),
            band(bounds, self.strip_fraction()),
            self.scroll_offset,
            &self.settings.settings().bars,
        )
    }

    /// Share of the window height the strip uses. Without passthrough the
    /// window itself is already sized to the configured fraction.
    fn strip_fraction(&self) -> f32 {
        if SUPPORTS_PASSTHROUGH {
            self.settings.settings().window.height_fraction
        } else {
            1.0
        }
    }

    /// Applies changed settings to running state: animation speeds, scroll
    /// bounds and the docked window size.
    fn apply_settings(&mut self) -> Task<Message> {
        let s = self.settings.settings();
        let (speed, stiffness) = (s.motion.speed, s.motion.stiffness);
        self.morph.set_speed(speed);
        self.peek.set_speed(speed);
        self.settings_morph.set_speed(speed);
        let target = if self.expanded { 1.0 } else { 0.0 };
        self.expand_animation = AnimationState::new(self.expand_animation.value(), stiffness);
        self.expand_animation.set_target(target);
        self.scroll_offset = self.scroll_offset.min(self.strip_layout().max_scroll);
        self.animating = true;
        self.dock_window()
    }

    fn note_target_rect(&self) -> Rectangle {
        let expand = self.expand_animation.value();
        let notes = &self.settings.settings().notes;
        let max_height = (self.window_size.height - 2.0 * NOTE_MARGIN).max(notes.size / 2.0);
        let width = lerp(notes.size, notes.expanded_size, expand);
        let height = width.min(max_height);
        let saved = self.note_drag_pos.or_else(|| {
            let id = self.active_note?;
            let note = self.store.notes().iter().find(|n| n.id == id)?;
            note.position.map(|[x, y]| Point::new(x, y))
        });
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

        let right = self.window_size.width - STRIP_WIDTH - NOTE_GAP;

        let min_center = NOTE_MARGIN + height / 2.0;
        let max_center = self.window_size.height - NOTE_MARGIN - height / 2.0;
        let center_y = if max_center > min_center {
            self.anchor_y.clamp(min_center, max_center)
        } else {
            self.window_size.height / 2.0
        };

        Rectangle::new(
            Point::new(right - width, center_y - height / 2.0),
            Size::new(width, height),
        )
    }

    /// Bar centers plus the add and settings buttons', which magnify along with them.
    fn magnification_centers(&self) -> Vec<f32> {
        let strip = self.strip_layout();
        strip
            .bars
            .iter()
            .chain([&strip.add_button, &strip.settings_button])
            .map(|r| r.y + r.height / 2.0)
            .collect()
    }

    fn bar_centers(&self) -> Vec<f32> {
        self.strip_layout()
            .bars
            .iter()
            .map(|bar| bar.y + bar.height / 2.0)
            .collect()
    }
}
