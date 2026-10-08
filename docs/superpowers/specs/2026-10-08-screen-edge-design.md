# Screen Edge — Design Spec

## Overview

The note strip can dock to the **right** edge of the screen, as it does today, or to the **left** or **top** edge. It should feel the same on every edge, just on another side of the screen. Right stays the default. The edge is chosen in Settings.

The user approved approach A in chat: edge-local geometry with a single mapping layer. Bottom is out of scope.

Branch: `feat/screen-edge`.

## Goals and success criteria

- With the edge set to Right, behaviour and pixels are identical to today, and the existing test suite passes unchanged.
- **Left** mirrors Right. The bars hug the left edge and grow to the right, and the peek, chips, toast, note and bubble open to the right of the strip.
- **Top** turns the strip on its side. The bars sit side by side along the top edge, hang down and magnify downward. Everything opens below the strip. Text is never rotated.
- Changing the setting re-docks at once, with no restart.

## Decisions

| # | Topic | Decision |
|---|---|---|
| E1 | Setting | Settings → Window gets **"Screen edge"** with the choices Right, Left and Top, shown as a three-way segmented control matching the existing controls. It is persisted as `window.edge` (`"right"` / `"left"` / `"top"`), with a default of `"right"`. Unknown values read as Right. Reset Window restores Right. |
| E2 | Edge-local frame | All strip geometry is computed in an edge-local frame. **along** is the axis parallel to the edge, measured from its start (top for Right and Left, left for Top). **away** is the distance from the screen edge into the screen. A `pub enum Edge { Right, Left, Top }` in a new pure module `src/edge.rs` maps between the edge-local frame and window coordinates for points and rects. Mapping must round-trip. **Right is the identity of today's layout:** for Right, the edge-local frame *is* today's coordinates, with along = y and away = window.width − x. |
| E3 | Content boxes | Boxes that hold text keep their real width and height: the peek, chips, the undo toast, the note, the color bubble, and the settings, search and export panels. They are never transformed. Each one is placed on the away side of its anchor: left of a Right strip, right of a Left strip, below a Top strip. It is centred along the edge on its anchor, then clamped into the window, as today. On Top, the peek's "length" grows downward and its width is the note width. |
| E4 | Strip length | The existing `window.height_fraction` setting stays and is reinterpreted as the share of the **edge length**: the screen height for Left and Right, the screen width for Top. Its settings label becomes **"Strip length"**. The stored key is unchanged. |
| E5 | Docking | `dock_window` places the window flush to the chosen edge and centres it along that edge. On Top, the reference is the top of the usable area: below the macOS menu bar, or the top of the work area on Windows and Linux, wherever the monitor or work-area data the app already has allows it. On a non-passthrough window (Linux), the docked thickness is STRIP_WIDTH, measured along the away axis: a height for Top. With passthrough, the window keeps covering the monitor; only the strip's band moves. |
| E6 | Strip drawing | The bars are laid out along the edge, with their thickness growing away from it. Magnification follows the cursor's along position. The slots (+, 🔍, settings where present) come after the last bar along the edge: below the bars on Left and Right, to their right on Top. The pin notch sits at the bar's start along the edge. Stack edges sit on the bar's away side. The progress fill grows from the bar's end along the edge, which is its bottom on Left and Right and its right end on Top. On Top the fill therefore grows leftward from the right end. This is the edge-local equivalent of "from the bottom". All of these are expressed once, in edge-local terms. |
| E7 | Interactions | Hover, the peek target, drag reorder (the insertion slot), stack drop zones (the middle 50% along the edge), the file-drop bar hit, the wheel scroll direction (scroll moves along the edge; Top maps both wheel axes to along), and the search slot and add slot hit areas all use edge-local coordinates. |
| E8 | Note morph | A note unfolds from its bar toward the away side, and folds back into its bar. Notes with a saved position (dragged by the user) keep their window position after an edge change. On a Linux window resize from the edge change, they are clamped into the screen as they are today. |
| E9 | Auto-hide | The strip slides off toward its edge (the away offset goes negative). The 2 px edge zone is the outermost 2 px on the chosen edge, within the strip's band along that edge. On Linux the sliver is 2 px thick on the chosen edge. |
| E10 | Live change | Changing the edge closes the peek and any chip, cancels a running drag, keeps an open note open (it re-anchors), and re-docks the window. Passthrough regions are recomputed. |

