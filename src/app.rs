use crate::animation::{lerp, morph_frame, AnimationState, MagnificationState, Morph};
use crate::bar_strip::{bar_strip, compute_layout, StripLayout, STRIP_WIDTH};
use crate::note::NoteColor;
use crate::note_panel::{post_it, PostIt};
use crate::store::NoteStore;

use iced::widget::{container, mouse_area, opaque, pin, stack, text_editor, Space};
use iced::{keyboard, window, Element, Fill, Point, Rectangle, Size, Subscription, Task};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

const NOTE_SIZE: f32 = 320.0;
const NOTE_EXPANDED_SIZE: f32 = 560.0;
const NOTE_GAP: f32 = 18.0;
const NOTE_MARGIN: f32 = 24.0;
/// Window width while a note is open: room for the expanded note and its shadow.
const OPEN_WIDTH: f32 = STRIP_WIDTH + NOTE_GAP + NOTE_EXPANDED_SIZE + NOTE_MARGIN;
/// Share of the monitor height the docked window occupies (centered).
const HEIGHT_FRACTION: f32 = 0.9;

#[derive(Debug, Clone)]
pub enum Message {
    StripHover(Option<f32>),
    BarClicked(usize),
    AddNote,
    NoteEdited(text_editor::Action),
    TitleEdited(String),
    NoteHovered(bool),
    ClosePanel,
    DeleteRequested,
    ConfirmDelete(bool),
    ToggleColorPicker,
    ColorChosen(NoteColor),
    ExpandNote,
    ShrinkNote,
    DragStart(usize),
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
}

pub struct App {
    store: NoteStore,
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
}

#[derive(Debug, Clone)]
pub struct DragState {
    pub bar_index: usize,
    pub origin_y: f32,
    pub current_y: f32,
}

