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
    assert_eq!(app.anchor_along, stack_bar.center().y);
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

fn enter(app: &mut App) {
    let _ = app.update(Message::NoteEdited(text_editor::Action::Edit(
        text_editor::Edit::Enter,
    )));
}

#[test]
fn enter_continues_a_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_note(&dir, "1. a");
    let _ = app.update(Message::BodyClicked(None));
    enter(&mut app);
    assert_eq!(app.editor_content.as_ref().unwrap().text(), "1. a\n2. ");
    let at = cursor_of(&app);
    assert_eq!((at.line, at.column), (1, 3));
    assert_eq!(app.store.notes()[0].content, "1. a\n2. ");
    // One undo step takes the whole continuation back.
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor_content.as_ref().unwrap().text(), "1. a");
}

#[test]
fn enter_on_an_empty_item_ends_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_note(&dir, "- a");
    let _ = app.update(Message::BodyClicked(None));
    enter(&mut app);
    enter(&mut app);
    assert_eq!(app.editor_content.as_ref().unwrap().text(), "- a\n");
    let at = cursor_of(&app);
    assert_eq!((at.line, at.column), (1, 0));
    let _ = app.update(Message::Undo);
    assert_eq!(app.editor_content.as_ref().unwrap().text(), "- a\n- ");
}

#[test]
fn enter_with_a_selection_or_in_code_is_a_plain_break() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_note(&dir, "```\n- a");
    let _ = app.update(Message::BodyClicked(None));
    enter(&mut app);
    assert_eq!(app.editor_content.as_ref().unwrap().text(), "```\n- a\n");

    let mut app = app_with_note(&dir, "- ab");
    let _ = app.update(Message::BodyClicked(None));
    let _ = app.update(Message::NoteEdited(text_editor::Action::Select(
        text_editor::Motion::Left,
    )));
    enter(&mut app);
    assert_eq!(app.editor_content.as_ref().unwrap().text(), "- a\n");
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
    let before = app.anchor_along;
    let _ = app.update(Message::TogglePin);
    assert_eq!(app.store.notes()[0].id, ids[2]);
    assert!(app.store.notes()[0].pinned);
    assert!(app.store.is_dirty());
    assert_eq!(app.active_note, Some(ids[2]));
    assert_eq!(app.id_entry(ids[2]), Some(0));
    assert!(app.anchor_along < before, "anchor follows the moved bar");
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
fn the_reminder_picker_card_takes_presses_while_open() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_note(&dir, "x");
    let note = app.note_frame().unwrap().rect;
    // The calendar shares the bubble overlay above the note.
    let card = Rectangle::new(Point::new(note.x, note.y - 260.0), Size::new(240.0, 220.0));
    let on_card = card.center();
    assert!(!app.is_interactive(on_card));
    let _ = app.update(Message::OpenReminderPicker);
    assert!(app.reminder_picker.is_some());
    let _ = app.update(Message::ColorBubbleMoved(card));
    assert!(app.is_interactive(on_card), "the calendar takes clicks");
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
    app.settings_panel.anchor_along = 123.0;
    let _ = app.update(Message::ToggleSettings);
    assert!(!app.settings_panel.morph.is_opening());
    let _ = app.update(Message::ToggleSettings);
    assert!(app.settings_panel.morph.is_opening());
    assert_eq!(app.settings_panel.anchor_along, 123.0);
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

/// Ticks `app` once, 16 ms after `now`; returns the new instant.
fn tick_once(app: &mut App, now: Instant) -> Instant {
    let now = now + Duration::from_millis(16);
    let _ = app.update(Message::Tick(now));
    now
}

#[test]
fn actions_expand_animates() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(&dir);
    app.window_size = Size::new(1400.0, 900.0);
    app.store.add_note(&crate::note::PALETTE);
    let _ = app.update(Message::ActionsHovered(true));
    assert!(app.animating, "no frames for the unfold");
    let strip = app.strip_layout();
    assert_eq!(strip.actions_progress, 0.0, "jumped open");
    let origin = strip.actions_slot.center();
    // Starts at the collapsed slot.
    for pill in strip.pill_rects() {
        assert!(pill.center().distance(origin) < 0.5, "{pill:?}");
    }
    let mut now = Instant::now();
    let mut last_progress = 0.0;
    let mut last_x = [origin.x; 3];
    let mut ticks = 0;
    while app.strip_layout().actions_progress < 1.0 {
        now = tick_once(&mut app, now);
        ticks += 1;
        assert!(ticks < 60, "never finished");
        let strip = app.strip_layout();
        assert!(strip.actions_progress > last_progress, "progress stalled");
        last_progress = strip.actions_progress;
        // On Right the pills slide left, away from the edge.
        for (i, pill) in strip.pill_rects().iter().enumerate() {
            assert!(pill.center().x <= last_x[i] + 0.01, "pill {i} went back");
            last_x[i] = pill.center().x;
        }
    }
    // ~180 ms at speed 1.
    assert!((8..=16).contains(&ticks), "{ticks} ticks");
    let strip = app.strip_layout();
    assert_eq!(
        strip.pill_rects(),
        [strip.add_button, strip.search_button, strip.settings_button]
    );
    // Idle once settled.
    tick_once(&mut app, now);
    assert!(!app.animating, "frames while idle");
}

