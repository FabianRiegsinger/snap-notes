use crate::animation::{AnimationState, MagnificationState};
use crate::bar_strip::bar_strip;
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
    Tick(Instant),
    SaveTick,
    ToggleVisibility,
    Quit,
}

pub struct App {
    store: NoteStore,
    magnification: MagnificationState,
    cursor_in_strip: bool,
    active_note: Option<Uuid>,
    editor_content: Option<text_editor::Content>,
    panel_slide: AnimationState,
    expanded: bool,
    expand_animation: AnimationState,
    color_picker_open: bool,
    drag: Option<DragState>,
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
        let store = NoteStore::load(data_path());
        (
            Self {
                store,
                magnification: MagnificationState::new(),
                cursor_in_strip: false,
                active_note: None,
                editor_content: None,
                panel_slide: AnimationState::new(0.0),
                expanded: false,
                expand_animation: AnimationState::new(0.0),
                color_picker_open: false,
                drag: None,
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
                let centers: Vec<f32> = self.bar_centers();
                self.magnification.update(y, &centers, 1.0 / 60.0);
                self.animating = true;
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
                let mag_active = self.magnification.update(None, &centers, dt);
                let panel_active = self.panel_slide.tick(dt);
                let expand_active = self.expand_animation.tick(dt);
                self.animating = mag_active || panel_active || expand_active;

                if self.panel_slide.value() < 0.01 && self.active_note.is_some() && self.panel_slide.value() < 0.01 {
                    if self.panel_slide.value() < 0.001 {
                        self.active_note = None;
                        self.editor_content = None;
                    }
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
            Message::ColorChosen(_) => {}
            Message::ExpandNote => {}
            Message::ShrinkNote => {}
            Message::DragStart(_) => {}
            Message::DragMove(_) => {}
            Message::DragEnd => {}
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
        let strip = bar_strip(
            self.store.notes(),
            &self.magnification,
            self.cursor_in_strip,
        );

        if let (Some(id), Some(content)) = (self.active_note, &self.editor_content) {
            if let Some(note) = self.store.notes().iter().find(|n| n.id == id) {
                let panel = note_panel(
                    note,
                    content,
                    self.panel_slide.value(),
                    self.confirm_delete.is_some(),
                );
                return row![panel, strip].into();
            }
        }

        row![strip].into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let save = iced::time::every(Duration::from_secs(1)).map(|_| Message::SaveTick);

        if self.animating {
            let tick =
                iced::time::every(Duration::from_millis(16)).map(Message::Tick);
            Subscription::batch([tick, save])
        } else {
            save
        }
    }

    fn bar_centers(&self) -> Vec<f32> {
        let bar_height = 30.0;
        let gap = 4.0;
        self.store
            .notes()
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let scale = self.magnification.scale(i);
                i as f32 * (bar_height + gap) + (bar_height * scale) / 2.0
            })
            .collect()
    }
}