fn data_path() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("."))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("notes.json")
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let mut store = NoteStore::load(data_path());
        if store.notes().is_empty() {
            store.seed_templates();
            let _ = store.save();
        }
        (
            Self {
                store,
                magnification: MagnificationState::new(),
                cursor_y: None,
                active_note: None,
                editor_content: None,
                morph: Morph::new(),
                anchor_y: 0.0,
                pending_delete: None,
                expanded: false,
                expand_animation: AnimationState::new(0.0),
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
                window_size: Size::new(STRIP_WIDTH, 600.0),
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
                if let Some(cursor_y) = y {
                    if self.drag.is_some() {
                        return self.update(Message::DragMove(cursor_y));
                    }
                }
            }
            Message::BarClicked(index) => return self.open_note(index),
            Message::AddNote => {
                self.store.add_note();
                self.store.mark_dirty();
                let last = self.store.notes().len() - 1;
                self.scroll_offset = self.strip_layout().max_scroll;
                return self.open_note(last);
            }
            Message::NoteHovered(hovered) => {
                self.note_hovered = hovered;
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

                let centers = self.bar_centers();
                let mag_active = self.magnification.update(self.cursor_y, &centers, dt);
                let morph_active = self.morph.tick(dt);
                let expand_active = self.expand_animation.tick(dt);
                self.animating = mag_active || morph_active || expand_active;
                if !self.animating {
                    self.last_tick = None;
                }

                if self.active_note.is_some() && self.morph.is_closed() {
                    return self.finish_close();
                }
            }
            Message::SaveTick => {
                if self.store.should_save() {
                    let _ = self.store.save();
                    self.store.did_save();
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
            Message::DragStart(index) => {
                let centers = self.bar_centers();
                let origin_y = centers.get(index).copied().unwrap_or(0.0);
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
                std::process::exit(0);
            }
            Message::WindowReady(id, monitor) => {
                self.window_id = Some(id);
                self.monitor = monitor;
                return self.set_window_width(STRIP_WIDTH);
            }
            Message::WindowResized(size) => {
                self.window_size = size;
            }
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                use keyboard::key::Named;
                match key.as_ref() {
                    keyboard::Key::Named(Named::Escape) => return self.update(Message::ClosePanel),
                    keyboard::Key::Character("n") if modifiers.command() => {
                        return self.update(Message::AddNote)
                    }
                    _ => {}
                }
            }
            Message::Key(_) => {}
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let strip = container(bar_strip(
            self.store.notes(),
            &self.magnification,
            &self.drag,
            self.scroll_offset,
        ))
        .width(Fill)
        .height(Fill)
        .align_x(iced::Alignment::End);

        let mut layers: Vec<Element<'_, Message>> = Vec::new();

        if let (Some(id), Some(content)) = (self.active_note, &self.editor_content) {
            if let Some(index) = self.store.notes().iter().position(|n| n.id == id) {
                let note = &self.store.notes()[index];
                let source = self.strip_layout().bars[index];
                let frame = morph_frame(source, self.note_target_rect(), self.morph.progress());
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
                        content,
                        size: rect.size(),
                        morph_progress: self.morph.progress(),
                        content_alpha: frame.content_alpha,
                        confirm_delete: self.confirm_delete.is_some(),
                        expanded: self.expanded,
                        color_picker_open: self.color_picker_open,
                        hovered: self.note_hovered,
                    });
                    let note_view = mouse_area(note_view)
                        .on_enter(Message::NoteHovered(true))
                        .on_exit(Message::NoteHovered(false));
                    layers.push(pin(opaque(note_view)).x(rect.x).y(rect.y).into());
                }
            }
        }

        layers.push(strip.into());
        stack(layers).width(Fill).height(Fill).into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let save = iced::time::every(Duration::from_secs(1)).map(|_| Message::SaveTick);
        let resized = window::resize_events().map(|(_id, size)| Message::WindowResized(size));
        let keys = keyboard::listen().map(Message::Key);

        if self.animating {
            // Frame-synced ticks keep the morph in step with the display.
            let tick = window::frames().map(Message::Tick);
            Subscription::batch([tick, save, resized, keys])
        } else {
            Subscription::batch([save, resized, keys])
        }
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

        if self.window_size.width < OPEN_WIDTH {
            self.set_window_width(OPEN_WIDTH)
        } else {
            Task::none()
        }
    }

    fn finish_close(&mut self) -> Task<Message> {
        self.active_note = None;
        self.note_hovered = false;
        self.editor_content = None;
        self.color_picker_open = false;
        self.confirm_delete = None;
        self.expanded = false;
        self.expand_animation = AnimationState::new(0.0);
        if let Some(id) = self.pending_delete.take() {
            self.store.delete_note(id);
            let _ = self.store.save();
            self.store.did_save();
            self.scroll_offset = self.scroll_offset.min(self.strip_layout().max_scroll);
        }
        self.set_window_width(STRIP_WIDTH)
    }

    /// Docks the window to the right screen edge, vertically centered, at the
    /// given width. The strip is centered inside, so it sits at the middle of
    /// the right screen border.
    fn set_window_width(&mut self, width: f32) -> Task<Message> {
        let (Some(id), Some(monitor)) = (self.window_id, self.monitor) else {
            return Task::none();
        };
        let height = (monitor.height * HEIGHT_FRACTION).round();
        let y = ((monitor.height - height) / 2.0).round();
        self.window_size = Size::new(width, height);
        Task::batch([
            window::move_to(id, Point::new(monitor.width - width, y)),
            window::resize(id, self.window_size),
        ])
    }

    fn strip_layout(&self) -> StripLayout {
        let bounds = Rectangle::new(
            Point::new(self.window_size.width - STRIP_WIDTH, 0.0),
            Size::new(STRIP_WIDTH, self.window_size.height),
        );
        compute_layout(
            self.store.notes().len(),
            |i| self.magnification.scale(i),
            bounds,
            self.scroll_offset,
        )
    }

    fn note_target_rect(&self) -> Rectangle {
        let expand = self.expand_animation.value();
        let max_height = (self.window_size.height - 2.0 * NOTE_MARGIN).max(NOTE_SIZE / 2.0);
        let width = lerp(NOTE_SIZE, NOTE_EXPANDED_SIZE, expand);
        let height = lerp(NOTE_SIZE, NOTE_EXPANDED_SIZE, expand).min(max_height);
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

    fn bar_centers(&self) -> Vec<f32> {
        self.strip_layout()
            .bars
            .iter()
            .map(|bar| bar.y + bar.height / 2.0)
            .collect()
    }
}