#[test]
fn actions_collapse_mid_way_reverses_smoothly() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(&dir);
    app.window_size = Size::new(1400.0, 900.0);
    let _ = app.update(Message::ActionsHovered(true));
    let mut now = Instant::now();
    for _ in 0..5 {
        now = tick_once(&mut app, now);
    }
    let mid = app.strip_layout().actions_progress;
    assert!(mid > 0.0 && mid < 1.0, "{mid}");
    let pills = app.strip_layout().pill_rects();
    let _ = app.update(Message::ActionsHovered(false));
    assert!(!app.actions_open);
    let strip = app.strip_layout();
    assert_eq!(strip.actions_progress, mid, "the collapse jumped");
    for (a, b) in strip.pill_rects().iter().zip(pills) {
        assert!(a.center().distance(b.center()) < 0.5, "pill jumped");
    }
    now = tick_once(&mut app, now);
    let back = app.strip_layout().actions_progress;
    assert!(back < mid && back > 0.0, "{back}");
    // And forward again from there.
    let _ = app.update(Message::ActionsHovered(true));
    assert_eq!(app.strip_layout().actions_progress, back);
    tick_once(&mut app, now);
    assert!(app.strip_layout().actions_progress > back);
    settle(&mut app);
    assert_eq!(app.strip_layout().actions_progress, 1.0);
    let _ = app.update(Message::ActionsHovered(false));
    settle(&mut app);
    assert_eq!(app.strip_layout().actions_progress, 0.0);
    assert!(!app.animating);
}

#[test]
fn actions_pills_clickable_only_when_expanded() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(&dir);
    app.window_size = Size::new(1400.0, 900.0);
    let _ = app.update(Message::ActionsHovered(true));
    let target = |app: &App| app.strip_layout().settings_button.center();
    let press = |app: &App| {
        let strip = app.strip_layout();
        strip.press_message(target(app), keyboard::Modifiers::default())
    };
    let mut now = Instant::now();
    now = tick_once(&mut app, now);
    assert!(app.strip_layout().actions_progress < 1.0);
    assert!(press(&app).is_none(), "a half-drawn pill took the click");
    assert_eq!(app.strip_layout().slot_at(target(&app)), None);
    settle(&mut app);
    assert!(matches!(press(&app), Some(Message::ToggleSettings)));
    let at = target(&app);
    // Collapsing: nothing but the Actions slot.
    let _ = app.update(Message::ActionsHovered(false));
    tick_once(&mut app, now);
    let strip = app.strip_layout();
    assert!(strip.actions_progress > 0.0);
    assert!(strip
        .press_message(at, keyboard::Modifiers::default())
        .is_none());
}

#[test]
fn settings_panel_can_be_dragged() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(&dir);
    app.window_size = Size::new(1400.0, 900.0);
    let _ = app.update(Message::ToggleSettings);
    settle(&mut app);
    let start = app.settings_frame().unwrap().rect.position();
    app.last_cursor = Some(Point::new(start.x + 20.0, start.y + 8.0));
    let _ = app.update(Message::SettingsDragStart);
    assert!(app.settings_drag.is_some());
    let _ = app.update(Message::CursorMoved(Point::new(
        start.x + 120.0,
        start.y + 80.0,
    )));
    app.mouse_down = false;
    let _ = app.update(Message::MouseButton(false));
    let after = app.settings_frame().unwrap().rect.position();
    assert!((after.x - start.x).abs() > 50.0 || (after.y - start.y).abs() > 50.0);
    assert_eq!(app.settings_pos, Some(after));
    let _ = app.update(Message::SettingsResetPosition);
    settle(&mut app);
    assert!(app.settings_pos.is_none());
    let reset = app.settings_frame().unwrap().rect.position();
    assert!((reset.x - start.x).abs() < 1.0 && (reset.y - start.y).abs() < 1.0);
}

#[test]
fn top_panel_keeps_its_bottom_margin() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = on_edge(app_in(&dir), Edge::Top);
    app.window_size = Size::new(1400.0, 500.0);
    let rect = app.settings_target_rect();
    assert!(rect.height < PANEL_MAX_HEIGHT, "must be height-limited");
    assert_eq!(rect.y + rect.height, app.window_size.height - NOTE_MARGIN);
}

