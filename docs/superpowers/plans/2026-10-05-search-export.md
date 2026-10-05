# Search, Export, Copy, Styled Editing and Peek Width Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add search across notes, styled editing, export to .md/.txt, copy-to-clipboard and a peek as wide as the note.

**Architecture:** Three new pure modules: `search`, `export` and `rich_highlight` (a line-level highlighter plus an iced `Highlighter`). Two new views: `search_panel` and `export_panel`, styled like Settings. Small changes to the strip (a search slot, dimming), the tray (two items), the note header (a copy button), the editor (the highlighter) and the peek (its width).

**Tech Stack:** Rust 2021, iced 0.14 (`text_editor::highlight_with`, `clipboard::write`), rfd (save dialog), chrono (date in the file name), Lucide subset via `pyftsubset`.

**Spec:** `docs/superpowers/specs/2026-10-05-search-export-design.md`

## Global Constraints

- The decisions S1–S6, E1–E6, C1, P1 and R1–R3 in the spec are binding. Exact strings:
  - `"Untitled"`
  - `"Search…"` and `"Export…"` (tray items and the Settings button)
  - `"Couldn't save: <reason>"` and `"Exported N notes"` (singular for N = 1: `"Exported 1 note"`)
  - file name `snap-notes-YYYY-MM-DD.<ext>`
