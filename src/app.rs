use crate::animation::{ease_out_cubic, morph_frame, MagnificationState, Morph, MorphFrame};
use crate::bar_strip::{
    band, compute_layout, peek_target, BarStrip, StripLayout, SETTINGS_SLOT, STRIP_WIDTH,
};
use crate::history::History;
use crate::images;
use crate::note::NoteColor;
use crate::note_panel::{focus_body, post_it, PostIt};
use crate::peek::note_peek_width;
use crate::platform::{self, SUPPORTS_PASSTHROUGH};
use crate::resize::{resize_frame, resized, Edges, MIN_SIZE};
use crate::rich::{self, BlockKind, Doc};
use crate::rich_view;
use crate::search::{self, Hit};
use crate::search_panel::{focus_field, search_panel, SearchView};
use crate::settings::{SettingKey, SettingToggle, Settings, SettingsGroup, SettingsStore};
use crate::settings_panel::{settings_panel, SettingsView, PANEL_MAX_HEIGHT, PANEL_WIDTH};
use crate::store::NoteStore;
use crate::theme;
use crate::tray;

use iced::widget::{container, mouse_area, opaque, pin, stack, text_editor, Space};
use iced::{
    event, keyboard, mouse, window, Element, Fill, Point, Rectangle, Size, Subscription, Task,
    Vector,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(crate) const NOTE_GAP: f32 = 18.0;
pub(crate) const NOTE_MARGIN: f32 = 24.0;
/// While mouse passthrough is on, how often the OS cursor is polled to notice
/// it entering the strip or the note again.
const CURSOR_POLL: Duration = Duration::from_millis(50);
const PEEK_POLL: Duration = Duration::from_millis(100);
/// How long focus changes caused by switching the Dock icon are ignored.
const FOCUS_GRACE: Duration = Duration::from_millis(500);
/// A deleted note's bar shrinks away in this long (at speed 1).
const COLLAPSE_SECS: f32 = 0.18;
/// How long the copy button shows its check.
const COPIED_FOR: Duration = Duration::from_millis(1500);
/// Toolbar and body fade in this long after switching edit/render mode.
const MODE_FADE_SECS: f32 = 0.12;

#[derive(Debug, Clone)]
pub enum Message {
    ThemeChanged(theme::Mode),
    StripHover(Option<f32>),
    BarClicked(usize),
    AddNote,
    ToggleSettings,
    CloseSettings,
    /// Cmd/Ctrl+F, the strip's search slot or the tray item.
    ToggleSearch,
    SearchChanged(String),
    SearchResultPicked(Uuid),
    /// The panel's close button, a click beside it, or an Esc the focused
    /// search field took (it only drops its focus on Esc).
    CloseSearch,
    SettingChanged(SettingKey, f32),
    // Only the macOS/Windows settings panel shows these toggles.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    SettingToggled(SettingToggle),
    ResetGroup(SettingsGroup),
    /// Palette slot whose preset grid is open (`None` closes it).
    PaletteSlotSelected(Option<usize>),
    PaletteColorChosen(NoteColor),
    NoteEdited(text_editor::Action),
    /// A click on the rendered body: the block's source line, or `None`
    /// below the last block.
    BodyClicked(Option<usize>),
    /// A press on rendered text: the source offset of the character under it.
    BodyPressed(usize),
    /// The trash button on the open peek of the note at this index.
    PeekDeleteRequested(usize),
    /// The peek's "Delete" answer: deletes its note.
    PeekDeleteConfirmed,
    /// The peek's "Cancel" answer.
    PeekDeleteCancelled,
    /// Leave edit mode and show the note rendered again.
    EditorBlurred,
    /// Flip the task checkbox on this source line.
    ToggleTask(usize),
    LinkClicked(String),
    TitleEdited(String),
    NoteHovered(bool),
    NoteDragStart,
    NoteResetPosition,
    /// A press on the open note's border starts resizing it.
    ResizeStart(Edges),
    ClosePanel,
    DeleteRequested,
    CopyNote,
    ConfirmDelete(bool),
    ToggleColorPicker,
    /// Open or close the toolbar's text color grid.
    ToggleTextColorPicker,
    /// Wrap the editor selection in this format's markup.
    FormatApplied(rich::Format),
    /// A file was dropped on the window; images are added to the open note.
    ImageDropped(PathBuf),
    /// Cmd/Ctrl+V in the editor: pastes the clipboard's text or image.
    PasteRequested,
    /// Text from the system clipboard, read for the note with this id;
    /// ignored unless that note is still open and being edited.
    ClipboardText(Uuid, String),
    /// Cmd/Ctrl+Z: steps the open note's body back.
    Undo,
    /// Cmd+Shift+Z (or Ctrl+Y off macOS): steps forward again.
    Redo,
    /// The toolbar's image button: opens the file dialog.
    PickImage,
    /// The file dialog opened for the note with this id returned; ignored
    /// unless that note is still the open one.
    ImagePicked(Uuid, Option<PathBuf>),
    ColorChosen(NoteColor),
    DragStart(usize, f32),
    DragMove(f32),
    DragEnd,
    StripScroll(f32),
    Tick(Instant),
    SaveTick,
    ToggleVisibility,
    /// The tray icon was created (or failed to be).
    TrayReady(bool),
    TrayMenu(String),
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

/// What a paste found on the clipboard.
#[derive(Debug, PartialEq)]
enum ClipboardContent {
    Text(String),
    Image {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
    Empty,
}

/// Text wins over an image: copied web content often carries both, while a
/// screenshot is image-only.
fn classify(text: Option<String>, image: Option<(Vec<u8>, u32, u32)>) -> ClipboardContent {
    match (text.filter(|t| !t.is_empty()), image) {
        (Some(text), _) => ClipboardContent::Text(text),
        (None, Some((rgba, width, height))) => ClipboardContent::Image {
            rgba,
            width,
            height,
        },
        (None, None) => ClipboardContent::Empty,
    }
}

fn read_clipboard() -> ClipboardContent {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return ClipboardContent::Empty;
    };
    let text = clipboard.get_text().ok();
    let image = if text.as_ref().is_some_and(|t| !t.is_empty()) {
        None
    } else {
        clipboard.get_image().ok().and_then(|img| {
            Some((
                img.bytes.into_owned(),
                u32::try_from(img.width).ok()?,
                u32::try_from(img.height).ok()?,
            ))
        })
    };
    classify(text, image)
}

pub struct App {
    pub(crate) theme: theme::Theme,
    store: NoteStore,
    settings: SettingsStore,
    magnification: MagnificationState,
    cursor_y: Option<f32>,
    active_note: Option<Uuid>,
    editor_content: Option<text_editor::Content>,
    /// The open note shows its editor rather than its rendered `doc`.
    editing: bool,
    /// The open note's content, parsed for the rendered view.
    doc: Doc,
    /// Folder of the notes file; note images live in `images/` inside it.
    data_dir: PathBuf,
    /// Image references in `doc` that are missing or can't be decoded.
    broken_images: HashSet<String>,
    /// Undo/redo for the open note's body: its text and cursor.
    history: History<(String, text_editor::Cursor)>,
    /// The last press on rendered text, so quick follow-up clicks (which land
    /// on the editor it opened) select the word, then the line, around it.
    rendered_click: Option<RenderedClick>,
    /// The open peek asks whether to delete this note.
    peek_confirm_delete: Option<Uuid>,
    morph: Morph,
    anchor_y: f32,
    pending_delete: Option<Uuid>,
    /// A deleted note whose bar is shrinking away; it is removed from the
    /// store once the bar is gone.
    collapsing: Option<(Uuid, Morph)>,
    /// Fades the toolbar and body in after switching edit/render mode.
    mode_fade: Morph,
    color_picker_open: bool,
    text_color_picker_open: bool,
    note_hovered: bool,
    drag: Option<DragState>,
    scroll_offset: f32,
    confirm_delete: Option<Uuid>,
    /// The note last copied and when; its copy button shows a check for
    /// `COPIED_FOR` after.
    copied_at: Option<(Uuid, Instant)>,
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
    /// The open note's border is being dragged.
    note_resize: Option<NoteResize>,
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
    /// The search panel is showing (kept while it folds back into its slot).
    search_open: bool,
    search_morph: Morph,
    /// Search slot center when the panel opened.
    search_anchor_y: f32,
    search_query: String,
    /// Keyboard focus to give once the panel or note showing it has
    /// faded its content in (the widget doesn't exist before).
    pending_focus: Option<PendingFocus>,
    /// The menu bar / tray icon exists.
    tray_ok: bool,
    /// The tray icon could not be created, so the Dock icon is shown this
    /// session regardless of the saved setting.
    tray_failed: bool,
    /// Switching the Dock / taskbar icon briefly takes focus away from the
    /// window; until then losing focus doesn't close the note or settings.
    keep_open_until: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PendingFocus {
    SearchField,
    Body,
}

/// Clicks counted from a press on the rendered note.
struct RenderedClick {
    at: Instant,
    /// Source offset of the first press.
    offset: usize,
    count: u8,
}

/// How long after a click the next one still counts as the same series
/// (iced's own double-click window).
const MULTI_CLICK: Duration = Duration::from_millis(300);

/// A resize of the open note in progress.
struct NoteResize {
    edges: Edges,
    /// Note rect and cursor position when the resize started.
    start: Rectangle,
    grab: Point,
    rect: Rectangle,
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

/// Seeds a fresh store with template notes and deletes images no note uses.
/// If the notes file exists but could not be loaded, neither happens: the
/// file is not overwritten and its images are kept.
fn prepare_store(store: &mut NoteStore, palette: &[NoteColor]) {
    if store.load_failed() {
        eprintln!("could not load the notes file; leaving it and its images untouched");
        return;
    }
    if store.notes().is_empty() {
        store.seed_templates(palette);
        let _ = store.save();
    }
    if let Err(e) = images::sweep(store.dir(), store.notes()) {
        eprintln!("could not clean up note images: {e}");
    }
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let settings = SettingsStore::load(data_dir().join("settings.json"));
        // Hide the Dock icon as early as possible so it barely flashes.
        if !settings.settings().app.show_dock_icon {
            platform::set_dock_policy(false);
        }
        let mut store = NoteStore::load(data_dir().join("notes.json"));
        prepare_store(&mut store, &settings.settings().palette);
        (
            Self::new(store, settings),
            Task::batch([
                window::oldest().then(|id| match id {
                    Some(id) => window::monitor_size(id).map(move |m| Message::WindowReady(id, m)),
                    None => Task::none(),
                }),
                iced::system::theme().map(|m| Message::ThemeChanged(m.into())),
            ]),
        )
    }

    /// App state around already loaded stores, before any window exists.
    fn new(store: NoteStore, settings: SettingsStore) -> Self {
        let s = settings.settings();
        let morph = Morph::new(s.motion.speed);
        let peek = Morph::peek(s.motion.speed);
        let settings_morph = Morph::new(s.motion.speed);
        let search_morph = Morph::new(s.motion.speed);
        let mode_fade = Self::settled_fade(s.motion.speed);
        let window_size = Size::new(Self::docked_width(s, false), 600.0);
        let data_dir = store.dir().to_path_buf();
        Self {
            theme: theme::Theme::default(),
            store,
            settings,
            magnification: MagnificationState::new(),
            cursor_y: None,
            active_note: None,
            editor_content: None,
            editing: false,
            doc: Doc { blocks: Vec::new() },
            data_dir,
            broken_images: HashSet::new(),
            history: History::default(),
            rendered_click: None,
            peek_confirm_delete: None,
            morph,
            anchor_y: 0.0,
            pending_delete: None,
            collapsing: None,
            mode_fade,
            color_picker_open: false,
            text_color_picker_open: false,
            note_hovered: false,
            drag: None,
            scroll_offset: 0.0,
            confirm_delete: None,
            copied_at: None,
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
            note_resize: None,
            note_drag_pos: None,
            hover_bar: None,
            peek_note: None,
            peek,
            settings_open: false,
            settings_morph,
            settings_anchor_y: 0.0,
            palette_slot: None,
            search_open: false,
            search_morph,
            search_anchor_y: 0.0,
            search_query: String::new(),
            pending_focus: None,
            tray_ok: false,
            tray_failed: false,
            keep_open_until: None,
        }
    }

    pub fn theme_mode(&self) -> theme::Mode {
        self.theme.mode
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ThemeChanged(mode) => {
                self.theme = theme::Theme::new(mode);
            }
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
            Message::BarClicked(index) => return self.open_note(index),
            Message::ToggleSettings => {
                if self.settings_open && self.settings_morph.is_opening() {
                    return self.update(Message::CloseSettings);
                }
                if self.settings_open {
                    // Still folding away: unfold again from where it is.
                    self.settings_morph.open();
                    self.animating = true;
                    return Task::none();
                }
                let close_note = self.update(Message::ClosePanel);
                self.close_search();
                self.hide_peek();
                let gear = self.strip_layout().settings_anchor();
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
            Message::ToggleSearch => return self.toggle_search(),
            Message::SearchChanged(query) => self.search_query = query,
            Message::SearchResultPicked(id) => return self.pick_result(id),
            Message::CloseSearch => self.close_search(),
            Message::SettingChanged(key, value) => {
                self.settings.settings_mut().set(key, value);
                return self.apply_settings();
            }
            Message::SettingToggled(toggle) => {
                self.settings.settings_mut().toggle(toggle);
                if toggle == SettingToggle::DockIcon {
                    self.keep_open_until = Some(Instant::now() + FOCUS_GRACE);
                }
                return self.apply_app_visibility();
            }
            Message::ResetGroup(group) => {
                if group == SettingsGroup::Palette {
                    let changes = self.settings.settings_mut().reset_palette();
                    if self.store.recolor_many(&changes) {
                        self.store.mark_dirty();
                    }
                    self.reparse();
                } else {
                    self.settings.settings_mut().reset(group);
                }
                if group == SettingsGroup::App {
                    self.keep_open_until = Some(Instant::now() + FOCUS_GRACE);
                    return self.apply_app_visibility();
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
                        self.reparse();
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
            Message::PeekDeleteRequested(index) => {
                let id = self.store.notes().get(index).map(|n| n.id);
                if id.is_some() && id == self.peek_note {
                    self.peek_confirm_delete = id;
                }
            }
            Message::PeekDeleteCancelled => self.peek_confirm_delete = None,
            Message::PeekDeleteConfirmed => {
                if let Some(id) = self.peek_confirm_delete.take() {
                    self.hide_peek();
                    if self.active_note == Some(id) {
                        // The note is open too: fold it away like its own trash button does.
                        self.confirm_delete = Some(id);
                        return self.update(Message::ConfirmDelete(true));
                    }
                    self.start_collapse(id);
                }
            }
            Message::BodyPressed(offset) => {
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
            Message::ClosePanel => {
                if self.active_note.is_some() {
                    self.morph.close();
                    self.animating = true;
                    self.color_picker_open = false;
                    self.text_color_picker_open = false;
                    self.confirm_delete = None;
                }
            }
            Message::DeleteRequested => {
                if let Some(id) = self.active_note {
                    self.confirm_delete = Some(id);
                }
            }
            Message::CopyNote => {
                let note = self
                    .active_note
                    .and_then(|id| self.store.notes().iter().find(|n| n.id == id));
                if let Some(note) = note {
                    let text = crate::note::copy_text(note);
                    self.copied_at = Some((note.id, Instant::now()));
                    self.animating = true;
                    return iced::clipboard::write(text);
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
                let morph_active = self.morph.tick(dt)
                    | self.peek.tick(dt)
                    | self.settings_morph.tick(dt)
                    | self.search_morph.tick(dt)
                    | self.mode_fade.tick(dt);
                let collapse_active = self.collapsing.as_mut().is_some_and(|(_, m)| m.tick(dt));
                if self.collapsing.is_some() && !collapse_active {
                    self.finish_collapse();
                }
                if self.peek.is_closed() {
                    self.peek_note = None;
                }
                let settings_closed = self.settings_open && self.settings_morph.is_closed();
                if settings_closed {
                    self.settings_open = false;
                    self.palette_slot = None;
                }
                let search_closed = self.search_open && self.search_morph.is_closed();
                if search_closed {
                    self.search_open = false;
                }
                if self
                    .copied_at
                    .is_some_and(|(_, t)| now.saturating_duration_since(t) >= COPIED_FOR)
                {
                    self.copied_at = None;
                }
                self.animating =
                    mag_active || morph_active || collapse_active || self.copied_at.is_some();
                if !self.animating {
                    self.last_tick = None;
                }

                let focus = self.take_pending_focus();
                if self.active_note.is_some() && self.morph.is_closed() {
                    return Task::batch([focus, self.finish_close()]);
                }
                if settings_closed || search_closed {
                    // Without passthrough the window shrinks back to the strip.
                    return Task::batch([focus, self.dock_window()]);
                }
                return focus;
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
            }
            Message::ToggleTextColorPicker => {
                self.text_color_picker_open = !self.text_color_picker_open;
                self.color_picker_open = false;
            }
            Message::FormatApplied(format) => return self.apply_format(format),
            Message::ImageDropped(path) => return self.import_image(&path),
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
                if let Some(id) = self.active_note {
                    if let Some(note) = self.store.note_mut(id) {
                        note.color = color;
                        note.updated_at = chrono::Utc::now();
                    }
                    self.store.mark_dirty();
                    self.color_picker_open = false;
                }
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
                let mut tasks = Vec::new();
                if !self.visible {
                    self.close_search();
                    tasks.push(self.update(Message::CloseSettings));
                    tasks.push(self.update(Message::ClosePanel));
                    self.hide_peek();
                }
                if let Some(id) = self.window_id {
                    let shown = self.visible;
                    tasks.push(window::run(id, move |w| tray::set_notes_shown(w, shown)).discard());
                }
                tasks.push(self.update_passthrough(self.last_cursor));
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
                let settings_showing = self.settings_open && self.settings_morph.is_opening();
                let search_showing = self.search_open && self.search_morph.is_opening();
                let showing = match message {
                    Message::ToggleSettings => settings_showing,
                    Message::ToggleSearch => search_showing,
                    _ => false,
                };
                if showing {
                    // The menu items open their panel; they never close it.
                    return Task::none();
                }
                let needs_notes = matches!(
                    message,
                    Message::AddNote | Message::ToggleSettings | Message::ToggleSearch
                );
                if needs_notes && !self.visible {
                    tasks.push(self.update(Message::ToggleVisibility));
                }
                tasks.push(self.update(message));
                return Task::batch(tasks);
            }
            Message::Quit => {
                self.finish_collapse();
                let _ = self.store.save();
                let _ = self.settings.save();
                std::process::exit(0);
            }
            Message::WindowReady(id, monitor) => {
                self.window_id = Some(id);
                self.monitor = monitor;
                let shadow = window::run(id, platform::disable_native_shadow).discard();
                let dock = self.dock_window();
                let show_icon = self.settings.settings().app.show_menu_bar_icon;
                let tray = window::run(id, move |w| tray::create(w, true, show_icon))
                    .map(Message::TrayReady);
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
            // Hidden notes don't react to the keyboard.
            Message::Key(_) if !self.visible => {}
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                use keyboard::key::Named;
                match key.as_ref() {
                    keyboard::Key::Named(Named::Escape) if self.search_morph.is_opening() => {
                        self.close_search()
                    }
                    keyboard::Key::Named(Named::Escape) if self.settings_open => {
                        return self.update(Message::CloseSettings)
                    }
                    keyboard::Key::Named(Named::Escape) if self.editing => {
                        return self.update(Message::EditorBlurred)
                    }
                    keyboard::Key::Named(Named::Escape) => return self.update(Message::ClosePanel),
                    keyboard::Key::Character(",") if modifiers.command() => {
                        return self.update(Message::ToggleSettings)
                    }
                    keyboard::Key::Character("n") if modifiers.command() => {
                        return self.update(Message::AddNote)
                    }
                    keyboard::Key::Character("f") if modifiers.command() => {
                        return self.toggle_search()
                    }
                    // In edit mode the body editor's own key binding handles
                    // undo/redo; the title field ignores them, so skip it here.
                    keyboard::Key::Character(c) if !self.editing && self.active_note.is_some() => {
                        if let Some(step) = crate::note_panel::history_key(c, modifiers) {
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
                self.last_cursor = Some(position);
                if let Some(offset) = self.note_drag {
                    self.note_drag_pos = Some(position - offset);
                }
                let room = self.note_room();
                if let Some(resize) = &mut self.note_resize {
                    resize.rect = resized(resize.start, resize.edges, position - resize.grab, room);
                }
                // Leaving the open peek (not toward the strip) closes it.
                let in_strip = position.x >= self.window_size.width - STRIP_WIDTH;
                if self.cursor_y.is_some() && !in_strip && !self.cursor_on_peek() {
                    self.cursor_y = None;
                    self.animating = true;
                    self.update_hover();
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
                if !down {
                    self.finish_note_resize();
                }
            }
            Message::WindowUnfocused
                if self.keep_open_until.is_some_and(|t| Instant::now() < t) => {}
            Message::WindowUnfocused => {
                // Clicking another app (through the passthrough area) closes
                // the note and the panels.
                self.close_search();
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
        if !self.visible {
            return Space::new().width(Fill).height(Fill).into();
        }
        let strip = container(BarStrip {
            notes: self.store.notes(),
            magnification: &self.magnification,
            drag: &self.drag,
            scroll_offset: self.scroll_offset,
            peek: self
                .peek_note
                .and_then(|id| self.store.notes().iter().position(|n| n.id == id))
                .map(|index| (index, self.peek.progress())),
            bars: &self.settings.settings().bars,
            height_fraction: self.strip_fraction(),
            paper_tint: self.settings.settings().notes.paper_tint,
            default_note_width: self.settings.settings().notes.size,
            peek_confirm: self.peek_confirm_delete.is_some(),
            theme: self.theme,
            open: self
                .active_note
                .and_then(|id| self.store.notes().iter().position(|n| n.id == id))
                .map(|index| (index, self.morph.progress())),
            collapse: self.collapse(),
            dimmed: self.dimmed_bars(),
        })
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
                        theme: self.theme,
                        palette: &self.settings.settings().palette,
                        paper_tint: self.settings.settings().notes.paper_tint,
                        idle_control_alpha: self.settings.settings().notes.idle_control_alpha,
                        content,
                        editing: self.editing,
                        doc: &self.doc,
                        data_dir: &self.data_dir,
                        broken_images: &self.broken_images,
                        size: rect.size(),
                        morph_progress: self.morph.progress(),
                        content_alpha: frame.content_alpha,
                        confirm_delete: self.confirm_delete.is_some(),
                        copied: self.copied_for(note.id),
                        color_picker_open: self.color_picker_open,
                        text_color_picker_open: self.text_color_picker_open,
                        hovered: self.note_hovered,
                        dragging: self.note_drag.is_some(),
                        mode_fade: self.mode_fade.progress(),
                    });
                    let note_view = mouse_area(note_view)
                        .on_enter(Message::NoteHovered(true))
                        .on_exit(Message::NoteHovered(false));
                    let note_view = resize_frame(note_view);
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
                    theme: self.theme,
                    settings: self.settings.settings(),
                    size: rect.size(),
                    morph_progress: self.settings_morph.progress(),
                    content_alpha: frame.content_alpha,
                    selected_slot: self.palette_slot,
                    tray_ok: self.tray_ok,
                    dock_forced: self.tray_failed,
                });
                layers.push(pin(opaque(panel)).x(rect.x).y(rect.y).into());
            }
        }

        if let Some(frame) = self.search_frame() {
            let rect = frame.rect;
            if rect.width >= 1.0 && rect.height >= 1.0 {
                layers.push(
                    mouse_area(Space::new().width(Fill).height(Fill))
                        .on_press(Message::CloseSearch)
                        .into(),
                );
                let panel = search_panel(SearchView {
                    theme: self.theme,
                    query: &self.search_query,
                    hits: self.search_hits(),
                    size: rect.size(),
                    morph_progress: self.search_morph.progress(),
                    content_alpha: frame.content_alpha,
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
        let pointer = event::listen_with(|event, status, _id| match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if status == event::Status::Captured => Some(Message::CloseSearch),
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::CursorLeftWindow),
            iced::Event::Mouse(mouse::Event::ButtonPressed(_)) => Some(Message::MouseButton(true)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                Some(Message::MouseButton(false))
            }
            iced::Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused),
            iced::Event::Window(window::Event::FileDropped(path)) => {
                Some(Message::ImageDropped(path))
            }
            _ => None,
        });

        let system_theme = iced::system::theme_changes().map(|m| Message::ThemeChanged(m.into()));
        let mut subs = vec![save, resized, keys, pointer, system_theme];
        if self.animating {
            // Frame-synced ticks keep the morph in step with the display.
            subs.push(window::frames().map(Message::Tick));
        }
        if self.passthrough {
            subs.push(iced::time::every(CURSOR_POLL).map(|_| Message::PollCursor));
        }
        if self.tray_ok {
            subs.push(tray::menu_events().map(Message::TrayMenu));
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
        // A deleted note's bar can't be opened while it collapses.
        if self.is_collapsing(index) {
            return Task::none();
        }

        if self.active_note == Some(id) && self.morph.is_opening() {
            return self.update(Message::ClosePanel);
        }

        let switching = self.active_note.is_some_and(|active| active != id);
        self.editor_content = Some(text_editor::Content::with_text(&note.content));
        self.history.clear();
        self.rendered_click = None;
        self.editing = note.content.trim().is_empty();
        self.hide_peek();
        // A note and the panels never show at the same time.
        if self.settings_open {
            self.settings_morph.close();
        }
        self.close_search();
        self.active_note = Some(id);
        self.reparse();
        self.pending_delete = None;
        self.color_picker_open = false;
        self.text_color_picker_open = false;
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

    /// Wraps the editor selection in `format`'s markup, writes the result
    /// back to the note and leaves the wrapped text selected.
    fn apply_format(&mut self, format: rich::Format) -> Task<Message> {
        self.text_color_picker_open = false;
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

    /// Copies an image file into the open note's folder and references it;
    /// does nothing without an open note and logs files that don't import.
    fn import_image(&mut self, path: &std::path::Path) -> Task<Message> {
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
    fn apply_paste(&mut self, content: ClipboardContent) -> Task<Message> {
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
    fn extend_rendered_click(&mut self) {
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
    fn step_history(&mut self, forward: bool) -> Task<Message> {
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
    fn insert_image(&mut self, rel: &str) -> Task<Message> {
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
    fn reparse(&mut self) {
        let Some(note) = self
            .active_note
            .and_then(|id| self.store.notes().iter().find(|n| n.id == id))
        else {
            return;
        };
        self.doc = rich::parse(&note.content, &self.settings.settings().palette);
        self.broken_images = self
            .doc
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Image { path, .. } if !images::is_usable(&self.data_dir, path) => {
                    Some(path.clone())
                }
                _ => None,
            })
            .collect();
    }

    fn finish_close(&mut self) -> Task<Message> {
        if self.note_drag.is_some() {
            self.finish_note_drag();
        }
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
        self.confirm_delete = None;
        self.note_resize = None;
        if let Some(id) = self.pending_delete.take() {
            self.start_collapse(id);
        }
        self.dock_window()
    }

    /// Starts shrinking the deleted note's bar away. A collapse still
    /// running for another note finishes at once.
    fn start_collapse(&mut self, id: Uuid) {
        self.finish_collapse();
        let mut morph = Morph::with_secs(COLLAPSE_SECS, self.settings.settings().motion.speed);
        morph.restart();
        self.collapsing = Some((id, morph));
        if self.peek_note == Some(id) || self.hover_bar.is_some_and(|(hovered, _)| hovered == id) {
            self.hide_peek();
        }
        self.animating = true;
    }

    /// Removes the collapsing note (if any) for good.
    fn finish_collapse(&mut self) {
        let Some((id, _)) = self.collapsing.take() else {
            return;
        };
        let index = self.store.notes().iter().position(|n| n.id == id);
        // A bar dragged below the removed one moves up a slot; a drag on
        // the removed bar itself is dropped.
        if let (Some(drag), Some(index)) = (&mut self.drag, index) {
            if drag.bar_index == index {
                self.drag = None;
            } else if drag.bar_index > index {
                drag.bar_index -= 1;
            }
        }
        self.store.delete_note(id);
        let _ = self.store.save();
        self.store.did_save();
        self.scroll_offset = self.scroll_offset.min(self.strip_layout().max_scroll);
    }

    /// The collapsing bar's index and how far it has shrunk (eased).
    fn collapse(&self) -> Option<(usize, f32)> {
        let (id, morph) = self.collapsing.as_ref()?;
        let index = self.store.notes().iter().position(|n| n.id == *id)?;
        Some((index, ease_out_cubic(morph.progress())))
    }

    fn is_collapsing(&self, index: usize) -> bool {
        self.collapse().is_some_and(|(c, _)| c == index)
    }

    /// Fades the toolbar and body in again after an edit/render switch; a
    /// switch mid-fade starts over from transparent.
    fn restart_mode_fade(&mut self) {
        self.mode_fade.restart();
        self.animating = true;
    }

    /// A mode fade that has already finished, so a note opens fully visible.
    fn settled_fade(speed: f32) -> Morph {
        let mut fade = Morph::with_secs(MODE_FADE_SECS, speed);
        fade.open();
        // Any step at least as long as the fade lands exactly on 1.
        fade.tick(2.0 * MODE_FADE_SECS / speed.max(0.01));
        fade
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
                    self.active_note.is_some() || self.settings_open || self.search_open,
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
        if !self.visible {
            return false;
        }
        let strip = self.strip_layout();
        let top = strip.bars.first().map_or(strip.add_hit_area.y, |b| b.y)
            - self.settings.settings().bars.gap;
        let bottom = strip.hit_bottom();
        let over_strip = position.x >= self.window_size.width - STRIP_WIDTH
            && (top..=bottom).contains(&position.y);
        let over_panel = |frame: Option<MorphFrame>| {
            frame.is_some_and(|frame| frame.rect.expand(8.0).contains(position))
        };
        let over_peek = self.peek_rect().is_some_and(|rect| rect.contains(position));
        over_strip
            || over_peek
            || over_panel(self.note_frame())
            || over_panel(self.settings_frame())
            || over_panel(self.search_frame())
    }

    /// Whether `id`'s copy button still shows its check.
    fn copied_for(&self, id: Uuid) -> bool {
        self.copied_at
            .is_some_and(|(copied, at)| copied == id && at.elapsed() < COPIED_FOR)
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
    /// Saves the resized note's size, and its position since resizing
    /// from the left or top edge moves it.
    fn finish_note_resize(&mut self) {
        let Some(resize) = self.note_resize.take() else {
            return;
        };
        let rect = resize.rect;
        if let Some(note) = self.active_note.and_then(|id| self.store.note_mut(id)) {
            note.size = Some([rect.width, rect.height]);
            note.position = Some([rect.x, rect.y]);
            note.updated_at = chrono::Utc::now();
            self.store.mark_dirty();
        }
    }

    /// The area a note may occupy: the window minus its margins.
    fn note_room(&self) -> Rectangle {
        Rectangle::new(
            Point::new(NOTE_MARGIN, NOTE_MARGIN),
            Size::new(
                self.window_size.width - 2.0 * NOTE_MARGIN,
                self.window_size.height - 2.0 * NOTE_MARGIN,
            ),
        )
    }

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
            (Some(y), None, None) if !self.settings_open && !self.search_open => {
                let strip = self.strip_layout();
                strip
                    .bars
                    .iter()
                    .position(|bar| (bar.y..=bar.y + bar.height).contains(&y))
                    .filter(|i| !self.is_collapsing(*i))
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

    fn cursor_on_peek(&self) -> bool {
        match (self.last_cursor, self.peek_rect()) {
            (Some(cursor), Some(rect)) => rect.contains(cursor),
            _ => false,
        }
    }

    /// The open peek's full area, while a peek is open or opening.
    fn peek_rect(&self) -> Option<Rectangle> {
        if !self.peek.is_opening() {
            return None;
        }
        let id = self.peek_note?;
        let index = self.store.notes().iter().position(|n| n.id == id)?;
        let bar = *self.strip_layout().bars.get(index)?;
        let strip = Rectangle::new(
            Point::new(self.window_size.width - STRIP_WIDTH, 0.0),
            Size::new(STRIP_WIDTH, self.window_size.height),
        );
        let note = &self.store.notes()[index];
        Some(peek_target(
            bar,
            strip,
            note,
            self.peek_confirm_delete.is_some(),
            note_peek_width(note, self.settings.settings().notes.size, bar),
        ))
    }

    fn hide_peek(&mut self) {
        self.hover_bar = None;
        self.peek_note = None;
        self.peek_confirm_delete = None;
        self.peek = Morph::peek(self.settings.settings().motion.speed);
    }

    /// The settings panel's current frame, morphing out of the gear slot.
    fn settings_frame(&self) -> Option<MorphFrame> {
        if !self.settings_open {
            return None;
        }
        let source = self.strip_layout().settings_anchor();
        Some(self.panel_frame(source, self.settings_anchor_y, &self.settings_morph))
    }

    /// The search panel's current frame, morphing out of the search slot.
    fn search_frame(&self) -> Option<MorphFrame> {
        if !self.search_open {
            return None;
        }
        let source = self.strip_layout().search_button;
        Some(self.panel_frame(source, self.search_anchor_y, &self.search_morph))
    }

    /// A panel's frame on its way from the strip slot `source` to its place
    /// left of the strip, centered on `anchor_y` where it fits.
    fn panel_frame(&self, source: Rectangle, anchor_y: f32, morph: &Morph) -> MorphFrame {
        let height = (self.window_size.height - 2.0 * NOTE_MARGIN).min(PANEL_MAX_HEIGHT);
        let right = self.window_size.width - STRIP_WIDTH - NOTE_GAP;
        let min_center = NOTE_MARGIN + height / 2.0;
        let max_center = self.window_size.height - NOTE_MARGIN - height / 2.0;
        let center_y = if max_center > min_center {
            anchor_y.clamp(min_center, max_center)
        } else {
            self.window_size.height / 2.0
        };
        let target = Rectangle::new(
            Point::new(right - PANEL_WIDTH, center_y - height / 2.0),
            Size::new(PANEL_WIDTH, height),
        );
        morph_frame(source, target, morph.progress())
    }

    /// Opens the search panel, or closes it while it is showing. It closes
    /// the note and the settings, and starts with an empty query.
    fn toggle_search(&mut self) -> Task<Message> {
        if self.search_open && self.search_morph.is_opening() {
            self.close_search();
            return Task::none();
        }
        self.pending_focus = Some(PendingFocus::SearchField);
        self.animating = true;
        if self.search_open {
            // Still folding away: unfold again from where it is.
            self.search_morph.open();
            return focus_field();
        }
        let close_note = self.update(Message::ClosePanel);
        let close_settings = self.update(Message::CloseSettings);
        self.hide_peek();
        let slot = self.strip_layout().search_button;
        self.search_anchor_y = slot.y + slot.height / 2.0;
        self.search_open = true;
        self.search_query.clear();
        self.search_morph.open();
        Task::batch([close_note, close_settings, self.dock_window()])
    }

    /// Folds the search panel back into its slot (if it is showing).
    fn close_search(&mut self) {
        if self.search_open {
            self.search_morph.close();
            self.animating = true;
        }
        if self.pending_focus == Some(PendingFocus::SearchField) {
            self.pending_focus = None;
        }
    }

    /// Opens the picked note in edit mode with its first body match
    /// selected, or the cursor at the start of the body for a title-only
    /// match, and closes the search.
    fn pick_result(&mut self, id: Uuid) -> Task<Message> {
        let Some(index) = self.store.notes().iter().position(|n| n.id == id) else {
            return Task::none();
        };
        let matched = self
            .search_hits()
            .into_iter()
            .find(|hit| hit.note_id == id)
            .and_then(|hit| hit.body_match);
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

    /// The focus waiting for its widget, once the widget is drawn.
    fn take_pending_focus(&mut self) -> Task<Message> {
        let shown = |frame: Option<MorphFrame>| frame.is_some_and(|f| f.content_alpha >= 0.01);
        match self.pending_focus {
            Some(PendingFocus::SearchField) if shown(self.search_frame()) => {
                self.pending_focus = None;
                focus_field()
            }
            Some(PendingFocus::Body) if !self.editing || self.active_note.is_none() => {
                self.pending_focus = None;
                Task::none()
            }
            Some(PendingFocus::Body) if shown(self.note_frame()) => {
                self.pending_focus = None;
                focus_body()
            }
            _ => Task::none(),
        }
    }

    /// Search results for the current query, for the panel.
    fn search_hits(&self) -> Vec<Hit> {
        search::find(self.store.notes(), &self.search_query)
    }

    /// Bars to dim, by index: while the search is open with a query, the
    /// notes that don't match it. Empty otherwise.
    fn dimmed_bars(&self) -> Vec<bool> {
        if !self.search_open || self.search_query.trim().is_empty() {
            return Vec::new();
        }
        search::matching(self.store.notes(), &self.search_query)
            .into_iter()
            .map(|matched| !matched)
            .collect()
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
            SETTINGS_SLOT,
            self.collapse(),
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

    fn dock_icon_shown(&self) -> bool {
        self.settings.settings().app.show_dock_icon || self.tray_failed
    }

    /// Shows or hides the menu bar icon and the Dock icon to match settings.
    fn apply_app_visibility(&self) -> Task<Message> {
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

    /// Applies changed settings to running state: animation speeds, scroll
    /// bounds and the docked window size.
    fn apply_settings(&mut self) -> Task<Message> {
        let s = self.settings.settings();
        let speed = s.motion.speed;
        self.morph.set_speed(speed);
        self.peek.set_speed(speed);
        self.settings_morph.set_speed(speed);
        self.mode_fade.set_speed(speed);
        if let Some((_, collapse)) = &mut self.collapsing {
            collapse.set_speed(speed);
        }
        self.scroll_offset = self.scroll_offset.min(self.strip_layout().max_scroll);
        self.animating = true;
        self.dock_window()
    }

    fn note_target_rect(&self) -> Rectangle {
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

    /// Bar centers plus the add, search and settings buttons', which
    /// magnify along with them.
    fn magnification_centers(&self) -> Vec<f32> {
        let strip = self.strip_layout();
        strip
            .bars
            .iter()
            .copied()
            .chain([strip.add_button, strip.search_button])
            .chain(strip.settings_button)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingToggle;

    fn app_in(dir: &tempfile::TempDir) -> App {
        let store = NoteStore::load(dir.path().join("notes.json"));
        let settings = SettingsStore::load(dir.path().join("settings.json"));
        App::new(store, settings)
    }

    /// Runs frame ticks until animations settle.
    fn settle(app: &mut App) {
        let mut now = Instant::now();
        for _ in 0..240 {
            now += Duration::from_millis(16);
            let _ = app.update(Message::Tick(now));
        }
    }

    /// An app with one note whose peek is open (peek delay 0).
    fn app_with_open_peek(dir: &tempfile::TempDir) -> App {
        let mut app = app_in(dir);
        app.store.add_note(&crate::note::PALETTE);
        app.settings
            .settings_mut()
            .set(crate::settings::SettingKey::PeekDelay, 0.0);
        app.window_size = Size::new(1200.0, 900.0);
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::CursorMoved(bar.center()));
        let _ = app.update(Message::StripHover(Some(bar.center().y)));
        let _ = app.update(Message::PeekTick(Instant::now() + Duration::from_secs(1)));
        assert!(app.peek_note.is_some() && app.peek.is_opening());
        app
    }

    #[test]
    fn theme_change_updates_app_theme() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        assert_eq!(app.theme.mode, theme::Mode::Light);
        let _ = app.update(Message::ThemeChanged(theme::Mode::Dark));
        assert_eq!(app.theme.mode, theme::Mode::Dark);
    }

    #[test]
    fn peek_delete_asks_first_then_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let id = app.store.notes()[0].id;
        let _ = app.update(Message::PeekDeleteRequested(0));
        assert_eq!(app.peek_confirm_delete, Some(id));
        assert_eq!(app.store.notes().len(), 1);
        let _ = app.update(Message::PeekDeleteConfirmed);
        settle(&mut app);
        assert!(app.store.notes().is_empty());
        // Deleting saves at once, like deleting the open note.
        assert!(!app.store.is_dirty());
        assert_eq!(app.peek_note, None);
        assert_eq!(app.peek_confirm_delete, None);
    }

    #[test]
    fn peek_delete_cancel_keeps_the_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let _ = app.update(Message::PeekDeleteRequested(0));
        let _ = app.update(Message::PeekDeleteCancelled);
        assert_eq!(app.peek_confirm_delete, None);
        assert_eq!(app.store.notes().len(), 1);
        assert!(app.peek_note.is_some());
    }

    #[test]
    fn peek_delete_only_for_the_peeked_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        app.store.add_note(&crate::note::PALETTE);
        let _ = app.update(Message::PeekDeleteRequested(1));
        assert_eq!(app.peek_confirm_delete, None);
        let _ = app.update(Message::PeekDeleteConfirmed);
        assert_eq!(app.store.notes().len(), 2);
    }

    #[test]
    fn closing_the_peek_drops_its_delete_question() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let _ = app.update(Message::PeekDeleteRequested(0));
        app.hide_peek();
        assert_eq!(app.peek_confirm_delete, None);
        let _ = app.update(Message::PeekDeleteConfirmed);
        assert_eq!(app.store.notes().len(), 1);
    }

    #[test]
    fn hovering_the_peek_keeps_it_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let peek = app.peek_rect().expect("open peek has a rect");
        // Left of the strip, still on the peek.
        let on_peek = Point::new(peek.x + 10.0, peek.center().y);
        assert!(on_peek.x < app.window_size.width - STRIP_WIDTH);
        let _ = app.update(Message::CursorMoved(on_peek));
        let _ = app.update(Message::StripHover(None));
        assert!(app.peek.is_opening(), "peek closed while hovered");
        assert!(app.is_interactive(on_peek));

        let outside = Point::new(peek.x - 20.0, peek.center().y);
        let _ = app.update(Message::CursorMoved(outside));
        let _ = app.update(Message::StripHover(None));
        assert!(!app.peek.is_opening(), "peek stayed open after leaving it");
    }

    /// An app with one open note, fully unfolded.
    fn app_with_open_note(dir: &tempfile::TempDir) -> App {
        let mut app = app_in(dir);
        app.store.add_note(&crate::note::PALETTE);
        app.window_size = Size::new(1400.0, 900.0);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        app
    }

    /// An app with one fully open note holding `content`.
    fn app_with_note(dir: &tempfile::TempDir, content: &str) -> App {
        let mut app = app_in(dir);
        let id = app.store.add_note(&crate::note::PALETTE);
        app.store.note_mut(id).unwrap().content = content.into();
        app.store.did_save();
        app.window_size = Size::new(1400.0, 900.0);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        app
    }

    /// Runs frame ticks until `done` holds (at most 240).
    fn tick_until(app: &mut App, done: impl Fn(&App) -> bool) {
        let mut now = Instant::now();
        for _ in 0..240 {
            if done(app) {
                return;
            }
            now += Duration::from_millis(16);
            let _ = app.update(Message::Tick(now));
        }
        panic!("condition never held");
    }

    #[test]
    fn deleting_collapses_the_bar_before_removing_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "first");
        app.store.add_note(&crate::note::PALETTE);
        let id = app.store.notes()[0].id;
        let _ = app.update(Message::DeleteRequested);
        let _ = app.update(Message::ConfirmDelete(true));
        tick_until(&mut app, |a| a.active_note.is_none());
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(id));
        assert!(app.store.notes().iter().any(|n| n.id == id));
        settle(&mut app);
        assert!(app.store.notes().iter().all(|n| n.id != id));
        assert_eq!(app.store.notes().len(), 1);
        assert!(app.collapsing.is_none());
    }

    #[test]
    fn peek_delete_collapses_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let id = app.store.notes()[0].id;
        let _ = app.update(Message::PeekDeleteRequested(0));
        let _ = app.update(Message::PeekDeleteConfirmed);
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(id));
        assert_eq!(app.store.notes().len(), 1);
        assert!(app.animating);
        settle(&mut app);
        assert!(app.store.notes().is_empty());
        assert!(app.collapsing.is_none());
    }

    #[test]
    fn second_delete_finishes_running_collapse() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let b = app.store.add_note(&crate::note::PALETTE);
        let a = app.store.notes()[0].id;
        let _ = app.update(Message::PeekDeleteRequested(0));
        let _ = app.update(Message::PeekDeleteConfirmed);
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(a));

        app.peek_note = Some(b);
        let _ = app.update(Message::PeekDeleteRequested(1));
        let _ = app.update(Message::PeekDeleteConfirmed);
        assert!(app.store.notes().iter().all(|n| n.id != a));
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(b));
        assert_eq!(app.store.notes().len(), 1);
    }

    #[test]
    fn collapsing_bar_ignores_clicks() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        app.store.add_note(&crate::note::PALETTE);
        let _ = app.update(Message::PeekDeleteRequested(0));
        let _ = app.update(Message::PeekDeleteConfirmed);
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::BarClicked(0));
        assert_eq!(app.active_note, None);
        // A real click on the bar arrives as a drag without movement.
        let _ = app.update(Message::DragStart(0, bar.center().y));
        let _ = app.update(Message::DragEnd);
        assert_eq!(app.active_note, None);
        let _ = app.update(Message::StripHover(Some(bar.center().y)));
        assert_eq!(app.hover_bar, None);
        // The other bar still opens.
        let _ = app.update(Message::BarClicked(1));
        assert!(app.active_note.is_some());
    }

    /// An app with `n` docked notes and their ids, top to bottom.
    fn app_with_bars(dir: &tempfile::TempDir, n: usize) -> (App, Vec<Uuid>) {
        let mut app = app_in(dir);
        let ids = (0..n)
            .map(|_| app.store.add_note(&crate::note::PALETTE))
            .collect();
        app.window_size = Size::new(1200.0, 900.0);
        (app, ids)
    }

    #[test]
    fn drag_on_deleted_bar_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        let bar = app.strip_layout().bars[1];
        let _ = app.update(Message::DragStart(1, bar.center().y));
        app.start_collapse(ids[1]);
        settle(&mut app);
        assert!(app.drag.is_none());
        // The press is released on the bar that slid into its place.
        let _ = app.update(Message::DragEnd);
        assert_eq!(app.active_note, None);
        let order: Vec<Uuid> = app.store.notes().iter().map(|n| n.id).collect();
        assert_eq!(order, vec![ids[0], ids[2]]);
    }

    #[test]
    fn drag_on_deleted_last_bar_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 2);
        let bar = app.strip_layout().bars[1];
        let _ = app.update(Message::DragStart(1, bar.center().y));
        // Dragged to the top: dropping it would reorder past the end.
        let _ = app.update(Message::DragMove(0.0));
        app.start_collapse(ids[1]);
        settle(&mut app);
        assert!(app.drag.is_none());
        let _ = app.update(Message::DragEnd);
        assert_eq!(app.active_note, None);
        let order: Vec<Uuid> = app.store.notes().iter().map(|n| n.id).collect();
        assert_eq!(order, vec![ids[0]]);
    }

    #[test]
    fn mode_switch_fade_reaches_full_opacity() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hello");
        assert_eq!(app.mode_fade.progress(), 1.0);
        let _ = app.update(Message::BodyClicked(Some(0)));
        assert_eq!(app.mode_fade.progress(), 0.0);
        let _ = app.update(Message::EditorBlurred);
        let _ = app.update(Message::BodyClicked(None));
        assert!(app.animating);
        settle(&mut app);
        assert_eq!(app.mode_fade.progress(), 1.0);
    }

    fn escape() -> Message {
        use keyboard::key::{Code, Named, Physical};
        Message::Key(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            modified_key: keyboard::Key::Named(Named::Escape),
            physical_key: Physical::Code(Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    fn cursor_of(app: &App) -> text_editor::Position {
        app.editor_content.as_ref().unwrap().cursor().position
    }

    #[test]
    fn opening_empty_note_starts_in_edit_mode() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_with_note(&dir, "  \n");
        assert!(app.editing);
    }

    #[test]
    fn opening_note_with_text_starts_rendered() {
        let dir = tempfile::tempdir().unwrap();
        let app = app_with_note(&dir, "# Hi\nthere");
        assert!(!app.editing);
        assert_eq!(app.doc.blocks[0].kind, crate::rich::BlockKind::Heading(1));
    }

    #[test]
    fn body_click_enters_edit_mode_at_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "a\nb\nc");
        let _ = app.update(Message::BodyClicked(Some(2)));
        assert!(app.editing);
        let at = cursor_of(&app);
        assert_eq!((at.line, at.column), (2, 0));

        let _ = app.update(Message::EditorBlurred);
        let _ = app.update(Message::BodyClicked(None));
        assert!(app.editing);
        assert_eq!(cursor_of(&app).line, 2);
    }

    #[test]
    fn blur_reparses_and_leaves_edit_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "plain");
        let _ = app.update(Message::BodyClicked(Some(0)));
        let _ = app.update(Message::NoteEdited(text_editor::Action::SelectAll));
        let _ = app.update(Message::NoteEdited(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("# x".into())),
        )));
        assert_eq!(app.store.notes()[0].content, "# x");
        let _ = app.update(Message::EditorBlurred);
        assert!(!app.editing);
        assert_eq!(app.doc.blocks[0].kind, crate::rich::BlockKind::Heading(1));
    }

    #[test]
    fn escape_leaves_edit_mode_before_folding() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "text");
        let _ = app.update(Message::BodyClicked(Some(0)));
        let _ = app.update(escape());
        assert!(!app.editing);
        assert!(app.active_note.is_some() && app.morph.is_opening());
        let _ = app.update(escape());
        assert!(!app.morph.is_opening());
    }

    #[test]
    fn toggle_task_updates_note_and_doc() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "- [ ] t");
        let _ = app.update(Message::ToggleTask(0));
        assert_eq!(app.store.notes()[0].content, "- [x] t");
        assert!(app.store.is_dirty());
        assert!(!app.editing);
        assert_eq!(app.editor_content.as_ref().unwrap().text(), "- [x] t");
        assert!(matches!(
            app.doc.blocks[0].kind,
            crate::rich::BlockKind::ListItem {
                task: Some(true),
                ..
            }
        ));
    }

    #[test]
    fn bold_wraps_selection_and_keeps_it_selected() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "a word b");
        let _ = app.update(Message::BodyClicked(Some(0)));
        app.editor_content
            .as_mut()
            .unwrap()
            .move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 6 },
                selection: Some(text_editor::Position { line: 0, column: 2 }),
            });
        let _ = app.update(Message::FormatApplied(crate::rich::Format::Bold));
        assert_eq!(app.store.notes()[0].content, "a **word** b");
        assert!(app.store.is_dirty());
        let content = app.editor_content.as_ref().unwrap();
        assert_eq!(content.text().trim_end(), "a **word** b");
        assert_eq!(content.selection().as_deref(), Some("word"));
    }

    /// Opens "Title" for editing and presses Enter `times` at its end.
    fn title_then_enter(dir: &tempfile::TempDir, times: usize) -> App {
        let mut app = app_with_note(dir, "Title");
        let _ = app.update(Message::BodyClicked(None));
        for _ in 0..times {
            let _ = app.update(Message::NoteEdited(text_editor::Action::Edit(
                text_editor::Edit::Enter,
            )));
        }
        let at = cursor_of(&app);
        assert_eq!((at.line, at.column), (times, 0));
        app
    }

    #[test]
    fn bold_after_enter_goes_on_new_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = title_then_enter(&dir, 1);
        let _ = app.update(Message::FormatApplied(crate::rich::Format::Bold));
        assert_eq!(app.editor_content.as_ref().unwrap().text(), "Title\n****");
        assert_eq!(app.store.notes()[0].content, "Title\n****");
        let at = cursor_of(&app);
        assert_eq!((at.line, at.column), (1, 2));
    }

    #[test]
    fn format_is_a_no_op_outside_edit_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "word");
        assert!(!app.editing);
        let _ = app.update(Message::FormatApplied(crate::rich::Format::Bold));
        assert_eq!(app.store.notes()[0].content, "word");
        assert!(!app.store.is_dirty());
    }

    fn png_file(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut bytes = Vec::new();
        let mut enc = png::Encoder::new(&mut bytes, 1, 1);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&[0, 0, 0, 255])
            .unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn image_ref(content: &str) -> &str {
        let start = content.find("![](").unwrap();
        &content[start..content[start..].find(')').unwrap() + start + 1]
    }

    #[test]
    fn dropped_image_appends_in_render_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hi");
        let src = png_file(&dir, "p.png");
        let _ = app.update(Message::ImageDropped(src));
        let content = app.store.notes()[0].content.clone();
        let rel = content
            .strip_prefix("hi\n![](")
            .and_then(|r| r.strip_suffix(')'))
            .unwrap();
        assert!(rel.starts_with("images/") && rel.ends_with(".png"));
        assert!(dir.path().join(rel).is_file());
        assert!(app
            .doc
            .blocks
            .iter()
            .any(|b| matches!(b.kind, BlockKind::Image { .. })));
        assert!(!app.editing);
        assert!(app.store.is_dirty());
    }

    #[test]
    fn dropped_image_inserts_at_cursor_in_edit_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "a\nb");
        let _ = app.update(Message::BodyClicked(Some(1)));
        let _ = app.update(Message::ImageDropped(png_file(&dir, "p.png")));
        let content = app.store.notes()[0].content.clone();
        let r = image_ref(&content);
        assert_eq!(content, format!("a\n{r}\nb"));
        assert!(app.editing);
        let at = cursor_of(&app);
        assert_eq!((at.line, at.column), (1, r.len()));
    }

    #[test]
    fn dropped_image_after_blank_line_stays_below_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = title_then_enter(&dir, 2);
        let _ = app.update(Message::ImageDropped(png_file(&dir, "p.png")));
        let content = app.store.notes()[0].content.clone();
        let r = image_ref(&content).to_string();
        assert_eq!(content, format!("Title\n\n{r}"));
        assert_eq!(
            app.editor_content.as_ref().unwrap().text(),
            format!("Title\n\n{r}")
        );
        let at = cursor_of(&app);
        assert_eq!((at.line, at.column), (2, r.len()));
    }

    #[test]
    fn dropped_image_mid_line_goes_on_own_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(Some(0)));
        app.editor_content
            .as_mut()
            .unwrap()
            .move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 1 },
                selection: None,
            });
        let _ = app.update(Message::ImageDropped(png_file(&dir, "p.png")));
        let content = app.store.notes()[0].content.clone();
        assert_eq!(content, format!("a\n{}\nb", image_ref(&content)));
    }

    #[test]
    fn picked_image_for_other_note_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "a");
        let first = app.active_note.unwrap();
        let _ = app.update(Message::ClosePanel);
        settle(&mut app);
        let second = app.store.add_note(&crate::note::PALETTE);
        app.store.did_save();
        let _ = app.update(Message::BarClicked(1));
        settle(&mut app);
        assert_eq!(app.active_note, Some(second));
        let _ = app.update(Message::ImagePicked(first, Some(png_file(&dir, "p.png"))));
        assert_eq!(app.store.notes()[0].content, "a");
        assert_eq!(app.store.notes()[1].content, "");
        assert!(!app.store.is_dirty());
    }

    fn rgba_1x1() -> (Vec<u8>, u32, u32) {
        (vec![0, 0, 0, 255], 1, 1)
    }

    #[test]
    fn classify_prefers_text_over_image() {
        assert_eq!(
            classify(Some("t".into()), Some(rgba_1x1())),
            ClipboardContent::Text("t".into())
        );
        assert!(matches!(
            classify(Some(String::new()), Some(rgba_1x1())),
            ClipboardContent::Image { .. }
        ));
        assert_eq!(classify(None, None), ClipboardContent::Empty);
    }

    #[test]
    fn paste_text_inserts_text_at_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(Some(0)));
        app.editor_content
            .as_mut()
            .unwrap()
            .move_to(text_editor::Cursor {
                position: text_editor::Position { line: 0, column: 1 },
                selection: None,
            });
        let _ = app.apply_paste(ClipboardContent::Text("XY".into()));
        assert_eq!(app.store.notes()[0].content, "aXYb");
        assert!(app.store.is_dirty());
    }

    #[test]
    fn paste_image_only_inserts_image() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "a");
        let _ = app.update(Message::BodyClicked(Some(0)));
        let (rgba, width, height) = rgba_1x1();
        let _ = app.apply_paste(classify(None, Some((rgba, width, height))));
        let content = app.store.notes()[0].content.clone();
        assert!(content.starts_with("![](images/") && content.ends_with(".png)\na"));
    }

    #[test]
    fn paste_with_text_and_image_prefers_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "");
        let _ = app.apply_paste(classify(Some("hello".into()), Some(rgba_1x1())));
        assert_eq!(app.store.notes()[0].content, "hello");
        assert!(!dir.path().join("images").exists());
    }

    #[test]
    fn paste_falls_back_to_system_clipboard_without_arboard_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(Some(0)));
        let task = app.apply_paste(classify(None, None));
        assert!(task.units() > 0, "no clipboard read was started");
        assert_eq!(app.store.notes()[0].content, "ab");
    }

    #[test]
    fn copy_note_marks_copied() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "body");
        let task = app.update(Message::CopyNote);
        assert!(task.units() > 0, "no clipboard write was started");
        let (_, at) = app.copied_at.expect("copied_at set");
        assert!(app.animating);
        let _ = app.update(Message::Tick(at + Duration::from_millis(1000)));
        assert!(app.copied_at.is_some());
        assert!(app.animating, "must keep ticking until the icon flips back");
        let _ = app.update(Message::Tick(at + Duration::from_millis(1600)));
        assert!(app.copied_at.is_none());
    }

    #[test]
    fn copied_check_belongs_to_its_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 2);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        let a = app.active_note.unwrap();
        let _ = app.update(Message::CopyNote);
        assert!(app.copied_for(a));
        let b = ids.into_iter().find(|id| *id != a).unwrap();
        assert!(!app.copied_for(b));
    }

    #[test]
    fn copy_note_uses_the_stores_latest_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(Some(0)));
        type_char(&mut app, 'c');
        let id = app.active_note.unwrap();
        let note = app.store.notes().iter().find(|n| n.id == id).unwrap();
        assert!(crate::note::copy_text(note).contains('c'));
    }

    fn type_char(app: &mut App, c: char) {
        let _ = app.update(Message::NoteEdited(text_editor::Action::Edit(
            text_editor::Edit::Insert(c),
        )));
    }

    fn cmd_z(shift: bool) -> Message {
        use keyboard::key::{NativeCode, Physical};
        let key = keyboard::Key::Character(if shift { "Z" } else { "z" }.into());
        let mut modifiers = keyboard::Modifiers::COMMAND;
        if shift {
            modifiers |= keyboard::Modifiers::SHIFT;
        }
        Message::Key(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn undo_reverts_typing_and_redo_restores_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(None));
        type_char(&mut app, 'c');
        type_char(&mut app, 'd');
        assert_eq!(app.store.notes()[0].content, "abcd");
        let _ = app.update(Message::Undo);
        assert_eq!(app.store.notes()[0].content, "ab");
        assert_eq!(cursor_of(&app).column, 2);
        let _ = app.update(Message::Redo);
        assert_eq!(app.store.notes()[0].content, "abcd");
        let _ = app.update(Message::Redo);
        assert_eq!(app.store.notes()[0].content, "abcd");
    }

    #[test]
    fn words_undo_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "");
        let _ = app.update(Message::BodyClicked(None));
        for c in "hi you".chars() {
            type_char(&mut app, c);
        }
        let _ = app.update(Message::Undo);
        assert_eq!(app.store.notes()[0].content, "hi ");
        let _ = app.update(Message::Undo);
        assert_eq!(app.store.notes()[0].content, "hi");
        let _ = app.update(Message::Undo);
        assert_eq!(app.store.notes()[0].content, "");
    }

    #[test]
    fn undo_reverts_toolbar_format() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hello world");
        let _ = app.update(Message::BodyClicked(Some(0)));
        let _ = app.update(Message::NoteEdited(text_editor::Action::SelectWord));
        let _ = app.update(Message::FormatApplied(rich::Format::Bold));
        let _ = app.update(Message::Undo);
        assert_eq!(app.store.notes()[0].content, "hello world");
    }

    #[test]
    fn undo_reverts_checkbox_toggle_in_rendered_view() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "- [ ] t");
        assert!(!app.editing);
        let _ = app.update(Message::ToggleTask(0));
        assert_eq!(app.store.notes()[0].content, "- [x] t");
        let _ = app.update(Message::Undo);
        assert_eq!(app.store.notes()[0].content, "- [ ] t");
        assert!(matches!(
            app.doc.blocks[0].kind,
            BlockKind::ListItem {
                task: Some(false),
                ..
            }
        ));
    }

    #[test]
    fn reopening_a_note_starts_a_fresh_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(None));
        type_char(&mut app, 'c');
        let _ = app.update(Message::ClosePanel);
        settle(&mut app);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        let _ = app.update(Message::Undo);
        assert_eq!(app.store.notes()[0].content, "abc");
    }

    #[test]
    fn cmd_z_in_rendered_view_checks_title_focus_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "- [ ] t");
        let _ = app.update(Message::ToggleTask(0));
        let task = app.update(cmd_z(false));
        // Undo waits for the focus check, so nothing changed yet.
        assert!(task.units() > 0);
        assert_eq!(app.store.notes()[0].content, "- [x] t");
    }

    fn selected(app: &App) -> Option<String> {
        app.editor_content.as_ref().unwrap().selection()
    }

    fn editor_click() -> Message {
        Message::NoteEdited(text_editor::Action::Click(Point::ORIGIN))
    }

    #[test]
    fn press_in_formatted_view_puts_cursor_at_that_character() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x\na **bold** c");
        let _ = app.update(Message::BodyPressed(7));
        assert!(app.editing);
        let at = cursor_of(&app);
        assert_eq!((at.line, at.column), (1, 5));
        assert_eq!(selected(&app), None);
    }

    #[test]
    fn quick_clicks_after_a_press_select_word_then_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x\na **bold** c");
        let _ = app.update(Message::BodyPressed(7));
        // The second click lands on the editor that just appeared.
        let _ = app.update(editor_click());
        assert_eq!(selected(&app).as_deref(), Some("bold"));
        // The editor sees its own first click and reports a double.
        let _ = app.update(Message::NoteEdited(text_editor::Action::SelectWord));
        assert_eq!(selected(&app).as_deref(), Some("a **bold** c"));
    }

    #[test]
    fn double_click_in_formatted_view_then_bold() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hello world");
        let _ = app.update(Message::BodyPressed(8));
        let _ = app.update(editor_click());
        let _ = app.update(Message::FormatApplied(rich::Format::Bold));
        assert_eq!(app.store.notes()[0].content, "hello **world**");
    }

    #[test]
    fn slow_second_click_is_an_ordinary_click() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hello world");
        let _ = app.update(Message::BodyPressed(8));
        app.rendered_click.as_mut().unwrap().at -= Duration::from_secs(1);
        let _ = app.update(editor_click());
        assert_eq!(selected(&app), None);
    }

    #[test]
    fn bold_applies_to_word_selected_by_double_click() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hello world");
        let _ = app.update(Message::BodyClicked(Some(0)));
        let _ = app.update(Message::NoteEdited(text_editor::Action::SelectWord));
        let _ = app.update(Message::FormatApplied(rich::Format::Bold));
        assert_eq!(app.store.notes()[0].content, "**hello** world");
    }

    #[test]
    fn late_clipboard_text_pastes_into_its_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(None));
        let id = app.active_note.unwrap();
        let _ = app.update(Message::ClipboardText(id, "XY".into()));
        assert_eq!(app.store.notes()[0].content, "abXY");
    }

    #[test]
    fn late_clipboard_text_ignored_after_leaving_edit_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(None));
        let id = app.active_note.unwrap();
        let _ = app.update(Message::EditorBlurred);
        let _ = app.update(Message::ClipboardText(id, "XY".into()));
        assert_eq!(app.store.notes()[0].content, "ab");
        assert!(!app.store.is_dirty());
    }

    #[test]
    fn late_clipboard_text_for_other_note_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "ab");
        let _ = app.update(Message::BodyClicked(None));
        let _ = app.update(Message::ClipboardText(Uuid::new_v4(), "XY".into()));
        assert_eq!(app.store.notes()[0].content, "ab");
        assert!(!app.store.is_dirty());
    }

    #[test]
    fn paste_ignored_when_not_editing() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hi");
        assert!(!app.editing);
        let _ = app.update(Message::PasteRequested);
        assert_eq!(app.store.notes()[0].content, "hi");
    }

    #[test]
    fn dropped_non_image_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "hi");
        let txt = dir.path().join("a.txt");
        std::fs::write(&txt, "x").unwrap();
        let _ = app.update(Message::ImageDropped(txt));
        assert_eq!(app.store.notes()[0].content, "hi");
        assert!(!app.store.is_dirty());
    }

    #[test]
    fn drop_without_open_note_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.store.add_note(&crate::note::PALETTE);
        app.store.did_save();
        let _ = app.update(Message::ImageDropped(png_file(&dir, "p.png")));
        assert_eq!(app.store.notes()[0].content, "");
        assert!(!app.store.is_dirty());
        assert!(!dir.path().join("images").exists());
    }

    #[test]
    fn text_and_note_color_pickers_exclude_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let _ = app.update(Message::ToggleColorPicker);
        let _ = app.update(Message::ToggleTextColorPicker);
        assert!(app.text_color_picker_open && !app.color_picker_open);
        let _ = app.update(Message::ToggleColorPicker);
        assert!(app.color_picker_open && !app.text_color_picker_open);
    }

    #[test]
    fn blur_while_rendered_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "- [ ] t");
        let doc = app.doc.clone();
        let _ = app.update(Message::EditorBlurred);
        assert!(!app.editing);
        assert_eq!(app.doc, doc);
        assert_eq!(app.store.notes()[0].content, "- [ ] t");
        assert!(!app.store.is_dirty());
        assert!(app.morph.is_opening());
    }

    #[test]
    fn undecodable_image_is_marked_broken() {
        let dir = tempfile::tempdir().unwrap();
        let rel = format!("images/{}.png", Uuid::new_v4());
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        std::fs::write(dir.path().join(&rel), b"not a png").unwrap();
        let app = app_with_note(&dir, &format!("![]({rel})"));
        assert!(app.broken_images.contains(&rel));
    }

    #[test]
    fn valid_image_is_not_broken() {
        let dir = tempfile::tempdir().unwrap();
        let rel = images::import_png(dir.path(), &[255; 16], 2, 2).unwrap();
        let app = app_with_note(&dir, &format!("![]({rel})"));
        assert!(matches!(app.doc.blocks[0].kind, BlockKind::Image { .. }));
        assert!(app.broken_images.is_empty());
    }

    #[test]
    fn failed_load_keeps_notes_file_and_images() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes.json");
        std::fs::write(&notes, "{not json").unwrap();
        let image = images::import_png(dir.path(), &[255; 16], 2, 2).unwrap();
        let mut store = NoteStore::load(notes.clone());
        prepare_store(&mut store, &crate::note::PALETTE);
        assert!(dir.path().join(&image).is_file());
        assert_eq!(std::fs::read_to_string(&notes).unwrap(), "{not json");
        assert!(store.notes().is_empty());
    }

    #[test]
    fn fresh_store_is_seeded_and_swept() {
        let dir = tempfile::tempdir().unwrap();
        let image = images::import_png(dir.path(), &[255; 16], 2, 2).unwrap();
        let mut store = NoteStore::load(dir.path().join("notes.json"));
        prepare_store(&mut store, &crate::note::PALETTE);
        assert_eq!(store.notes().len(), 3);
        assert!(!dir.path().join(&image).exists());
    }

    #[test]
    fn jpeg_gif_and_webp_images_decode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("images")).unwrap();
        let mut content = String::new();
        for ext in ["jpg", "gif", "webp"] {
            let rel = format!("images/{}.{ext}", Uuid::new_v4());
            image::RgbImage::from_pixel(2, 2, image::Rgb([200, 30, 30]))
                .save(dir.path().join(&rel))
                .unwrap();
            content.push_str(&format!("![]({rel})\n\n"));
        }
        let app = app_with_note(&dir, &content);
        assert_eq!(app.doc.blocks.len(), 3);
        assert!(app.broken_images.is_empty(), "{:?}", app.broken_images);
    }

    #[test]
    fn link_scheme_filter() {
        use crate::rich_view::openable;
        assert!(openable("https://a").is_some());
        assert!(openable("http://a").is_some());
        assert!(openable("mailto:a@b").is_some());
        assert!(openable("file:///etc").is_none());
        assert!(openable("javascript:x").is_none());
    }

    #[test]
    fn openable_returns_the_trimmed_url_it_checked() {
        use crate::rich_view::openable;
        assert_eq!(
            openable("  \thttps://a/?x=1&y=2 "),
            Some("https://a/?x=1&y=2")
        );
        assert_eq!(openable(" javascript:x"), None);
    }

    #[test]
    fn resizing_saves_size_and_position() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_note(&dir);
        let start = app.note_target_rect();
        let grab = Point::new(start.x + start.width - 2.0, start.center_y());
        let _ = app.update(Message::CursorMoved(grab));
        let edges = crate::resize::edges_at(start, grab).expect("on the right edge");
        let _ = app.update(Message::ResizeStart(edges));
        // Stays inside the window margins, which the docked note is 58 px from.
        let _ = app.update(Message::CursorMoved(grab + Vector::new(40.0, 25.0)));
        assert_eq!(app.note_target_rect().width, start.width + 40.0);
        let _ = app.update(Message::MouseButton(false));
        let note = &app.store.notes()[0];
        assert_eq!(note.size, Some([start.width + 40.0, start.height]));
        assert_eq!(note.position, Some([start.x, start.y]));
    }

    #[test]
    fn note_opens_at_its_saved_size() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let id = app.store.add_note(&crate::note::PALETTE);
        app.store.note_mut(id).unwrap().size = Some([500.0, 410.0]);
        app.window_size = Size::new(1400.0, 900.0);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        assert_eq!(app.note_target_rect().size(), Size::new(500.0, 410.0));
    }

    #[test]
    fn reopening_closing_settings_keeps_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleSettings);
        settle(&mut app);
        app.settings_anchor_y = 123.0;
        let _ = app.update(Message::ToggleSettings);
        assert!(!app.settings_morph.is_opening());
        let _ = app.update(Message::ToggleSettings);
        assert!(app.settings_morph.is_opening());
        assert_eq!(app.settings_anchor_y, 123.0);
    }

    #[test]
    fn tray_settings_item_only_opens() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::TrayMenu("settings".into()));
        settle(&mut app);
        let _ = app.update(Message::TrayMenu("settings".into()));
        assert!(app.settings_open && app.settings_morph.is_opening());
    }

    fn cmd_f() -> Message {
        use keyboard::key::{NativeCode, Physical};
        let key = keyboard::Key::Character("f".into());
        Message::Key(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::COMMAND,
            text: None,
            repeat: false,
        })
    }

    /// An app with docked notes holding these (title, content) pairs.
    fn app_with_notes(dir: &tempfile::TempDir, notes: &[(&str, &str)]) -> (App, Vec<Uuid>) {
        let mut app = app_in(dir);
        let ids = notes
            .iter()
            .map(|(title, content)| {
                let id = app.store.add_note(&crate::note::PALETTE);
                let note = app.store.note_mut(id).unwrap();
                note.title = (*title).into();
                note.content = (*content).into();
                id
            })
            .collect();
        app.store.did_save();
        app.window_size = Size::new(1400.0, 900.0);
        (app, ids)
    }

    fn search_showing(app: &App) -> bool {
        app.search_open && app.search_morph.is_opening()
    }

    #[test]
    fn cmd_f_opens_search_and_closes_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleSettings);
        settle(&mut app);
        let _ = app.update(cmd_f());
        assert!(search_showing(&app));
        assert!(!app.settings_morph.is_opening());
        settle(&mut app);
        assert!(!app.settings_open);
        // Settings close the search the same way.
        let _ = app.update(Message::ToggleSettings);
        assert!(!app.search_morph.is_opening());
        assert!(app.settings_morph.is_opening());
        // Cmd+F again toggles it.
        let _ = app.update(cmd_f());
        let _ = app.update(cmd_f());
        assert!(!app.search_morph.is_opening());
    }

    #[test]
    fn typing_filters_results() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(
            &dir,
            &[
                ("Groceries", "milk"),
                ("Work", "call Milan"),
                ("Other", "x"),
            ],
        );
        let _ = app.update(Message::ToggleSearch);
        assert!(app.search_hits().is_empty());
        let _ = app.update(Message::SearchChanged("MIL".into()));
        let hits: Vec<Uuid> = app.search_hits().iter().map(|h| h.note_id).collect();
        assert_eq!(hits, vec![ids[0], ids[1]]);
        let _ = app.update(Message::SearchChanged("groc".into()));
        let hits: Vec<Uuid> = app.search_hits().iter().map(|h| h.note_id).collect();
        assert_eq!(hits, vec![ids[0]]);
    }

    #[test]
    fn picking_a_result_opens_note_with_match_selected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("t", "first line\nsay Café now")]);
        let _ = app.update(Message::ToggleSearch);
        let _ = app.update(Message::SearchChanged("café".into()));
        let _ = app.update(Message::SearchResultPicked(ids[0]));
        assert_eq!(app.active_note, Some(ids[0]));
        assert!(app.editing);
        assert!(!app.search_morph.is_opening());
        assert_eq!(selected(&app).as_deref(), Some("Café"));
        let cursor = app.editor_content.as_ref().unwrap().cursor();
        assert_eq!(
            (cursor.position.line, cursor.position.column),
            (1, "say Café".len())
        );
        settle(&mut app);
        assert!(!app.search_open);
        assert_eq!(selected(&app).as_deref(), Some("Café"));

        // A title-only match puts the cursor at the start of the body.
        let (mut app, ids) = app_with_notes(&dir, &[("Plan", "a\nb")]);
        let _ = app.update(Message::ToggleSearch);
        let _ = app.update(Message::SearchChanged("plan".into()));
        let _ = app.update(Message::SearchResultPicked(ids[0]));
        assert!(app.editing);
        assert_eq!(selected(&app), None);
        let at = cursor_of(&app);
        assert_eq!((at.line, at.column), (0, 0));
    }

    #[test]
    fn picking_a_result_switches_notes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "alpha"), ("b", "beta needle")]);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        let _ = app.update(cmd_f());
        // Opening the search folds the open note; pick while it folds.
        assert!(!app.morph.is_opening());
        let _ = app.update(Message::SearchChanged("needle".into()));
        let _ = app.update(Message::SearchResultPicked(ids[1]));
        assert_eq!(app.active_note, Some(ids[1]));
        assert!(app.morph.is_opening());
        let text = app.editor_content.as_ref().unwrap().text();
        assert_eq!(text.trim_end(), "beta needle");
        assert_eq!(selected(&app).as_deref(), Some("needle"));
        settle(&mut app);
        assert_eq!(app.active_note, Some(ids[1]));
        assert_eq!(selected(&app).as_deref(), Some("needle"));
        assert!(!app.search_open);
    }

    #[test]
    fn escape_closes_search_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleSearch);
        settle(&mut app);
        let _ = app.update(escape());
        assert!(!app.search_morph.is_opening());
        settle(&mut app);
        assert!(!app.search_open);

        // Esc swallowed by the focused search field closes it too.
        let _ = app.update(Message::ToggleSearch);
        settle(&mut app);
        let _ = app.update(Message::CloseSearch);
        assert!(!app.search_morph.is_opening());
        // Without the search open a captured Esc does nothing.
        let (mut app, _) = app_with_notes(&dir, &[("a", "b")]);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        let _ = app.update(Message::CloseSearch);
        assert!(app.morph.is_opening());
    }

    #[test]
    fn tray_search_item_opens_search() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleVisibility);
        assert!(!app.visible);
        let _ = app.update(Message::TrayMenu("search".into()));
        assert!(app.visible);
        assert!(search_showing(&app));
        settle(&mut app);
        // The menu item opens the search; it never closes it.
        let _ = app.update(Message::TrayMenu("search".into()));
        assert!(search_showing(&app));
    }

    #[test]
    fn non_matching_bars_dim() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "red"), ("b", "blue"), ("Red", "")]);
        let _ = app.update(Message::SearchChanged("red".into()));
        // Nothing dims while the panel is closed.
        assert!(app.dimmed_bars().is_empty());
        let _ = app.update(Message::ToggleSearch);
        assert!(app.dimmed_bars().iter().all(|d| !d));
        let _ = app.update(Message::SearchChanged("red".into()));
        assert_eq!(app.dimmed_bars(), vec![false, true, false]);
        let _ = app.update(Message::SearchChanged("  ".into()));
        assert!(app.dimmed_bars().iter().all(|d| !d));
    }

    #[test]
    fn search_suppresses_the_hover_peek() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let _ = app.update(Message::ToggleSearch);
        assert!(app.peek_note.is_none());
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::StripHover(Some(bar.center().y)));
        assert_eq!(app.hover_bar, None);
    }

    #[test]
    fn tray_failure_shows_dock_without_saving_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.settings.settings_mut().toggle(SettingToggle::DockIcon);
        app.settings.did_save();
        let _ = app.update(Message::TrayReady(false));
        assert!(app.dock_icon_shown());
        assert!(!app.settings.settings().app.show_dock_icon);
        assert!(!app.settings.is_dirty());
    }
}