#[test]
fn expanded_settings_stays_interactive() {
    // Regression: when the expanded row sticks past STRIP_WIDTH,
    // passthrough must still treat Settings as interactive or the
    // window ignores the cursor and the row collapses before a click.
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(&dir);
    app.window_size = Size::new(1400.0, 900.0);
    app.settings.settings_mut().bars.width = 18.0;
    app.store.add_note(&crate::note::PALETTE);
    let _ = app.update(Message::ActionsHovered(true));
    let settings = app.strip_layout().settings_button.center();
    assert!(
        !app.near_edge(settings, STRIP_WIDTH),
        "precondition: Settings is past the strip band ({settings:?})"
    );
    assert!(
        app.is_interactive(settings),
        "Settings at {settings:?} must stay interactive while Actions is open"
    );
    assert!(app.over_strip(settings));
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
    assert!(!app.settings_panel.open);
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
    assert!(app.settings_panel.open && app.settings_panel.morph.is_opening());
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
    app.search_panel.open && app.search_panel.morph.is_opening()
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
    assert!(app.settings_panel.open);
}

#[test]
fn cmd_f_opens_search_and_closes_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(&dir);
    let _ = app.update(Message::ToggleSettings);
    settle(&mut app);
    let _ = app.update(cmd_f());
    assert!(search_showing(&app));
    assert!(!app.settings_panel.morph.is_opening());
    settle(&mut app);
    assert!(!app.settings_panel.open);
    // Settings close the search the same way.
    let _ = app.update(Message::ToggleSettings);
    assert!(!app.search_panel.morph.is_opening());
    assert!(app.settings_panel.morph.is_opening());
    // Cmd+F again toggles it.
    let _ = app.update(cmd_f());
    let _ = app.update(cmd_f());
    assert!(!app.search_panel.morph.is_opening());
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
    assert!(!app.search_panel.morph.is_opening());
    assert_eq!(selected(&app).as_deref(), Some("Café"));
    let cursor = app.editor_content.as_ref().unwrap().cursor();
    assert_eq!(
        (cursor.position.line, cursor.position.column),
        (1, "say Café".len())
    );
    settle(&mut app);
    assert!(!app.search_panel.open);
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
    assert!(app.search_panel.morph.is_opening());
    let _ = app.update(Message::SearchChanged("needle".into()));
    let _ = app.update(Message::SearchSubmitted);
    assert_eq!(app.active_note, Some(ids[0]));
    assert!(app.editing);
    assert!(!app.search_panel.morph.is_opening());
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
    assert!(!app.search_panel.open);
}

#[test]
fn escape_closes_search_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_in(&dir);
    let _ = app.update(Message::ToggleSearch);
    settle(&mut app);
    let _ = app.update(escape());
    assert!(!app.search_panel.morph.is_opening());
    settle(&mut app);
    assert!(!app.search_panel.open);

    // Esc swallowed by the focused search field closes it too.
    let _ = app.update(Message::ToggleSearch);
    settle(&mut app);
    let _ = app.update(Message::CloseSearch);
    assert!(!app.search_panel.morph.is_opening());
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
    assert!(app.search_panel.open && !app.search_panel.morph.is_opening());
    let _ = app.update(Message::ToggleSearch);
    assert!(search_showing(&app));
    assert!(!app.settings_panel.morph.is_opening());
    settle(&mut app);
    assert!(!app.settings_panel.open);
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
    assert!(app.search_panel.open && app.morph.is_opening());
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
    assert!(app.settings_panel.open && !app.settings_panel.morph.is_opening());
    let _ = app.update(Message::ToggleSettings);
    assert!(app.settings_panel.morph.is_opening());
    assert!(!app.search_panel.morph.is_opening());
    settle(&mut app);
    assert!(!app.search_panel.open);
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
    app.export.is_some() && app.export_panel.morph.is_opening()
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
    assert!(!app.settings_panel.morph.is_opening());
    let _ = app.update(Message::ToggleExport);
    assert!(!app.export_panel.morph.is_opening());
    settle(&mut app);
    assert!(app.export.is_none());
    assert!(!app.settings_panel.open);
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
    assert!(!app.export_panel.morph.is_opening());
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
    assert!(!app.export_panel.morph.is_opening());
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
    assert!(!app.export_panel.morph.is_opening());
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
        assert!(!app.settings_panel.morph.is_opening());
        assert!(!app.search_panel.morph.is_opening() && !app.export_panel.morph.is_opening());
        assert!(app.active_note.is_some() && app.morph.is_opening());
        settle(&mut app);
        assert!(!app.settings_panel.open && !app.search_panel.open && app.export.is_none());
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
    assert!(!app.export_panel.morph.is_opening());
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
        assert!(app.export.is_some() && !app.export_panel.morph.is_opening());
        let _ = app.update(Message::ToggleExport);
        assert!(export_showing(&app));
        assert!(!app.settings_panel.morph.is_opening());
        assert!(!app.search_panel.morph.is_opening());
        assert!(!app.morph.is_opening());
        settle(&mut app);
        assert!(!app.settings_panel.open && !app.search_panel.open && app.active_note.is_none());
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
    }
    // And the other panels close the export the same way.
    for other in [Message::ToggleSettings, Message::ToggleSearch] {
        let _ = app.update(Message::ToggleExport);
        settle(&mut app);
        let _ = app.update(other);
        assert!(!app.export_panel.morph.is_opening());
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
    app.settings.settings_mut().motion.reminder_alert = crate::settings::ReminderAlertStyle::Pulse;
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

/// The open note's title as its title field shows it.
fn title_field(app: &App) -> String {
    app.title_field_text().unwrap()
}

fn tag(app: &App) -> Option<String> {
    reminder::tag_text(&app.store.notes()[0].title).map(str::to_string)
}

#[test]
fn editing_visible_title_keeps_reminder() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    let _ = app.update(Message::TitleEdited("Call @2026-10-15 16:30".into()));
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(title_field(&app), "Call");
    let set = app.store.notes()[0].reminder_set_at;
    assert!(set.is_some());
    for typed in ["Call ", "Call B", "Call Bob"] {
        let _ = app.update(Message::TitleEdited(typed.into()));
        assert_eq!(
            title_field(&app),
            typed,
            "the field changed under the cursor"
        );
    }
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(title_field(&app), "Call Bob");
    let note = &app.store.notes()[0];
    assert_eq!(reminder::tag_text(&note.title), Some("@2026-10-15 16:30"));
    assert_eq!(note.reminder_set_at, set, "an unchanged tag re-armed");
    assert_eq!(reminder::display(&note.title), "Call Bob");
}

