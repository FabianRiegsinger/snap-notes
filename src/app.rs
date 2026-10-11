use crate::animation::{ease_out_cubic, morph_frame, MagnificationState, Morph, MorphFrame};
use crate::autohide::{self, AutoHide};
use crate::bar_strip::{
    along_center, along_span, band, compute_layout, hide_translation, peek_target, stack_target,
    BarStrip, LayoutInput, StripLayout, HIDE_SHIFT, STRIP_WIDTH,
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
    RangeEnd, SettingKey, SettingToggle, Settings, SettingsGroup, SettingsStore, SettingsTab,
};
use crate::settings_panel::{settings_panel, SettingsView, PANEL_MAX_HEIGHT, PANEL_WIDTH};
use crate::store::{Deleted, NoteStore};
use crate::strip_model::{self, Entry};
use crate::theme;
use crate::tray;

use chrono::Datelike;
use iced::widget::{container, mouse_area, opaque, pin, stack, text_editor, Space};
use iced::{
    event, keyboard, mouse, window, Element, Fill, Point, Rectangle, Size, Subscription, Task,
    Vector,
};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use panels::Panel;

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

/// How long the Actions row takes to unfold or fold back, at speed 1.
const ACTIONS_SECS: f32 = 0.18;

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

/// Where a cursor that didn't move is after the window docked to `edge`
/// moved from `old` to `new`: it keeps its screen spot, across the edge (see
/// `pointer_after_resize`) and along it. `None` once it is outside.
fn pointer_after_dock(
    pointer: Option<Point>,
    edge: Edge,
    old: Rectangle,
    new: Rectangle,
) -> Option<Point> {
    let p = pointer_after_resize(
        pointer,
        edge,
        edge.thickness(old.size()),
        edge.thickness(new.size()),
    )?;
    match edge {
        Edge::Right | Edge::Left => {
            let y = p.y + old.y - new.y;
            (0.0..=new.height)
                .contains(&y)
                .then_some(Point::new(p.x, y))
        }
        Edge::Top => {
            let x = p.x + old.x - new.x;
            (0.0..=new.width).contains(&x).then_some(Point::new(x, p.y))
        }
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
    /// The global hotkey's Record button: starts recording the next
    /// modified key press as the hotkey, or cancels the recording.
    HotkeyRecord,
    /// Text typed into a slider's min or max field.
    RangeEdited(SettingKey, RangeEnd, String),
    /// Enter in a slider's min or max field: the field shows the bound in
    /// force again.
    RangeSubmitted(SettingKey, RangeEnd),
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
    /// The trash button on the open peek of the note at this index: the
    /// peek asks whether to delete it.
    PeekDeleteRequest(usize),
    /// Leave edit mode and show the note rendered again.
    EditorBlurred,
    /// Flip the task checkbox on this source line.
    ToggleTask(usize),
    LinkClicked(String),
    TitleEdited(String),
    /// Enter in the title field: its edit is done.
    TitleCommitted,
    /// Whether the title field still has the focus, after a click.
    TitleBlurred(bool),
    NoteHovered(bool),
    NoteDragStart,
    NoteResetPosition,
    /// Drag the settings panel from its top grip.
    SettingsDragStart,
    /// Double-click the settings grip: snap back beside the strip.
    SettingsResetPosition,
    /// Drag the search panel from its top grip.
    SearchDragStart,
    /// Double-click the search grip: snap back beside the strip.
    SearchResetPosition,
    /// A press on the open note's border starts resizing it.
    ResizeStart(Edges),
    ClosePanel,
    /// The open note's trash button: its header asks whether to delete it.
    RequestDelete(Uuid),
    /// "Delete" in the header's or the peek's question: the note folds or
    /// collapses away, then is deleted (and can be undone).
    ConfirmDelete,
    /// "Cancel" in the delete question, or anything else that drops it.
    CancelDelete,
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
    /// Picker: open the reminder as an event in the calendar app.
    ReminderToCalendar,
    /// Picker: start an email with the note and its reminder.
    ReminderByEmail,
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

/// The title field while it is being edited: its text shows as typed, tag
/// and all, until the edit is committed.
struct TitleEdit {
    id: Uuid,
    /// The field's text.
    draft: String,
    /// The reminder tag hidden when the edit began, and its anchor.
    hidden: Option<String>,
    anchor: Option<chrono::DateTime<chrono::Utc>>,
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
    anchor_along: f32,
    pending_delete: Option<Uuid>,
    /// The header or the peek asks whether to delete this note.
    delete_ask: Option<DeleteAsk>,
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
    /// Files and links handed to the system's default app (tests only).
    #[cfg(test)]
    opened: Vec<String>,
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
    /// The settings panel, morphing out of the gear.
    settings_panel: Panel,
    settings_tab: SettingsTab,
    /// Slider min/max fields being typed into, as typed.
    range_drafts: HashMap<(SettingKey, RangeEnd), String>,
    /// The title field's edit in progress, if any.
    title_edit: Option<TitleEdit>,
    /// The strip Actions slot shows New / Search / Settings side by side
    /// (the target; `actions_morph` animates toward it).
    actions_open: bool,
    actions_morph: Morph,
    palette_slot: Option<usize>,
    /// The search panel, morphing out of its slot.
    search_panel: Panel,
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
    /// The export panel's choices while it is showing (kept while it folds
    /// back into the gear); `export_panel.open` mirrors `is_some()`.
    export: Option<ExportState>,
    /// The export panel, morphing out of the gear like the settings.
    export_panel: Panel,
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
    /// The next modified key press becomes the global hotkey (Esc
    /// cancels); no shortcut runs meanwhile.
    hotkey_recording: bool,
    /// Why the last press while recording didn't make a hotkey.
    hotkey_hint: Option<&'static str>,
    /// A mouse press just ended the recording, so the Record button's
    /// message on its release must not start it again.
    hotkey_press_stopped: bool,
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

/// Where a trash button asks "Delete?" about a note.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DeleteAsk {
    /// In the open note's header.
    Header(Uuid),
    /// On the open peek.
    Peek(Uuid),
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
        let settings = SettingsStore::load(platform::data_dir().join("settings.json"));
        // Hide the Dock icon as early as possible so it barely flashes.
        if !settings.settings().app.show_dock_icon {
            platform::set_dock_policy(false);
        }
        let mut store = NoteStore::load(platform::data_dir().join("notes.json"));
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
        let actions_morph = Morph::with_secs(ACTIONS_SECS, s.motion.speed);
        let mode_fade = Self::settled_fade(s.motion.speed);
        let settings_panel = Panel::new(s.motion.speed);
        let search_panel = Panel::new(s.motion.speed);
        let export_panel = Panel::new(s.motion.speed);
        let window_size = s.window.edge.size(
            600.0,
            Self::docked_thickness(s, SUPPORTS_PASSTHROUGH, false),
        );
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
            anchor_along: 0.0,
            pending_delete: None,
            delete_ask: None,
            collapsing: None,
            mode_fade,
            color_picker_open: false,
            color_hex: String::new(),
            color_bubble_rect: None,
            text_color_picker_open: false,
            reminder_picker: None,
            #[cfg(test)]
            opened: Vec::new(),
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
            settings_panel,
            settings_tab: SettingsTab::default(),
            range_drafts: HashMap::new(),
            title_edit: None,
            actions_open: false,
            actions_morph,
            palette_slot: None,
            search_panel,
            export: None,
            export_panel,
            export_generation: 0,
            export_dialog_open: false,
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
            hotkey_recording: false,
            hotkey_hint: None,
            hotkey_press_stopped: false,
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
        SettingsStore::load(platform::data_dir().join("settings.json"))
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
        let task = Task::batch([task, self.settle_settings_session()]);
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
            Message::ThemeChanged(..)
            | Message::EdgeChosen(..)
            | Message::ToggleVisibility
            | Message::HotkeyPressed
            | Message::TrayReady(..)
            | Message::TrayMenu(..)
            | Message::Quit
            | Message::WindowReady(..)
            | Message::WindowResized(..)
            | Message::WindowUnfocused => self.handle_docking(message),
            Message::StripHover(..)
            | Message::BarClicked(..)
            | Message::ActionsHovered(..)
            | Message::PeekDeleteRequest(..)
            | Message::StripFileDropped(..)
            | Message::FileHovered
            | Message::FileHoverEnded
            | Message::DragStart(..)
            | Message::DragMove(..)
            | Message::DragEnd
            | Message::StackOnto { .. }
            | Message::Unstack(..)
            | Message::OpenStackMember(..)
            | Message::StripScroll(..)
            | Message::PeekScroll(..)
            | Message::PeekTick(..)
            | Message::AutoHideWake(..) => self.handle_strip(message),
            Message::ToggleSettings
            | Message::CloseSettings
            | Message::ToggleSearch
            | Message::SearchChanged(..)
            | Message::SearchResultPicked(..)
            | Message::SearchSubmitted
            | Message::CloseSearch
            | Message::ToggleExport
            | Message::ExportToggleNote(..)
            | Message::ExportAll(..)
            | Message::ExportFormatChosen(..)
            | Message::ExportRequested
            | Message::ExportPicked(..)
            | Message::ExportCancelled(..)
            | Message::ExportStatusExpired(..)
            | Message::SettingChanged(..)
            | Message::RangeEdited(..)
            | Message::RangeSubmitted(..)
            | Message::HotkeyRecord
            | Message::SettingToggled(..)
            | Message::ReminderAlertChosen(..)
            | Message::ResetGroup(..)
            | Message::SettingsTabSelected(..)
            | Message::PaletteSlotSelected(..)
            | Message::PaletteColorChosen(..)
            | Message::SettingsDragStart
            | Message::SettingsResetPosition
            | Message::SearchDragStart
            | Message::SearchResetPosition => self.handle_panels(message),
            Message::AddNote
            | Message::NoteEdited(..)
            | Message::BodyClicked(..)
            | Message::BodyPressed(..)
            | Message::EditorBlurred
            | Message::ToggleTask(..)
            | Message::LinkClicked(..)
            | Message::TitleEdited(..)
            | Message::TitleCommitted
            | Message::TitleBlurred(..)
            | Message::NoteHovered(..)
            | Message::NoteDragStart
            | Message::NoteResetPosition
            | Message::ResizeStart(..)
            | Message::ClosePanel
            | Message::RequestDelete(..)
            | Message::ConfirmDelete
            | Message::CancelDelete
            | Message::CopyNote
            | Message::TogglePin
            | Message::CopiedExpired(..)
            | Message::UndoDelete
            | Message::UndoExpired(..)
            | Message::ToggleColorPicker
            | Message::ToggleTextColorPicker
            | Message::FormatApplied(..)
            | Message::ImageDropped(..)
            | Message::ClipboardNote
            | Message::ClipboardEmptyExpired(..)
            | Message::PasteRequested
            | Message::ClipboardText(..)
            | Message::Undo
            | Message::Redo
            | Message::PickImage
            | Message::ImagePicked(..)
            | Message::ColorChosen(..)
            | Message::ColorAdjusted(..)
            | Message::ColorHexEdited(..)
            | Message::ColorHexSubmitted
            | Message::CloseColorBubble
            | Message::ColorBubbleMoved(..)
            | Message::SaveTick => self.handle_notes(message),
            Message::Tick(now) => self.tick(now),
            Message::ToggleReminderPicker
            | Message::OpenReminderPicker
            | Message::ReminderPickerDay(..)
            | Message::ReminderPickerMonth(..)
            | Message::ReminderPickerHour(..)
            | Message::ReminderPickerMinute(..)
            | Message::ReminderPickerDone
            | Message::ReminderPickerClear
            | Message::ReminderToCalendar
            | Message::ReminderByEmail
            | Message::ReminderTick => self.handle_reminders(message),
            Message::Key(..)
            | Message::CursorMoved(..)
            | Message::CursorLeftWindow
            | Message::MouseButton(..)
            | Message::PollCursor
            | Message::CursorPolled(..) => self.handle_input(message),
        }
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
            peek_confirm: self.peek_confirming(),
            panel_open: self.panel_open(),
            actions_expanded: self.actions_open,
            actions_progress: self.actions_progress(),
            x_offset: self.strip_away_offset(),
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
                        title_text: self.title_field_text().unwrap_or_default(),
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
                        confirm_delete: self.delete_ask == Some(DeleteAsk::Header(note.id)),
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
                    morph_progress: self.settings_panel.morph.progress(),
                    content_alpha: frame.content_alpha,
                    tab: self.settings_tab,
                    range_drafts: &self.range_drafts,
                    selected_slot: self.palette_slot,
                    tray_ok: self.tray_ok,
                    dock_forced: self.tray_failed,
                    hotkey_error: self.hotkey_error.as_deref(),
                    hotkey_label: self
                        .settings
                        .settings()
                        .app
                        .global_hotkey_combo
                        .label(cfg!(target_os = "macos")),
                    hotkey_recording: self.hotkey_recording,
                    hotkey_hint: self.hotkey_hint,
                    dragging: self.settings_panel.drag.is_some(),
                });
                layers.push(pin(opaque(panel)).x(rect.x).y(rect.y).into());
            }
        }

        if let Some(frame) = self.search_frame() {
            let rect = frame.rect;
            if rect.width >= 1.0 && rect.height >= 1.0 {
                // Only while unfolding: a folding panel mustn't block clicks
                // on the note opening in its place.
                if self.search_panel.morph.is_opening() {
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
                    morph_progress: self.search_panel.morph.progress(),
                    content_alpha: frame.content_alpha,
                    dragging: self.search_panel.drag.is_some(),
                });
                layers.push(pin(opaque(panel)).x(rect.x).y(rect.y).into());
            }
        }

        if let (Some(frame), Some(state)) = (self.export_frame(), &self.export) {
            let rect = frame.rect;
            if rect.width >= 1.0 && rect.height >= 1.0 {
                // Only while unfolding, as for the search. While the save
                // dialog is open a click beside the panel does nothing.
                if self.export_panel.morph.is_opening() {
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
                    morph_progress: self.export_panel.morph.progress(),
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
}

mod docking;
mod input;
mod notes;
mod panels;
mod reminders;
mod strip;
mod tick;

#[cfg(test)]
mod tests;