## Architecture

- **`src/edge.rs` (new, pure, unit-tested).**
  - `Edge { Right, Left, Top }` with `serde`-friendly names.
  - `fn to_window(self, p: LocalPoint, window: Size) -> Point` and `fn to_local(self, p: Point, window: Size) -> LocalPoint`.
  - `fn rect_to_window(self, r: LocalRect, window: Size) -> Rectangle`.
  - `fn place_away(self, anchor: Rectangle, content: Size, gap: f32, bounds: Rectangle) -> Rectangle` for content boxes (E3).
  - `fn edge_length(self, monitor: Size) -> f32` and `fn dock_origin(self, window: Size, monitor: Size, top_inset: f32) -> Point`.
  - `LocalPoint` and `LocalRect` are named `along`/`away` structs, not iced types, so the two frames can't be mixed up by accident.
- **`bar_strip.rs`:** `compute_layout` works in local coordinates and maps through `Edge`. The drawing helpers (notch, stack edges, progress) take local rects. Hit-testing converts the cursor to local coordinates first. `BarStrip` gets an `edge: Edge` field.
- **`peek.rs`:** `peek_layout` places the peek with `place_away`. Its internal layout (title, rows, buttons) stays in the peek's own box, unchanged.
- **`app.rs`:**
  - `strip_layout`, `dock_window`, `docked_width`/`window_width` (now a thickness along the away axis), `strip_band`, `in_edge`, `is_interactive`, the note and panel morph anchors, drop routing, and `strip_x_offset` (now an away offset) all go through `Edge`.
  - `SettingToggled`/`SettingChanged` for the edge re-docks (E10).
- **`settings.rs` / `settings_panel.rs`:** `WindowSettings.edge: Edge`, the segmented control, and the "Strip length" label.
- **`main.rs`:** `dock_right` becomes `dock(edge)` for the initial position. The initial window size uses the saved edge.

## Error handling

- An unknown `window.edge` reads as Right.
- A missing top inset (menu bar or work area unknown) counts as 0.
- Monitor changes re-dock on the current edge.

## Testing

- **`edge`:** round trips for every edge; Right is the identity; `place_away` puts boxes on the correct side and clamps them; `dock_origin` for every edge, including the top inset.
- **`bar_strip`:**
  - bar rects for the same entries on every edge, mapped back to local, are equal;
  - hit-tests at mapped points select the same bar, slot and drop zone on every edge;
  - the progress fill, notch and stack-edge rects sit on the expected sides.
- **`peek`:** the peek sits right of a Left strip and below a Top strip, and is clamped.
- **`app`:**
  - the setting round-trips and its default is Right;
  - an edge change re-docks, closes the peek and keeps an open note open;
  - auto-hide on Left and Top hides toward the edge, and the edge zone is on the chosen edge;
  - the Linux sliver (pure helper) has the right thickness and axis for each edge;
  - file drops on a Top-strip bar append to that bar's note.
- **The whole existing suite** passes unchanged on Right.
- **Manual (macOS):** each edge for hover, peek, opening and folding a note, dragging, stacking, auto-hide, the color bubble and the toast.

## README

- Settings → Window: "Screen edge" (Right, Left, Top) and "Strip length".
- The introduction notes that the strip can sit on the left or top edge.
- The overview illustration stays right-edge.

## Out of scope

- The bottom edge.
- A different edge per monitor.
- Rotated text.
- Moving the edge by dragging the strip.