#[test]
fn typing_a_tag_moves_it_into_the_reminder() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    let _ = app.update(Message::TitleEdited("Call @15:00".into()));
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(title_field(&app), "Call");
    let _ = app.update(Message::TitleEdited("Call @tue".into()));
    // Shown as typed while editing, with the label already live.
    assert_eq!(title_field(&app), "Call @tue");
    assert_eq!(tag(&app).as_deref(), Some("@tue"));
    assert!(matches!(
        reminder::status(&app.store.notes()[0]),
        reminder::Status::Pending(_)
    ));
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(title_field(&app), "Call");
    assert_eq!(tag(&app).as_deref(), Some("@tue"));
}

#[test]
fn typing_a_partial_tag_is_not_captured() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    let _ = app.update(Message::TitleEdited("Call @15:00".into()));
    let _ = app.update(Message::TitleCommitted);
    let set = app.store.notes()[0].reminder_set_at;
    for typed in ["Call @mon", "Call @mond", "Call @monday"] {
        let _ = app.update(Message::TitleEdited(typed.into()));
        assert_eq!(title_field(&app), typed, "captured while typing");
    }
    // Back to the hidden tag mid-way: its anchor is kept.
    let _ = app.update(Message::TitleEdited("Call @mond".into()));
    assert_eq!(tag(&app).as_deref(), Some("@15:00"));
    assert_eq!(app.store.notes()[0].reminder_set_at, set);
    let _ = app.update(Message::TitleEdited("Call @monday".into()));
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(tag(&app).as_deref(), Some("@monday"));
    assert_eq!(title_field(&app), "Call");
}

#[test]
fn date_then_time_captured_together() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    for typed in [
        "Call @2026-10-15",
        "Call @2026-10-15 ",
        "Call @2026-10-15 16:30",
    ] {
        let _ = app.update(Message::TitleEdited(typed.into()));
        assert_eq!(title_field(&app), typed);
    }
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(tag(&app).as_deref(), Some("@2026-10-15 16:30"));
    assert_eq!(title_field(&app), "Call");
}

#[test]
fn tag_captured_on_enter_and_on_blur() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    // Enter (the field's submit).
    let _ = app.update(Message::TitleEdited("A @tue".into()));
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(title_field(&app), "A");
    // The body taking the focus.
    let _ = app.update(Message::TitleEdited("B @wed".into()));
    let _ = app.update(Message::BodyClicked(None));
    assert_eq!(title_field(&app), "B");
    // The field losing the focus some other way (the focus check).
    let _ = app.update(Message::TitleEdited("C @thu".into()));
    let _ = app.update(Message::TitleBlurred(true));
    assert_eq!(title_field(&app), "C @thu", "still focused");
    let _ = app.update(Message::TitleBlurred(false));
    assert_eq!(title_field(&app), "C");
    // The note closing.
    let _ = app.update(Message::TitleEdited("D @fri".into()));
    let _ = app.update(Message::ClosePanel);
    assert!(app.title_edit.is_none());
    assert_eq!(reminder::visible_title(&app.store.notes()[0].title), "D");
}

#[test]
fn invalid_tag_stays_visible() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    let _ = app.update(Message::TitleEdited("Call @frx".into()));
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(title_field(&app), "Call @frx");
    let note = &app.store.notes()[0];
    assert_eq!(reminder::status(note), reminder::Status::Invalid);
}