- Tray order: Show/Hide Notes, New Note, Search…, Export…, Settings…, Quit. Tray ids: `search`, `export`.
- Search is case-insensitive using Unicode lowercase, returns at most 50 results, and keeps strip order. Snippets keep about 30 chars of context on each side.
- Search and Settings never show together; export replaces Settings while it's open; Esc closes search or export first.
- All UI uses the theme tokens and colors (`theme.rs`), Lucide icons and the pressed style (`note_panel::pressable`). No emoji or hard-coded colors in chrome.
- CI must stay green after every task: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --bin snap-notes`.
- Comments and naming follow the surrounding code; no section banners.

## Review Focus

1. **Search with an Unicode-folding edge case.** Some characters change byte length when lowercased (e.g. `İ` → `i̇`). Offsets taken from a lowercased copy would then point at the wrong place in the original, and slicing there could panic. Search must map offsets back to the original text correctly and never panic. Test in Task 2: `search_offsets_survive_case_folding`.
2. **Export of a note with only an image or an empty body.** Expect valid output with the title heading and no stray blank lines beyond the format. Test in Task 3: `export_empty_and_image_only_notes`.
3. **Highlighter on lines with unmatched markers.** For example `**bold` without a closer, `{coral` unclosed, or a lone backtick. Expect plain text and no panic. Test in Task 4: `unmatched_markers_stay_plain`.
4. **Picking a search result while another note is open or folding.** Expect the picked note to open, the other one to fold, and no stale editor content. Test in Task 7: `picking_a_result_switches_notes`.
5. **The save dialog's focus loss.** It must not fold the open note or close the export panel. Test in Task 8: `export_dialog_keeps_panel_open`, which sets `keep_open_until` and sends `WindowUnfocused`.

---

### Task 1: Icons for search, copy and export

**Files:**
- Modify: `assets/fonts/subset.sh` (`ICONS` list: add `search copy download`), `assets/fonts/lucide.subset.ttf` (regenerated), `src/icons.rs`.

**Interfaces:**
- Produces: `Icon::Search`, `Icon::Copy` and `Icon::Download`. `Icon::ALL` grows to 18.

- [ ] **Step 1:** Add the three variants and the codepoints from the pinned lucide-static `info.json` (the version `subset.sh` already pins). Extend `ALL`.
- [ ] **Step 2:** Run `cargo test --bin snap-notes icons::`. Expected: FAIL (`every_icon_is_in_the_icon_font`).
- [ ] **Step 3:** Add the names to `ICONS` in `subset.sh` and re-run the script to regenerate `lucide.subset.ttf`. If the network fails, report BLOCKED.
- [ ] **Step 4:** Run `cargo test --bin snap-notes icons::`. Expected: PASS. Then run clippy and fmt.
- [ ] **Step 5:** Commit: `feat: Lucide search, copy and download icons`.

### Task 2: `search.rs`

**Files:**
- Create: `src/search.rs` (and add `mod search;` in `main.rs`).

**Interfaces:**
- Produces:
  - `pub struct Hit { pub note_id: Uuid, pub title: String, pub snippet: String, pub highlight: Range<usize>, pub body_match: Option<usize> }`
  - `pub fn find(notes: &[Note], query: &str) -> Vec<Hit>`
  - `pub const MAX_RESULTS: usize = 50`

- [ ] **Step 1: Tests:**
  - `matches_title_and_body_case_insensitively`, which includes `"É"` matching `"é"`.
  - `title_only_match_shows_first_body_line`, with `body_match == None`.
  - `snippet_trims_with_ellipsis_and_highlights_match`. For a body of 200 chars with a match at char 100: the snippet starts and ends with `…`, and `&snippet[highlight]` equals the original-case match.
  - `empty_or_blank_query_finds_nothing`.
  - `results_follow_strip_order_and_cap_at_50`, using 60 matching notes.
  - `search_offsets_survive_case_folding`. Use a content like `"İstanbul and Ünal"` with query `"ünal"`. `body_match` must index the original content at `Ü`, and nothing panics.
- [ ] **Step 2:** Run them. Expected: compile errors.
- [ ] **Step 3:** Implement. Lowercase char by char while keeping a map from the folded text's byte offsets back to the original text, so ranges always land on original char boundaries. Measure the snippet context in chars.
- [ ] **Step 4:** Run them. Expected: PASS. Then run clippy and fmt (narrow `#[allow(dead_code)]` with `// used from Task 7` where needed).
- [ ] **Step 5:** Commit: `feat: search across notes`.

### Task 3: `export.rs`

**Files:**
- Create: `src/export.rs` (and add `mod export;`).

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum ExportFormat { Markdown, Text }` with `pub fn extension(self) -> &'static str` (`"md"`/`"txt"`)
  - `pub fn render(notes: &[&Note], format: ExportFormat) -> String`
  - `pub fn suggested_name(date: chrono::NaiveDate, format: ExportFormat) -> String`
  - `pub fn write(path: &Path, contents: &str) -> io::Result<()>`, atomic: write `<path>.tmp`, then rename

- [ ] **Step 1: Tests:**
  - `markdown_has_heading_per_note`: two notes, one untitled. The output equals `"# Groceries\n\nmilk\n\n# Untitled\n\nx\n"` exactly. (State the exact separator in the test and keep the format consistent: notes are separated by one blank line, and the output ends with a single `\n`.)
  - `markdown_strips_custom_tags`: `"{coral}red{/} and ==hi=="` becomes `"red and hi"`, while `**b**` and `[l](https://x)` stay.
  - `text_uses_plain_text_and_underlined_titles`: `"Groceries\n=========\n\n…"`, with `plain_text` applied to the body.
  - `export_empty_and_image_only_notes`.
  - `suggested_name_has_date_and_extension`: gives `snap-notes-2026-10-05.md`.
  - `write_is_atomic`: write into a tempdir. The file exists with the contents, and no `.tmp` is left behind.
- [ ] **Step 2:** Run them. Expected: compile errors.
- [ ] **Step 3:** Implement. Tag stripping reuses the `rich` tag rules: write `pub fn strip_tags(content: &str) -> String` in `rich.rs`, with the same escapes and the `==` flanking rule. Test it in `rich.rs` with `strip_tags_keeps_markdown`.
- [ ] **Step 4:** Run them. Expected: PASS. Then run clippy and fmt.
- [ ] **Step 5:** Commit: `feat: render and write note exports`.

### Task 4: Styled editing (`rich_highlight.rs`)

**Files:**
- Create: `src/rich_highlight.rs`.
- Modify: `src/note_panel.rs` (the body editor uses `highlight_with`), `src/main.rs`.

**Interfaces:**
- Consumes: `rich::COLOR_NAMES`, `theme::Theme`, `NoteColor`.
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub enum Style { Plain, Marker, Bold, Italic, BoldItalic, Code, Color(NoteColor), Link, Heading, CodeBlock }`
  - `pub fn spans(line: &str, in_code_block: bool, palette: &[NoteColor]) -> (Vec<(Range<usize>, Style)>, bool)`
  - `pub struct Highlighter`, implementing `iced::advanced::text::Highlighter` with `Settings = HighlightSettings { palette: Vec<NoteColor>, mode: theme::Mode }` and `Highlight = Style`.
  - `pub fn format(style: &Style, theme: &iced::Theme) -> highlighter::Format<Font>`. The `to_format` fn pointer can't capture state, so derive the ink from `iced::Theme`'s light or dark palette, which `main.rs` already maps from our mode. Markers use ink at 35 %, links use the theme's link color, colors use the note color, and fonts use the Inter variants or monospace.

- [ ] **Step 1: Tests:**
  - `bold_text_and_faint_markers`: `"a **b** c"` gives `Marker` on both `**` and `Bold` on `b`.
  - `italic_code_and_color`: `"*i* `c` {coral}r{/}"` gives `Italic`, `Code` and `Color(palette[0])` on `r`. The tag ranges get `Marker`.
  - `heading_and_quote_markers`: `"## Title"` gives `Marker` on `## ` and `Heading` on `Title`.
  - `link_text_and_url_markers`: `"[x](https://e.com)"` gives `Link` on `x`. Brackets and `(url)` get `Marker`.
  - `fenced_code_block_spans_lines`: ```` "```" ```` opens the block, the next line is all `CodeBlock`, and the closing fence ends it.
  - `unmatched_markers_stay_plain`.
  - `multibyte_ranges_are_char_boundaries`.
  - `hostile_lines_do_not_panic`: reuse the parser's hostile list.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Implement `spans` as a single pass over the line, modeled on the inline rules of `rich::push_text` and the tag scanner. Implement `Highlighter`, keeping the code-block state per line (`change_line` resets from that line). Wire it into the editor: `.highlight_with::<rich_highlight::Highlighter>(settings, rich_highlight::format)`.
- [ ] **Step 4:** Run the suite, clippy and fmt. Expected: green.
- [ ] **Step 5:** Commit: `feat: style Markdown while editing`.

### Task 5: Copy a note

**Files:**
- Modify: `src/note_panel.rs` (header button), `src/app.rs`.

**Interfaces:**
- Produces:
  - `Message::CopyNote` and `App.copied_at: Option<Instant>`.
  - `pub fn copy_text(note: &Note) -> String`, in `app.rs` or `note.rs`.
  - `PostIt` gains `pub copied: bool` (true for 1.5 s after copying).

- [ ] **Step 1: Tests:**
  - `copy_text_joins_title_and_body`: gives `"T\n\nbody"`, and just `"body"` when the title is blank.
  - `copy_note_marks_copied`: `CopyNote` returns a task with `units() > 0` and sets `copied_at`. After a tick 1.6 s later, the note's `copied` is false.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Implement:
  - The header button goes left of 🗑, showing `Icon::Copy`, or `Icon::Check` while copied.
  - Use `iced::clipboard::write(text)`.
  - Keep `animating` or a timer so the icon flips back after 1.5 s.
- [ ] **Step 4:** Run the suite, clippy and fmt.
- [ ] **Step 5:** Commit: `feat: copy a note to the clipboard`.

### Task 6: Peek width

**Files:** Modify `src/peek.rs`, `src/bar_strip.rs`, `src/app.rs`.

**Interfaces:**
- `peek_layout(bar, bounds, progress, text, confirming, width: f32)` and `peek_target(bar, bounds, note, confirming, width: f32)`.
- `pub fn peek_width(note: &Note, default: f32) -> f32`, which returns `note.size.map_or(default, |s| s[0])`.
- The `BarStrip` field `pub default_note_width: f32`.

- [ ] **Step 1: Tests:**
  - `peek_width_follows_note_or_default`.
  - `open_peek_uses_given_width`: `rect.width == width`, the right edge stays at the bar's right edge, and the trash rect is still inside the peek.
  - Existing peek tests: pass the old `PEEK_WIDTH` (260) explicitly so they keep their meaning.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Implement:
  - Inner width, text truncation and wrapping derive from the width. Remove the fixed `PEEK_WIDTH` from the layout and keep it only if tests still need it.
  - The app and the strip pass the width for the peeked note.
- [ ] **Step 4:** Run the suite, clippy and fmt.
- [ ] **Step 5:** Commit: `feat: peek as wide as the note`.

### Task 7: Search UI

**Files:**
- Create: `src/search_panel.rs`.
- Modify: `src/app.rs`, `src/bar_strip.rs` (search slot and dimming), `src/tray.rs` (Search… item), `src/main.rs`.

**Interfaces:**
- Consumes: `search::{find, Hit}`, the `Icon::Search` glyph (Task 1), and the settings panel's morph and paper pattern.
- Produces:
  - Messages: `ToggleSearch`, `SearchChanged(String)`, `SearchResultPicked(Uuid)`.
  - App state: `search_open: bool`, `search_query: String`, `search_morph: Morph`.
  - `StripLayout.search_hit_area: Rectangle` and `BarStrip.dimmed: Option<&HashSet<usize>>` (or `Vec<bool>`).

- [ ] **Step 1: Tests (app and strip):**
  - `cmd_f_opens_search_and_closes_settings`.
  - `typing_filters_results` checks `find` through app state, so the view gets hits.
  - `picking_a_result_opens_note_with_match_selected`: the selection equals the match range in the editor.
  - `picking_a_result_switches_notes`.
  - `escape_closes_search_first`.
  - `tray_search_item_opens_search`.
  - `search_slot_hit_test`: the slot sits below `+` and its hit area triggers `ToggleSearch`.
  - `non_matching_bars_dim`: the dimmed set equals the non-matching indices while the query is non-empty.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Implement per S1–S6.
  - The panel morphs like Settings and is anchored at the search slot. It has a `text_input` (with an id, focused when the panel opens) and a scrollable list of result buttons. Each result shows the title in SemiBold and the snippet with the match drawn as a bold span via `rich_text`.
  - Strip: draw the magnifier slot with quads.
  - Tray: add the "Search…" item with id `search`.
- [ ] **Step 4:** Run the suite, clippy and fmt.
- [ ] **Step 5:** Commit: `feat: search panel`.

### Task 8: Export UI

**Files:**
- Create: `src/export_panel.rs`.
- Modify: `src/app.rs`, `src/settings.rs` (`SettingsGroup::Data`), `src/settings_panel.rs` ("Export…" button), `src/tray.rs` ("Export…" item), `src/main.rs`.

**Interfaces:**
- Consumes: `export::*`, `Icon::Download`, rfd (already a dependency).
- Produces:
  - Messages: `ToggleExport`, `ExportToggleNote(Uuid)`, `ExportAll(bool)`, `ExportFormatChosen(ExportFormat)`, `ExportRequested`, `ExportPicked(Option<PathBuf>)`.
  - State: `App.export: Option<ExportState { selected: HashSet<Uuid>, format: ExportFormat, status: Option<(String, Instant)> }>`.

- [ ] **Step 1: Tests:**
  - `export_opens_with_all_selected`.
  - `toggling_and_all_none`.
  - `export_requested_with_nothing_selected_does_nothing`.
  - `export_picked_writes_file_in_order`: tempdir path. The file contents equal `export::render` of the selected notes in strip order, the status is `"Exported 2 notes"`, and the panel closes after a 2 s tick.
  - `export_cancelled_does_nothing`.
  - `export_write_error_shows_message`: the path is a directory. The status starts with `"Couldn't save: "`.
  - `export_dialog_keeps_panel_open`.
  - `tray_export_item_opens_export`.
  - `settings_data_group_has_export_button`: the settings group list contains `Data`, and its label is `"Data"`.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Implement per E1–E6.
  - `ExportRequested` sets `keep_open_until` and returns `Task::perform(rfd::AsyncFileDialog::new().set_file_name(suggested_name(today, format)).add_filter(..).save_file(), ..)`.
  - The panel reuses the settings card look and replaces Settings while open.
- [ ] **Step 4:** Run the suite, clippy and fmt.
- [ ] **Step 5:** Commit: `feat: export notes from settings and the tray`.

### Task 9: README

**Files:** Modify `README.md`.

- [ ] **Step 1:** Document everything from the spec's README section. Add a Cmd/Ctrl+F row to the shortcut table. Under Menu bar / tray, list the new items.
- [ ] **Step 2:** Run fmt, clippy and tests. Expected: green.
- [ ] **Step 3:** Commit: `docs: search, export, copy and styled editing`.
