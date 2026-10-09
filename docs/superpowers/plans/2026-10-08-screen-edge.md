# Screen Edge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the note strip dock to the right (default), left or top screen edge, chosen in Settings, with the same feel on every edge.

**Architecture:** A pure `src/edge.rs` maps an edge-local frame (`along` the edge, `away` from it) to window coordinates. The strip's layout, drawing and hit-tests work in local coordinates, and text-bearing boxes are placed on the away side of their anchor without being transformed. For Right the mapping is the identity of today's layout.

**Tech Stack:** Rust 2021, iced 0.14.

**Spec:** `docs/superpowers/specs/2026-10-08-screen-edge-design.md`

## Global Constraints

- Right behaves exactly as today: the existing test suite passes unchanged (assertions keep their values; only call sites may gain an `Edge::Right` argument).
- Persisted setting `window.edge` takes the values `"right"`, `"left"` or `"top"`. The default is `"right"`, and an unknown value reads as Right.
- Settings → Window labels, exact text: **"Screen edge"** with the options **"Right"**, **"Left"** and **"Top"**. The existing height setting's label becomes **"Strip length"**; its stored key `height_fraction` is unchanged.
- Text is never rotated. Peek, chips, toast, note, color bubble and panels keep their real size and sit on the away side of their anchor.
- Out of scope: the bottom edge.
- Checks: `cargo test --bin snap-notes`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`. Cross-check with `-D warnings` for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`, through `RUSTC=~/.rustup/toolchains/1.90.0-aarch64-apple-darwin/bin/rustc ~/.rustup/toolchains/1.90.0-aarch64-apple-darwin/bin/cargo check --all-targets --target <t>`.
- Write the failing test first (RED). Commits end with a trailer naming the authoring model.

## Review Focus

1. **A dragged bar on the Top edge**, whether reordering or stacking: the drop target must use the cursor's x, not its y. Task 2 test: `top_drag_reorders_by_x`.
2. **A peek near the far end of a Top strip** (rightmost bar): it must clamp inside the window, not run off the right edge. Task 2 test: `top_peek_clamps_at_window_end`.
3. **Mouse-wheel scrolling an overflowing strip on Top**: a vertical wheel must still scroll the strip along x. Task 2 test: `top_wheel_scrolls_along`.
4. **Changing the edge while a note is open or the peek is showing**: the note stays open and re-anchors, the peek closes, and nothing panics on stale indices. Task 3 test: `edge_change_keeps_note_open_and_closes_peek`.
5. **Auto-hide on Left/Top**: the strip slides off toward its own edge, and the 2 px edge zone is on that edge. A cursor at the *right* edge must not reveal a Left strip. Task 3 test: `auto_hide_edge_zone_follows_edge`.

---

### Task 1: The `Edge` mapping and the setting

**Files:**
- Create: `src/edge.rs` (and its `mod` line in `src/main.rs`)
- Modify: `src/settings.rs` (WindowSettings, read/write, reset), `src/settings_panel.rs` (Window group: segmented "Screen edge", "Strip length" label)

**Interfaces:**
- Produces:
  - `pub enum Edge { Right, Left, Top }`. It derives `Debug, Clone, Copy, PartialEq, Eq, Default` (Default = Right) and has `fn as_str(self) -> &'static str` and `fn parse(&str) -> Edge`, where unknown input gives Right.
  - `pub struct Local { pub along: f32, pub away: f32 }` and `pub struct LocalRect { pub along: f32, pub away: f32, pub length: f32, pub thickness: f32 }`. In a `LocalRect`, `along`/`away` is the corner nearest the edge's start and the screen edge.
  - `impl Edge`:
    - `fn to_window(self, p: Local, window: Size) -> Point`
    - `fn to_local(self, p: Point, window: Size) -> Local`
    - `fn rect_to_window(self, r: LocalRect, window: Size) -> Rectangle`
    - `fn rect_to_local(self, r: Rectangle, window: Size) -> LocalRect`
    - `fn edge_length(self, size: Size) -> f32`: the height for Right and Left, the width for Top.
    - `fn place_away(self, anchor: Rectangle, content: Size, gap: f32, bounds: Rectangle) -> Rectangle`: puts `content` on the away side of `anchor`, centred on it along the edge, then clamped into `bounds`.
    - `fn dock_origin(self, window: Size, monitor: Size, top_inset: f32) -> Point`: flush to the edge and centred along it. Top uses `y = top_inset`.
  - `WindowSettings.edge: Edge` and `SettingsMessage`/`Message::EdgeChosen(Edge)`. Follow how other Window-group values are sent, and persist the value as `window.edge`.