#[test]
fn legacy_bare_weekday_tag_does_not_fire_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.json");
    let mut store = NoteStore::load(path.clone());
    let id = store.add_note(&crate::note::PALETTE);
    let note = store.note_mut(id).unwrap();
    note.title = "Team @monday standup".into();
    note.updated_at = chrono::Utc::now() - chrono::Duration::days(14);
    note.reminder_set_at = None;
    store.save().unwrap();

    let mut app = app_in(&dir);
    let note = &app.store.notes()[0];
    assert!(note.reminder_set_at.is_some(), "anchor not frozen");
    let _ = app.update(Message::ReminderTick);
    assert!(app.pulsing.is_empty(), "a legacy tag alerted on load");
    let note = &app.store.notes()[0];
    assert_eq!(reminder::due(note, chrono::Local::now()), None);
}

#[test]
fn resting_at_the_edge_outside_the_band_does_not_cycle() {
    use crate::autohide::Phase;
    let dir = tempfile::tempdir().unwrap();
    let mut app = auto_hide_app(&dir);
    let mut now = hide_strip(&mut app, Instant::now());
    let corner = Point::new(app.window_size.width - 1.0, 5.0);
    assert!(
        !app.in_band(app.along(corner)),
        "precondition: outside the band"
    );
    let _ = app.update(Message::CursorMoved(corner));
    let mut reveals = 0;
    let mut hid_after_reveal = false;
    let mut last = app.auto_hide.phase();
    for _ in 0..(5000 / 16) {
        now += Duration::from_millis(16);
        let _ = app.update(Message::Tick(now));
        let phase = app.auto_hide.phase();
        if phase == Phase::Revealing && last != Phase::Revealing {
            reveals += 1;
        }
        if reveals > 0 && phase == Phase::Hiding {
            hid_after_reveal = true;
        }
        last = phase;
    }
    assert_eq!(reveals, 1);
    assert!(!hid_after_reveal, "the strip hid under a resting cursor");
    assert_eq!(app.auto_hide.phase(), Phase::Shown);
    // Moving away hides it after the grace.
    let _ = app.update(Message::CursorMoved(Point::new(700.0, 450.0)));
    run_for(&mut app, now, 2000);
    assert!(
        app.auto_hide.offset() > 0.0,
        "didn't hide once the cursor left"
    );
}

#[test]
fn no_fire_while_typing_a_partial_past_date() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    // On the way to "@2026-01-020…" the text parses as a past date.
    let _ = app.update(Message::TitleEdited("Call @2026-01-02".into()));
    let _ = app.update(Message::ReminderTick);
    assert!(app.pulsing.is_empty(), "fired while typing");
    assert_eq!(app.store.notes()[0].reminder_fired, None);
}

#[test]
fn commit_sets_fresh_anchor_for_a_new_tag() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    let id = app.active_note.unwrap();
    let days_ago = |d| Some(chrono::Utc::now() - chrono::Duration::days(d));
    // A new tag, typed long before the commit, counts from the commit.
    let _ = app.update(Message::TitleEdited("Call @16:00".into()));
    app.store.note_mut(id).unwrap().reminder_set_at = days_ago(3);
    let before = chrono::Utc::now();
    let _ = app.update(Message::TitleCommitted);
    let anchor = app.store.notes()[0].reminder_set_at.unwrap();
    assert!(anchor >= before, "the anchor wasn't refreshed on commit");
    // The same tag as before the edit keeps its anchor.
    app.store.note_mut(id).unwrap().reminder_set_at = days_ago(1);
    let kept = app.store.notes()[0].reminder_set_at;
    let _ = app.update(Message::TitleEdited("Call @17:00".into()));
    let _ = app.update(Message::TitleEdited("Call".into()));
    let _ = app.update(Message::TitleCommitted);
    assert_eq!(tag(&app).as_deref(), Some("@16:00"));
    assert_eq!(app.store.notes()[0].reminder_set_at, kept);
}

#[test]
fn picker_tag_survives_a_following_title_edit() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    let _ = app.update(Message::TitleEdited("Call".into()));
    let _ = app.update(Message::ToggleReminderPicker);
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
    let _ = app.update(Message::ReminderPickerDay(date));
    if let Some(draft) = &mut app.reminder_picker {
        draft.hour = 16;
        draft.minute = 45;
    }
    let _ = app.update(Message::ReminderPickerDone);
    assert_eq!(title_field(&app), "Call");
    let _ = app.update(Message::TitleEdited("Call Bob".into()));
    assert_eq!(tag(&app).as_deref(), Some("@2026-11-03 16:45"));
    // Clearing it sticks too.
    let _ = app.update(Message::OpenReminderPicker);
    let _ = app.update(Message::ReminderPickerClear);
    let _ = app.update(Message::TitleEdited("Call Bobby".into()));
    assert_eq!(tag(&app), None);
}

