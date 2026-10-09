use crate::animation::{ease_out_cubic, morph_frame, MagnificationState, Morph, MorphFrame};
use crate::autohide::{self, AutoHide};
use crate::bar_strip::{
    along_center, along_span, band, compute_layout, hide_translation, peek_target, stack_target,
    BarStrip, StripLayout, HIDE_SHIFT, STRIP_WIDTH,
};
use crate::edge::{Edge, LocalRect};
use crate::export::{self, ExportFormat};
use crate::export_panel::{export_panel, ExportJob, ExportState, ExportStatus, ExportView};
use crate::history::History;
use crate::hotkey;
use crate::images;
use crate::note::NoteColor;
use crate::note_panel::{focus_body, post_it, PostIt};
use crate::notify;
use crate::peek::{entry_peek_text, note_peek_width};
use crate::platform::{self, SUPPORTS_PASSTHROUGH};
use crate::reminder;
use crate::reminder_picker::ReminderDraft;
use crate::resize::{resize_frame, resized, Edges, MIN_SIZE};
use crate::rich::{self, BlockKind, Doc};
use crate::rich_view;
use crate::search::{self, Hit};
use crate::search_panel::{focus_field, search_panel, SearchView};
use crate::settings::{
    SettingKey, SettingToggle, Settings, SettingsGroup, SettingsStore, SettingsTab,
};
use crate::settings_panel::{settings_panel, SettingsView, PANEL_MAX_HEIGHT, PANEL_WIDTH};
use crate::store::{Deleted, NoteStore};
use crate::strip_model::{self, Entry};
use crate::theme;
use crate::tray;

use iced::widget::{container, mouse_area, opaque, pin, stack, text_editor, Space};
use chrono::Datelike;
use iced::{
    event, keyboard, mouse, window, Element, Fill, Point, Rectangle, Size, Subscription, Task,
    Vector,
};
use std::borrow::Cow;
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
/// How long the export panel shows its success before closing.
const EXPORTED_FOR: Duration = Duration::from_secs(2);
/// How often pending reminders are checked.
const REMINDER_POLL: Duration = Duration::from_secs(30);
/// One breath of a fired reminder's bar (Pulse alert style).
const PULSE_SECS: f32 = 1.4;
/// One hop of a fired reminder's bar (Jump alert style).
const JUMP_SECS: f32 = 0.7;
/// How far a jumping bar moves away from the screen edge, in pixels.
const JUMP_REACH: f32 = 10.0;
/// How long a jumping bar settles to rest when the cursor enters it.
const JUMP_SETTLE_SECS: f32 = 0.15;
/// How long "Clipboard is empty" shows beside `+`.
const CLIPBOARD_HINT_FOR: Duration = Duration::from_millis(1500);
/// How long the undo toast stays after a delete.
const UNDO_FOR: Duration = Duration::from_secs(5);
/// How long `+` shakes after an empty clipboard.
const SHAKE_SECS: f32 = 0.3;

/// Depth of the edge zone at the window's border on the strip's edge that
/// reveals a hidden strip.
const EDGE_ZONE: f32 = 2.0;

/// Without passthrough, the docked window's thickness away from the edge
/// while the strip is hidden: just enough to notice the cursor at the
/// screen edge.
const EDGE_SLIVER: f32 = 2.0;

/// The strip's band along its edge, start and end: from the first bar (or
/// the add slot without bars) less a bar gap, to the last slot's hit area.
fn strip_band(strip: &StripLayout, gap: f32) -> (f32, f32) {
    let first = strip.bars.first().unwrap_or(&strip.add_hit_area);
    let start = along_span(*first, strip.edge).0 - gap;
    (start, strip.hit_bottom())
}

/// Where a cursor that didn't move is, relative to a right-docked window
/// that changed from `old_width` to `new_width`; `None` once it is outside.
fn shifted_pointer(pointer: Option<Point>, old_width: f32, new_width: f32) -> Option<Point> {
    pointer
        .map(|p| Point::new(p.x + new_width - old_width, p.y))
        .filter(|p| (0.0..=new_width).contains(&p.x))
}

/// Where a cursor that didn't move is, relative to a window docked to
/// `edge` whose thickness changed from `old` to `new`: the side on the
/// screen edge stays put. `None` once it is outside.
fn pointer_after_resize(pointer: Option<Point>, edge: Edge, old: f32, new: f32) -> Option<Point> {
    match edge {
        Edge::Right => shifted_pointer(pointer, old, new),
        Edge::Left => pointer.filter(|p| (0.0..=new).contains(&p.x)),
        Edge::Top => pointer.filter(|p| (0.0..=new).contains(&p.y)),
    }
}

/// The window docked to `edge` on `monitor`. With `passthrough` it covers
/// the monitor, below the `top_inset` (the menu bar) on Top. Without, it is
/// `thickness` away from the edge and the `fraction` of the edge long,
/// flush to the edge and centred along it, and no thicker than the screen
/// below the menu bar.
fn dock_frame(
    edge: Edge,
    monitor: Size,
    top_inset: f32,
    passthrough: bool,
    thickness: f32,
    fraction: f32,
) -> Rectangle {
    let size = match (passthrough, edge) {
        (true, Edge::Top) => Size::new(monitor.width, monitor.height - top_inset),
        (true, _) => monitor,
        (false, _) => {
            let room = match edge {
                Edge::Top => monitor.height - top_inset,
                Edge::Right | Edge::Left => edge.thickness(monitor),
            };
            let length = (edge.edge_length(monitor) * fraction).round();
            edge.size(length, thickness.min(room))
        }
    };
    let origin = edge.dock_origin(size, monitor, top_inset);
    // Whole pixels along the edge, where the window is centred.
    let origin = match edge {
        Edge::Right | Edge::Left => Point::new(origin.x, origin.y.round()),
        Edge::Top => Point::new(origin.x.round(), origin.y),
    };
    Rectangle::new(origin, size)
}

/// The `+` slot's sideways offset `t` seconds after an empty clipboard:
/// three cycles of ±3 px, settling within `SHAKE_SECS`.
fn add_shake(t: f32) -> f32 {
    if !(0.0..SHAKE_SECS).contains(&t) {
        return 0.0;
    }
    3.0 * (t * std::f32::consts::TAU * 10.0).sin() * (1.0 - t / SHAKE_SECS)
}

/// Away-from-edge displacement for a reminder jump at phase `t` seconds.
fn jump_amount(t: f32) -> f32 {
    let period = JUMP_SECS;
    if period <= 0.0 {
        return 0.0;
    }
    let phase = t.rem_euclid(period) / period;
    JUMP_REACH * (std::f32::consts::PI * phase).sin().max(0.0)
}

/// Eases a paused jump from `from` down to rest over `JUMP_SETTLE_SECS`.
fn jump_settle_offset(from: f32, elapsed: f32) -> f32 {
    if !(0.0..JUMP_SETTLE_SECS).contains(&elapsed) {
        return 0.0;
    }
    let t = (elapsed / JUMP_SETTLE_SECS).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    from * (1.0 - s)
}

/// A task that delivers `message` once `duration` has passed, so a timeout
/// needs no frames running meanwhile.
fn delayed(duration: Duration, message: Message) -> Task<Message> {
    // Built when the task runs: a tokio timer needs the runtime.
    let sleep = async move { tokio::time::sleep(duration).await };
    Task::perform(sleep, move |()| message)
}

