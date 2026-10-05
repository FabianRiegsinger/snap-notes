# Search, Export, Copy, Styled Editing and Peek Width — Design Spec

## Overview

This spec covers five features the user asked for. The user picked the recommended option at each step:

1. **Search** across all notes.
2. **Styled editing.** Rich text is visible while typing.
3. **Export** of all notes or selected ones, as `.txt` or `.md`, from Settings and from the tray menu.
4. **Copy** a note's whole content to the clipboard.
5. **Peek width.** The hover peek is as wide as the opened note.

All five are built on the branch `feat/search-export`.

## Decisions

| # | Topic | Decision |
|---|---|---|
| S1 | Opening search | Cmd/Ctrl+F, a 🔍 slot in the strip directly under `+` (on every platform), or a "Search…" item in the tray menu. |
| S2 | Search panel | Unfolds like Settings. The panel and Settings never show at the same time, and opening one closes the other. The search field is focused when the panel opens. Esc closes the panel. |
| S3 | Matching | Case-insensitive substring match over the title and the raw body (markup included), using Unicode lowercase. An empty or whitespace-only query shows no results and dims nothing. |
| S4 | Results | At most 50, in strip order. Each shows the title ("Untitled" if empty) and a one-line snippet: about 30 chars of context on each side of the first body match, with "…" where text was cut. The match is shown bold. A title-only match shows the first body line. |
| S5 | Opening a result | Opens the note in edit mode with the first body match selected, and closes the panel. For a title-only match the cursor goes to the start of the body. |
| S6 | Strip dimming | While the panel is open with a non-empty query, bars of non-matching notes are drawn at 30 % alpha. |
| E1 | Opening export | An "Export…" button in a new Settings group "Data", and an "Export…" item in the tray menu. Both open the export panel, which replaces Settings while it's open. |
| E2 | Export panel | A list of every note (title or "Untitled") with checkboxes, all checked at first, plus "All" and "None" buttons. A **.md**/**.txt** switch (default .md). An **Export** button, disabled while nothing is checked. |
| E3 | File | One file for all selected notes, in strip order, saved through the `rfd` save dialog. The suggested name is `snap-notes-YYYY-MM-DD.<ext>`. It is written atomically (temp file plus rename). |
| E4 | `.md` format | Per note: `# <title or Untitled>`, a blank line, the body, a blank line. Custom tags are made standard: `{…}` tags and `{/}` are removed, and `==x==` becomes `x`. Everything else stays as written, including image paths. |
| E5 | `.txt` format | Per note: the title, a line of `=` as long as the title, a blank line, `rich::plain_text(body)`, a blank line. |
| E6 | Errors | A failed write shows a one-line message in the panel, such as "Couldn't save: <reason>". A cancelled dialog does nothing. On success the panel shows "Exported N notes" for 2 s, then closes. |
| C1 | Copy | A Lucide `copy` button in the open note's header, left of 🗑. It copies `title + "\n\n" + body` with `iced::clipboard::write`, or just the body when the title is empty. The icon turns into `check` for 1.5 s. |
| P1 | Peek width | The peek is the note's saved width (`note.size[0]`), or else the default note size setting. Its height still fits the content. The peek's insets, the trash position and the hit areas are all derived from that width. |
| R1 | Styled editing | The body `text_editor` uses `highlight_with` with a custom `rich_highlight::Highlighter` (Settings = palette + theme mode). In the editor: **bold**, *italic*, `code` (monospace), and `{color}`/`{#hex}` text color are styled. Headings are bold. Link text gets the theme's link color. Markup markers (`**`, `*`, `~~`, `` ` ``, `#`, `>`, `- [ ]`, `{…}`, `{/}`, `==`, `[`, `](url)`) are drawn at ink 35 %. |
| R2 | Highlighter limits | iced 0.14's `highlighter::Format` has only `color` and `font`. So strikethrough, underline, highlight backgrounds and font sizes can't show in the editor; they appear in the formatted view as today. `==x==` and `{bg:…}` text keeps normal ink in the editor, and only the markers are faint. |
| R3 | Highlighter state | The highlighter works line by line. Inline styles never span lines, matching the parser's rule that tags end with their block. A fenced code block is tracked across lines (its lines are monospace, with no inline styling). |

## Architecture

### New modules (pure and unit-tested unless noted)
- **`src/search.rs`:** `pub struct Hit { note_id: Uuid, title: String, snippet: String, highlight: Range<usize>, body_match: Option<usize> }`.
  - `pub fn find(notes: &[Note], query: &str) -> Vec<Hit>`, following S3 and S4.
  - `highlight` is the match's byte range within `snippet`. `body_match` is the byte offset of the first match in `note.content`.
- **`src/export.rs`:**
  - `pub enum ExportFormat { Markdown, Text }`, with `extension()`.
  - `pub fn render(notes: &[&Note], format: ExportFormat) -> String`, following E4 and E5.
  - `pub fn suggested_name(date: NaiveDate, format) -> String`.
  - `pub fn write(path: &Path, contents: &str) -> io::Result<()>`, which is atomic.
- **`src/rich_highlight.rs`:** the `iced::advanced::text::Highlighter` implementation for R1–R3.
  - Built on a pure `pub fn spans(line: &str, in_code_block: bool, palette: &[NoteColor]) -> (Vec<(Range<usize>, Style)>, bool)`.
  - `Style` is one of `{ Marker, Bold, Italic, BoldItalic, Code, Color(NoteColor), Link, Heading, CodeBlock, Plain }`.
  - The returned `bool` says whether the line leaves a fenced code block open.
  - The Highlighter maps `Style` to `Format` through the theme.
- **`src/search_panel.rs`, `src/export_panel.rs` (views):** follow the settings panel's look and morph (paper card, theme tokens, pressed buttons).

### Changes to existing code
- **`app.rs`:**
  - New state: `search_open`, `search_query`, `search_morph`, `export: Option<ExportState { selected: HashSet<Uuid>, format, status }>`, `copied_at: Option<Instant>`.
  - New messages: `ToggleSearch`, `SearchChanged(String)`, `SearchResultPicked(Uuid)`, `ToggleExport`, `ExportToggleNote(Uuid)`, `ExportAll(bool)`, `ExportFormatChosen(ExportFormat)`, `ExportRequested`, `ExportPicked(Option<PathBuf>)`, `CopyNote`.
  - Key handling: Cmd/Ctrl+F toggles search. Esc closes search or export first, before any other Esc handling.
  - While the save dialog is open, `keep_open_until` is set, so the note or panel doesn't fold when focus moves to the dialog.
- **`bar_strip.rs`:**
  - A search slot under `+`, drawn from quads (a magnifier: a ring quad plus a short handle quad) and hit-tested like the add slot.
  - A `dim: Option<HashSet<usize>>`, or a matching-index set, for S6.
  - `compute_layout` gains the slot. `StripLayout` gains `search_hit_area`.
- **`tray.rs`:** the menu gets "Search…" and "Export…" items (ids `search` and `export`), in this order: Show/Hide, New Note, Search…, Export…, Settings…, Quit.
- **`settings.rs` / `settings_panel.rs`:** a new `SettingsGroup::Data` with an "Export…" button. It holds no persisted values.
- **`note_panel.rs`:**
  - The copy button (C1).
  - The body editor gets `highlight_with::<rich_highlight::Highlighter>`.
- **`peek.rs` / `bar_strip.rs` / `app.rs`:** the peek width comes from P1, through a `width` passed into `peek_layout` and `peek_target`.
- **`icons.rs` / `assets/fonts/`:** add Lucide `search`, `copy` and `download` to the subset (`subset.sh`, the `Icon` enum and the tests).

## Error handling
- **Export:** E6.
- **Search:** can't fail.
- **Clipboard:** a write is fire-and-forget, as iced's clipboard is.
- **Highlighter:** never panics on any line. This is covered by a hostile-input test like the parser's.

## Testing
- **`search.rs`:**
  - case-insensitive matching, including Unicode (`É` matches `é`);
  - title-only matches;
  - snippet trimming with "…";
  - the `highlight` range points at the match;
  - the 50-result cap;
  - empty or whitespace queries give nothing;
  - results follow strip order.
- **`export.rs`:**
  - both formats for 2 notes (one untitled);
  - tag stripping (`{coral}x{/}` becomes `x`, `==y==` becomes `y`);
  - `.txt` uses `plain_text`;
  - the suggested name;
  - an atomic write to a tempdir.
- **`rich_highlight.rs`:**
  - each style's ranges and its markers;
  - a fenced block spanning lines;
  - unknown tags stay plain;
  - multibyte text;
  - hostile inputs don't panic.
- **`app.rs`:**
  - Cmd+F opens search, and opening it closes Settings;
  - typing filters;
  - picking a result opens the note in edit mode with the match selected;
  - the export flow from request to picked path writes the file (tempdir), shows the status and handles cancel;
  - `CopyNote` sets `copied_at`;
  - tray ids map to messages.
- **`peek.rs` / `bar_strip.rs`:**
  - the peek width follows `note.size` or the default;
  - hit areas follow it;
  - the search slot hit-test.
- **Manual (with the run skill):**
  - search from all three entry points;
  - export to both formats, opening the files;
  - copy and paste into another app;
  - styled editing in light and dark modes;
  - the peek width for a resized note.

## README
- Search: the shortcut, the slot and the tray item.
- Export: where to find it and both formats.
- Copy: the button.
- Styled editing: what shows in the editor and what only shows in the formatted view.
- A shortcut table row for Cmd/Ctrl+F.

## Out of scope
- Search inside the formatted view of the open note.
- Regular expressions.
- Exporting to a folder (one file per note).
- Copying images to the export location.
- Full WYSIWYG editing.