#[test]
fn delete_mid_edit_then_undo_shows_no_raw_tag() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_open_note(&dir);
    let id = app.active_note.unwrap();
    let _ = app.update(Message::TitleEdited("Call @tue".into()));
    let _ = app.update(Message::DeleteNote(id));
    settle(&mut app);
    assert!(app.active_note.is_none());
    let _ = app.update(Message::UndoDelete);
    settle(&mut app);
    let _ = app.update(Message::BarClicked(0));
    settle(&mut app);
    assert_eq!(title_field(&app), "Call");
}

#[test]
fn copy_omits_the_raw_tag() {
    let mut note = crate::note::Note::new(crate::note::PALETTE[0]);
    note.title = "Call @2026-10-15 16:30".into();
    note.content = "body".into();
    note.reminder_set_at = Some(chrono::Utc::now());
    assert_eq!(
        crate::note::copy_text(&note),
        "Call\nReminder: Thu 15 Oct 16:30\n\nbody"
    );
    note.title = "@2026-10-15 16:30".into();
    assert_eq!(
        crate::note::copy_text(&note),
        "Reminder: Thu 15 Oct 16:30\n\nbody"
    );
    note.title = "Call @frx".into();
    assert_eq!(crate::note::copy_text(&note), "Call @frx\n\nbody");
}

#[test]
fn peek_and_search_show_display_title() {
    let dir = tempfile::tempdir().unwrap();
    let (app, ids) = app_with_notes(
        &dir,
        &[("Call @2026-10-15 16:30", "body"), ("Plan @tue", "x")],
    );
    let notes = app.store.notes();
    let call = notes.iter().find(|n| n.id == ids[0]).unwrap();
    let text = crate::peek::peek_text(call, 300.0);
    assert_eq!(text.title.as_deref(), Some("Call"));
    // A stack's rows.
    let mut stacked = notes.to_vec();
    let plan = stacked.iter().position(|n| n.id == ids[1]).unwrap();
    stacked[plan].stack = Some(ids[0]);
    let entries = crate::strip_model::entries(&stacked);
    let entry = entries.iter().find(|e| !e.members.is_empty()).unwrap();
    let rows = crate::peek::entry_peek_text(&stacked, entry, 300.0).stack;
    let titles: Vec<&str> = rows.iter().map(|(_, t)| t.as_str()).collect();
    assert!(
        titles.contains(&"Call") && titles.contains(&"Plan"),
        "{titles:?}"
    );
    // Search hits, and the hidden tag never matches.
    let hits = crate::search::search(notes, "call").hits;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Call");
    assert!(crate::search::search(notes, "2026").hits.is_empty());
    assert!(crate::search::search(notes, "tue").hits.is_empty());
    // Export panel rows.
    assert_eq!(crate::export_panel::row_label(call), "Call");
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
    assert_eq!(app.strip_away_offset(), 0.0);

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
    assert_eq!(app.strip_away_offset(), STRIP_WIDTH + HIDE_MARGIN);

    let edge = edge_point(&app);
    assert!(app.is_interactive(edge));
    let bar = app.strip_layout().bars[0].center();
    assert!(!app.is_interactive(bar), "the hidden bar took the click");
    let column = Point::new(app.window_size.width - 30.0, edge.y);
    assert!(!app.is_interactive(column));
    let left_of_edge = Point::new(app.window_size.width - 3.0, edge.y);
    assert!(!app.is_interactive(left_of_edge));
    // Anywhere along the edge reveals, outside the band too.
    let below_band = Point::new(edge.x, app.window_size.height - 2.0);
    assert!(app.is_interactive(below_band));
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
    assert!(!app.settings_panel.open);
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
    assert_eq!(app.strip_away_offset(), 0.0);
    assert!(app.is_interactive(app.strip_layout().bars[0].center()));
    run_for(&mut app, t, 2000);
    assert_eq!(app.auto_hide.offset(), 0.0);
}

/// Docks `app` the way `dock_window` would without passthrough.
fn dock_without_passthrough(app: &mut App) {
    app.window_size.width = app.window_thickness(false);
}

