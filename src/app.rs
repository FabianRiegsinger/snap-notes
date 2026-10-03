use crate::animation::{AnimationState, MagnificationState};
use crate::bar_strip::bar_strip;
use crate::color_picker::color_picker;
use crate::note::NoteColor;
use crate::note_panel::note_panel;
use crate::store::NoteStore;

use iced::widget::{row, text_editor};
use iced::{Element, Subscription, Task};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum Message {
    StripHover(Option<f32>),
    BarClicked(usize),
    AddNote,
    NoteEdited(text_editor::Action),
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
}

pub struct App {
    store: NoteStore,
    magnification: MagnificationState,
    cursor_in_strip: bool,
    cursor_y: Option<f32>,
    active_note: Option<Uuid>,
    editor_content: Option<text_editor::Content>,
    panel_slide: AnimationState,
    expanded: bool,
    expand_animation: AnimationState,
    color_picker_open: bool,
    drag: Option<DragState>,
    scroll_offset: f32,
    confirm_delete: Option<Uuid>,
    animating: bool,
    visible: bool,
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
                cursor_in_strip: false,
                cursor_y: None,
                active_note: None,
                editor_content: None,
                panel_slide: AnimationState::new(0.0),
                expanded: false,
                expand_animation: AnimationState::new(0.0),
                color_picker_open: false,
                drag: None,
                scroll_offset: 0.0,
                confirm_delete: None,
                animating: false,
                visible: true,
            },
            Task::none(),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StripHover(y) => {
                self.cursor_in_strip = y.is_some();
                self.cursor_y = y;
                self.animating = true;
                if let Some(cursor_y) = y {
                    if self.drag.is_some() {
                        return self.update(Message::DragMove(cursor_y));
                    }
                }
            }
            Message::BarClicked(index) => {
                if let Some(note) = self.store.notes().get(index) {
                    let id = note.id;
                    let content = text_editor::Content::with_text(&note.content);
                    self.active_note = Some(id);
                    self.editor_content = Some(content);
                    self.panel_slide.set_target(1.0);
                    self.color_picker_open = false;
                    self.confirm_delete = None;
                    self.animating = true;
                }
            }
            Message::AddNote => {
                let id = self.store.add_note();
                self.active_note = Some(id);
                self.editor_content = Some(text_editor::Content::new());
                self.panel_slide.set_target(1.0);
                self.animating = true;
                self.store.mark_dirty();
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
                self.panel_slide.set_target(0.0);
                self.animating = true;
                self.color_picker_open = false;
                self.confirm_delete = None;
            }
            Message::DeleteRequested => {
                if let Some(id) = self.active_note {
                    self.confirm_delete = Some(id);
                }
            }
            Message::ConfirmDelete(confirmed) => {
                if confirmed {
                    if let Some(id) = self.confirm_delete.take() {
                        self.store.delete_note(id);
                        let _ = self.store.save();
                        self.store.did_save();
                        self.active_note = None;
                        self.editor_content = None;
                        self.panel_slide.set_target(0.0);
                        self.animating = true;
                    }
                } else {
                    self.confirm_delete = None;
                }
            }
            Message::Tick(_now) => {
                let dt = 1.0 / 60.0;
                let centers = self.bar_centers();
                let mag_active = self.magnification.update(self.cursor_y, &centers, dt);
                let panel_active = self.panel_slide.tick(dt);
                let expand_active = self.expand_animation.tick(dt);
                self.animating = mag_active || panel_active || expand_active;

                if self.active_note.is_some() && self.panel_slide.value() < 0.001 {
                    self.active_note = None;
                    self.editor_content = None;
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
                        return self.update(Message::BarClicked(drag.bar_index));
                    }
                }
            }
            Message::StripScroll(delta) => {
                self.scroll_offset = (self.scroll_offset - delta).max(0.0);
            }
            Message::ToggleVisibility => {
                self.visible = !self.visible;
            }
            Message::Quit => {
                let _ = self.store.save();
                std::process::exit(0);
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        use iced::widget::column;

        let strip = bar_strip(
            self.store.notes(),
            &self.magnification,
            self.cursor_in_strip,
            &self.drag,
            self.scroll_offset,
        );

        if let (Some(id), Some(content)) = (self.active_note, &self.editor_content) {
            if let Some(note) = self.store.notes().iter().find(|n| n.id == id) {
                let expand_progress = self.expand_animation.value();
                let panel_width = 300.0 + (400.0 * expand_progress);
                let panel = note_panel(
                    note,
                    content,
                    self.panel_slide.value(),
                    self.confirm_delete.is_some(),
                    self.expanded,
                    panel_width,
                );

                let mut panel_col: iced::widget::Column<'_, Message> = column![panel];

                if self.color_picker_open {
                    panel_col = panel_col.push(color_picker(&note.color));
                }

                return row![panel_col, strip].into();
            }
        }

        row![strip].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let save = iced::time::every(Duration::from_secs(1)).map(|_| Message::SaveTick);

        if self.animating {
            let tick = iced::time::every(Duration::from_millis(16)).map(Message::Tick);
            Subscription::batch([tick, save])
        } else {
            save
        }
    }

    fn bar_centers(&self) -> Vec<f32> {
        let bar_height = 30.0;
        let gap = 4.0;
        let mut y = 0.0f32;
        self.store
            .notes()
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let scale = self.magnification.scale(i);
                let h = bar_height * scale;
                let center = y + h / 2.0;
                y += h + gap;
                center
            })
            .collect()
    }
}