#[derive(Debug, Clone)]
pub enum Message {
    ThemeChanged(theme::Mode),
    StripHover(Option<f32>),
    BarClicked(usize),
    AddNote,
    /// The cursor entered or left the strip's Actions control.
    ActionsHovered(bool),
    ToggleSettings,
    CloseSettings,
    /// Cmd/Ctrl+F, the strip's search slot or the tray item.
    ToggleSearch,
    SearchChanged(String),
    SearchResultPicked(Uuid),
    /// Enter in the search field: opens the first result, if there is one.
    SearchSubmitted,
    /// The panel's close button, a click beside it, or an Esc the focused
    /// search field took (it only drops its focus on Esc).
    CloseSearch,
    /// Opens the export panel in place of the settings (the Settings'
    /// button or the tray item), or closes it while it is showing.
    ToggleExport,
    ExportToggleNote(Uuid),
    /// The panel's "All" (`true`) and "None" (`false`).
    ExportAll(bool),
    ExportFormatChosen(ExportFormat),
    /// The Export button: opens the save dialog.
    ExportRequested,
    /// The save dialog returned what to write.
    ExportPicked(ExportJob),
    /// The save dialog was cancelled; the panel generation that opened it.
    ExportCancelled(u64),
    /// `EXPORTED_FOR` after the success shown at this instant in the panel
    /// of this generation: the panel closes, if it still shows it.
    ExportStatusExpired(u64, Instant),
    SettingChanged(SettingKey, f32),
    SettingToggled(SettingToggle),
    EdgeChosen(Edge),
    ReminderAlertChosen(crate::settings::ReminderAlertStyle),
    ResetGroup(SettingsGroup),
    SettingsTabSelected(SettingsTab),
    /// Palette slot whose preset grid is open (`None` closes it).
    PaletteSlotSelected(Option<usize>),
    PaletteColorChosen(NoteColor),
    NoteEdited(text_editor::Action),
    /// A click on the rendered body: the block's source line, or `None`
    /// below the last block.
    BodyClicked(Option<usize>),
    /// A press on rendered text: the source offset of the character under it.
    BodyPressed(usize),
    /// The trash button on the open peek of the note at this index:
    /// deletes its note at once.
    PeekDelete(usize),
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
    /// The open note's trash button: it folds away, then is deleted.
    DeleteNote(Uuid),
    CopyNote,
    TogglePin,
    /// `COPIED_FOR` after the copy of this note with this number: the copy
    /// button's check goes, unless something was copied again since.
    CopiedExpired(Uuid, u64),
    /// The undo toast, or Cmd/Ctrl+Z with no note open: brings the last
    /// deleted note back.
    UndoDelete,
    /// `UNDO_FOR` after the delete at this instant: its toast goes, unless
    /// another delete replaced it.
    UndoExpired(Instant),
    ToggleColorPicker,
    /// Open or close the toolbar's text color grid.
    ToggleTextColorPicker,
    /// Wrap the editor selection in this format's markup.
    FormatApplied(rich::Format),
    /// A file was dropped on the window; images are added to the open note.
    /// Without one, the drop goes to the strip as `StripFileDropped`.
    ImageDropped(PathBuf),
    /// A file dropped with no note open: onto this bar (entry) index, or
    /// `None` for the add slot, empty strip space or beside the strip.
    StripFileDropped(Option<usize>, PathBuf),
    /// A file is dragged over the window: no cursor moves arrive meanwhile,
    /// so the last cursor position is forgotten until one does.
    FileHovered,
    /// The dragged files left the window without a drop.
    FileHoverEnded,
    /// The tray item or an Alt/Option-click on `+`: a new note holding the
    /// clipboard's text or image.
    ClipboardNote,
    /// The "Clipboard is empty" hint shown at this instant has had its time.
    ClipboardEmptyExpired(Instant),
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
    /// A preset in the color bubble: recolors the open note and closes it.
    ColorChosen(NoteColor),
    /// A press or drag on the bubble's color wheel: recolors the open note
    /// at once and keeps the bubble open.
    ColorAdjusted(NoteColor),
    /// The bubble's hex field changed; a valid color applies at once.
    ColorHexEdited(String),
    /// Enter in the hex field.
    ColorHexSubmitted,
    /// Esc or a press outside the bubble.
    CloseColorBubble,
    /// Where the bubble's card is on screen, so presses on it never fall
    /// through the window.
    ColorBubbleMoved(Rectangle),
    /// Toolbar Reminder button: open or close the calendar picker.
    ToggleReminderPicker,
    /// Header trigger time: open the calendar picker.
    OpenReminderPicker,
    ReminderPickerDay(chrono::NaiveDate),
    ReminderPickerMonth(i32),
    ReminderPickerHour(i32),
    ReminderPickerMinute(i32),
    ReminderPickerDone,
    ReminderPickerClear,
    DragStart(usize, f32),
    DragMove(f32),
    DragEnd,
    /// A dragged bar dropped onto another bar's middle: its note (with its
    /// stack) goes under that bar's note. Both are bar (entry) indices.
    StackOnto {
        dragged: usize,
        target: usize,
    },
    /// The stack peek's "Unstack" button on this bar.
    Unstack(usize),
    /// A title in the stack peek's list.
    OpenStackMember(Uuid),
    StripScroll(f32),
    /// The wheel on the open peek: its body's new scroll, in pixels.
    PeekScroll(f32),
    Tick(Instant),
    SaveTick,
    /// Fires the reminders that have come due.
    ReminderTick,
    ToggleVisibility,
    /// The global hotkey: shows the notes and opens a new one.
    HotkeyPressed,
    /// The tray icon was created (or failed to be).
    TrayReady(bool),
    TrayMenu(String),
    Quit,
    /// The window exists: its monitor's size and how far the usable area
    /// starts below the monitor's top (the menu bar).
    WindowReady(window::Id, Option<Size>, f32),
    WindowResized(Size),
    Key(keyboard::Event),
    CursorMoved(Point),
    CursorLeftWindow,
    MouseButton(bool),
    WindowUnfocused,
    PollCursor,
    PeekTick(Instant),
    CursorPolled(Option<Point>),
    /// Auto-hide's dwell or grace deadline at this instant has come.
    AutoHideWake(Instant),
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

/// Largest text file whose text a drop adds.
const MAX_TEXT_BYTES: u64 = 1024 * 1024;

/// The text of a regular `.txt` or `.md` file of at most `MAX_TEXT_BYTES`
/// that is valid UTF-8, without trailing line breaks. A FIFO or device is
/// never read, since reading it could block.
fn read_text_file(path: &std::path::Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "txt" | "md") {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_TEXT_BYTES {
        return None;
    }
    let text = String::from_utf8(std::fs::read(path).ok()?).ok()?;
    Some(text.trim_end_matches(['\r', '\n']).to_string())
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
    morph: Morph,
    anchor_y: f32,
    pending_delete: Option<Uuid>,
    /// A deleted note whose bar is shrinking away; it is removed from the
    /// store once the bar is gone.
    collapsing: Option<(Uuid, Morph)>,
    /// Fades the toolbar and body in after switching edit/render mode.
    mode_fade: Morph,
    color_picker_open: bool,
    /// The color bubble's hex field, as typed.
    color_hex: String,
    /// The bubble's card on screen, as last reported while open.
    color_bubble_rect: Option<Rectangle>,
    text_color_picker_open: bool,
    /// Calendar draft while the reminder picker bubble is open.
    reminder_picker: Option<ReminderDraft>,
    note_hovered: bool,
    drag: Option<DragState>,
    scroll_offset: f32,
    /// The latest delete, while its undo toast shows.
    last_deleted: Option<LastDelete>,
    /// The note last copied and the copy's number; its copy button shows a
    /// check for `COPIED_FOR` after.
    copied_at: Option<(Uuid, u64)>,
    /// Counts copies, so a check's timer only clears its own copy.
    copies: u64,
    /// When a clipboard note found nothing: `+` shakes and a chip says so.
    clipboard_empty_at: Option<Instant>,
    animating: bool,
    last_tick: Option<Instant>,
    /// Notes whose reminder fired and that haven't been opened since; their
    /// bars jump or pulse per the reminder-alert setting.
    pulsing: HashSet<Uuid>,
    /// Seconds into the alert animation, advanced by frames while alerting.
    pulse_phase: f32,
    /// Jump settle on hover: note id, when settle started, offset at pause.
    jump_settle: Option<(Uuid, Instant, f32)>,
    visible: bool,
    window_id: Option<window::Id>,
    monitor: Option<Size>,
    /// How far the usable area starts below the monitor's top (the macOS
    /// menu bar); 0 when unknown.
    top_inset: f32,
    window_size: Size,
    /// Where `dock_window` last placed the window.
    window_origin: Option<Point>,
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
    /// The note whose peek body was scrolled, and how far; every peek
    /// starts at the top.
    peek_scroll: Option<(Uuid, f32)>,
    peek: Morph,
    /// The settings panel is showing (kept while it folds back into the gear).
    settings_open: bool,
    settings_morph: Morph,
    /// Gear center when the panel opened; the panel stays centered on it.
    settings_anchor_y: f32,
    settings_tab: SettingsTab,
    /// The strip Actions slot shows New / Search / Settings side by side.
    actions_open: bool,
    palette_slot: Option<usize>,
    /// The search panel is showing (kept while it folds back into its slot).
    search_open: bool,
    search_morph: Morph,
    /// Search slot center when the panel opened.
    search_anchor_y: f32,
    search_query: String,
    /// Results for `search_query`, and the ids of every matching note (for
    /// dimming), as of `search_synced`: the store revision and query they
    /// were computed for.
    search_results: Vec<Hit>,
    search_matches: HashSet<Uuid>,
    search_synced: Option<(u64, String)>,
    /// The strip's bars as of store revision `entries_synced`; see
    /// [`App::entries`].
    strip_entries: Vec<Entry>,
    /// Task progress per entry of `strip_entries`, from the same revision.
    strip_progress: Vec<Option<(usize, usize)>>,
    entries_synced: u64,
    /// The export panel is showing (kept while it folds back into the gear).
    export: Option<ExportState>,
    export_morph: Morph,
    /// Gear center when the export panel opened.
    export_anchor_y: f32,
    /// Counts fresh openings of the export panel; see
    /// [`ExportState::generation`].
    export_generation: u64,
    /// The export's save dialog is open. It outlives the panel that opened
    /// it (the dialog doesn't block the app everywhere), so a reopened
    /// panel can't start a second one.
    export_dialog_open: bool,
    /// Keyboard focus to give once the panel or note showing it has
    /// faded its content in (the widget doesn't exist before).
    pending_focus: Option<PendingFocus>,
    /// The registered global hotkey; dropping it unregisters it.
    hotkey: Option<hotkey::Manager>,
    /// Why the last hotkey registration failed; shown in Settings.
    hotkey_error: Option<String>,
    /// The menu bar / tray icon exists.
    tray_ok: bool,
    /// The tray icon could not be created, so the Dock icon is shown this
    /// session regardless of the saved setting.
    tray_failed: bool,
    /// Switching the Dock / taskbar icon briefly takes focus away from the
    /// window; until then losing focus doesn't close the note or settings.
    keep_open_until: Option<Instant>,
    /// Slides the strip off the screen edge while it is not in use.
    auto_hide: AutoHide,
    /// Reveal the strip on auto-hide's next step, without the dwell.
    force_reveal: bool,
    /// The deadline a wake-up is already scheduled for.
    auto_hide_wake: Option<Instant>,
    /// Where the cursor was last seen, by an event or (while passthrough
    /// is on) a poll; auto-hide goes by it.
    pointer: Option<Point>,
    /// Files are being dragged over the window.
    file_hover: bool,
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

/// A finished delete that can still be undone.
struct LastDelete {
    deleted: Deleted,
    /// The note's bar pulsed for a fired reminder.
    pulsing: bool,
    /// When the delete finished.
    at: Instant,
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
                    Some(id) => window::monitor_size(id).then(move |m| {
                        window::run(id, platform::top_inset)
                            .map(move |inset| Message::WindowReady(id, m, inset))
                    }),
                    None => Task::none(),
                }),
                iced::system::theme().map(|m| Message::ThemeChanged(m.into())),
                // Reminders missed while the app was closed fire now.
                Task::done(Message::ReminderTick),
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
        let export_morph = Morph::new(s.motion.speed);
        let mode_fade = Self::settled_fade(s.motion.speed);
        let window_size = s
            .window
            .edge
            .size(600.0, Self::docked_width(s, SUPPORTS_PASSTHROUGH, false));
        let data_dir = store.dir().to_path_buf();
        let strip_entries = strip_model::entries(store.notes());
        let strip_progress = strip_model::progress(store.notes(), &strip_entries);
        let entries_synced = store.revision();
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
            morph,
            anchor_y: 0.0,
            pending_delete: None,
            collapsing: None,
            mode_fade,
            color_picker_open: false,
            color_hex: String::new(),
            color_bubble_rect: None,
            text_color_picker_open: false,
            reminder_picker: None,
            note_hovered: false,
            drag: None,
            scroll_offset: 0.0,
            last_deleted: None,
            copied_at: None,
            copies: 0,
            clipboard_empty_at: None,
            animating: false,
            last_tick: None,
            pulsing: HashSet::new(),
            pulse_phase: 0.0,
            jump_settle: None,
            visible: true,
            window_id: None,
            monitor: None,
            top_inset: 0.0,
            window_size,
            window_origin: None,
            passthrough: false,
            mouse_down: false,
            last_cursor: None,
            note_drag: None,
            note_resize: None,
            note_drag_pos: None,
            hover_bar: None,
            peek_note: None,
            peek_scroll: None,
            peek,
            settings_open: false,
            settings_morph,
            settings_anchor_y: 0.0,
            settings_tab: SettingsTab::default(),
            actions_open: false,
            palette_slot: None,
            search_open: false,
            search_morph,
            export: None,
            export_morph,
            export_anchor_y: 0.0,
            export_generation: 0,
            export_dialog_open: false,
            search_anchor_y: 0.0,
            search_query: String::new(),
            search_results: Vec::new(),
            search_matches: HashSet::new(),
            search_synced: None,
            strip_entries,
            strip_progress,
            entries_synced,
            pending_focus: None,
            hotkey: None,
            hotkey_error: None,
            tray_ok: false,
            tray_failed: false,
            keep_open_until: None,
            // Always starts Shown; the grace applies from the first step.
            auto_hide: AutoHide::default(),
            force_reveal: false,
            auto_hide_wake: None,
            pointer: None,
            file_hover: false,
        }
    }

    /// The screen edge saved in the settings, for the window's first
    /// placement before the app boots.
    pub fn saved_edge() -> Edge {
        SettingsStore::load(data_dir().join("settings.json"))
            .settings()
            .window
            .edge
    }

    pub fn theme_mode(&self) -> theme::Mode {
        self.theme.mode
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        // A tick steps auto-hide itself, by its own frame time.
        let tick = matches!(message, Message::Tick(_));
        let task = self.handle(message);
        self.sync_entries();
        self.sync_search();
        if tick {
            return task;
        }
        let auto_hide = self.drive_auto_hide(Instant::now(), 0.0);
        Task::batch([task, auto_hide])
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
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
            Message::BarClicked(entry) => {
                if let Some(index) = self.clicked_note(entry) {
                    return self.open_note(index);
                }
            }
            Message::ActionsHovered(open) => {
                self.actions_open = open;
            }
            Message::ToggleSettings => {
                if self.settings_open && self.settings_morph.is_opening() {
                    return self.update(Message::CloseSettings);
                }
                self.force_reveal = true;
                let close_note = self.update(Message::ClosePanel);
                self.close_search();
                self.close_export();
                self.hide_peek();
                // Still folding away: unfold again from where it is.
                if !self.settings_open {
                    let gear = self.strip_layout().settings_anchor();
                    self.settings_anchor_y = along_center(gear, self.edge());
                    self.settings_open = true;
                    self.settings_tab = SettingsTab::default();
                    self.palette_slot = None;
                }
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
            Message::EdgeChosen(edge) => return self.change_edge(edge),
            Message::ReminderAlertChosen(style) => {
                self.settings.settings_mut().motion.reminder_alert = style;
                self.jump_settle = None;
                self.animating |= self.pulse_running();
                return self.apply_settings();
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
                }
                if group == SettingsGroup::App {
                    self.keep_open_until = Some(Instant::now() + FOCUS_GRACE);
                    self.sync_hotkey();
                    return self.apply_app_visibility();
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
                        reminder::retitle(note, title, chrono::Local::now());
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
            Message::ClosePanel => {
                if self.active_note.is_some() {
                    self.morph.close();
                    self.animating = true;
                    self.color_picker_open = false;
                    self.text_color_picker_open = false;
                    self.reminder_picker = None;
                }
            }
            Message::DeleteNote(id) => {
                if self.active_note == Some(id) {
                    // Fold the note back into its bar first, then delete it.
                    // Only the latest delete can be undone.
                    self.last_deleted = None;
                    self.pending_delete = Some(id);
                    self.morph.close();
                    self.animating = true;
                }
            }
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
                    | self.export_morph.tick(dt)
                    | self.mode_fade.tick(dt);
                let collapse_active = self.collapsing.as_mut().is_some_and(|(_, m)| m.tick(dt));
                let collapsed = if self.collapsing.is_some() && !collapse_active {
                    self.finish_collapse()
                } else {
                    Task::none()
                };
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
                let export_closed = self.export.is_some() && self.export_morph.is_closed();
                if export_closed {
                    self.export = None;
                }
                // Hidden notes don't alert; they resume once shown.
                let pulse_active = self.pulse_running();
                if pulse_active {
                    let period = match self.settings.settings().motion.reminder_alert {
                        crate::settings::ReminderAlertStyle::Jump => JUMP_SECS,
                        crate::settings::ReminderAlertStyle::Pulse => PULSE_SECS,
                    };
                    self.pulse_phase = (self.pulse_phase + dt) % period;
                }
                let settle_active = self.jump_settle_active();
                let shake_active = self.shake_secs(now).is_some();
                let auto_hide = self.drive_auto_hide(now, dt);
                let slide_active = matches!(
                    self.auto_hide.phase(),
                    autohide::Phase::Hiding | autohide::Phase::Revealing
                );
                self.animating = mag_active
                    || morph_active
                    || collapse_active
                    || pulse_active
                    || settle_active
                    || shake_active
                    || slide_active;
                if !self.animating {
                    self.last_tick = None;
                }

                let focus = Task::batch([collapsed, self.take_pending_focus(), auto_hide]);
                if self.active_note.is_some() && self.morph.is_closed() {
                    return Task::batch([focus, self.finish_close()]);
                }
                if settings_closed || search_closed || export_closed {
                    // Without passthrough the window shrinks back to the strip.
                    return Task::batch([focus, self.dock_window()]);
                }
                return focus;
            }
            Message::ReminderTick => return self.fire_reminders(chrono::Local::now()),
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
            Message::ToggleReminderPicker => {
                if self.reminder_picker.is_some() {
                    self.reminder_picker = None;
                } else {
                    self.open_reminder_picker();
                }
            }
            Message::OpenReminderPicker => self.open_reminder_picker(),
            Message::ReminderPickerDay(date) => {
                if let Some(draft) = &mut self.reminder_picker {
                    draft.date = date;
                    draft.view_month = chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
                        .expect("valid month");
                }
            }
            Message::ReminderPickerMonth(delta) => {
                if let Some(draft) = &mut self.reminder_picker {
                    crate::reminder_picker::shift_month(draft, delta);
                }
            }
            Message::ReminderPickerHour(delta) => {
                if let Some(draft) = &mut self.reminder_picker {
                    crate::reminder_picker::adjust_time(draft, delta, 0);
                }
            }
            Message::ReminderPickerMinute(delta) => {
                if let Some(draft) = &mut self.reminder_picker {
                    crate::reminder_picker::adjust_time(draft, 0, delta);
                }
            }
            Message::ReminderPickerDone => {
                let Some(draft) = self.reminder_picker.take() else {
                    return Task::none();
                };
                let Some(at) = draft.at() else {
                    return Task::none();
                };
                let Some(id) = self.active_note else {
                    return Task::none();
                };
                let title = self
                    .store
                    .notes()
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| reminder::with_reminder(&n.title, at))
                    .unwrap_or_else(|| reminder::with_reminder("", at));
                if let Some(note) = self.store.note_mut(id) {
                    reminder::retitle(note, title, chrono::Local::now());
                    note.reminder_fired = None;
                    note.updated_at = chrono::Utc::now();
                }
                self.pulsing.remove(&id);
                self.store.mark_dirty();
            }
            Message::ReminderPickerClear => {
                let Some(id) = self.active_note else {
                    self.reminder_picker = None;
                    return Task::none();
                };
                self.reminder_picker = None;
                let title = self
                    .store
                    .notes()
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| reminder::without_reminder(&n.title))
                    .unwrap_or_default();
                if let Some(note) = self.store.note_mut(id) {
                    reminder::retitle(note, title, chrono::Local::now());
                    note.reminder_fired = None;
                    note.updated_at = chrono::Utc::now();
                }
                self.pulsing.remove(&id);
                self.store.mark_dirty();
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
            Message::StripFileDropped(entry, path) => return self.drop_on_strip(entry, &path),
            Message::FileHovered => {
                self.file_hover = true;
                self.last_cursor = None;
            }
            Message::FileHoverEnded => self.file_hover = false,
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
                let settings_showing = self.settings_open && self.settings_morph.is_opening();
                let search_showing = self.search_open && self.search_morph.is_opening();
                let export_showing = self.export.is_some() && self.export_morph.is_opening();
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
                    keyboard::Key::Named(Named::Escape) if self.color_picker_open => {
                        self.color_picker_open = false
                    }
                    keyboard::Key::Named(Named::Escape) if self.reminder_picker.is_some() => {
                        self.reminder_picker = None
                    }
                    keyboard::Key::Named(Named::Escape) if self.search_morph.is_opening() => {
                        self.close_search()
                    }
                    keyboard::Key::Named(Named::Escape)
                        if self.export.is_some() && self.export_morph.is_opening() =>
                    {
                        self.close_export()
                    }
                    keyboard::Key::Named(Named::Escape) if self.settings_open => {
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
                let room = self.note_room();
                if let Some(resize) = &mut self.note_resize {
                    resize.rect = resized(resize.start, resize.edges, position - resize.grab, room);
                }
                // Leaving the open peek (not toward the strip) closes it.
                let in_strip = self.near_edge(position, STRIP_WIDTH);
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
                // The save dialog takes the focus as long as it is open.
                if !self.export_dialog_open {
                    self.close_export();
                }
                let close_settings = self.update(Message::CloseSettings);
                return Task::batch([close_settings, self.update(Message::ClosePanel)]);
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
            // `update` steps auto-hide after every message; this one only
            // lets the next wake-up be scheduled.
            Message::AutoHideWake(deadline) => {
                if self.auto_hide_wake == Some(deadline) {
                    self.auto_hide_wake = None;
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // The strip borrows the cached entries, which `update` keeps synced.
        debug_assert_eq!(self.entries_synced, self.store.revision());
        if !self.visible {
            return Space::new().width(Fill).height(Fill).into();
        }
        let strip = container(BarStrip {
            notes: self.store.notes(),
            entries: &self.strip_entries,
            progress: &self.strip_progress,
            magnification: &self.magnification,
            drag: &self.drag,
            scroll_offset: self.scroll_offset,
            peek: self.strip_peek(),
            peek_scroll: self.peek_body_scroll(),
            bars: &self.settings.settings().bars,
            height_fraction: self.strip_fraction(),
            paper_tint: self.settings.settings().notes.paper_tint,
            default_note_width: self.settings.settings().notes.size,
            theme: self.theme,
            open: self
                .active_note
                .and_then(|id| self.id_entry(id))
                .map(|entry| (entry, self.morph.progress())),
            collapse: self.collapse(),
            dimmed: self.dimmed_bars(),
            pulse: self.bar_pulse(),
            jump: self.bar_jump(),
            alert: self.bar_alert(),
            add_shake: self.shake_secs(Instant::now()).map_or(0.0, add_shake),
            clipboard_hint: self.clipboard_empty_at.is_some(),
            toast: self.toast_visible(),
            panel_open: self.panel_open(),
            actions_expanded: self.actions_open,
            x_offset: self.strip_x_offset(),
            edge: self.edge(),
        })
        .width(Fill)
        .height(Fill);
        let strip = match self.edge() {
            Edge::Right => strip.align_x(iced::Alignment::End),
            Edge::Left | Edge::Top => strip,
        };

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
                        copied: self.copied_for(note.id),
                        pinned: self.top_pinned(note),
                        color_picker_open: self.color_picker_open,
                        color_hex: &self.color_hex,
                        text_color_picker_open: self.text_color_picker_open,
                        hovered: self.note_hovered,
                        dragging: self.note_drag.is_some(),
                        mode_fade: self.mode_fade.progress(),
                        reminder: reminder::status(note),
                        reminder_draft: self.reminder_picker.as_ref(),
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
                    tab: self.settings_tab,
                    selected_slot: self.palette_slot,
                    tray_ok: self.tray_ok,
                    dock_forced: self.tray_failed,
                    hotkey_error: self.hotkey_error.as_deref(),
                });
                layers.push(pin(opaque(panel)).x(rect.x).y(rect.y).into());
            }
        }

        if let Some(frame) = self.search_frame() {
            let rect = frame.rect;
            if rect.width >= 1.0 && rect.height >= 1.0 {
                // Only while unfolding: a folding panel mustn't block clicks
                // on the note opening in its place.
                if self.search_morph.is_opening() {
                    layers.push(
                        mouse_area(Space::new().width(Fill).height(Fill))
                            .on_press(Message::CloseSearch)
                            .into(),
                    );
                }
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

        if let (Some(frame), Some(state)) = (self.export_frame(), &self.export) {
            let rect = frame.rect;
            if rect.width >= 1.0 && rect.height >= 1.0 {
                // Only while unfolding, as for the search. While the save
                // dialog is open a click beside the panel does nothing.
                if self.export_morph.is_opening() {
                    let backdrop = Space::new().width(Fill).height(Fill);
                    layers.push(if self.export_dialog_open {
                        opaque(backdrop)
                    } else {
                        mouse_area(backdrop).on_press(Message::ToggleExport).into()
                    });
                }
                let panel = export_panel(ExportView {
                    theme: self.theme,
                    notes: self.store.notes(),
                    state,
                    can_export: self.export_enabled(),
                    size: rect.size(),
                    morph_progress: self.export_morph.progress(),
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
            iced::Event::Window(window::Event::FileHovered(_)) => Some(Message::FileHovered),
            iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Message::FileHoverEnded),
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
        // Always on: the hotkey's event handler can only be installed once.
        subs.push(hotkey::events().map(|()| Message::HotkeyPressed));
        if self.hover_bar.is_some() && !self.peek.is_opening() {
            subs.push(iced::time::every(PEEK_POLL).map(Message::PeekTick));
        }
        if self.reminders_pending() {
            subs.push(iced::time::every(REMINDER_POLL).map(|_| Message::ReminderTick));
        }
        Subscription::batch(subs)
    }

    fn open_note(&mut self, index: usize) -> Task<Message> {
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
    fn open_note_id(&mut self, id: Uuid) -> Task<Message> {
        // A deleted note can't be opened while its bar collapses.
        if self.dying(id) {
            return Task::none();
        }

        if self.active_note == Some(id) && self.morph.is_opening() {
            return self.update(Message::ClosePanel);
        }

        let switching = self.active_note.is_some_and(|active| active != id);
        if switching {
            // A drag or resize of the old note ends with it.
            if self.note_drag.is_some() {
                self.finish_note_drag();
            }
            self.finish_note_resize();
        }
        // Starting the collapse may have removed a note, moving this one.
        let Some(index) = self.store.notes().iter().position(|n| n.id == id) else {
            return Task::none();
        };
        let note = &self.store.notes()[index];
        self.pulsing.remove(&id);
        if self.jump_settle.is_some_and(|(sid, _, _)| sid == id) {
            self.jump_settle = None;
        }
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
        self.close_export();
        self.active_note = Some(id);
        self.reparse();
        self.color_picker_open = false;
        self.text_color_picker_open = false;
        self.reminder_picker = None;
        if let Some(bar) = self
            .note_entry(index)
            .and_then(|entry| self.strip_layout().bars.get(entry).copied())
        {
            self.anchor_y = along_center(bar, self.edge());
        }
        if switching {
            self.morph.restart();
        } else {
            self.morph.open();
        }
        self.animating = true;
        self.dock_window()
    }

    /// Gives the open note `color`; it is saved after the usual debounce.
    fn recolor_open_note(&mut self, color: NoteColor) {
        let Some(note) = self.active_note.and_then(|id| self.store.note_mut(id)) else {
            return;
        };
        note.color = color;
        note.updated_at = chrono::Utc::now();
        self.store.mark_dirty();
    }

    /// Wraps the editor selection in `format`'s markup, writes the result
    /// back to the note and leaves the wrapped text selected.
    fn apply_format(&mut self, format: rich::Format) -> Task<Message> {
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
    fn create_note(&mut self, title: Option<String>, content: String) -> Task<Message> {
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
    fn note_open(&self) -> bool {
        self.active_note.is_some() && self.morph.is_opening()
    }

    /// A file dropped with no note open: appended to the note the bar at
    /// `entry` stands for, or else a new note titled with the file's stem.
    fn drop_on_strip(&mut self, entry: Option<usize>, path: &std::path::Path) -> Task<Message> {
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
    fn dropped_text(&self, path: &std::path::Path) -> String {
        if let Some(text) = read_text_file(path) {
            return text;
        }
        match images::import(&self.data_dir, path) {
            Ok(rel) => format!("![]({rel})"),
            Err(_) => path.display().to_string(),
        }
    }

    /// Adds `text` below the note's body, after a blank line unless the
    /// body is empty. Blank text adds nothing.
    fn append_to_note(&mut self, id: Uuid, text: &str) {
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
    }

    /// A new note with the clipboard's text or image; nothing for an empty
    /// clipboard.
    fn clipboard_note(&mut self, content: ClipboardContent) -> Task<Message> {
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
    fn shake_secs(&self, now: Instant) -> Option<f32> {
        let t = now
            .saturating_duration_since(self.clipboard_empty_at?)
            .as_secs_f32();
        (t < SHAKE_SECS).then_some(t)
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
        self.reminder_picker = None;
        self.note_resize = None;
        let collapse = match self.pending_delete.take() {
            Some(id) => self.start_collapse(id),
            None => Task::none(),
        };
        Task::batch([collapse, self.dock_window()])
    }

    /// Opens the reminder calendar for the active note, prefilling from its
    /// current trigger or the next whole hour.
    fn open_reminder_picker(&mut self) {
        let Some(id) = self.active_note else {
            return;
        };
        let now = chrono::Local::now();
        let draft = self
            .store
            .notes()
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| reminder::at(n).map(ReminderDraft::from_at))
            .unwrap_or_else(|| ReminderDraft::from_now(now));
        self.reminder_picker = Some(draft);
        self.color_picker_open = false;
        self.text_color_picker_open = false;
        self.color_bubble_rect = None;
    }

    /// Starts shrinking the deleted note's bar away. A collapse still
    /// running for another note finishes at once (see `finish_collapse`
    /// for the task it returns).
    fn start_collapse(&mut self, id: Uuid) -> Task<Message> {
        let finished = self.finish_collapse();
        // Only the latest delete can be undone, so not one before this.
        self.last_deleted = None;
        let mut morph = Morph::with_secs(COLLAPSE_SECS, self.settings.settings().motion.speed);
        morph.restart();
        self.collapsing = Some((id, morph));
        if self.peek_note == Some(id) || self.hover_bar.is_some_and(|(hovered, _)| hovered == id) {
            self.hide_peek();
        }
        self.animating = true;
        finished
    }

    /// Fires every reminder due at `now`: a notification, an alerting bar,
    /// and the fired time saved at once so it never fires twice. Alerting
    /// bars stay visible while the strip may remain auto-hidden.
    fn fire_reminders(&mut self, now: chrono::DateTime<chrono::Local>) -> Task<Message> {
        let due: Vec<_> = self
            .store
            .notes()
            .iter()
            .filter_map(|note| Some((note.id, reminder::due(note, now)?)))
            .collect();
        if due.is_empty() {
            return Task::none();
        }
        let had_alert = !self.pulsing.is_empty();
        for (id, time) in due {
            let Some(note) = self.store.note_mut(id) else {
                continue;
            };
            note.reminder_fired = Some(time);
            notify::reminder(&reminder::display(&note.title));
            // The open note is already in view.
            if !(self.active_note == Some(id) && self.morph.is_opening()) {
                self.begin_alert(id);
            }
        }
        let _ = self.store.save();
        self.store.did_save();
        self.animating |= self.pulse_running();
        // Widen off the edge sliver so alerting bars have room to draw.
        if !had_alert && !self.pulsing.is_empty() {
            return self.dock_window();
        }
        Task::none()
    }

    /// Starts (or joins) the alert animation for `id`.
    fn begin_alert(&mut self, id: Uuid) {
        if self.pulsing.is_empty() {
            // Jump starts mid-rise so the hop is visible immediately;
            // Pulse's cosine is already lit at phase 0.
            self.pulse_phase = match self.settings.settings().motion.reminder_alert {
                crate::settings::ReminderAlertStyle::Jump => JUMP_SECS * 0.25,
                crate::settings::ReminderAlertStyle::Pulse => 0.0,
            };
        }
        self.pulsing.insert(id);
    }

    /// Some bar alerts and the notes are on screen.
    fn pulse_running(&self) -> bool {
        self.visible && !self.pulsing.is_empty()
    }

    /// Some note has a reminder that hasn't fired yet.
    fn reminders_pending(&self) -> bool {
        self.store
            .notes()
            .iter()
            .any(|note| reminder::pending(note).is_some())
    }

    /// How strongly each bar pulses (0..=1); empty while nothing pulses or
    /// when the alert style is Jump. A stacked note's reminder pulses its
    /// stack's bar.
    fn bar_pulse(&self) -> Vec<f32> {
        use crate::settings::ReminderAlertStyle;
        if self.pulsing.is_empty()
            || self.settings.settings().motion.reminder_alert != ReminderAlertStyle::Pulse
        {
            return Vec::new();
        }
        let wave = 0.5 - 0.5 * (std::f32::consts::TAU * self.pulse_phase / PULSE_SECS).cos();
        let amount = 0.25 + 0.75 * wave;
        let notes = self.store.notes();
        self.entries()
            .iter()
            .map(|entry| {
                let pulses = std::iter::once(entry.top)
                    .chain(entry.members.iter().copied())
                    .any(|i| self.pulsing.contains(&notes[i].id));
                if pulses {
                    amount
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// Which bars have an uncleared fired reminder (for alert-only draw/hit
    /// while the strip is auto-hidden).
    fn bar_alert(&self) -> Vec<bool> {
        if self.pulsing.is_empty() {
            return Vec::new();
        }
        let notes = self.store.notes();
        self.entries()
            .iter()
            .map(|entry| {
                std::iter::once(entry.top)
                    .chain(entry.members.iter().copied())
                    .any(|i| self.pulsing.contains(&notes[i].id))
            })
            .collect()
    }

    /// Away-from-edge jump offset per bar; empty while nothing alerts or when
    /// the alert style is Pulse. Hover settles that bar to rest.
    fn bar_jump(&self) -> Vec<f32> {
        use crate::settings::ReminderAlertStyle;
        if self.pulsing.is_empty()
            || self.settings.settings().motion.reminder_alert != ReminderAlertStyle::Jump
        {
            return Vec::new();
        }
        let amount = jump_amount(self.pulse_phase);
        let notes = self.store.notes();
        let hover_id = self.hover_bar.map(|(id, _)| id);
        self.entries()
            .iter()
            .map(|entry| {
                let ids: Vec<Uuid> = std::iter::once(entry.top)
                    .chain(entry.members.iter().copied())
                    .map(|i| notes[i].id)
                    .collect();
                let jumps = ids.iter().any(|id| self.pulsing.contains(id));
                if !jumps {
                    return 0.0;
                }
                if let Some((sid, since, from)) = self.jump_settle {
                    if ids.contains(&sid) {
                        return jump_settle_offset(from, since.elapsed().as_secs_f32());
                    }
                }
                if hover_id.is_some_and(|id| ids.contains(&id) && self.pulsing.contains(&id)) {
                    return 0.0;
                }
                amount
            })
            .collect()
    }

    /// Removes the collapsing note (if any) for good and shows the undo
    /// toast for it, replacing any earlier one. The task ends the toast
    /// after `UNDO_FOR` and docks the window, which widens for it.
    fn finish_collapse(&mut self) -> Task<Message> {
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
    fn collapse(&self) -> Option<(usize, f32)> {
        let (id, morph) = self.collapsing.as_ref()?;
        let entry = self.id_entry(*id)?;
        if !self.entries()[entry].members.is_empty() {
            return None;
        }
        Some((entry, ease_out_cubic(morph.progress())))
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

    /// Window thickness away from the edge (the width on Right and Left,
    /// the height on Top): room for an open note when `wide` (see
    /// `needs_wide_window`), else the edge sliver when `sliver`, else just
    /// the strip. With passthrough the window is always wide, so opening and
    /// closing never resize it (a resize is a separate move + resize and
    /// flickers for a frame).
    fn docked_width(settings: &Settings, wide: bool, sliver: bool) -> f32 {
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
    fn wants_sliver(&self, passthrough: bool) -> bool {
        !passthrough && self.auto_hide.is_hidden() && !self.needs_wide_window(passthrough)
    }

    /// The window is currently the edge sliver.
    fn is_sliver(&self) -> bool {
        self.edge().thickness(self.window_size) <= EDGE_SLIVER
    }

    /// Whether the window needs room on the strip's away side: always with
    /// `passthrough`, otherwise for an open note or panel, a chip that
    /// stays up on its own, such as the undo toast (hover chips don't
    /// widen it), or an alerting bar that must stay visible while the
    /// strip is otherwise auto-hidden.
    fn needs_wide_window(&self, passthrough: bool) -> bool {
        passthrough
            || self.active_note.is_some()
            || self.settings_open
            || self.search_open
            || self.export.is_some()
            || self.clipboard_empty_at.is_some()
            || self.last_deleted.is_some()
            || !self.pulsing.is_empty()
    }

    /// The docked window's thickness away from the edge (see
    /// `docked_width`), with or without `passthrough`.
    fn window_width(&self, passthrough: bool) -> f32 {
        Self::docked_width(
            self.settings.settings(),
            self.needs_wide_window(passthrough),
            self.wants_sliver(passthrough),
        )
    }

    /// Docks the window to the chosen screen edge, centred along it. The
    /// strip is centred inside, so it sits at the middle of that screen
    /// border. Does nothing if the window is already there.
    fn dock_window(&mut self) -> Task<Message> {
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
            self.window_width(SUPPORTS_PASSTHROUGH),
            self.settings.settings().window.height_fraction,
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
            self.pointer = pointer_after_resize(
                self.pointer,
                edge,
                edge.thickness(self.window_size),
                edge.thickness(size),
            );
        }
        self.window_size = size;
        self.window_origin = Some(origin);
        Task::batch([window::move_to(id, origin), window::resize(id, size)])
    }

    /// Whether the window should catch the mouse at `position`: over the bar
    /// stack, the open note, the settings or the undo toast. Everywhere else
    /// clicks go to the apps behind.
    /// While auto-hide has the strip (partly) slid away, only the edge
    /// zone, alerting bars (and their peek), and an open note or panel
    /// count.
    fn is_interactive(&self, position: Point) -> bool {
        if !self.visible {
            return false;
        }
        let over_panel = |frame: Option<MorphFrame>| {
            frame.is_some_and(|frame| frame.rect.expand(8.0).contains(position))
        };
        let strip_used = if self.strip_x_offset() > 0.0 {
            self.in_edge(position) || self.over_alerting_ui(position)
        } else {
            self.in_use_area(position)
        };
        // The color bubble can reach beyond the note.
        let over_bubble = self.color_picker_open
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

    /// The screen edge the strip docks to.
    fn edge(&self) -> Edge {
        self.settings.settings().window.edge
    }

    /// `position`'s coordinate along the edge.
    fn along(&self, position: Point) -> f32 {
        self.edge().to_local(position, self.window_size).along
    }

    /// Whether `position` is at most `depth` from the window's border on
    /// the strip's edge (or beyond it). On Top a negative y, in the menu bar
    /// above the window, counts too: the menu bar is part of the edge zone.
    fn near_edge(&self, position: Point, depth: f32) -> bool {
        match self.edge() {
            Edge::Right => position.x >= self.window_size.width - depth,
            Edge::Left => position.x <= depth,
            Edge::Top => position.y <= depth,
        }
    }

    /// Whether `along` (a coordinate along the edge) is within the strip's
    /// band.
    fn in_band(&self, along: f32) -> bool {
        let (start, end) = strip_band(&self.strip_layout(), self.settings.settings().bars.gap);
        (start..=end).contains(&along)
    }

    /// Whether `position` is over the strip's column (its row on Top)
    /// within its band.
    fn over_strip(&self, position: Point) -> bool {
        self.near_edge(position, STRIP_WIDTH) && self.in_band(self.along(position))
    }

    /// Whether `position` is in the edge zone: the window's outermost
    /// `EDGE_ZONE` px on the strip's edge, within the strip's band.
    ///
    /// In the edge sliver that is anywhere inside the window, within the band.
    fn in_edge(&self, position: Point) -> bool {
        if self.is_sliver() {
            let edge = self.edge();
            let across = match edge {
                Edge::Right | Edge::Left => position.x,
                Edge::Top => position.y,
            };
            return (0.0..=edge.thickness(self.window_size)).contains(&across)
                && self.in_band(self.along(position));
        }
        self.near_edge(position, EDGE_ZONE) && self.over_strip(position)
    }

    /// Whether `position` is where the strip is in use: its column within
    /// the band, the open peek or the undo toast.
    fn in_use_area(&self, position: Point) -> bool {
        self.over_strip(position)
            || self.peek_rect().is_some_and(|rect| rect.contains(position))
            || self
                .toast_rect()
                .is_some_and(|rect| rect.contains(position))
    }

    /// Whether `position` is over a bar (or its peek) that is alerting
    /// while the strip is otherwise slid away.
    fn over_alerting_ui(&self, position: Point) -> bool {
        if self.pulsing.is_empty() {
            return false;
        }
        if self
            .peek_rect()
            .is_some_and(|rect| rect.contains(position))
        {
            return self
                .peek_note
                .is_some_and(|id| self.pulsing.contains(&id));
        }
        let alerts = self.bar_alert();
        self.strip_layout()
            .bars
            .iter()
            .enumerate()
            .any(|(i, bar)| alerts.get(i).copied().unwrap_or(false) && bar.contains(position))
    }

    /// What auto-hide goes by now. An open note doesn't block hiding.
    fn auto_hide_inputs(&self) -> autohide::Inputs {
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
    fn drive_auto_hide(&mut self, now: Instant, dt: f32) -> Task<Message> {
        let was_in_place = self.strip_x_offset() == 0.0;
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
        if was_in_place != (self.strip_x_offset() == 0.0) {
            tasks.push(self.update_passthrough(self.pointer));
        }
        Task::batch(tasks)
    }

    /// How far toward its screen edge the strip is drawn while auto-hide
    /// slides it away (an away offset; see `hide_translation`).
    fn strip_x_offset(&self) -> f32 {
        self.auto_hide.offset() * HIDE_SHIFT
    }

    /// Where the strip's `rect` is drawn while auto-hide slides it away.
    fn slid(&self, rect: Rectangle) -> Rectangle {
        rect + hide_translation(self.edge(), self.strip_x_offset())
    }

    /// Search, settings or export is open.
    fn panel_open(&self) -> bool {
        self.search_open || self.settings_open || self.export.is_some()
    }

    /// The undo toast is up and no panel covers it.
    fn toast_visible(&self) -> bool {
        self.last_deleted.is_some() && !self.panel_open()
    }

    /// The undo toast's area, while the strip draws it: not under a panel,
    /// nor while the peek, a drag or the empty-clipboard chip hides it.
    fn toast_rect(&self) -> Option<Rectangle> {
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

    /// The peeked bar and how far its peek is open, as the strip shows it.
    fn strip_peek(&self) -> Option<(usize, f32)> {
        self.peek_note
            .and_then(|id| self.id_entry(id))
            .map(|entry| (entry, self.peek.progress()))
    }

    /// How far the open peek's body is scrolled: 0 for a note not scrolled
    /// since it was last peeked.
    fn peek_body_scroll(&self) -> f32 {
        self.peek_scroll
            .filter(|(id, _)| self.peek_note == Some(*id))
            .map_or(0.0, |(_, scroll)| scroll)
    }

    /// Whether `id`'s copy button still shows its check.
    fn copied_for(&self, id: Uuid) -> bool {
        self.copied_at.is_some_and(|(copied, _)| copied == id)
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
            (Some(y), None, None)
                if !self.settings_open && !self.search_open && self.export.is_none() =>
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

    /// Pauses a jumping alert when the cursor enters its bar: settle to rest
    /// then hold; clearing hover resumes the hop.
    fn sync_jump_settle(&mut self, hovered: Option<Uuid>) {
        use crate::settings::ReminderAlertStyle;
        if self.settings.settings().motion.reminder_alert != ReminderAlertStyle::Jump {
            self.jump_settle = None;
            return;
        }
        match hovered {
            Some(id) if self.pulsing.contains(&id) => {
                let from = jump_amount(self.pulse_phase);
                self.jump_settle = Some((id, Instant::now(), from));
            }
            _ => self.jump_settle = None,
        }
    }

    fn jump_settle_active(&self) -> bool {
        self.jump_settle
            .as_ref()
            .is_some_and(|(_, since, from)| {
                *from != 0.0 && since.elapsed().as_secs_f32() < JUMP_SETTLE_SECS
            })
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

    fn hide_peek(&mut self) {
        self.hover_bar = None;
        self.jump_settle = None;
        self.peek_note = None;
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
        let source = self.strip_layout().search_anchor();
        Some(self.panel_frame(source, self.search_anchor_y, &self.search_morph))
    }

    /// The export panel's current frame, morphing out of the gear slot like
    /// the settings it replaces.
    fn export_frame(&self) -> Option<MorphFrame> {
        self.export.as_ref()?;
        let source = self.strip_layout().settings_anchor();
        Some(self.panel_frame(source, self.export_anchor_y, &self.export_morph))
    }

    /// A panel's frame on its way from the strip slot `source` to its place
    /// on the strip's away side, centered on `anchor_y` (a coordinate along
    /// the edge) where it fits.
    fn panel_frame(&self, source: Rectangle, anchor_y: f32, morph: &Morph) -> MorphFrame {
        let room = match self.edge() {
            Edge::Right | Edge::Left => self.window_size.height - 2.0 * NOTE_MARGIN,
            // Below the strip, its height is what is left under it.
            Edge::Top => self.window_size.height - STRIP_WIDTH - NOTE_GAP - NOTE_MARGIN,
        };
        let height = room.min(PANEL_MAX_HEIGHT);
        let target = self.beside_strip(Size::new(PANEL_WIDTH, height), anchor_y);
        morph_frame(self.slid(source), target, morph.progress())
    }

    /// A box of `size` on the strip's away side, `NOTE_GAP` from the strip,
    /// centered along the edge on `anchor` where it fits within the
    /// margins, else in the middle.
    fn beside_strip(&self, size: Size, anchor: f32) -> Rectangle {
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
                away: STRIP_WIDTH + NOTE_GAP,
                length,
                thickness: edge.thickness(size),
            },
            self.window_size,
        )
    }

    /// Opens the search panel, or closes it while it is showing. It closes
    /// the note and the settings, and starts with an empty query.
    fn toggle_search(&mut self) -> Task<Message> {
        self.force_reveal = true;
        if self.search_open && self.search_morph.is_opening() {
            self.close_search();
            return Task::none();
        }
        let close_note = self.update(Message::ClosePanel);
        let close_settings = self.update(Message::CloseSettings);
        self.close_export();
        self.hide_peek();
        // Still folding away: unfold again from where it is, query kept.
        if !self.search_open {
            let slot = self.strip_layout().search_anchor();
            self.search_anchor_y = along_center(slot, self.edge());
            self.search_open = true;
            self.search_query.clear();
        }
        self.search_morph.open();
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
    fn toggle_export(&mut self) -> Task<Message> {
        if self.export.is_some() && self.export_morph.is_opening() {
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
                self.export_anchor_y = along_center(gear, self.edge());
                self.export_generation += 1;
                self.export = Some(ExportState::new(self.store.notes(), self.export_generation));
            }
        }
        self.export_morph.open();
        self.animating = true;
        Task::batch([close_note, close_settings, self.dock_window()])
    }

    /// Folds the export panel back into the gear (if it is showing).
    fn close_export(&mut self) {
        if self.export.is_some() {
            self.export_morph.close();
            self.animating = true;
        }
    }

    /// Opens the save dialog for the selected notes, unless one is open
    /// already (from this panel or an earlier one) or none of them exists
    /// any more. The dialog's task carries the notes and the format, so the
    /// export happens even if the panel folds meanwhile.
    fn request_export(&mut self) -> Task<Message> {
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
    fn export_enabled(&self) -> bool {
        !self.export_dialog_open
            && self
                .export
                .as_ref()
                .is_some_and(|state| state.can_export(self.store.notes()))
    }

    /// The export panel, if it is the opening `generation` belongs to.
    fn export_opened_by(&mut self, generation: u64) -> Option<&mut ExportState> {
        self.export
            .as_mut()
            .filter(|state| state.generation == generation)
    }

    /// Writes the job's notes that still exist, in its order, and shows how
    /// that went in the panel that started it, or logs it once that panel
    /// is gone. A success shown there closes the panel `EXPORTED_FOR` later.
    fn run_export(&mut self, job: ExportJob) -> Task<Message> {
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

    /// Recomputes the search results while the panel is open and the
    /// query or the notes changed since they were computed.
    fn sync_search(&mut self) {
        if !self.search_open {
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
    fn search_hits(&self) -> &[Hit] {
        &self.search_results
    }

    /// Bars to dim, by index: while the search is open with a query, the
    /// entries none of whose notes match it. Empty otherwise.
    fn dimmed_bars(&self) -> Vec<bool> {
        if !self.search_open || self.search_query.trim().is_empty() {
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

    fn note_frame(&self) -> Option<MorphFrame> {
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

    /// Where the strip widget sits in the window: `STRIP_WIDTH` thick along
    /// the whole window border on its edge.
    fn strip_bounds(&self) -> Rectangle {
        let edge = self.edge();
        edge.rect_to_window(
            LocalRect {
                along: 0.0,
                away: 0.0,
                length: edge.edge_length(self.window_size),
                thickness: STRIP_WIDTH,
            },
            self.window_size,
        )
    }

    fn strip_layout(&self) -> StripLayout {
        let bounds = self.strip_bounds();
        let edge = self.edge();
        compute_layout(
            self.entries().len(),
            |i| self.magnification.scale(i),
            band(bounds, self.strip_fraction(), edge),
            self.scroll_offset,
            &self.settings.settings().bars,
            self.actions_open,
            self.collapse(),
            edge,
        )
    }

    /// Share of the window's length along the edge the strip uses. Without
    /// passthrough the window itself is already sized to the configured
    /// fraction.
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

    /// Registers or unregisters the global hotkey to match the setting.
    /// Only the macOS/Windows settings show the toggle; elsewhere it is
    /// always on. Waits for the window, so the event loop is running.
    fn sync_hotkey(&mut self) {
        if self.window_id.is_none() {
            return;
        }
        let wanted = !cfg!(any(windows, target_os = "macos"))
            || self.settings.settings().is_on(SettingToggle::GlobalHotkey);
        if !wanted {
            self.hotkey = None;
            self.hotkey_error = None;
        } else if self.hotkey.is_none() {
            self.apply_registration(hotkey::register());
        }
    }

    /// Keeps the registered hotkey, or the reason it failed.
    fn apply_registration(&mut self, result: Result<hotkey::Manager, String>) {
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

    /// Docks the strip to `edge` right away (E10).
    fn change_edge(&mut self, edge: Edge) -> Task<Message> {
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
    fn edge_changed(&mut self, old: Edge) -> Task<Message> {
        let edge = self.edge();
        debug_assert_ne!(old, edge, "the edge didn't change");
        self.hide_peek();
        self.cursor_y = None;
        self.drag = None;
        self.clipboard_empty_at = None;
        self.actions_open = false;
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
        self.settings_anchor_y = along_center(strip.settings_anchor(), edge);
        self.search_anchor_y = along_center(strip.search_anchor(), edge);
        self.export_anchor_y = along_center(strip.settings_anchor(), edge);
        Task::batch([dock, self.update_passthrough(self.pointer)])
    }

    /// Applies changed settings to running state: animation speeds, scroll
    /// bounds and the docked window size.
    fn apply_settings(&mut self) -> Task<Message> {
        let s = self.settings.settings();
        let speed = s.motion.speed;
        self.morph.set_speed(speed);
        self.peek.set_speed(speed);
        self.settings_morph.set_speed(speed);
        self.search_morph.set_speed(speed);
        self.export_morph.set_speed(speed);
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

        // Below a Top strip, the note keeps to the room left under it.
        let height = match self.edge() {
            Edge::Right | Edge::Left => height,
            Edge::Top => height.min(
                (self.window_size.height - STRIP_WIDTH - NOTE_GAP - NOTE_MARGIN)
                    .max(MIN_SIZE.height),
            ),
        };
        self.beside_strip(Size::new(width, height), self.anchor_y)
    }

    /// Bar centers plus the Actions slot(s), which magnify along with them.
    fn magnification_centers(&self) -> Vec<f32> {
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
    fn entries(&self) -> Cow<'_, [Entry]> {
        if self.entries_synced == self.store.revision() {
            Cow::Borrowed(&self.strip_entries)
        } else {
            Cow::Owned(strip_model::entries(self.store.notes()))
        }
    }

    fn sync_entries(&mut self) {
        if self.entries_synced != self.store.revision() {
            self.strip_entries = strip_model::entries(self.store.notes());
            self.strip_progress = strip_model::progress(self.store.notes(), &self.strip_entries);
            self.entries_synced = self.store.revision();
        }
    }

    /// The note bar `entry` stands for (the stack's top), by note index.
    /// While a stack's top is being deleted, its bar stands for the next
    /// note, which takes its place.
    fn entry_note(&self, entry: usize) -> Option<usize> {
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
    fn clicked_note(&self, entry: usize) -> Option<usize> {
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
    fn dying(&self, id: Uuid) -> bool {
        self.collapsing.as_ref().is_some_and(|(c, _)| *c == id)
    }

    /// The open note's bar may have moved; its panel follows.
    fn follow_open_bar(&mut self) {
        self.sync_entries();
        if let Some(bar) = self
            .active_note
            .and_then(|id| self.id_entry(id))
            .and_then(|entry| self.strip_layout().bars.get(entry).copied())
        {
            self.anchor_y = along_center(bar, self.edge());
        }
    }

    /// The bar holding the note at `index`: a stacked note's is its stack's.
    fn note_entry(&self, index: usize) -> Option<usize> {
        self.entries().iter().position(|e| e.holds(index))
    }

    /// Whether the note's stack (or the note itself) is pinned: a pinned top
    /// pins its whole stack.
    fn top_pinned(&self, note: &crate::note::Note) -> bool {
        match note.stack {
            Some(top) => self.store.notes().iter().any(|n| n.id == top && n.pinned),
            None => note.pinned,
        }
    }

    fn id_entry(&self, id: Uuid) -> Option<usize> {
        let index = self.store.notes().iter().position(|n| n.id == id)?;
        self.note_entry(index)
    }

    /// Maps moving bar `from` to slot `to` of the remaining bars onto the
    /// store: the top's note index and the note index its group lands at.
    fn reorder_notes(&self, from: usize, to: usize) -> Option<(usize, usize)> {
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

    fn bar_centers(&self) -> Vec<f32> {
        let strip = self.strip_layout();
        strip
            .bars
            .iter()
            .map(|bar| along_center(*bar, strip.edge))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autohide::REVEAL_DWELL;
    use crate::bar_strip::HIDE_MARGIN;
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
        open_peek(&mut app);
        app
    }

    /// Opens the peek of the first bar (peek delay 0).
    fn open_peek(app: &mut App) {
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::CursorMoved(bar.center()));
        let _ = app.update(Message::StripHover(Some(bar.center().y)));
        let _ = app.update(Message::PeekTick(Instant::now() + Duration::from_secs(1)));
        assert!(app.peek_note.is_some() && app.peek.is_opening());
    }

    #[test]
    fn peek_scroll_lasts_until_the_peek_opens_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        assert_eq!(app.peek_body_scroll(), 0.0);
        let _ = app.update(Message::PeekScroll(42.0));
        assert_eq!(app.peek_body_scroll(), 42.0);
        app.hide_peek();
        assert_eq!(app.peek_body_scroll(), 0.0);
        open_peek(&mut app);
        assert_eq!(app.peek_body_scroll(), 0.0);
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
    fn peek_trash_deletes_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let id = app.store.notes()[0].id;
        let _ = app.update(Message::PeekDelete(0));
        // No question first: the bar starts collapsing right away.
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(id));
        assert_eq!(app.peek_note, None);
        settle(&mut app);
        assert!(app.store.notes().is_empty());
        // Deleting saves at once, like deleting the open note.
        assert!(!app.store.is_dirty());
        assert_eq!(
            app.last_deleted.as_ref().map(|l| l.deleted.note.id),
            Some(id)
        );
    }

    #[test]
    fn peek_delete_only_for_the_peeked_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        app.store.add_note(&crate::note::PALETTE);
        let _ = app.update(Message::PeekDelete(1));
        assert!(app.collapsing.is_none());
        settle(&mut app);
        assert_eq!(app.store.notes().len(), 2);
    }

    #[test]
    fn peek_delete_needs_the_open_peek() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        app.hide_peek();
        let _ = app.update(Message::PeekDelete(0));
        settle(&mut app);
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
    fn header_trash_deletes_without_confirm() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "first");
        app.store.add_note(&crate::note::PALETTE);
        let id = app.store.notes()[0].id;
        let _ = app.update(Message::DeleteNote(id));
        // The note folds into its bar first; nothing asks.
        assert_eq!(app.pending_delete, Some(id));
        assert!(!app.morph.is_opening());
        tick_until(&mut app, |a| a.active_note.is_none());
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(id));
        assert!(app.store.notes().iter().any(|n| n.id == id));
        settle(&mut app);
        assert!(app.store.notes().iter().all(|n| n.id != id));
        assert_eq!(app.store.notes().len(), 1);
        assert!(app.collapsing.is_none());
        assert!(!app.store.is_dirty());
        assert_eq!(
            app.last_deleted.as_ref().map(|l| l.deleted.note.id),
            Some(id)
        );
    }

    #[test]
    fn header_trash_ignores_another_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "first");
        let other = app.store.add_note(&crate::note::PALETTE);
        let _ = app.update(Message::DeleteNote(other));
        assert_eq!(app.pending_delete, None);
        assert!(app.morph.is_opening());
    }

    #[test]
    fn peek_delete_collapses_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let id = app.store.notes()[0].id;
        let _ = app.update(Message::PeekDelete(0));
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
        let _ = app.update(Message::PeekDelete(0));
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(a));

        app.peek_note = Some(b);
        let _ = app.update(Message::PeekDelete(1));
        assert!(app.store.notes().iter().all(|n| n.id != a));
        assert_eq!(app.collapsing.as_ref().map(|(c, _)| *c), Some(b));
        assert_eq!(app.store.notes().len(), 1);
    }

    #[test]
    fn collapsing_bar_ignores_clicks() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        app.store.add_note(&crate::note::PALETTE);
        let _ = app.update(Message::PeekDelete(0));
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
    fn stacked_strip_opens_the_right_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 4);
        assert!(app.store.stack(ids[2], ids[1]));
        // Bars: 0, 1 (with 2 hidden under it), 3.
        assert_eq!(app.strip_layout().bars.len(), 3);
        assert_eq!(app.entry_note(2), Some(3));
        assert_eq!(app.note_entry(2), Some(1));
        assert_eq!(app.note_entry(3), Some(2));
        let _ = app.update(Message::BarClicked(2));
        assert_eq!(app.active_note, Some(ids[3]));
        let _ = app.update(Message::BarClicked(1));
        assert_eq!(app.active_note, Some(ids[1]));
    }

    #[test]
    fn reorder_moves_whole_stack() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 4);
        assert!(app.store.stack(ids[1], ids[0]));
        let order = |app: &App| app.store.notes().iter().map(|n| n.id).collect::<Vec<_>>();
        // The stack's bar dragged below the last bar.
        let bars = app.strip_layout().bars;
        let _ = app.update(Message::DragStart(0, bars[0].center().y));
        let _ = app.update(Message::DragMove(bars[2].y + bars[2].height + 1.0));
        let _ = app.update(Message::DragEnd);
        assert_eq!(order(&app), vec![ids[2], ids[3], ids[0], ids[1]]);
        // The stack dragged back above a plain bar.
        let bars = app.strip_layout().bars;
        let _ = app.update(Message::DragStart(2, bars[2].center().y));
        let _ = app.update(Message::DragMove(bars[1].y));
        let _ = app.update(Message::DragEnd);
        assert_eq!(order(&app), vec![ids[2], ids[0], ids[1], ids[3]]);
        // A plain bar dragged past the stack lands after its last member.
        let bars = app.strip_layout().bars;
        let _ = app.update(Message::DragStart(0, bars[0].center().y));
        let _ = app.update(Message::DragMove(bars[2].y));
        let _ = app.update(Message::DragEnd);
        assert_eq!(order(&app), vec![ids[0], ids[1], ids[2], ids[3]]);
    }

    #[test]
    fn drag_on_deleted_bar_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        let bar = app.strip_layout().bars[1];
        let _ = app.update(Message::DragStart(1, bar.center().y));
        let _ = app.start_collapse(ids[1]);
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
        let _ = app.start_collapse(ids[1]);
        settle(&mut app);
        assert!(app.drag.is_none());
        let _ = app.update(Message::DragEnd);
        assert_eq!(app.active_note, None);
        let order: Vec<Uuid> = app.store.notes().iter().map(|n| n.id).collect();
        assert_eq!(order, vec![ids[0]]);
    }

    fn order(app: &App) -> Vec<Uuid> {
        app.store.notes().iter().map(|n| n.id).collect()
    }

    #[test]
    fn dragging_onto_bar_stacks() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        let bars = app.strip_layout().bars;
        let _ = app.update(Message::DragStart(2, bars[2].center().y));
        let _ = app.update(Message::DragMove(bars[0].center().y));
        let _ = app.update(Message::DragEnd);
        assert!(app.drag.is_none());
        assert_eq!(order(&app), vec![ids[0], ids[2], ids[1]]);
        assert_eq!(app.store.notes()[1].stack, Some(ids[0]));
        assert!(app.store.is_dirty());
        assert_eq!(app.entries().len(), 2);
        assert_eq!(app.id_entry(ids[2]), Some(0));
        // A drop on a bar's edge still reorders.
        let bars = app.strip_layout().bars;
        let _ = app.update(Message::DragStart(1, bars[1].center().y));
        let _ = app.update(Message::DragMove(bars[0].y + 1.0));
        let _ = app.update(Message::DragEnd);
        assert_eq!(order(&app), vec![ids[1], ids[0], ids[2]]);
        assert_eq!(app.entries().len(), 2);
    }

    #[test]
    fn unstack_restores_members_after_top() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 4);
        let _ = app.update(Message::StackOnto {
            dragged: 3,
            target: 1,
        });
        let _ = app.update(Message::StackOnto {
            dragged: 0,
            target: 0,
        });
        assert_eq!(app.entries().len(), 3);
        let _ = app.update(Message::StackOnto {
            dragged: 0,
            target: 1,
        });
        // [1, 3, 0] under 1, then 2.
        assert_eq!(order(&app), vec![ids[1], ids[3], ids[0], ids[2]]);
        assert_eq!(app.entries().len(), 2);
        app.store.did_save();
        let _ = app.update(Message::Unstack(0));
        assert_eq!(order(&app), vec![ids[1], ids[3], ids[0], ids[2]]);
        assert!(app.store.notes().iter().all(|n| n.stack.is_none()));
        assert_eq!(app.entries().len(), 4);
        assert!(app.store.is_dirty());
    }

    #[test]
    fn opening_stack_member_opens_that_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        assert!(app.store.stack(ids[1], ids[0]));
        let _ = app.update(Message::OpenStackMember(ids[1]));
        assert_eq!(app.active_note, Some(ids[1]));
        // Its bar is the stack's.
        assert_eq!(app.id_entry(ids[1]), Some(0));
        let stack_bar = app.strip_layout().bars[0];
        assert_eq!(app.anchor_y, stack_bar.center().y);
    }

    #[test]
    fn deleting_stack_top_promotes_member() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        let a = app.store.notes()[0].id;
        let a1 = app.store.add_note(&crate::note::PALETTE);
        let a2 = app.store.add_note(&crate::note::PALETTE);
        let b = app.store.add_note(&crate::note::PALETTE);
        assert!(app.store.stack(a1, a));
        assert!(app.store.stack(a2, a));
        app.sync_entries();
        let _ = app.update(Message::PeekDelete(0));
        // The stack's bar stays while its top goes.
        assert_eq!(app.collapse(), None);
        settle(&mut app);
        assert_eq!(order(&app), vec![a1, a2, b]);
        assert_eq!(app.store.notes()[0].stack, None);
        assert_eq!(app.store.notes()[1].stack, Some(a1));
        assert_eq!(app.entries().len(), 2);
        // Deleting a member just removes it.
        let _ = app.update(Message::OpenStackMember(a2));
        settle(&mut app);
        let _ = app.update(Message::DeleteNote(a2));
        settle(&mut app);
        assert_eq!(order(&app), vec![a1, b]);
        assert!(app.store.notes().iter().all(|n| n.stack.is_none()));
    }

    #[test]
    fn stack_bar_while_top_collapses_opens_member() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        assert!(app.store.stack(ids[1], ids[0]));
        app.sync_entries();
        let _ = app.start_collapse(ids[0]);
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::StripHover(Some(bar.center().y)));
        assert_eq!(app.hover_bar.map(|(id, _)| id), Some(ids[1]));
        // A real click arrives as a drag without movement.
        let _ = app.update(Message::DragStart(0, bar.center().y));
        let _ = app.update(Message::DragEnd);
        assert_eq!(app.active_note, Some(ids[1]));
    }

    #[test]
    fn deleting_a_dragged_bar_beside_a_stack_drops_the_drag() {
        let dir = tempfile::tempdir().unwrap();
        // [A, a1 (under A), B]
        let (mut app, ids) = app_with_bars(&dir, 3);
        assert!(app.store.stack(ids[1], ids[0]));
        app.sync_entries();
        let bar = app.strip_layout().bars[1];
        let _ = app.update(Message::DragStart(1, bar.center().y));
        let _ = app.start_collapse(ids[2]);
        assert_eq!(app.collapse().map(|(entry, _)| entry), Some(1));
        settle(&mut app);
        assert!(app.drag.is_none());
        let _ = app.update(Message::DragEnd);
        assert_eq!(app.active_note, None);
        assert_eq!(order(&app), vec![ids[0], ids[1]]);
        assert_eq!(app.entries().len(), 1);
    }

    #[test]
    fn search_matching_only_a_member_keeps_the_stack_lit() {
        let dir = tempfile::tempdir().unwrap();
        // [A, a1 (under A), B]
        let (mut app, ids) = app_with_notes(&dir, &[("A", ""), ("a1", "needle"), ("B", "")]);
        assert!(app.store.stack(ids[1], ids[0]));
        let _ = app.update(Message::ToggleSearch);
        let _ = app.update(Message::SearchChanged("needle".into()));
        assert_eq!(app.dimmed_bars(), vec![false, true]);
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
    fn toggle_pin_moves_note_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", ""), ("b", ""), ("c", "")]);
        let _ = app.update(Message::BarClicked(2));
        settle(&mut app);
        let before = app.anchor_y;
        let _ = app.update(Message::TogglePin);
        assert_eq!(app.store.notes()[0].id, ids[2]);
        assert!(app.store.notes()[0].pinned);
        assert!(app.store.is_dirty());
        assert_eq!(app.active_note, Some(ids[2]));
        assert_eq!(app.id_entry(ids[2]), Some(0));
        assert!(app.anchor_y < before, "anchor follows the moved bar");
        app.store.save().unwrap();
        let reloaded = NoteStore::load(dir.path().join("notes.json"));
        assert!(reloaded.notes()[0].pinned);
        let _ = app.update(Message::TogglePin);
        assert!(!app.store.notes()[0].pinned);
        assert_eq!(app.active_note, Some(ids[2]));
    }

    #[test]
    fn pinning_a_member_pins_its_stack() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", ""), ("b", ""), ("c", "")]);
        assert!(app.store.stack(ids[2], ids[1]));
        app.sync_entries();
        let member = app.store.notes().iter().position(|n| n.id == ids[2]);
        let _ = app.open_note(member.unwrap());
        settle(&mut app);
        let _ = app.update(Message::TogglePin);
        let order: Vec<_> = app.store.notes().iter().map(|n| n.id).collect();
        assert_eq!(order, vec![ids[1], ids[2], ids[0]]);
        assert!(app.store.notes()[..2].iter().all(|n| n.pinned));
        assert!(!app.store.notes()[2].pinned);
        assert_eq!(app.active_note, Some(ids[2]));
    }

    #[test]
    fn copy_check_expires_without_frames() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "body");
        let task = app.update(Message::CopyNote);
        // The clipboard write and the timer.
        assert_eq!(task.units(), 2);
        let (id, at) = app.copied_at.expect("copied_at set");
        assert!(!app.animating, "the check must not keep frames running");
        let _ = app.update(Message::CopiedExpired(id, at));
        assert!(app.copied_at.is_none());

        // A timer from an earlier copy leaves a newer one alone.
        let _ = app.update(Message::CopyNote);
        let (_, first): (Uuid, u64) = app.copied_at.unwrap();
        let _ = app.update(Message::CopyNote);
        let (_, second) = app.copied_at.unwrap();
        assert_ne!(first, second);
        let _ = app.update(Message::CopiedExpired(id, first));
        assert_eq!(app.copied_at, Some((id, second)));
        let _ = app.update(Message::CopiedExpired(id, second));
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

    fn file_in(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// The note a drop or clipboard note just created and opened.
    fn new_note(app: &App) -> &crate::note::Note {
        let note = app.store.notes().last().unwrap();
        assert_eq!(app.active_note, Some(note.id));
        note
    }

    #[test]
    fn dropping_text_file_creates_note_with_title() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.window_size = Size::new(1400.0, 900.0);
        let path = file_in(&dir, "Call Anna @15:00.md", b"# Hi\nthere\n");
        let _ = app.update(Message::ImageDropped(path));
        assert_eq!(app.store.notes().len(), 1);
        let note = new_note(&app);
        assert_eq!(note.title, "Call Anna @15:00");
        assert_eq!(note.content, "# Hi\nthere");
        // The tag in the file name sets a reminder from now.
        assert!(note.reminder_set_at.is_some());
        assert!(!app.editing, "a note with content opens rendered");
        assert!(app.store.is_dirty());
    }

    #[test]
    fn dropping_on_bar_appends() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "first"), ("b", ""), ("c", "")]);
        // The third note is stacked under the first: their bar is the first.
        app.store.note_mut(ids[2]).unwrap().stack = Some(ids[0]);
        app.store.did_save();
        app.sync_entries();
        let bars = app.strip_layout().bars;
        assert_eq!(bars.len(), 2);
        let _ = app.update(Message::FileHovered);
        let _ = app.update(Message::CursorMoved(bars[0].center()));
        let _ = app.update(Message::ImageDropped(file_in(&dir, "x.txt", b"more")));
        assert_eq!(app.store.notes()[0].content, "first\n\nmore");
        assert_eq!(app.store.notes()[0].title, "a");
        assert_eq!(app.store.notes()[2].content, "");
        // An empty body takes the text without a blank line.
        let _ = app.update(Message::FileHovered);
        let _ = app.update(Message::CursorMoved(bars[1].center()));
        let _ = app.update(Message::ImageDropped(file_in(&dir, "y.md", b"new")));
        assert_eq!(app.store.notes()[1].content, "new");
        assert_eq!(app.store.notes().len(), 3);
        assert_eq!(app.active_note, None);
        assert!(app.store.is_dirty());
    }

    #[test]
    fn drop_after_file_hover_without_cursor_move_creates_new_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "first")]);
        app.sync_entries();
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::CursorMoved(bar.center()));
        let _ = app.update(Message::FileHovered);
        let _ = app.update(Message::ImageDropped(file_in(&dir, "x.txt", b"more")));
        assert_eq!(app.store.notes()[0].content, "first");
        assert_eq!(app.store.notes().len(), 2);
        assert_eq!(new_note(&app).content, "more");
    }

    #[test]
    fn dropping_on_add_or_empty_space_creates_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "first")]);
        app.sync_entries();
        let layout = app.strip_layout();
        for (n, at) in [layout.add_button.center(), Point::new(5.0, 5.0)]
            .into_iter()
            .enumerate()
        {
            let _ = app.update(Message::ClosePanel);
            settle(&mut app);
            app.last_cursor = Some(at);
            let _ = app.update(Message::ImageDropped(file_in(&dir, "x.txt", b"x")));
            assert_eq!(app.store.notes().len(), n + 2);
            assert_eq!(app.store.notes()[0].content, "first");
        }
    }

    #[test]
    fn dropping_image_creates_image_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.window_size = Size::new(1400.0, 900.0);
        let _ = app.update(Message::ImageDropped(png_file(&dir, "shot.png")));
        let note = new_note(&app);
        assert_eq!(note.title, "shot");
        let rel = note
            .content
            .strip_prefix("![](")
            .and_then(|r| r.strip_suffix(')'))
            .unwrap();
        assert!(rel.starts_with("images/") && rel.ends_with(".png"));
        assert!(dir.path().join(rel).is_file());
        assert!(!app.editing);
    }

    #[test]
    fn dropping_other_file_adds_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.window_size = Size::new(1400.0, 900.0);
        let path = file_in(&dir, "data.bin", &[0, 1, 2]);
        let _ = app.update(Message::ImageDropped(path.clone()));
        let note = new_note(&app);
        assert_eq!(note.title, "data");
        assert_eq!(note.content, path.display().to_string());
    }

    #[test]
    fn oversized_or_binary_text_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.window_size = Size::new(1400.0, 900.0);
        let limit = 1024 * 1024;
        let fits = file_in(&dir, "fits.txt", &vec![b'a'; limit]);
        let big = file_in(&dir, "big.txt", &vec![b'a'; limit + 1]);
        let binary = file_in(&dir, "bad.md", &[0xff, 0xfe, 0x00]);
        let _ = app.update(Message::StripFileDropped(None, fits));
        assert_eq!(new_note(&app).content.len(), limit);
        for path in [big, binary] {
            let _ = app.update(Message::StripFileDropped(None, path.clone()));
            assert_eq!(new_note(&app).content, path.display().to_string());
        }
    }

    #[test]
    fn clipboard_note_from_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.window_size = Size::new(1400.0, 900.0);
        let _ = app.clipboard_note(ClipboardContent::Text("- [ ] milk\n".into()));
        let note = new_note(&app);
        assert_eq!(note.content, "- [ ] milk");
        assert_eq!(note.title, "");
        assert!(!app.editing);
        assert!(app.store.is_dirty());
    }

    #[test]
    fn clipboard_note_from_image() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        app.window_size = Size::new(1400.0, 900.0);
        let (rgba, width, height) = rgba_1x1();
        let _ = app.clipboard_note(ClipboardContent::Image {
            rgba,
            width,
            height,
        });
        let note = new_note(&app);
        let rel = image_ref(&note.content)
            .strip_prefix("![](")
            .and_then(|r| r.strip_suffix(')'))
            .unwrap();
        assert_eq!(note.content, format!("![]({rel})"));
        assert!(dir.path().join(rel).is_file());
        assert!(!app.editing);
    }

    #[test]
    fn empty_clipboard_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.clipboard_note(ClipboardContent::Empty);
        assert!(app.store.notes().is_empty());
        assert_eq!(app.active_note, None);
        assert!(!app.store.is_dirty());
    }

    #[test]
    fn empty_clipboard_sets_hint_and_expires() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.clipboard_note(ClipboardContent::Empty);
        let at = app.clipboard_empty_at.expect("the hint shows");
        assert!(app.animating, "the shake runs on frames");
        // Frames stop once the shake settles; the chip stays.
        let _ = app.update(Message::Tick(at + Duration::from_millis(100)));
        assert!(app.animating);
        let _ = app.update(Message::Tick(at + Duration::from_millis(400)));
        assert!(!app.animating);
        assert_eq!(app.clipboard_empty_at, Some(at));
        // An older hint's expiry leaves a newer one alone.
        let _ = app.update(Message::ClipboardEmptyExpired(at - Duration::from_secs(1)));
        assert_eq!(app.clipboard_empty_at, Some(at));
        let _ = app.update(Message::ClipboardEmptyExpired(at));
        assert_eq!(app.clipboard_empty_at, None);
    }

    #[test]
    fn clipboard_hint_widens_docked_window_without_passthrough() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        assert!(!app.needs_wide_window(false), "idle: just the strip");
        assert!(app.needs_wide_window(true), "passthrough is always wide");
        let _ = app.clipboard_note(ClipboardContent::Empty);
        let at = app.clipboard_empty_at.unwrap();
        assert!(app.needs_wide_window(false), "room for the chip");
        let _ = app.update(Message::ClipboardEmptyExpired(at));
        assert!(!app.needs_wide_window(false), "shrinks back");
    }

    /// Deletes the docked note `id` and lets its bar collapse.
    fn delete_docked(app: &mut App, id: Uuid) {
        let _ = app.start_collapse(id);
        settle(app);
        assert!(app.store.notes().iter().all(|n| n.id != id));
    }

    #[test]
    fn undo_restores_deleted_note_and_saves() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        app.store.did_save();
        delete_docked(&mut app, ids[1]);
        let _ = app.update(Message::UndoDelete);
        assert_eq!(order(&app), ids);
        assert!(app.last_deleted.is_none());
        // Saved at once, and nothing opens.
        assert!(!app.store.is_dirty());
        let saved = NoteStore::load(dir.path().join("notes.json"));
        let saved: Vec<Uuid> = saved.notes().iter().map(|n| n.id).collect();
        assert_eq!(saved, ids);
        assert_eq!(app.active_note, None);
        assert_eq!(app.entries().len(), 3);
        // Only once.
        let _ = app.update(Message::UndoDelete);
        assert_eq!(order(&app), ids);
    }

    #[test]
    fn undo_restores_stack_top_with_members() {
        let dir = tempfile::tempdir().unwrap();
        // [A, a1, a2 (under A), B]
        let (mut app, ids) = app_with_bars(&dir, 4);
        assert!(app.store.stack(ids[1], ids[0]));
        assert!(app.store.stack(ids[2], ids[0]));
        app.sync_entries();
        delete_docked(&mut app, ids[0]);
        assert_eq!(app.store.notes()[0].stack, None);
        let _ = app.update(Message::UndoDelete);
        assert_eq!(order(&app), ids);
        let stacks: Vec<Option<Uuid>> = app.store.notes().iter().map(|n| n.stack).collect();
        assert_eq!(stacks, vec![None, Some(ids[0]), Some(ids[0]), None]);
        assert_eq!(app.entries().len(), 2);
    }

    #[test]
    fn cmd_z_without_open_note_undoes_delete() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 2);
        delete_docked(&mut app, ids[0]);
        let _ = app.update(cmd_z(false));
        assert_eq!(order(&app), ids);
        assert!(app.last_deleted.is_none());
    }

    #[test]
    fn cmd_z_with_open_note_is_editor_undo() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "- [ ] t");
        let other = app.store.add_note(&crate::note::PALETTE);
        app.sync_entries();
        delete_docked(&mut app, other);
        assert!(app.active_note.is_some());
        let _ = app.update(Message::ToggleTask(0));
        assert_eq!(app.store.notes()[0].content, "- [x] t");
        let task = app.update(cmd_z(false));
        // The open note's undo runs after its title focus check: the task
        // then sends the editor's undo, which reverts the toggle ...
        assert!(task.units() > 0);
        let step = crate::note_panel::history_key('z', keyboard::Modifiers::COMMAND);
        assert!(matches!(step, Some(Message::Undo)));
        let _ = app.update(step.unwrap());
        assert_eq!(app.store.notes()[0].content, "- [ ] t");
        // ... and the deleted note stays deleted, its toast up.
        assert!(app.store.notes().iter().all(|n| n.id != other));
        assert!(app.last_deleted.is_some());
    }

    #[test]
    fn cmd_z_with_search_open_does_not_restore() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 2);
        delete_docked(&mut app, ids[0]);
        let _ = app.toggle_search();
        let _ = app.update(cmd_z(false));
        assert_eq!(order(&app), vec![ids[1]], "the search field's undo");
        assert!(app.last_deleted.is_some());
        app.close_search();
        let _ = app.update(Message::ToggleSettings);
        let _ = app.update(cmd_z(false));
        assert_eq!(order(&app), vec![ids[1]], "not with the settings open");
    }

    #[test]
    fn undo_toast_expires() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 2);
        assert!(!app.needs_wide_window(false));
        delete_docked(&mut app, ids[0]);
        let at = app.last_deleted.as_ref().map(|l| l.at).unwrap();
        // Without passthrough the window widens to show the toast.
        assert!(app.needs_wide_window(false));
        // An older toast's expiry leaves a newer one alone.
        let _ = app.update(Message::UndoExpired(at - Duration::from_secs(1)));
        assert!(app.last_deleted.is_some());
        let _ = app.update(Message::UndoExpired(at));
        assert!(app.last_deleted.is_none());
        assert!(!app.needs_wide_window(false));
        // Expired: nothing to undo.
        let _ = app.update(Message::UndoDelete);
        assert_eq!(order(&app), vec![ids[1]]);
    }

    #[test]
    fn second_delete_replaces_toast() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        delete_docked(&mut app, ids[0]);
        let first = app.last_deleted.as_ref().map(|l| l.at).unwrap();
        delete_docked(&mut app, ids[1]);
        assert_eq!(
            app.last_deleted.as_ref().map(|l| l.deleted.note.id),
            Some(ids[1])
        );
        // The first toast's timer no longer clears the second.
        let _ = app.update(Message::UndoExpired(first));
        assert!(app.last_deleted.is_some());
        let _ = app.update(Message::UndoDelete);
        assert_eq!(order(&app), vec![ids[1], ids[2]]);
    }

    #[test]
    fn toast_area_is_interactive() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 2);
        assert_eq!(app.toast_rect(), None);
        delete_docked(&mut app, ids[0]);
        let toast = app.toast_rect().expect("the toast shows");
        // Its left end reaches past the strip, where clicks would otherwise
        // fall through.
        let left_end = Point::new(toast.x + 2.0, toast.center_y());
        assert!(left_end.x < app.window_size.width - STRIP_WIDTH);
        assert!(app.is_interactive(left_end));
        let at = app.last_deleted.as_ref().map(|l| l.at).unwrap();
        let _ = app.update(Message::UndoExpired(at));
        assert!(!app.is_interactive(left_end));
    }

    #[test]
    fn toast_hidden_and_inert_while_search_open() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 2);
        delete_docked(&mut app, ids[0]);
        let toast = app.toast_rect().expect("the toast shows");
        let _ = app.toggle_search();
        settle(&mut app);
        // Under the panel the strip neither draws the toast nor lets it
        // take clicks: they belong to the panel.
        assert!(!app.toast_visible());
        assert_eq!(app.toast_rect(), None);
        // It comes back once the panel closes, while its time lasts.
        app.close_search();
        settle(&mut app);
        assert!(app.toast_visible());
        assert_eq!(app.toast_rect(), Some(toast));
        assert!(app.is_interactive(Point::new(toast.x + 2.0, toast.center_y())));
    }

    #[test]
    fn toast_rect_none_while_hidden_by_peek_or_clipboard_chip() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        app.settings
            .settings_mut()
            .set(crate::settings::SettingKey::PeekDelay, 0.0);
        delete_docked(&mut app, ids[2]);
        assert!(app.toast_rect().is_some());
        // The empty-clipboard chip takes the toast's place.
        app.clipboard_empty_at = Some(Instant::now());
        assert_eq!(app.toast_rect(), None);
        app.clipboard_empty_at = None;
        // A bar dragged for real hides it; a press that hasn't moved doesn't.
        let y = app.strip_layout().bars[1].center_y();
        app.drag = Some(DragState {
            bar_index: 1,
            origin_y: y,
            current_y: y + 2.0,
        });
        assert!(app.toast_rect().is_some());
        app.drag.as_mut().unwrap().current_y = y - 40.0;
        assert_eq!(app.toast_rect(), None);
        app.drag = None;
        // So does the peek, once it starts to grow.
        open_peek(&mut app);
        let _ = app.update(Message::Tick(Instant::now()));
        assert!(app.peek.progress() > 0.0);
        assert_eq!(app.toast_rect(), None);
    }

    #[test]
    fn undo_restores_pulse() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("Plain", ""), (DUE_TITLE, "")]);
        let _ = app.update(Message::ReminderTick);
        assert!(app.pulsing.contains(&ids[1]));
        delete_docked(&mut app, ids[1]);
        assert!(app.pulsing.is_empty());
        let _ = app.update(Message::UndoDelete);
        assert!(app.pulsing.contains(&ids[1]));
        assert!(app.bar_jump()[1] > 0.0);
        assert!(app.animating, "the alert runs on frames again");
        // A note that didn't alert comes back without.
        delete_docked(&mut app, ids[0]);
        let _ = app.update(Message::UndoDelete);
        assert!(!app.pulsing.contains(&ids[0]));
    }

    #[test]
    fn undo_during_second_delete_does_not_restore_first() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 4);
        delete_docked(&mut app, ids[0]);
        // The next delete from the peek is still collapsing: the first
        // one's undo is gone.
        let _ = app.start_collapse(ids[1]);
        let _ = app.update(Message::UndoDelete);
        settle(&mut app);
        assert_eq!(order(&app), vec![ids[2], ids[3]]);
        // The same while an open note folds away before its delete.
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        assert_eq!(app.active_note, Some(ids[2]));
        let _ = app.update(Message::DeleteNote(ids[2]));
        let _ = app.update(Message::UndoDelete);
        settle(&mut app);
        assert_eq!(order(&app), vec![ids[3]]);
        // Its own undo still works once it is gone.
        let _ = app.update(Message::UndoDelete);
        assert_eq!(order(&app), vec![ids[2], ids[3]]);
    }

    #[test]
    fn undo_keeps_note_fields() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_bars(&dir, 3);
        app.store.set_pinned(ids[1], true);
        let note = app.store.note_mut(ids[1]).unwrap();
        note.title = "Call @15:00".into();
        note.content = "body\n- [x] done".into();
        note.size = Some([320.0, 240.0]);
        let set_at = chrono::Utc::now();
        note.reminder_set_at = Some(set_at);
        note.reminder_fired = Some(set_at);
        app.sync_entries();
        delete_docked(&mut app, ids[1]);
        let _ = app.update(Message::UndoDelete);
        let note = &app.store.notes()[0];
        assert_eq!(note.id, ids[1]);
        assert_eq!(note.title, "Call @15:00");
        assert_eq!(note.content, "body\n- [x] done");
        assert!(note.pinned);
        assert_eq!(note.size, Some([320.0, 240.0]));
        assert_eq!(note.reminder_set_at, Some(set_at));
        assert_eq!(note.reminder_fired, Some(set_at));
    }

    #[test]
    fn jump_amount_hops_and_settles() {
        assert_eq!(jump_amount(0.0), 0.0);
        assert!(jump_amount(JUMP_SECS * 0.25) > JUMP_REACH * 0.5);
        assert_eq!(jump_amount(JUMP_SECS), 0.0);
        assert_eq!(jump_settle_offset(8.0, 0.0), 8.0);
        assert_eq!(jump_settle_offset(8.0, JUMP_SETTLE_SECS), 0.0);
        assert_eq!(jump_settle_offset(8.0, JUMP_SETTLE_SECS + 0.1), 0.0);
        let mid = jump_settle_offset(8.0, JUMP_SETTLE_SECS * 0.5);
        assert!(mid > 0.0 && mid < 8.0, "mid {mid}");
    }

    #[test]
    fn add_shake_settles() {
        assert_eq!(add_shake(0.0), 0.0);
        let peak = (0..300)
            .map(|ms| add_shake(ms as f32 / 1000.0).abs())
            .fold(0.0, f32::max);
        assert!(peak > 2.0 && peak <= 3.0, "peak {peak}");
        // Three cycles: the offset changes sign six times.
        let signs: Vec<bool> = (1..300)
            .map(|ms| add_shake(ms as f32 / 1000.0))
            .filter(|x| x.abs() > 1e-3)
            .map(|x| x > 0.0)
            .collect();
        assert_eq!(signs.windows(2).filter(|w| w[0] != w[1]).count(), 5);
        // Later swings are smaller.
        assert!(add_shake(0.275).abs() < add_shake(0.025).abs());
        for t in [0.3, 0.5, 2.0, -0.1] {
            assert_eq!(add_shake(t), 0.0, "t {t}");
        }
    }

    #[test]
    fn color_button_toggles_the_bubble_with_the_note_color_as_hex() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let hex = app.store.notes()[0].color.to_hex();
        let _ = app.update(Message::ToggleColorPicker);
        assert!(app.color_picker_open);
        assert_eq!(app.color_hex, hex);
        let _ = app.update(Message::ToggleColorPicker);
        assert!(!app.color_picker_open);
    }

    #[test]
    fn wheel_recolors_the_note_and_its_bar_live() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let _ = app.update(Message::ToggleColorPicker);
        assert!(!app.store.is_dirty());
        let color = NoteColor::new(0.2, 0.4, 0.8);
        let _ = app.update(Message::ColorAdjusted(color));
        // The bar shows the note's color straight from the store.
        let top = app.entries()[0].top;
        assert_eq!(app.store.notes()[top].color, color);
        assert_eq!(app.entries_synced, app.store.revision());
        assert!(app.store.is_dirty(), "saved through the debounce");
        assert!(app.color_picker_open, "dragging keeps the bubble open");
        assert_eq!(app.color_hex, color.to_hex());
    }

    #[test]
    fn hex_field_applies_valid_colors_as_typed() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let _ = app.update(Message::ToggleColorPicker);
        let _ = app.update(Message::ColorHexEdited("45b7d1".into()));
        assert_eq!(app.store.notes()[0].color.to_hex(), "#45B7D1");
        // The field keeps what was typed.
        assert_eq!(app.color_hex, "45b7d1");
        let _ = app.update(Message::ColorHexEdited("#12".into()));
        assert_eq!(app.store.notes()[0].color.to_hex(), "#45B7D1");
        assert_eq!(app.color_hex, "#12");
        assert!(app.color_picker_open);
        let _ = app.update(Message::ColorHexSubmitted);
        assert!(!app.color_picker_open);
    }

    #[test]
    fn preset_recolors_and_closes_the_bubble() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let _ = app.update(Message::ToggleColorPicker);
        let mint = crate::note::DEFAULT_PALETTE[3];
        let _ = app.update(Message::ColorChosen(mint));
        assert_eq!(app.store.notes()[0].color, mint);
        assert_eq!(app.color_hex, mint.to_hex());
        assert!(!app.color_picker_open);
    }

    #[test]
    fn escape_closes_the_bubble_before_the_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let _ = app.update(Message::ToggleColorPicker);
        let _ = app.update(escape());
        assert!(!app.color_picker_open);
        assert!(app.morph.is_opening(), "the note stays open");
        assert!(app.active_note.is_some());
    }

    #[test]
    fn switching_notes_closes_the_bubble() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        app.store.add_note(&crate::note::PALETTE);
        let _ = app.update(Message::ToggleColorPicker);
        // The other note's bar: the morph restarts for it.
        let other = app.store.notes()[1].id;
        let entry = app.id_entry(other).unwrap();
        let _ = app.update(Message::BarClicked(entry));
        assert_eq!(app.active_note, Some(other));
        assert!(!app.color_picker_open);
    }

    #[test]
    fn undo_key_leaves_the_body_alone_while_the_bubble_is_open() {
        use keyboard::key::{Code, Physical};
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let _ = app.update(Message::ToggleColorPicker);
        let cmd_z = Message::Key(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("z".into()),
            modified_key: keyboard::Key::Character("z".into()),
            physical_key: Physical::Code(Code::KeyZ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::COMMAND,
            text: None,
            repeat: false,
        });
        // No undo step is sent: the hex field has the keys.
        assert_eq!(app.update(cmd_z.clone()).units(), 0);
        let _ = app.update(Message::CloseColorBubble);
        assert!(app.update(cmd_z).units() > 0, "body undo when closed");
        assert_eq!(app.store.notes()[0].content, "x");
    }

    #[test]
    fn press_outside_closes_the_bubble() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let _ = app.update(Message::ToggleColorPicker);
        let _ = app.update(Message::CloseColorBubble);
        assert!(!app.color_picker_open);
        assert!(app.active_note.is_some());
    }

    #[test]
    fn the_bubble_card_takes_presses_while_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "x");
        let note = app.note_frame().unwrap().rect;
        // The card sits above the note, outside its frame.
        let card = Rectangle::new(Point::new(note.x, note.y - 200.0), Size::new(220.0, 150.0));
        let on_card = card.center();
        assert!(!app.is_interactive(on_card));
        let _ = app.update(Message::ToggleColorPicker);
        let _ = app.update(Message::ColorBubbleMoved(card));
        assert!(app.is_interactive(on_card));
        let _ = app.update(Message::CloseColorBubble);
        assert!(!app.is_interactive(on_card));
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
    fn actions_slot_follows_hover() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        assert!(!app.actions_open);
        let _ = app.update(Message::ActionsHovered(true));
        assert!(app.actions_open);
        assert!(app.strip_layout().actions_expanded);
        let _ = app.update(Message::ActionsHovered(false));
        assert!(!app.actions_open);
    }

    #[test]
    fn settings_reopen_on_the_application_tab() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleSettings);
        settle(&mut app);
        assert_eq!(app.settings_tab, SettingsTab::Application);
        let _ = app.update(Message::PaletteSlotSelected(Some(0)));
        let _ = app.update(Message::SettingsTabSelected(SettingsTab::Styling));
        assert_eq!(app.settings_tab, SettingsTab::Styling);
        assert_eq!(app.palette_slot, None);
        let _ = app.update(Message::CloseSettings);
        settle(&mut app);
        assert!(!app.settings_open);
        let _ = app.update(Message::ToggleSettings);
        assert_eq!(app.settings_tab, SettingsTab::Application);
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
                reminder::retitle(note, (*title).into(), chrono::Local::now());
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

    fn cmd_key_on(ch: &str, code: keyboard::key::Code) -> Message {
        let key = keyboard::Key::Character(ch.into());
        Message::Key(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: keyboard::key::Physical::Code(code),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::COMMAND,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn non_latin_layout_keeps_app_shortcuts() {
        use keyboard::key::Code;
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        // Russian layout: F types "а", N types "т", the comma key types "б".
        let _ = app.update(cmd_key_on("а", Code::KeyF));
        assert!(search_showing(&app));
        let _ = app.update(cmd_key_on("а", Code::KeyF));
        settle(&mut app);
        let notes = app.store.notes().len();
        let _ = app.update(cmd_key_on("т", Code::KeyN));
        assert_eq!(app.store.notes().len(), notes + 1);
        settle(&mut app);
        let _ = app.update(cmd_key_on("б", Code::Comma));
        assert!(app.settings_open);
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
    fn enter_opens_first_result() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "needle one"), ("b", "needle two")]);
        let _ = app.update(Message::ToggleSearch);
        // Without hits Enter does nothing.
        let _ = app.update(Message::SearchChanged("zzz".into()));
        let _ = app.update(Message::SearchSubmitted);
        assert_eq!(app.active_note, None);
        assert!(app.search_morph.is_opening());
        let _ = app.update(Message::SearchChanged("needle".into()));
        let _ = app.update(Message::SearchSubmitted);
        assert_eq!(app.active_note, Some(ids[0]));
        assert!(app.editing);
        assert!(!app.search_morph.is_opening());
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

    /// Ticks a few frames: far enough into a fold that it is still running.
    fn tick_a_little(app: &mut App) {
        let mut now = Instant::now();
        for _ in 0..3 {
            now += Duration::from_millis(16);
            let _ = app.update(Message::Tick(now));
        }
    }

    #[test]
    fn reopening_search_mid_fold_closes_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleSearch);
        settle(&mut app);
        let _ = app.update(Message::ToggleSettings);
        tick_a_little(&mut app);
        assert!(app.search_open && !app.search_morph.is_opening());
        let _ = app.update(Message::ToggleSearch);
        assert!(search_showing(&app));
        assert!(!app.settings_morph.is_opening());
        settle(&mut app);
        assert!(!app.settings_open);
    }

    #[test]
    fn reopening_search_mid_fold_folds_the_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "needle")]);
        let _ = app.update(Message::ToggleSearch);
        let _ = app.update(Message::SearchChanged("needle".into()));
        settle(&mut app);
        let _ = app.update(Message::SearchResultPicked(ids[0]));
        tick_a_little(&mut app);
        assert!(app.search_open && app.morph.is_opening());
        let _ = app.update(Message::ToggleSearch);
        assert!(search_showing(&app));
        assert!(!app.morph.is_opening());
        // The query survives the reopen.
        assert_eq!(app.search_query, "needle");
    }

    #[test]
    fn reopening_settings_mid_fold_closes_search() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleSettings);
        settle(&mut app);
        let _ = app.update(Message::ToggleSearch);
        tick_a_little(&mut app);
        assert!(app.settings_open && !app.settings_morph.is_opening());
        let _ = app.update(Message::ToggleSettings);
        assert!(app.settings_morph.is_opening());
        assert!(!app.search_morph.is_opening());
        settle(&mut app);
        assert!(!app.search_open);
    }

    #[test]
    fn search_results_follow_store_changes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "red"), ("b", "blue")]);
        let _ = app.update(Message::ToggleSearch);
        let _ = app.update(Message::SearchChanged("red".into()));
        assert_eq!(app.search_hits().len(), 1);
        // A note added while the search is open shows up.
        let added = app.store.add_note(&crate::note::PALETTE);
        app.store.note_mut(added).unwrap().content = "red too".into();
        let _ = app.update(Message::SaveTick);
        let hits: Vec<Uuid> = app.search_hits().iter().map(|h| h.note_id).collect();
        assert_eq!(hits, vec![ids[0], added]);
        assert_eq!(app.dimmed_bars(), vec![false, true, false]);
        // Dimming follows the notes when they are reordered.
        app.store.reorder(0, 2);
        assert_eq!(app.dimmed_bars(), vec![true, false, false]);
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
        assert!(
            !app.settings.settings().app.show_dock_icon,
            "hidden by default"
        );
        app.settings.did_save();
        let _ = app.update(Message::TrayReady(false));
        assert!(app.dock_icon_shown());
        assert!(!app.settings.settings().app.show_dock_icon);
        assert!(!app.settings.is_dirty());
    }

    fn export_showing(app: &App) -> bool {
        app.export.is_some() && app.export_morph.is_opening()
    }

    fn export_status(app: &App) -> Option<String> {
        app.export
            .as_ref()?
            .status
            .as_ref()
            .map(|(s, _)| s.message())
    }

    /// Clicks Export and has the dialog return `path`, as the dialog task
    /// would.
    fn export_via_dialog(app: &mut App, path: PathBuf) {
        let task = app.update(Message::ExportRequested);
        assert!(task.units() > 0, "no dialog was started");
        let state = app.export.as_ref().unwrap();
        let job = ExportJob {
            generation: state.generation,
            path,
            notes: state.selected_in(app.store.notes()),
            format: state.format,
        };
        let _ = app.update(Message::ExportPicked(job));
    }

    #[test]
    fn export_opens_with_all_selected() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1"), ("b", "2")]);
        let _ = app.update(Message::ToggleSettings);
        settle(&mut app);
        let _ = app.update(Message::ToggleExport);
        assert!(export_showing(&app));
        let state = app.export.as_ref().unwrap();
        assert_eq!(state.selected, ids.iter().copied().collect());
        assert_eq!(state.format, ExportFormat::Markdown);
        assert!(state.status.is_none());
        // Export replaces Settings, and closing it doesn't bring them back.
        assert!(!app.settings_morph.is_opening());
        let _ = app.update(Message::ToggleExport);
        assert!(!app.export_morph.is_opening());
        settle(&mut app);
        assert!(app.export.is_none());
        assert!(!app.settings_open);
    }

    #[test]
    fn toggling_and_all_none() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1"), ("b", "2")]);
        let _ = app.update(Message::ToggleExport);
        let _ = app.update(Message::ExportToggleNote(ids[0]));
        assert_eq!(
            app.export.as_ref().unwrap().selected,
            HashSet::from([ids[1]])
        );
        let _ = app.update(Message::ExportToggleNote(ids[0]));
        assert_eq!(app.export.as_ref().unwrap().selected.len(), 2);
        let _ = app.update(Message::ExportAll(false));
        assert!(app.export.as_ref().unwrap().selected.is_empty());
        let _ = app.update(Message::ExportAll(true));
        assert_eq!(
            app.export.as_ref().unwrap().selected,
            ids.iter().copied().collect()
        );
        let _ = app.update(Message::ExportFormatChosen(ExportFormat::Text));
        assert_eq!(app.export.as_ref().unwrap().format, ExportFormat::Text);
    }

    #[test]
    fn export_requested_with_nothing_selected_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        let _ = app.update(Message::ExportAll(false));
        let task = app.update(Message::ExportRequested);
        assert_eq!(task.units(), 0);
        assert!(app.keep_open_until.is_none());
    }

    #[test]
    fn export_picked_writes_file_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1"), ("b", "2"), ("c", "3")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let _ = app.update(Message::ExportToggleNote(ids[1]));
        let path = dir.path().join("out.md");
        export_via_dialog(&mut app, path.clone());
        let notes = app.store.notes();
        let expected = crate::export::render(&[&notes[0], &notes[2]], ExportFormat::Markdown);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(export_status(&app).as_deref(), Some("Exported 2 notes"));
        let (generation, at) = {
            let state = app.export.as_ref().unwrap();
            (state.generation, state.status.as_ref().unwrap().1)
        };
        let _ = app.update(Message::ExportStatusExpired(generation, at));
        assert!(!app.export_morph.is_opening());
        settle(&mut app);
        assert!(app.export.is_none());

        // One note is singular.
        let _ = app.update(Message::ToggleExport);
        let _ = app.update(Message::ExportAll(false));
        let _ = app.update(Message::ExportToggleNote(ids[0]));
        export_via_dialog(&mut app, dir.path().join("one.txt"));
        assert_eq!(export_status(&app).as_deref(), Some("Exported 1 note"));
    }

    #[test]
    fn export_auto_close_uses_a_timer() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        assert!(!app.animating);
        let task = app.update(Message::ExportRequested);
        assert!(task.units() > 0, "no dialog was started");
        let state = app.export.as_ref().unwrap();
        let generation = state.generation;
        let job = ExportJob {
            generation,
            path: dir.path().join("out.md"),
            notes: state.selected_in(app.store.notes()),
            format: state.format,
        };
        let task = app.update(Message::ExportPicked(job));
        assert_eq!(task.units(), 1, "no close timer was started");
        assert_eq!(export_status(&app).as_deref(), Some("Exported 1 note"));
        assert!(!app.animating, "the success must not keep frames running");
        let at = app.export.as_ref().unwrap().status.as_ref().unwrap().1;
        let _ = app.update(Message::Tick(at + Duration::from_secs(5)));
        assert!(export_showing(&app), "frames no longer close the panel");

        // Another opening's timer leaves this one alone.
        let _ = app.update(Message::ExportStatusExpired(generation + 1, at));
        assert!(export_showing(&app));
        let _ = app.update(Message::ExportStatusExpired(generation, at));
        assert!(!app.export_morph.is_opening());
    }

    #[test]
    fn export_cancelled_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let _ = app.update(Message::ExportRequested);
        assert!(app.export_dialog_open);
        let generation = app.export.as_ref().unwrap().generation;
        let _ = app.update(Message::ExportCancelled(generation));
        assert!(!app.export_dialog_open);
        assert!(export_status(&app).is_none());
        assert!(export_showing(&app));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn export_write_error_shows_message() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        export_via_dialog(&mut app, dir.path().to_path_buf());
        let status = export_status(&app).expect("error shown");
        assert!(status.starts_with("Couldn't save: "), "{status}");
        // It stays until the next action.
        let at = app.export.as_ref().unwrap().status.as_ref().unwrap().1;
        let _ = app.update(Message::Tick(at + Duration::from_secs(5)));
        assert!(export_showing(&app));
        assert!(export_status(&app).is_some());
        let _ = app.update(Message::ExportToggleNote(ids[0]));
        assert!(export_status(&app).is_none());
    }

    #[test]
    fn export_dialog_keeps_panel_open() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let task = app.update(Message::ExportRequested);
        assert!(task.units() > 0, "no dialog was started");
        assert!(app.keep_open_until.is_some());
        let _ = app.update(Message::WindowUnfocused);
        assert!(export_showing(&app));
    }

    #[test]
    fn second_export_request_while_picking_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let first = app.update(Message::ExportRequested);
        assert!(first.units() > 0);
        assert!(app.export_dialog_open);
        let second = app.update(Message::ExportRequested);
        assert_eq!(second.units(), 0, "a second dialog was started");
        assert!(!app.export_enabled());
    }

    #[test]
    fn reopened_panel_cannot_start_a_second_dialog() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let old = app.export.as_ref().unwrap().generation;
        let first = app.update(Message::ExportRequested);
        assert!(first.units() > 0, "no dialog was started");
        // Esc closes the panel while its dialog is still open.
        let _ = app.update(escape());
        settle(&mut app);
        assert!(app.export.is_none());
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        assert!(export_showing(&app));
        let second = app.update(Message::ExportRequested);
        assert_eq!(second.units(), 0, "a second dialog was started");
        assert!(!app.export_enabled());
        // The first dialog returning, even cancelled, frees the button.
        let _ = app.update(Message::ExportCancelled(old));
        assert!(!app.export_dialog_open);
        assert!(app.export_enabled());
        let third = app.update(Message::ExportRequested);
        assert!(third.units() > 0, "no dialog was started");
    }

    #[test]
    fn export_picked_after_panel_closed_still_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1"), ("b", "2")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let _ = app.update(Message::ExportRequested);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        assert!(app.export.is_none());
        let path = dir.path().join("late.md");
        let _ = app.update(Message::ExportPicked(ExportJob {
            generation: 1,
            path: path.clone(),
            notes: ids.clone(),
            format: ExportFormat::Markdown,
        }));
        let notes = app.store.notes();
        let expected = crate::export::render(&[&notes[0], &notes[1]], ExportFormat::Markdown);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        // A note deleted while the dialog was open is skipped.
        app.store.delete_note(ids[0]);
        let path = dir.path().join("later.md");
        let _ = app.update(Message::ExportPicked(ExportJob {
            generation: 1,
            path: path.clone(),
            notes: ids,
            format: ExportFormat::Markdown,
        }));
        let expected = crate::export::render(&[&app.store.notes()[0]], ExportFormat::Markdown);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    }

    #[test]
    fn stale_export_result_does_not_touch_a_reopened_panel() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let old = app.export.as_ref().unwrap().generation;
        let _ = app.update(Message::ExportRequested);
        // The panel folds away and opens afresh.
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        assert_ne!(app.export.as_ref().unwrap().generation, old);

        // The first dialog's result still writes, but not into this panel.
        let path = dir.path().join("stale.md");
        let task = app.update(Message::ExportPicked(ExportJob {
            generation: old,
            path: path.clone(),
            notes: ids,
            format: ExportFormat::Markdown,
        }));
        assert!(path.exists());
        assert!(export_status(&app).is_none());
        assert_eq!(task.units(), 0, "no close timer for another panel");
        assert!(export_showing(&app));
        // It frees the Export button all the same.
        assert!(app.export_enabled());
    }

    #[test]
    fn panel_stays_open_while_picking() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let _ = app.update(Message::ExportRequested);
        // Past the focus grace, as with a slow dialog.
        app.keep_open_until = None;
        let _ = app.update(Message::WindowUnfocused);
        assert!(export_showing(&app));
        let generation = app.export.as_ref().unwrap().generation;
        let _ = app.update(Message::ExportCancelled(generation));
        let _ = app.update(Message::WindowUnfocused);
        assert!(!app.export_morph.is_opening());
    }

    #[test]
    fn export_button_needs_an_existing_selected_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1"), ("b", "2")]);
        let _ = app.update(Message::ToggleExport);
        let _ = app.update(Message::ExportToggleNote(ids[1]));
        let state = app.export.as_ref().unwrap();
        assert!(state.can_export(app.store.notes()));
        app.store.delete_note(ids[0]);
        let state = app.export.as_ref().unwrap();
        assert!(!state.selected.is_empty());
        assert!(!state.can_export(app.store.notes()));
        let task = app.update(Message::ExportRequested);
        assert_eq!(task.units(), 0);
    }

    #[test]
    fn hotkey_error_set_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        assert_eq!(app.hotkey_error, None);
        app.apply_registration(Err("HotKey already registered".into()));
        assert_eq!(
            app.hotkey_error.as_deref(),
            Some("HotKey already registered")
        );
        assert!(app.hotkey.is_none());
        // Turning the hotkey off drops the stale reason (Settings only offers
        // the toggle on macOS and Windows).
        #[cfg(any(windows, target_os = "macos"))]
        {
            app.window_id = Some(window::Id::unique());
            app.settings
                .settings_mut()
                .toggle(SettingToggle::GlobalHotkey);
            assert!(!app.settings.settings().is_on(SettingToggle::GlobalHotkey));
            app.sync_hotkey();
            assert_eq!(app.hotkey_error, None);
        }
    }

    #[test]
    fn hotkey_shows_hidden_notes_and_opens_new_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        let _ = app.update(Message::ToggleVisibility);
        assert!(!app.visible);
        let _ = app.update(Message::HotkeyPressed);
        assert!(app.visible);
        assert_eq!(app.store.notes().len(), 2);
        let new = app.store.notes()[1].id;
        assert_eq!(app.active_note, Some(new));
        assert!(app.editing && app.morph.is_opening());
        assert_eq!(app.pending_focus, Some(PendingFocus::Body));
    }

    #[test]
    fn hotkey_closes_panels_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        for panel in [
            Message::ToggleSettings,
            Message::ToggleSearch,
            Message::ToggleExport,
        ] {
            let _ = app.update(panel);
            settle(&mut app);
            let before = app.store.notes().len();
            let _ = app.update(Message::HotkeyPressed);
            assert_eq!(app.store.notes().len(), before + 1);
            assert!(!app.settings_morph.is_opening());
            assert!(!app.search_morph.is_opening() && !app.export_morph.is_opening());
            assert!(app.active_note.is_some() && app.morph.is_opening());
            settle(&mut app);
            assert!(!app.settings_open && !app.search_open && app.export.is_none());
            assert!(app.active_note.is_some());
            let _ = app.update(Message::ClosePanel);
            settle(&mut app);
        }
    }

    #[test]
    fn tray_export_item_opens_export() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleVisibility);
        let _ = app.update(Message::TrayMenu("export".into()));
        assert!(app.visible);
        assert!(export_showing(&app));
        settle(&mut app);
        // The menu item opens the export; it never closes it.
        let _ = app.update(Message::TrayMenu("export".into()));
        assert!(export_showing(&app));
    }

    #[test]
    fn escape_closes_export_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_in(&dir);
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let _ = app.update(escape());
        assert!(!app.export_morph.is_opening());
    }

    #[test]
    fn reopening_export_mid_fold_closes_others() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "1")]);
        for other in [
            Message::ToggleSettings,
            Message::ToggleSearch,
            Message::BarClicked(0),
        ] {
            let _ = app.update(Message::ToggleExport);
            settle(&mut app);
            let _ = app.update(other);
            tick_a_little(&mut app);
            assert!(app.export.is_some() && !app.export_morph.is_opening());
            let _ = app.update(Message::ToggleExport);
            assert!(export_showing(&app));
            assert!(!app.settings_morph.is_opening());
            assert!(!app.search_morph.is_opening());
            assert!(!app.morph.is_opening());
            settle(&mut app);
            assert!(!app.settings_open && !app.search_open && app.active_note.is_none());
            let _ = app.update(Message::ToggleExport);
            settle(&mut app);
        }
        // And the other panels close the export the same way.
        for other in [Message::ToggleSettings, Message::ToggleSearch] {
            let _ = app.update(Message::ToggleExport);
            settle(&mut app);
            let _ = app.update(other);
            assert!(!app.export_morph.is_opening());
            settle(&mut app);
            assert!(app.export.is_none());
        }
    }

    /// A title whose reminder is long due, so a tick fires it.
    const DUE_TITLE: &str = "Call Anna @2026-01-02 09:00";

    fn due_time() -> chrono::DateTime<chrono::Utc> {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2026, 1, 2, 9, 0, 0)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn reminder_tick_fires_once() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("Plain", ""), (DUE_TITLE, "")]);
        assert!(app.reminders_pending());
        let _ = app.update(Message::ReminderTick);
        assert_eq!(app.store.notes()[1].reminder_fired, Some(due_time()));
        assert_eq!(app.store.notes()[0].reminder_fired, None);
        assert!(app.pulsing.contains(&ids[1]));
        let jump = app.bar_jump();
        assert_eq!(jump[0], 0.0);
        assert!(jump[1] > 0.0);
        assert!(app.bar_pulse().is_empty(), "Jump is the default alert");
        assert!(app.animating, "the alert runs on frames");
        // Saved at once: a crash right after can't fire it again.
        assert!(!app.store.is_dirty());
        let saved = NoteStore::load(dir.path().join("notes.json"));
        assert_eq!(saved.notes()[1].reminder_fired, Some(due_time()));
        assert!(!app.reminders_pending(), "no timer once nothing is pending");

        // A second tick fires nothing more.
        app.pulsing.clear();
        let _ = app.update(Message::ReminderTick);
        assert!(app.pulsing.is_empty());
    }

    #[test]
    fn reminder_pulse_style_uses_glow() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("Plain", ""), (DUE_TITLE, "")]);
        app.settings.settings_mut().motion.reminder_alert =
            crate::settings::ReminderAlertStyle::Pulse;
        let _ = app.update(Message::ReminderTick);
        assert!(app.pulsing.contains(&ids[1]));
        let pulse = app.bar_pulse();
        assert_eq!(pulse[0], 0.0);
        assert!(pulse[1] > 0.0);
        assert!(app.bar_jump().is_empty());
    }

    #[test]
    fn jump_settles_on_hover_and_resumes_after() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[(DUE_TITLE, "")]);
        app.settings
            .settings_mut()
            .set(crate::settings::SettingKey::PeekDelay, 1.0);
        let _ = app.update(Message::ReminderTick);
        assert!(app.bar_jump()[0] > 0.0);
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::StripHover(Some(bar.center().y)));
        assert_eq!(app.hover_bar.map(|(id, _)| id), Some(ids[0]));
        assert!(app.jump_settle.is_some());
        // Finish the settle instantly.
        if let Some((id, _, from)) = app.jump_settle {
            app.jump_settle = Some((
                id,
                Instant::now() - Duration::from_secs_f32(JUMP_SETTLE_SECS + 0.01),
                from,
            ));
        }
        assert_eq!(app.bar_jump()[0], 0.0);
        let _ = app.update(Message::StripHover(None));
        assert!(app.jump_settle.is_none());
        assert!(app.bar_jump()[0] > 0.0, "leaving hover resumes the hop");
    }

    #[test]
    fn fired_reminder_does_not_refire_after_reload() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[(DUE_TITLE, "")]);
        let _ = app.store.save();
        let _ = app.update(Message::ReminderTick);
        assert_eq!(app.pulsing.len(), 1);
        drop(app);

        let mut app = app_in(&dir);
        let _ = app.update(Message::ReminderTick);
        assert!(app.pulsing.is_empty());
        assert!(app.bar_jump().is_empty());
        assert!(app.bar_pulse().is_empty());
    }

    #[test]
    fn opening_note_stops_pulse() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[(DUE_TITLE, "")]);
        let _ = app.update(Message::ReminderTick);
        settle(&mut app);
        assert!(app.animating, "the alert keeps frames running");
        let _ = app.update(Message::BarClicked(0));
        assert!(!app.pulsing.contains(&ids[0]));
        let _ = app.update(Message::ClosePanel);
        settle(&mut app);
        assert!(!app.animating, "no frames once nothing alerts");
    }

    #[test]
    fn body_edit_does_not_rearm_legacy_reminder() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = NoteStore::load(dir.path().join("notes.json"));
            let id = store.add_note(&crate::note::PALETTE);
            let note = store.note_mut(id).unwrap();
            note.title = "Call @15:00".into();
            note.updated_at = chrono::Utc::now() - chrono::Duration::days(3);
            note.reminder_set_at = None;
            store.save().unwrap();
        }
        let mut app = app_in(&dir);
        // The frozen anchor is saved by the next save tick past the debounce.
        assert!(app.store.is_dirty());
        std::thread::sleep(crate::store::DEBOUNCE + Duration::from_millis(50));
        let _ = app.update(Message::SaveTick);
        assert!(!app.store.is_dirty(), "the frozen anchor gets saved");
        let saved = NoteStore::load(dir.path().join("notes.json"));
        assert!(!saved.is_dirty());
        assert!(saved.notes()[0].reminder_set_at.is_some());
        app.window_size = Size::new(1400.0, 900.0);
        let _ = app.update(Message::ReminderTick);
        let fired = app.store.notes()[0].reminder_fired;
        assert!(fired.is_some());

        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        let _ = app.update(Message::NoteEdited(text_editor::Action::Edit(
            text_editor::Edit::Insert('x'),
        )));
        assert!(app.store.notes()[0].content.contains('x'));
        let _ = app.update(Message::ClosePanel);
        settle(&mut app);
        let _ = app.update(Message::ReminderTick);
        assert_eq!(app.store.notes()[0].reminder_fired, fired);
        assert!(app.pulsing.is_empty());
        assert!(!app.reminders_pending());
    }

    #[test]
    fn hidden_notes_do_not_pulse() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[(DUE_TITLE, "")]);
        let _ = app.update(Message::ReminderTick);
        let _ = app.update(Message::ToggleVisibility);
        settle(&mut app);
        assert!(!app.animating, "no frames while hidden");
        assert!(app.pulsing.contains(&ids[0]));
        let _ = app.update(Message::ToggleVisibility);
        assert!(app.animating, "the pulse resumes once shown");
        settle(&mut app);
        assert!(app.animating);
    }

    #[test]
    fn stacked_reminder_pulses_its_stack_bar() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("Top", ""), (DUE_TITLE, ""), ("Other", "")]);
        assert!(app.store.stack(ids[1], ids[0]));
        let _ = app.update(Message::ReminderTick);
        let jump = app.bar_jump();
        assert_eq!(jump.len(), 2);
        assert!(jump[0] > 0.0);
        assert_eq!(jump[1], 0.0);
    }

    #[test]
    fn editing_the_tag_moves_the_reminder_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_note(&dir);
        let id = app.active_note.unwrap();
        let anchor = |app: &App| app.store.notes()[0].reminder_set_at;
        let _ = app.update(Message::TitleEdited("Call".into()));
        assert_eq!(anchor(&app), None);
        let _ = app.update(Message::TitleEdited("Call @tue".into()));
        let set = anchor(&app);
        assert!(set.is_some());
        // Other edits to the title keep it.
        let _ = app.update(Message::TitleEdited("Call Bob @tue".into()));
        assert_eq!(anchor(&app), set);
        app.store.note_mut(id).unwrap().reminder_set_at = None;
        let _ = app.update(Message::TitleEdited("Call Bob @wed".into()));
        assert!(anchor(&app).is_some());
    }

    #[test]
    fn reminder_picker_done_writes_absolute_tag() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_note(&dir);
        let _ = app.update(Message::TitleEdited("Call".into()));
        let _ = app.update(Message::ToggleReminderPicker);
        assert!(app.reminder_picker.is_some());
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
        let _ = app.update(Message::ReminderPickerDay(date));
        if let Some(draft) = &mut app.reminder_picker {
            draft.hour = 16;
            draft.minute = 45;
        }
        let _ = app.update(Message::ReminderPickerDone);
        assert!(app.reminder_picker.is_none());
        let note = &app.store.notes()[0];
        assert_eq!(note.title, "Call @2026-11-03 16:45");
        assert!(note.reminder_set_at.is_some());
    }

    #[test]
    fn reminder_picker_clear_strips_tag_and_dismiss_keeps_title() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_note(&dir);
        let _ = app.update(Message::TitleEdited("Call @15:00".into()));
        let _ = app.update(Message::OpenReminderPicker);
        assert!(app.reminder_picker.is_some());
        let _ = app.update(Message::CloseColorBubble);
        assert!(app.reminder_picker.is_none());
        assert_eq!(app.store.notes()[0].title, "Call @15:00");
        let _ = app.update(Message::OpenReminderPicker);
        let _ = app.update(Message::ReminderPickerClear);
        assert!(app.reminder_picker.is_none());
        assert_eq!(app.store.notes()[0].title, "Call");
        assert!(app.store.notes()[0].reminder_set_at.is_none());
    }

    #[test]
    fn directory_named_like_text_file_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.txt");
        std::fs::create_dir(&path).unwrap();
        assert_eq!(read_text_file(&path), None);
    }

    #[cfg(unix)]
    #[test]
    fn fifo_named_like_text_file_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.txt");
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        // Reading a FIFO with no writer blocks, so read on another thread.
        let (tx, rx) = std::sync::mpsc::channel();
        let read = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(read_text_file(&read));
        });
        let text = rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(text, Ok(None), "a FIFO is not read as text");
    }

    #[test]
    fn dropping_empty_text_file_on_bar_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = app_with_notes(&dir, &[("a", "first")]);
        let empty = file_in(&dir, "empty.txt", b"\n");
        let _ = app.update(Message::StripFileDropped(Some(0), empty.clone()));
        assert_eq!(app.store.notes()[0].content, "first");
        assert!(!app.store.is_dirty());
        // Off the bars it still makes a (blank) note.
        let _ = app.update(Message::StripFileDropped(None, empty));
        assert_eq!(app.store.notes().len(), 2);
        assert_eq!(new_note(&app).title, "empty");
    }

    #[test]
    fn clicking_stack_bar_opens_the_pulsing_member() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("Top", ""), ("Middle", ""), (DUE_TITLE, "")]);
        assert!(app.store.stack(ids[1], ids[0]));
        assert!(app.store.stack(ids[2], ids[0]));
        let _ = app.update(Message::ReminderTick);
        assert!(app.pulsing.contains(&ids[2]));
        let _ = app.update(Message::BarClicked(0));
        assert_eq!(app.active_note, Some(ids[2]));
        assert!(app.pulsing.is_empty());
        settle(&mut app);
        // Once nothing pulses, the bar opens its top again.
        let _ = app.update(Message::BarClicked(0));
        assert_eq!(app.active_note, Some(ids[0]));
        settle(&mut app);
        // Clicking the open note's bar still closes it.
        let _ = app.update(Message::BarClicked(0));
        assert!(!app.morph.is_opening());
    }

    #[test]
    fn real_click_on_stack_bar_opens_the_pulsing_member() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("Top", ""), (DUE_TITLE, "")]);
        assert!(app.store.stack(ids[1], ids[0]));
        let _ = app.update(Message::ReminderTick);
        let bar = app.strip_layout().bars[0];
        let _ = app.update(Message::DragStart(0, bar.center().y));
        let _ = app.update(Message::DragEnd);
        assert_eq!(app.active_note, Some(ids[1]));
    }

    #[test]
    fn drop_on_stack_bar_while_top_collapses_goes_to_member() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("Top", "top"), ("Next", "next")]);
        assert!(app.store.stack(ids[1], ids[0]));
        app.sync_entries();
        let _ = app.start_collapse(ids[0]);
        let path = file_in(&dir, "x.txt", b"more");
        let _ = app.update(Message::StripFileDropped(Some(0), path));
        assert_eq!(app.store.notes().len(), 2);
        assert_eq!(app.store.notes()[1].content, "next\n\nmore");
        assert_eq!(app.store.notes()[0].content, "top");
    }

    #[test]
    fn delete_survives_hotkey_mid_fold() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_note(&dir, "first");
        let id = app.store.notes()[0].id;
        let _ = app.update(Message::DeleteNote(id));
        tick_a_little(&mut app);
        assert_eq!(app.active_note, Some(id));
        let _ = app.update(Message::HotkeyPressed);
        settle(&mut app);
        assert!(app.store.notes().iter().all(|n| n.id != id));
        assert_eq!(app.store.notes().len(), 1);
        assert!(app.active_note.is_some_and(|active| active != id));
    }

    #[test]
    fn switching_notes_mid_drag_keeps_the_drag_off_the_new_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1"), ("b", "2")]);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        let start = app.note_target_rect().center();
        let _ = app.update(Message::CursorMoved(start));
        let _ = app.update(Message::NoteDragStart);
        let _ = app.update(Message::CursorMoved(start - iced::Vector::new(40.0, 30.0)));
        let _ = app.update(Message::BarClicked(1));
        assert_eq!(app.active_note, Some(ids[1]));
        assert!(app.note_drag.is_none() && app.note_drag_pos.is_none());
        let _ = app.update(Message::MouseButton(false));
        assert!(
            app.store.notes()[0].position.is_some(),
            "the old note keeps its drag"
        );
        assert_eq!(app.store.notes()[1].position, None);
    }

    #[test]
    fn switching_notes_mid_resize_keeps_the_resize_off_the_new_note() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, ids) = app_with_notes(&dir, &[("a", "1"), ("b", "2")]);
        let _ = app.update(Message::BarClicked(0));
        settle(&mut app);
        let corner = app.note_target_rect().position();
        let _ = app.update(Message::CursorMoved(corner));
        let _ = app.update(Message::ResizeStart(Edges {
            left: true,
            top: true,
            ..Edges::default()
        }));
        let _ = app.update(Message::CursorMoved(corner - iced::Vector::new(30.0, 30.0)));
        let _ = app.update(Message::BarClicked(1));
        assert_eq!(app.active_note, Some(ids[1]));
        assert!(app.note_resize.is_none());
        let _ = app.update(Message::MouseButton(false));
        assert!(
            app.store.notes()[0].size.is_some(),
            "the old note keeps its size"
        );
        assert_eq!(app.store.notes()[1].size, None);
    }

    /// `app` with auto-hide switched on (as the toggle does, without a step).
    fn with_auto_hide(mut app: App) -> App {
        if !app.settings.settings().is_on(SettingToggle::AutoHide) {
            app.settings.settings_mut().toggle(SettingToggle::AutoHide);
        }
        app
    }

    /// An idle app with one note and auto-hide on, still Shown.
    fn auto_hide_app(dir: &tempfile::TempDir) -> App {
        let mut app = app_in(dir);
        app.store.add_note(&crate::note::PALETTE);
        app.window_size = Size::new(1400.0, 900.0);
        with_auto_hide(app)
    }

    /// Frame ticks every 16 ms from `from` for `ms` milliseconds; returns
    /// the last tick's instant.
    fn run_for(app: &mut App, from: Instant, ms: u64) -> Instant {
        let mut now = from;
        let end = from + Duration::from_millis(ms);
        while now < end {
            now += Duration::from_millis(16);
            let _ = app.update(Message::Tick(now));
        }
        now
    }

    /// Lets the idle strip's grace run out and its slide finish.
    fn hide_strip(app: &mut App, from: Instant) -> Instant {
        let t = run_for(app, from, 1200);
        assert!(app.auto_hide.is_hidden(), "the idle strip didn't hide");
        t
    }

    /// A point in the edge zone, halfway down the strip's band.
    fn edge_point(app: &App) -> Point {
        let (top, bottom) = strip_band(&app.strip_layout(), app.settings.settings().bars.gap);
        Point::new(app.window_size.width - 1.0, (top + bottom) / 2.0)
    }

    #[test]
    fn auto_hide_setting_persists_and_starts_shown() {
        assert!(Settings::default().window.auto_hide);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut store = SettingsStore::load(path.clone());
        assert!(store.settings().is_on(SettingToggle::AutoHide));
        assert!(store.settings_mut().toggle(SettingToggle::AutoHide));
        assert!(!store.settings().window.auto_hide);
        store.save().unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["window"]["auto_hide"], serde_json::Value::Bool(false));

        let mut reloaded = SettingsStore::load(path.clone());
        assert!(!reloaded.settings().window.auto_hide);
        assert!(reloaded.settings_mut().toggle(SettingToggle::AutoHide));
        reloaded.save().unwrap();
        let reloaded = SettingsStore::load(path);
        assert!(reloaded.settings().window.auto_hide);
        let app = App::new(NoteStore::load(dir.path().join("notes.json")), reloaded);
        assert_eq!(app.auto_hide.phase(), crate::autohide::Phase::Shown);
        assert_eq!(app.strip_x_offset(), 0.0);

        let mut s = Settings::from_json(r#"{"window": {"auto_hide": false}}"#);
        assert!(!s.window.auto_hide);
        s.reset(SettingsGroup::Window);
        assert!(s.window.auto_hide);
    }

    #[test]
    fn idle_strip_hides_and_only_edge_is_interactive() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let t0 = Instant::now();
        // Shown at first, for the grace.
        let t = run_for(&mut app, t0, 700);
        assert_eq!(app.auto_hide.offset(), 0.0);
        hide_strip(&mut app, t);
        assert_eq!(app.strip_x_offset(), STRIP_WIDTH + HIDE_MARGIN);

        let edge = edge_point(&app);
        assert!(app.is_interactive(edge));
        let bar = app.strip_layout().bars[0].center();
        assert!(!app.is_interactive(bar), "the hidden bar took the click");
        let column = Point::new(app.window_size.width - 30.0, edge.y);
        assert!(!app.is_interactive(column));
        let left_of_edge = Point::new(app.window_size.width - 3.0, edge.y);
        assert!(!app.is_interactive(left_of_edge));
        let below_band = Point::new(edge.x, app.window_size.height - 2.0);
        assert!(!app.is_interactive(below_band));
    }

    #[test]
    fn edge_dwell_reveals_strip() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        hide_strip(&mut app, Instant::now());
        let edge = edge_point(&app);
        let before = Instant::now();
        let _ = app.update(Message::CursorMoved(edge));
        let after = Instant::now();
        assert_eq!(
            app.auto_hide
                .next_deadline()
                .map(|d| d >= before + REVEAL_DWELL),
            Some(true),
            "no wake-up for the dwell"
        );
        let _ = app.update(Message::Tick(before + Duration::from_millis(149)));
        assert!(app.auto_hide.is_hidden(), "revealed before the dwell");
        let t = run_for(&mut app, after + REVEAL_DWELL, 300);
        assert_eq!(app.auto_hide.offset(), 0.0, "the dwell didn't reveal");
        // Resting on the strip keeps it shown.
        run_for(&mut app, t, 2000);
        assert_eq!(app.auto_hide.offset(), 0.0);
        assert!(app.is_interactive(app.strip_layout().bars[0].center()));

        // Leaving the edge zone early resets the dwell.
        let _ = app.update(Message::CursorMoved(Point::new(100.0, 100.0)));
        let t = hide_strip(&mut app, t);
        let _ = app.update(Message::CursorMoved(edge));
        let _ = app.update(Message::CursorMoved(Point::new(100.0, edge.y)));
        assert_eq!(app.auto_hide.next_deadline(), None);
        run_for(&mut app, t, 500);
        assert!(app.auto_hide.is_hidden());
    }

    #[test]
    fn open_note_does_not_block_hiding_and_stays_interactive() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let _ = app.update(Message::BarClicked(0));
        let note = app.note_target_rect();
        let _ = app.update(Message::CursorMoved(note.center()));
        let t = hide_strip(&mut app, Instant::now());
        assert!(app.note_open());
        // The note stays where it is, fully usable.
        assert_eq!(app.note_frame().map(|f| f.rect), Some(note));
        assert!(app.is_interactive(note.center()));
        run_for(&mut app, t, 500);
        assert!(app.auto_hide.is_hidden());

        // Folding closed, it heads for its bar's hidden place.
        let _ = app.update(Message::ClosePanel);
        tick_until(&mut app, |app| app.morph.progress() < 0.5);
        let frame = app.note_frame().expect("still folding").rect;
        let bar = app.strip_layout().bars[0];
        let shown = morph_frame(bar, app.note_target_rect(), app.morph.progress()).rect;
        assert!(frame.x > shown.x, "folds toward the shown bar");
    }

    #[test]
    fn panel_open_blocks_hiding() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let _ = app.update(Message::ToggleSettings);
        let t = run_for(&mut app, Instant::now(), 3000);
        assert_eq!(app.auto_hide.offset(), 0.0, "hid under the settings");
        let _ = app.update(Message::CloseSettings);
        let t = run_for(&mut app, t, 400);
        assert!(!app.settings_open);
        run_for(&mut app, t, 1200);
        assert!(
            app.auto_hide.is_hidden(),
            "didn't hide once settings closed"
        );

        // A search opened while hidden reveals the strip and keeps it.
        let _ = app.update(cmd_f());
        let t = run_for(&mut app, Instant::now(), 300);
        assert_eq!(app.auto_hide.offset(), 0.0);
        run_for(&mut app, t, 2000);
        assert_eq!(app.auto_hide.offset(), 0.0, "hid under the search");
    }

    #[test]
    fn reminder_fire_keeps_strip_hidden_with_alert_only() {
        let dir = tempfile::tempdir().unwrap();
        let (app, ids) = app_with_notes(&dir, &[("Plain", ""), (DUE_TITLE, "")]);
        let mut app = with_auto_hide(app);
        hide_strip(&mut app, Instant::now());
        let _ = app.update(Message::ReminderTick);
        assert!(app.pulsing.contains(&ids[1]));
        assert!(app.auto_hide.is_hidden(), "the strip stays auto-hidden");
        assert!(app.animating, "the alert still runs frames");
        let alerts = app.bar_alert();
        assert_eq!(alerts, vec![false, true]);
        // Edge dwell still reveals the full strip.
        let before = Instant::now();
        let _ = app.update(Message::CursorMoved(edge_point(&app)));
        let t = run_for(&mut app, before + REVEAL_DWELL, 300);
        assert_eq!(app.auto_hide.offset(), 0.0, "edge dwell should reveal");
        // Opening the alerting note clears it; leave the strip so it can hide.
        let _ = app.update(Message::BarClicked(1));
        let _ = app.update(Message::ClosePanel);
        assert!(app.pulsing.is_empty());
        let _ = app.update(Message::CursorMoved(Point::new(100.0, 100.0)));
        run_for(&mut app, t.max(Instant::now()), 1500);
        assert!(app.auto_hide.is_hidden());
    }

    #[test]
    fn auto_hide_toggle_skips_other_settings_work() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        // With a window id, the fall-through would return the tray and
        // dock tasks (and sync the hotkey).
        app.window_id = Some(window::Id::unique());
        let task = app.handle(Message::SettingToggled(SettingToggle::AutoHide));
        assert_eq!(task.units(), 0, "the toggle ran other settings code");
        let other = app.handle(Message::SettingToggled(SettingToggle::DockIcon));
        assert!(other.units() > 0, "control: other toggles do apply");
    }

    #[test]
    fn wake_is_scheduled_once_per_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        hide_strip(&mut app, Instant::now());
        let _ = app.update(Message::CursorMoved(edge_point(&app)));
        let deadline = app.auto_hide.next_deadline().expect("no dwell started");
        assert_eq!(app.auto_hide_wake, Some(deadline));
        for _ in 0..3 {
            let _ = app.update(Message::CursorMoved(edge_point(&app)));
            let _ = app.update(Message::CursorPolled(Some(edge_point(&app))));
            assert_eq!(app.auto_hide.next_deadline(), Some(deadline));
            assert_eq!(app.auto_hide_wake, Some(deadline), "wake rescheduled");
        }
        // The wake itself clears it.
        let _ = app.handle(Message::AutoHideWake(deadline));
        assert_eq!(app.auto_hide_wake, None);
    }

    #[test]
    fn stale_file_hover_clears_on_cursor_move() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let _ = app.update(Message::FileHovered);
        let t = run_for(&mut app, Instant::now(), 2000);
        assert_eq!(app.auto_hide.offset(), 0.0, "hid under a file hover");
        // The drag ended without an event; the next cursor move shows it.
        let _ = app.update(Message::CursorMoved(Point::new(100.0, 100.0)));
        assert!(!app.file_hover);
        run_for(&mut app, t, 1200);
        assert!(app.auto_hide.is_hidden(), "stuck shown");
    }

    #[test]
    fn hotkey_reveals_strip() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        hide_strip(&mut app, Instant::now());
        let _ = app.update(Message::HotkeyPressed);
        assert_eq!(app.auto_hide.phase(), crate::autohide::Phase::Revealing);
        let t = run_for(&mut app, Instant::now(), 300);
        assert_eq!(app.auto_hide.offset(), 0.0);
        assert!(app.note_open(), "the hotkey's note didn't open");
        // The open note doesn't keep the strip (A14).
        run_for(&mut app, t, 1200);
        assert!(app.auto_hide.is_hidden());
        assert!(app.note_open());

        // So do Cmd/Ctrl+N and the tray items.
        for message in [
            cmd_key_on("n", keyboard::key::Code::KeyN),
            Message::TrayMenu("new".into()),
            Message::TrayMenu("settings".into()),
            Message::TrayMenu("clip".into()),
            Message::AddNote,
        ] {
            let _ = app.update(Message::CloseSettings);
            let _ = app.update(Message::ClosePanel);
            // An empty clipboard's hint (expiry message not delivered here).
            app.clipboard_empty_at = None;
            let t = run_for(&mut app, Instant::now(), 400);
            run_for(&mut app, t, 1200);
            assert!(app.auto_hide.is_hidden());
            let _ = app.update(message.clone());
            assert_eq!(
                app.auto_hide.phase(),
                crate::autohide::Phase::Revealing,
                "{message:?} didn't reveal"
            );
        }
    }

    #[test]
    fn tray_hide_overrides_auto_hide() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let t = hide_strip(&mut app, Instant::now());
        let edge = edge_point(&app);
        let _ = app.update(Message::TrayMenu("toggle".into()));
        assert!(!app.visible);
        // Fully hidden: no edge zone, no reveal.
        assert!(!app.is_interactive(edge));
        let _ = app.update(Message::CursorMoved(edge));
        let t = run_for(&mut app, t, 1000);
        assert!(!app.visible);
        assert!(!app.is_interactive(edge));
        // Show brings the strip back, then auto-hide applies again.
        let _ = app.update(Message::TrayMenu("toggle".into()));
        let _ = app.update(Message::CursorMoved(Point::new(100.0, 100.0)));
        assert!(app.visible);
        assert_eq!(app.auto_hide.offset(), 0.0);
        hide_strip(&mut app, t);
    }

    #[test]
    fn turning_auto_hide_off_reveals() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let t = hide_strip(&mut app, Instant::now());
        let _ = app.update(Message::SettingToggled(SettingToggle::AutoHide));
        assert!(!app.settings.settings().window.auto_hide);
        assert_eq!(app.auto_hide.offset(), 0.0);
        assert_eq!(app.strip_x_offset(), 0.0);
        assert!(app.is_interactive(app.strip_layout().bars[0].center()));
        run_for(&mut app, t, 2000);
        assert_eq!(app.auto_hide.offset(), 0.0);
    }

    /// Docks `app` the way `dock_window` would without passthrough.
    fn dock_without_passthrough(app: &mut App) {
        app.window_size.width = app.window_width(false);
    }

    #[test]
    fn linux_hidden_window_is_a_sliver() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let open = app.settings.settings().open_width();
        assert_eq!(app.window_width(false), STRIP_WIDTH);
        assert_eq!(app.window_width(true), open);
        hide_strip(&mut app, Instant::now());
        assert_eq!(app.window_width(false), EDGE_SLIVER);
        assert_eq!(app.window_width(true), open, "passthrough stays wide");

        // Anywhere inside the sliver within the band counts as the edge zone.
        dock_without_passthrough(&mut app);
        let _ = app.update(Message::CursorMoved(Point::new(0.5, edge_point(&app).y)));
        assert!(app.auto_hide.next_deadline().is_some(), "no dwell started");
        let _ = app.update(Message::CursorLeftWindow);
        assert_eq!(
            app.auto_hide.next_deadline(),
            None,
            "leaving kept the dwell"
        );
        // Entering alone, without a move inside, doesn't count: no event
        // maps to a message, so the pointer stays unknown.
        run_for(&mut app, Instant::now(), 300);
        assert_eq!(app.pointer, None, "a position was faked");
        assert!(app.auto_hide.is_hidden(), "revealed without a move");
    }

    #[test]
    fn sliver_width_restored_once_revealing() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let t = hide_strip(&mut app, Instant::now());
        dock_without_passthrough(&mut app);
        let entered = t.max(Instant::now());
        let _ = app.update(Message::CursorMoved(edge_point(&app)));
        let _ = app.update(Message::Tick(
            entered + REVEAL_DWELL + Duration::from_millis(5),
        ));
        assert_eq!(app.auto_hide.phase(), crate::autohide::Phase::Revealing);
        assert!(app.strip_x_offset() > 0.0, "the slide already ran");
        assert_eq!(
            app.window_width(false),
            STRIP_WIDTH,
            "a revealing strip must get its docked width back before sliding"
        );
    }

    #[test]
    fn sliver_not_used_while_note_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let _ = app.update(Message::BarClicked(0));
        let note = app.note_target_rect();
        let _ = app.update(Message::CursorMoved(note.center()));
        hide_strip(&mut app, Instant::now());
        assert!(app.note_open());
        let open = app.settings.settings().open_width();
        assert_eq!(app.window_width(false), open);
        dock_without_passthrough(&mut app);

        // The wide window's rightmost 2 px within the band are the edge.
        let edge = edge_point(&app);
        assert!(app.in_edge(edge));
        assert!(!app.in_edge(Point::new(app.window_size.width - 3.0, edge.y)));
        assert!(!app.in_edge(Point::new(edge.x, app.window_size.height - 2.0)));
        let _ = app.update(Message::CursorMoved(edge));
        assert!(app.auto_hide.next_deadline().is_some(), "no dwell started");

        // Once the note closes, the hidden window shrinks.
        let _ = app.update(Message::CursorMoved(app.note_target_rect().center()));
        let _ = app.update(Message::ClosePanel);
        tick_until(&mut app, |app| app.active_note.is_none());
        assert!(app.auto_hide.is_hidden());
        assert_eq!(app.window_width(false), EDGE_SLIVER);
    }

    #[test]
    fn hide_start_clears_stale_hover() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        let bar = app.strip_layout().bars[0].center();
        let _ = app.update(Message::StripHover(Some(bar.y)));
        assert!(app.cursor_y.is_some());
        // The pointer leaves without the strip seeing it go.
        let _ = app.update(Message::CursorLeftWindow);
        let t = run_for(&mut app, Instant::now(), 900);
        assert!(app.auto_hide.offset() > 0.0, "didn't start hiding");
        assert_eq!(app.cursor_y, None, "stale hover kept");
        assert!(app.hover_bar.is_none());
        run_for(&mut app, t, 400);
    }

    #[test]
    fn sliver_edge_requires_band() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        hide_strip(&mut app, Instant::now());
        dock_without_passthrough(&mut app);
        let mid = edge_point(&app).y;
        assert!(app.in_edge(Point::new(1.0, mid)));
        assert!(!app.in_edge(Point::new(-5.0, mid)), "left of the sliver");
        assert!(!app.in_edge(Point::new(3.0, mid)), "right of the sliver");
        let below = Point::new(1.0, app.window_size.height - 2.0);
        assert!(!app.in_edge(below), "below the band");
        let _ = app.update(Message::CursorMoved(below));
        assert_eq!(app.auto_hide.next_deadline(), None);
    }

    #[test]
    fn shrink_clears_pointer_left_outside() {
        let p = |x| Some(Point::new(x, 300.0));
        // Shrinking from the strip to the sliver: a pointer that was over
        // the strip's left part is now outside the window.
        assert_eq!(shifted_pointer(p(10.0), STRIP_WIDTH, EDGE_SLIVER), None);
        // One at the right border stays, at its new spot.
        assert_eq!(
            shifted_pointer(p(STRIP_WIDTH - 1.0), STRIP_WIDTH, EDGE_SLIVER),
            p(EDGE_SLIVER - 1.0)
        );
        // Widening keeps the screen spot.
        assert_eq!(
            shifted_pointer(p(1.0), EDGE_SLIVER, STRIP_WIDTH),
            p(STRIP_WIDTH - 1.0)
        );
        assert_eq!(shifted_pointer(None, EDGE_SLIVER, STRIP_WIDTH), None);
    }

    /// `app` docked to `edge`.
    fn on_edge(mut app: App, edge: Edge) -> App {
        app.settings.settings_mut().window.edge = edge;
        app
    }

    /// The middle of the strip's band, along the edge.
    fn band_middle(app: &App) -> f32 {
        let (start, end) = strip_band(&app.strip_layout(), app.settings.settings().bars.gap);
        (start + end) / 2.0
    }

    #[test]
    fn dock_window_follows_edge() {
        let monitor = Size::new(1440.0, 900.0);
        let s = Settings::default();
        let open = s.open_width();
        let strip = App::docked_width(&s, false, false);
        let wide = App::docked_width(&s, true, false);
        assert_eq!((strip, wide), (STRIP_WIDTH, open));
        let rect = |x, y, w, h| Rectangle::new(Point::new(x, y), Size::new(w, h));

        // Without passthrough: the thickness away from the edge, the strip
        // length along it, flush to the edge and centred along it.
        let frame = |edge, thickness| dock_frame(edge, monitor, 25.0, false, thickness, 0.9);
        assert_eq!(
            frame(Edge::Right, strip),
            rect(1440.0 - STRIP_WIDTH, 45.0, STRIP_WIDTH, 810.0)
        );
        assert_eq!(
            frame(Edge::Right, wide),
            rect(1440.0 - open, 45.0, open, 810.0)
        );
        assert_eq!(
            frame(Edge::Left, strip),
            rect(0.0, 45.0, STRIP_WIDTH, 810.0)
        );
        assert_eq!(frame(Edge::Left, wide), rect(0.0, 45.0, open, 810.0));
        // Top: as tall as the thickness, below the menu bar, and no taller
        // than the screen below it.
        assert_eq!(
            frame(Edge::Top, strip),
            rect(72.0, 25.0, 1296.0, STRIP_WIDTH)
        );
        assert!(open > 875.0);
        assert_eq!(frame(Edge::Top, wide), rect(72.0, 25.0, 1296.0, 875.0));

        // With passthrough the window covers the monitor (below the menu
        // bar on Top), whatever the thickness.
        let covering = |edge| dock_frame(edge, monitor, 25.0, true, strip, 0.9);
        assert_eq!(covering(Edge::Right), rect(0.0, 0.0, 1440.0, 900.0));
        assert_eq!(covering(Edge::Left), rect(0.0, 0.0, 1440.0, 900.0));
        assert_eq!(covering(Edge::Top), rect(0.0, 25.0, 1440.0, 875.0));
    }

    #[test]
    fn strip_band_and_edge_zone_per_edge() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = auto_hide_app(&dir);
        app.settings.settings_mut().window.auto_hide = false;
        let app = on_edge(app, Edge::Left);
        assert_eq!(
            app.strip_bounds(),
            Rectangle::new(Point::ORIGIN, Size::new(STRIP_WIDTH, 900.0))
        );
        let mid = band_middle(&app);
        assert!(app.in_edge(Point::new(1.0, mid)));
        assert!(!app.in_edge(Point::new(1399.0, mid)));
        assert!(!app.in_edge(Point::new(3.0, mid)));
        assert!(app.over_strip(Point::new(STRIP_WIDTH - 1.0, mid)));
        assert!(!app.over_strip(Point::new(1399.0, mid)));
        assert!(!app.in_edge(Point::new(1.0, 898.0)), "outside the band");

        let app = on_edge(app, Edge::Top);
        assert_eq!(
            app.strip_bounds(),
            Rectangle::new(Point::ORIGIN, Size::new(1400.0, STRIP_WIDTH))
        );
        let mid = band_middle(&app);
        assert!(app.in_edge(Point::new(mid, 1.0)));
        assert!(!app.in_edge(Point::new(mid, 899.0)));
        assert!(!app.in_edge(Point::new(1399.0, 1.0)), "outside the band");
        assert!(app.over_strip(Point::new(mid, STRIP_WIDTH - 1.0)));
        assert!(!app.over_strip(Point::new(mid, STRIP_WIDTH + 1.0)));
        assert!(app.is_interactive(Point::new(mid, 1.0)));
    }

    #[test]
    fn auto_hide_edge_zone_follows_edge() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = on_edge(auto_hide_app(&dir), Edge::Left);
        hide_strip(&mut app, Instant::now());
        let shift = crate::bar_strip::hide_translation(Edge::Left, app.strip_x_offset());
        let strip = app.strip_layout();
        for rect in strip.bars.iter().chain([&strip.actions_button]) {
            let drawn = *rect + shift;
            assert!(drawn.x + drawn.width < 0.0, "{drawn:?} still shows");
        }

        let mid = band_middle(&app);
        let right = Point::new(1399.0, mid);
        assert!(!app.is_interactive(right));
        let _ = app.update(Message::CursorMoved(right));
        assert_eq!(
            app.auto_hide.next_deadline(),
            None,
            "a right-edge dwell started on a Left strip"
        );

        let left = Point::new(1.0, mid);
        assert!(app.is_interactive(left));
        let entered = Instant::now();
        let _ = app.update(Message::CursorMoved(left));
        assert!(app.auto_hide.next_deadline().is_some(), "no dwell started");
        let _ = app.update(Message::Tick(
            entered + REVEAL_DWELL + Duration::from_millis(5),
        ));
        assert_eq!(app.auto_hide.phase(), crate::autohide::Phase::Revealing);
    }

    #[test]
    fn menu_bar_reveals_a_hidden_top_strip() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = on_edge(auto_hide_app(&dir), Edge::Top);
        hide_strip(&mut app, Instant::now());
        // Above the window, in the menu bar, within the band.
        let menu_bar = Point::new(band_middle(&app), -10.0);
        assert!(app.is_interactive(menu_bar));
        let entered = Instant::now();
        let _ = app.update(Message::CursorMoved(menu_bar));
        assert!(app.auto_hide.next_deadline().is_some(), "no dwell started");
        let _ = app.update(Message::Tick(
            entered + REVEAL_DWELL + Duration::from_millis(5),
        ));
        assert_eq!(app.auto_hide.phase(), crate::autohide::Phase::Revealing);
    }

    #[test]
    fn top_window_fits_a_short_screen() {
        let monitor = Size::new(1366.0, 768.0);
        let open = App::docked_width(&Settings::default(), true, false);
        assert!(open > 768.0, "the open window must not fit");
        let top = dock_frame(Edge::Top, monitor, 25.0, false, open, 0.9);
        assert_eq!((top.y, top.height), (25.0, 743.0), "{top:?}");
        let right = dock_frame(Edge::Right, monitor, 25.0, false, open, 0.9);
        assert_eq!(right.width, open);
    }

    /// `app` with a window on a 1440×900 monitor below a 25 px menu bar,
    /// docked to its edge.
    fn docked(mut app: App) -> App {
        app.window_id = Some(window::Id::unique());
        app.monitor = Some(Size::new(1440.0, 900.0));
        app.top_inset = 25.0;
        let _ = app.dock_window();
        app
    }

    /// `p`, seen in a window whose origin was `old`, in the current window:
    /// `None` once outside it.
    fn moved_into(app: &App, old: Point, p: Point) -> Option<Point> {
        let p = p + (old - app.window_origin.unwrap());
        Some(p).filter(|p| Rectangle::new(Point::ORIGIN, app.window_size).contains(*p))
    }

    #[test]
    fn edge_change_drops_a_pointer_outside_the_new_window() {
        let dir = tempfile::tempdir().unwrap();
        let (app, _) = app_with_notes(&dir, &[("a", "x")]);
        let mut app = docked(app);
        let old = app.window_origin.unwrap();
        let (pointer, cursor) = (Point::new(100.0, 10.0), Point::new(100.0, 400.0));
        app.pointer = Some(pointer);
        app.last_cursor = Some(cursor);
        let _ = app.update(Message::EdgeChosen(Edge::Top));
        assert_eq!(app.pointer, moved_into(&app, old, pointer));
        assert_eq!(app.last_cursor, moved_into(&app, old, cursor));
        if SUPPORTS_PASSTHROUGH {
            assert_eq!(app.pointer, None, "the pointer is in the menu bar");
            assert!(app.last_cursor.is_some());
        }
    }

    #[test]
    fn reset_window_reanchors_open_panels() {
        let dir = tempfile::tempdir().unwrap();
        let (app, _) = app_with_notes(&dir, &[("a", "x")]);
        let mut app = docked(on_edge(app, Edge::Top));
        let _ = app.update(Message::ToggleSettings);
        settle(&mut app);
        let top_anchor = app.settings_anchor_y;
        let old = app.window_origin.unwrap();
        let (pointer, cursor) = (Point::new(100.0, -30.0), Point::new(100.0, 400.0));
        app.pointer = Some(pointer);
        app.last_cursor = Some(cursor);

        let _ = app.update(Message::ResetGroup(SettingsGroup::Window));
        assert_eq!(app.edge(), Edge::Right);
        let expected = along_center(app.strip_layout().settings_anchor(), Edge::Right);
        assert_ne!(expected, top_anchor);
        assert_eq!(app.settings_anchor_y, expected, "settings not re-anchored");
        assert_eq!(app.pointer, moved_into(&app, old, pointer));
        assert_eq!(app.last_cursor, moved_into(&app, old, cursor));
        assert_eq!(app.pointer, None, "the pointer is outside the window");
    }

    #[test]
    fn linux_sliver_axis_per_edge() {
        let monitor = Size::new(1440.0, 900.0);
        let sliver = App::docked_width(&Settings::default(), false, true);
        assert_eq!(sliver, EDGE_SLIVER);
        let top = dock_frame(Edge::Top, monitor, 0.0, false, sliver, 0.9);
        assert_eq!((top.y, top.width, top.height), (0.0, 1296.0, 2.0));
        let left = dock_frame(Edge::Left, monitor, 0.0, false, sliver, 0.9);
        assert_eq!((left.x, left.width, left.height), (0.0, 2.0, 810.0));

        // The Top sliver's edge zone is its 2 px height, within the band.
        let dir = tempfile::tempdir().unwrap();
        let mut app = on_edge(auto_hide_app(&dir), Edge::Top);
        hide_strip(&mut app, Instant::now());
        assert_eq!(app.window_width(false), EDGE_SLIVER);
        app.window_size = top.size();
        assert!(app.is_sliver());
        let mid = band_middle(&app);
        assert!(app.in_edge(Point::new(mid, 1.0)));
        assert!(!app.in_edge(Point::new(mid, 3.0)), "below the sliver");
        assert!(!app.in_edge(Point::new(1.0, 1.0)), "outside the band");
    }

    #[test]
    fn note_unfolds_away_from_edge() {
        let dir = tempfile::tempdir().unwrap();
        let (app, ids) = app_with_notes(&dir, &[("a", "x"), ("b", "y")]);
        let mut app = on_edge(app, Edge::Top);
        let bar = app.strip_layout().bars[1];
        let _ = app.update(Message::BarClicked(1));
        assert_eq!(app.active_note, Some(ids[1]));
        let source = app.note_frame().unwrap().rect;
        assert!((source.x - bar.x).abs() < 0.01 && (source.y - bar.y).abs() < 0.01);
        assert!((source.width - bar.width).abs() < 0.01);
        assert!((source.height - bar.height).abs() < 0.01);
        settle(&mut app);
        let rect = app.note_frame().unwrap().rect;
        assert_eq!(rect, app.note_target_rect());
        assert_eq!(rect.y, STRIP_WIDTH + NOTE_GAP, "below the strip");
        assert!(
            (rect.center_x() - bar.center_x()).abs() < 0.01,
            "centred on its bar"
        );

        // A note with a saved position keeps it.
        app.store.note_mut(ids[0]).unwrap().position = Some([300.0, 200.0]);
        let _ = app.update(Message::BarClicked(0));
        assert_eq!(app.note_target_rect().position(), Point::new(300.0, 200.0));
    }

    #[test]
    fn edge_change_keeps_note_open_and_closes_peek() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_open_peek(&dir);
        app.store.add_note(&crate::note::PALETTE);
        app.sync_entries();
        let _ = app.update(Message::BarClicked(1));
        let open = app.active_note.unwrap();
        // The peek of the first bar shows beside the open note.
        app.peek_note = Some(app.store.notes()[0].id);
        app.peek.open();
        app.drag = Some(DragState {
            bar_index: 0,
            origin_y: 10.0,
            current_y: 40.0,
        });
        let monitor = Size::new(1440.0, 900.0);
        app.window_id = Some(window::Id::unique());
        app.monitor = Some(monitor);
        app.top_inset = 25.0;

        let task = app.update(Message::EdgeChosen(Edge::Top));
        assert!(task.units() > 0, "no re-dock");
        let expected = dock_frame(
            Edge::Top,
            monitor,
            25.0,
            SUPPORTS_PASSTHROUGH,
            app.window_width(SUPPORTS_PASSTHROUGH),
            app.settings.settings().window.height_fraction,
        );
        assert_eq!(app.window_size, expected.size(), "not re-docked");
        assert_eq!(app.active_note, Some(open));
        assert!(app.morph.is_opening(), "the note closed");
        assert!(app.peek.is_closed() && app.peek_note.is_none(), "peek kept");
        assert!(app.drag.is_none(), "drag kept");
        let bar = app.strip_layout().bars[app.id_entry(open).unwrap()];
        assert_eq!(app.anchor_y, bar.center_x(), "the note didn't re-anchor");

        app.settings.save().unwrap();
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["window"]["edge"], "top");
    }

    #[test]
    fn drop_on_top_bar_appends() {
        let dir = tempfile::tempdir().unwrap();
        let (app, _) = app_with_notes(&dir, &[("a", "first"), ("b", "second")]);
        let mut app = on_edge(app, Edge::Top);
        let bars = app.strip_layout().bars;
        assert!(bars[1].x > bars[0].x + bars[0].width, "bars side by side");
        let _ = app.update(Message::FileHovered);
        let _ = app.update(Message::CursorMoved(bars[1].center()));
        let _ = app.update(Message::ImageDropped(file_in(&dir, "x.txt", b"more")));
        assert_eq!(app.store.notes()[1].content, "second\n\nmore");
        assert_eq!(app.store.notes()[0].content, "first");
        assert_eq!(app.store.notes().len(), 2);
    }

    #[test]
    fn panels_open_away_from_edge() {
        for edge in [Edge::Left, Edge::Top] {
            let dir = tempfile::tempdir().unwrap();
            let (app, _) = app_with_notes(&dir, &[("a", "x")]);
            let mut app = on_edge(app, edge);
            let strip = app.strip_bounds();
            let away = |rect: Rectangle| match edge {
                Edge::Left => rect.x >= strip.x + strip.width + NOTE_GAP,
                _ => rect.y >= strip.y + strip.height + NOTE_GAP,
            };
            let inside = |app: &App, rect: Rectangle| {
                rect.x >= 0.0
                    && rect.y >= 0.0
                    && rect.x + rect.width <= app.window_size.width
                    && rect.y + rect.height <= app.window_size.height
            };

            let _ = app.update(Message::ToggleSettings);
            settle(&mut app);
            let rect = app.settings_frame().unwrap().rect;
            assert!(
                away(rect) && inside(&app, rect),
                "{edge:?} settings {rect:?}"
            );
            let _ = app.update(Message::CloseSettings);
            settle(&mut app);

            let _ = app.update(Message::ToggleSearch);
            settle(&mut app);
            let rect = app.search_frame().unwrap().rect;
            assert!(away(rect) && inside(&app, rect), "{edge:?} search {rect:?}");
            app.close_search();
            settle(&mut app);

            let _ = app.update(Message::ToggleExport);
            settle(&mut app);
            let rect = app.export_frame().unwrap().rect;
            assert!(away(rect) && inside(&app, rect), "{edge:?} export {rect:?}");
        }
    }
}