#[test]
fn linux_hidden_window_is_a_sliver() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = auto_hide_app(&dir);
    let open = app.settings.settings().open_width();
    assert_eq!(app.window_thickness(false), STRIP_WIDTH);
    assert_eq!(app.window_thickness(true), open);
    hide_strip(&mut app, Instant::now());
    assert_eq!(app.window_thickness(false), EDGE_SLIVER);
    assert_eq!(app.window_thickness(true), open, "passthrough stays wide");

    // Anywhere inside the sliver counts as the edge zone.
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
    assert!(app.strip_away_offset() > 0.0, "the slide already ran");
    assert_eq!(
        app.window_thickness(false),
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
    assert_eq!(app.window_thickness(false), open);
    dock_without_passthrough(&mut app);

    // The wide window's rightmost 2 px, anywhere along it, are the edge.
    let edge = edge_point(&app);
    assert!(app.in_edge(edge));
    assert!(!app.in_edge(Point::new(app.window_size.width - 3.0, edge.y)));
    assert!(app.in_edge(Point::new(edge.x, app.window_size.height - 2.0)));
    let _ = app.update(Message::CursorMoved(edge));
    assert!(app.auto_hide.next_deadline().is_some(), "no dwell started");

    // Once the note closes, the hidden window shrinks.
    let _ = app.update(Message::CursorMoved(app.note_target_rect().center()));
    let _ = app.update(Message::ClosePanel);
    tick_until(&mut app, |app| app.active_note.is_none());
    assert!(app.auto_hide.is_hidden());
    assert_eq!(app.window_thickness(false), EDGE_SLIVER);
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
fn sliver_edge_ignores_band() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = auto_hide_app(&dir);
    hide_strip(&mut app, Instant::now());
    dock_without_passthrough(&mut app);
    let mid = edge_point(&app).y;
    assert!(app.in_edge(Point::new(1.0, mid)));
    assert!(!app.in_edge(Point::new(-5.0, mid)), "left of the sliver");
    assert!(!app.in_edge(Point::new(3.0, mid)), "right of the sliver");
    let below = Point::new(1.0, app.window_size.height - 2.0);
    assert!(app.in_edge(below), "below the band");
    let _ = app.update(Message::CursorMoved(below));
    assert!(app.auto_hide.next_deadline().is_some(), "no dwell started");
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
    let strip = App::docked_thickness(&s, false, false);
    let wide = App::docked_thickness(&s, true, false);
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
    assert!(app.in_edge(Point::new(1.0, 898.0)), "outside the band");

    let app = on_edge(app, Edge::Top);
    assert_eq!(
        app.strip_bounds(),
        Rectangle::new(Point::ORIGIN, Size::new(1400.0, STRIP_WIDTH))
    );
    let mid = band_middle(&app);
    assert!(app.in_edge(Point::new(mid, 1.0)));
    assert!(!app.in_edge(Point::new(mid, 899.0)));
    assert!(app.in_edge(Point::new(1399.0, 1.0)), "outside the band");
    assert!(app.over_strip(Point::new(mid, STRIP_WIDTH - 1.0)));
    assert!(!app.over_strip(Point::new(mid, STRIP_WIDTH + 1.0)));
    assert!(app.is_interactive(Point::new(mid, 1.0)));
}

#[test]
fn auto_hide_edge_zone_follows_edge() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = on_edge(auto_hide_app(&dir), Edge::Left);
    hide_strip(&mut app, Instant::now());
    let shift = crate::bar_strip::hide_translation(Edge::Left, app.strip_away_offset());
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
    let open = App::docked_thickness(&Settings::default(), true, false);
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
    let top_anchor = app.settings_panel.anchor_along;
    let old = app.window_origin.unwrap();
    let (pointer, cursor) = (Point::new(100.0, -30.0), Point::new(100.0, 400.0));
    app.pointer = Some(pointer);
    app.last_cursor = Some(cursor);

    let _ = app.update(Message::ResetGroup(SettingsGroup::Window));
    assert_eq!(app.edge(), Edge::Right);
    let expected = along_center(app.strip_layout().settings_anchor(), Edge::Right);
    assert_ne!(expected, top_anchor);
    assert_eq!(
        app.settings_panel.anchor_along, expected,
        "settings not re-anchored"
    );
    assert_eq!(app.pointer, moved_into(&app, old, pointer));
    assert_eq!(app.last_cursor, moved_into(&app, old, cursor));
    assert_eq!(app.pointer, None, "the pointer is outside the window");
}

#[test]
fn linux_sliver_axis_per_edge() {
    let monitor = Size::new(1440.0, 900.0);
    let sliver = App::docked_thickness(&Settings::default(), false, true);
    assert_eq!(sliver, EDGE_SLIVER);
    let top = dock_frame(Edge::Top, monitor, 0.0, false, sliver, 0.9);
    assert_eq!((top.y, top.width, top.height), (0.0, 1296.0, 2.0));
    let left = dock_frame(Edge::Left, monitor, 0.0, false, sliver, 0.9);
    assert_eq!((left.x, left.width, left.height), (0.0, 2.0, 810.0));

    // The Top sliver's edge zone is its 2 px height, anywhere along it.
    let dir = tempfile::tempdir().unwrap();
    let mut app = on_edge(auto_hide_app(&dir), Edge::Top);
    hide_strip(&mut app, Instant::now());
    assert_eq!(app.window_thickness(false), EDGE_SLIVER);
    app.window_size = top.size();
    assert!(app.is_sliver());
    let mid = band_middle(&app);
    assert!(app.in_edge(Point::new(mid, 1.0)));
    assert!(!app.in_edge(Point::new(mid, 3.0)), "below the sliver");
    assert!(app.in_edge(Point::new(1.0, 1.0)), "outside the band");
}