- [ ] **Step 1: Write the failing tests** in `src/edge.rs`:
  - `right_is_identity`: for window `Size(800,600)`, `Edge::Right.to_window(Local{along:100,away:20})` returns `Point(780,100)`, and `to_local` reverses it.
  - `left_mirrors`: `Edge::Left.to_window(Local{along:100,away:20})` returns `Point(20,100)`.
  - `top_swaps_axes`: `Edge::Top.to_window(Local{along:100,away:20})` returns `Point(100,20)`.
  - `round_trips_every_edge`: for 3 edges × several points, `to_local(to_window(p)) ≈ p` within 1e-4. Do the same with `rect_to_local(rect_to_window(r)) ≈ r`.
  - `place_away_sides`: anchor `Rect(700,100,40,60)`, content `Size(200,120)`, gap 8. Right gives a rect whose `x + width == 692`. Left, with anchor `Rect(60,100,40,60)`, gives `x == 108`. Top, with anchor `Rect(100,30,60,40)`, gives `y == 78`.
  - `place_away_clamps`: a content box that would cross the bounds is clamped inside, with the same 8 px margin as `peek_layout` today.
  - `dock_origin_each_edge`: monitor `1440×900`, window `64×810`. Right gives `(1376,45)`, Left gives `(0,45)`. Top, with window `1296×64` and inset 25, gives `(72,25)`.
  - `edge_length_each`.
- [ ] **Step 2:** Run `cargo test --bin snap-notes edge`. Expected: FAIL, failing to compile.
- [ ] **Step 3:** Implement `src/edge.rs` with the signatures above.
- [ ] **Step 4: Settings tests** in `settings.rs`:
  - `edge_defaults_to_right_and_round_trips`: the default is Right, `"left"` and `"top"` round-trip through save and load, `"bottom"` and garbage read as Right, and Reset Window gives Right.
  - In `settings_panel.rs`: `window_group_shows_screen_edge_and_strip_length`. Use whatever pure label helper exists; if none does, extract one.
- [ ] **Step 5:** Implement the setting, the segmented control (match the existing segmented or toggle styling in `settings_panel.rs`) and the "Strip length" label. Nothing else reads `edge` yet.
- [ ] **Step 6:** Run the full suite, clippy, fmt and the cross-checks. Expected: all green.
- [ ] **Step 7:** Commit with the message `feat: screen edge setting and edge mapping`.

### Task 2: An edge-aware strip (layout, drawing, hit-tests, peek)

**Files:**
- Modify: `src/bar_strip.rs` (`compute_layout`, `band`, `StripLayout`, the drawing of bars, slots, notch, stack edges, progress, chips, toast and drag ghost, plus every hit-test and the wheel), `src/peek.rs` (`peek_layout` placement through `place_away`)

**Interfaces:**
- Consumes: Task 1's `Edge`, `Local`, `LocalRect`, `place_away` and `edge_length`.
- Produces:
  - `BarStrip.edge: Edge`.
  - `compute_layout(..., edge: Edge)`. `StripLayout` rects are still in **window coordinates**, so the app's consumers see no type change, and it gains `edge: Edge`.
  - `fn band(bounds: Rectangle, fraction: f32, edge: Edge) -> Rectangle`: centred along the edge, spanning the full away extent.
  - `peek_layout(bar, bounds, progress, text, width, edge)`.
  - `stack_target`/`insertion_slot`/`drop_zone` take the cursor's **along** coordinate, and callers convert it with `edge.to_local(cursor, bounds.size()).along`. Keep the existing names, and change only the meaning and doc of the `y: f32` parameters to "along".

- [ ] **Step 1: Write the failing tests** in `bar_strip.rs`:
  - `layout_is_edge_equivalent`: for the same entries and magnification, the layout on Left and on Top, mapped back with `rect_to_local`, equals the layout on Right mapped back. Check bars and slots within 1e-3.
  - `right_layout_unchanged`: snapshot a few bar rects on Right and assert today's values. Compute them with the pre-change code at the start, then hard-code them.
  - `hit_tests_follow_edge`: on each edge, pressing at the window point of bar 2's centre selects bar 2, and the add slot's centre gives `AddNote`.
  - `top_drag_reorders_by_x` (Review Focus 1): a drag on Top whose cursor x crosses the middle of bar 1 gives the stack target or insertion slot computed from x.
  - `top_wheel_scrolls_along` (Review Focus 3): on Top, a vertical wheel delta changes `scroll_offset` the way it does on Right.
  - `notch_and_progress_sides`: on Top, the pin notch rect is at the bar's start along x (its left end), and the progress fill is anchored at the bar's right end. On Left, the stack edges sit right of the bar.
  - In `peek.rs`, `peek_sits_away_from_edge`: the peek sits right of a Left bar and below a Top bar.
  - In `peek.rs`, `top_peek_clamps_at_window_end` (Review Focus 2): with the rightmost bar on Top, the peek stays inside the bounds.
- [ ] **Step 2:** Run the new tests. Expected: FAIL.
- [ ] **Step 3:** Rewrite the geometry in local coordinates, mapping it once through `Edge`.
  - Drawing helpers take `LocalRect`s.
  - The chip, toast and slot-tooltip anchors use `place_away`. Keep `chip_rect`/`toast_rect` signatures, but add `edge`.
  - The drag ghost follows the along axis.
  - The auto-hide translation (`with_translation`) becomes an away offset mapped through the edge, so the strip moves toward its edge. Keep the existing `x_offset` name if it's simpler, but make it an away offset converted with the edge.
- [ ] **Step 4:** Run the full suite. Expected: all old tests pass unchanged on Right, and the new ones pass.
- [ ] **Step 5:** Run clippy, fmt and the cross-checks.
- [ ] **Step 6:** Commit with the message `feat: lay out and hit-test the strip along any edge`.

### Task 3: App wiring (docking, auto-hide, notes, panels, drops, live change)

**Files:**
- Modify: `src/app.rs`, `src/main.rs` (initial `dock`), `src/color_bubble.rs` (only if its placement assumes "above the button" relative to the strip; it stays above the button in the note header, so expect no change)

**Interfaces:**
- Consumes: Tasks 1–2. `Edge`, `band(.., edge)`, `compute_layout(.., edge)`, `peek_layout(.., edge)`, and the along-based drop helpers.
- Produces:
  - `fn dock(edge: Edge, window: Size, monitor: Size) -> Point` in `main.rs`, replacing `dock_right`.
  - `App::edge() -> Edge`.
  - `Message::EdgeChosen(Edge)` handling (E10).

- [ ] **Step 1: Write the failing tests** in `app.rs`:
  - `dock_window_follows_edge`: use the pure helpers for window size and origin with passthrough on and off. Without passthrough, on Top, the window is `open_width`/STRIP_WIDTH **tall** and spans the strip length in width.
  - `strip_band_and_edge_zone_per_edge`: for Left, a cursor at x=1 within the band is `in_edge`, and one at x=width−1 is not. For Top, y=1 within the band is `in_edge`.
  - `auto_hide_edge_zone_follows_edge` (Review Focus 5): on Left, hiding moves the strip's drawn rects to x < 0. A cursor dwelling at the right edge does not reveal it; one at the left edge does.
  - `linux_sliver_axis_per_edge`: the sliver is 2 px **tall** on Top and 2 px wide on Left.
  - `note_unfolds_away_from_edge`: on Top, the note's morph source is the bar and its open rect sits below the strip. A note with a saved position keeps it.
  - `edge_change_keeps_note_open_and_closes_peek` (Review Focus 4): with a note open and the peek showing, `EdgeChosen(Edge::Top)` leaves the note open, sets the peek to closed, cancels any drag, returns a re-dock task, and persists `window.edge = "top"`.
  - `drop_on_top_bar_appends`: a file drop with the cursor over a Top-strip bar appends to that bar's note.
  - `panels_open_away_from_edge`: settings, search and export open on the away side for Left and Top.
- [ ] **Step 2:** Run them. Expected: FAIL.
- [ ] **Step 3:** Route every right-edge assumption through `self.edge()`. Find them with `grep -n "STRIP_WIDTH\|window_size.width\|monitor.width\|strip_x_offset\|dock_right" src/app.rs src/main.rs`. Cover:
  - `strip_layout`, `dock_window`, the width and thickness helpers, `strip_band`/`in_band`, `in_edge`, `is_interactive`, `over_strip`;
  - the note and panel morph anchors;
  - the `CursorMoved`/drag/drop routing to the along axis;
  - the auto-hide offset direction;
  - the Linux sliver axis;
  - the top inset: the macOS menu bar height if it's available from the existing monitor or work-area code, otherwise 0;
  - the initial position in `main.rs`.
- [ ] **Step 4:** Implement `EdgeChosen` (E10): persist the setting, close the peek and chips, cancel any drag, keep the open note, re-dock, and recompute passthrough.
- [ ] **Step 5:** Run the full suite, clippy, fmt and the cross-checks. Expected: all green.
- [ ] **Step 6:** Commit with the message `feat: dock the strip to the right, left or top edge`.

### Task 4: README

**Files:** Modify: `README.md`

- [ ] **Step 1:** In the intro and the strip section, say that the strip can sit on the right (the default), left or top edge. On the Settings table's Window row, add "Screen edge (Right, Left, Top)" and rename the height setting to "Strip length" (its share of the edge). Mention that everything opens on the side away from the edge.
- [ ] **Step 2:** Commit with the message `docs: screen edge in the README`.