#[test]
fn edge_dwell_anywhere_reveals() {
    for edge in [Edge::Right, Edge::Left, Edge::Top] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = on_edge(auto_hide_app(&dir), edge);
        hide_strip(&mut app, Instant::now());
        let size = app.window_size;
        // Near the window's corner, far outside the strip's band.
        let corner = match edge {
            Edge::Right => Point::new(size.width - 1.0, 5.0),
            Edge::Left => Point::new(1.0, size.height - 5.0),
            Edge::Top => Point::new(size.width - 5.0, 1.0),
        };
        assert!(!app.in_band(app.along(corner)), "{edge:?}: inside the band");
        assert!(app.in_edge(corner), "{edge:?}");
        assert!(app.is_interactive(corner), "{edge:?}");
        // Still only the outermost 2 px.
        let inside = match edge {
            Edge::Right => Point::new(size.width - 3.0, 5.0),
            Edge::Left => Point::new(3.0, size.height - 5.0),
            Edge::Top => Point::new(size.width - 5.0, 3.0),
        };
        assert!(!app.in_edge(inside), "{edge:?}: too deep");
        let entered = Instant::now();
        let _ = app.update(Message::CursorMoved(corner));
        assert!(
            app.auto_hide.next_deadline().is_some(),
            "{edge:?}: no dwell"
        );
        let _ = app.update(Message::Tick(
            entered + REVEAL_DWELL + Duration::from_millis(5),
        ));
        assert_eq!(
            app.auto_hide.phase(),
            crate::autohide::Phase::Revealing,
            "{edge:?}"
        );
    }
    // On Top the menu bar counts anywhere along it.
    let dir = tempfile::tempdir().unwrap();
    let mut app = on_edge(auto_hide_app(&dir), Edge::Top);
    hide_strip(&mut app, Instant::now());
    assert!(app.in_edge(Point::new(5.0, -10.0)));
}

#[test]
fn linux_sliver_spans_full_edge() {
    let monitor = Size::new(1440.0, 900.0);
    let dir = tempfile::tempdir().unwrap();
    let mut app = auto_hide_app(&dir);
    let fraction = app.settings.settings().window.height_fraction;
    assert!(fraction < 1.0);
    assert_eq!(
        app.dock_fraction(false),
        fraction,
        "shown: the strip's length"
    );
    hide_strip(&mut app, Instant::now());
    assert_eq!(app.dock_fraction(false), 1.0, "the sliver spans the edge");
    assert_eq!(
        app.dock_fraction(true),
        fraction,
        "passthrough is unaffected"
    );
    for edge in [Edge::Right, Edge::Left, Edge::Top] {
        let sliver = dock_frame(edge, monitor, 0.0, false, EDGE_SLIVER, 1.0);
        assert_eq!(edge.edge_length(sliver.size()), edge.edge_length(monitor));
        assert_eq!(edge.thickness(sliver.size()), EDGE_SLIVER);
    }

    // Anywhere inside the full-length sliver is the edge zone.
    let sliver = dock_frame(Edge::Right, monitor, 0.0, false, EDGE_SLIVER, 1.0);
    app.window_size = sliver.size();
    assert!(app.is_sliver());
    assert!(app.in_edge(Point::new(1.0, 3.0)));
    assert!(app.in_edge(Point::new(1.0, 897.0)));

    // Revealing shortens the window along the edge: the pointer keeps its
    // screen spot, or is gone once outside.
    let shown = dock_frame(Edge::Right, monitor, 0.0, false, STRIP_WIDTH, fraction);
    assert!(shown.y > 0.0);
    let at = |y| Some(Point::new(1.0, y));
    assert_eq!(
        pointer_after_dock(at(3.0), Edge::Right, sliver, shown),
        None
    );
    assert_eq!(
        pointer_after_dock(at(450.0), Edge::Right, sliver, shown),
        Some(Point::new(STRIP_WIDTH - 1.0, 450.0 - shown.y))
    );
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
    assert_eq!(
        rect.y,
        app.panel_away(),
        "below the strip and its Actions row"
    );
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
        app.window_thickness(SUPPORTS_PASSTHROUGH),
        app.settings.settings().window.height_fraction,
    );
    assert_eq!(app.window_size, expected.size(), "not re-docked");
    assert_eq!(app.active_note, Some(open));
    assert!(app.morph.is_opening(), "the note closed");
    assert!(app.peek.is_closed() && app.peek_note.is_none(), "peek kept");
    assert!(app.drag.is_none(), "drag kept");
    let bar = app.strip_layout().bars[app.id_entry(open).unwrap()];
    assert_eq!(
        app.anchor_along,
        bar.center_x(),
        "the note didn't re-anchor"
    );

    app.settings.save().unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap())
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
        let clear = crate::bar_strip::actions_row_extent(
            &app.settings.settings().bars,
            app.entries().len(),
            |_| 1.0,
        )
        .max(STRIP_WIDTH)
            + NOTE_GAP;
        let away = |rect: Rectangle| match edge {
            Edge::Left => rect.x + 0.1 >= clear,
            Edge::Top => rect.y + 0.1 >= clear,
            Edge::Right => true,
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
