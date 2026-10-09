use crate::animation::ease_out_cubic;
use crate::app::Message;
use crate::color_bubble::color_bubble;
use crate::color_picker::color_picker;
use crate::command_passthrough::command_passthrough;
use crate::icons::{icon, Icon};
use crate::note::{Note, NoteColor};
use crate::pass_wheel::pass_wheel;
use crate::press_shift::press_shift;
use crate::press_through::press_through;
use crate::reminder;
use crate::reminder_picker::{self, ReminderDraft};
use crate::rich::Doc;
use crate::rich_highlight;
use crate::rich_view;
use crate::theme::{self, space, RADIUS_CONTROL, RADIUS_SURFACE, TEXT_MD, TEXT_SM};
use crate::toolbar::{slide_padding, toolbar};

use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, Scrollable};
use iced::advanced::widget::{Id, Operation};
use iced::Rectangle;
use std::collections::HashSet;
use std::path::Path;

use iced::widget::{
    button, column, container, mouse_area, responsive, row, scrollable, stack, text, text_editor,
    text_input, tooltip, Space,
};
use iced::{border, gradient, Border, Color, Element, Fill, Length, Padding, Shadow, Size};
use iced::{keyboard, mouse, Theme, Vector};

/// Shown in an empty note, in the editor and in the formatted view.
pub(crate) const PLACEHOLDER: &str = "…";

const BODY_SCROLL_ID: &str = "note-body-scroll";
const BODY_EDITOR_ID: &str = "note-body-editor";
const TITLE_ID: &str = "note-title";
/// Space kept below the last line when following the caret (part of the
/// editor's bottom padding).
const CARET_MARGIN: f32 = 12.0;

/// Puts the keyboard focus into the body editor.
pub(crate) fn focus_body() -> iced::Task<Message> {
    iced::widget::operation::focus(BODY_EDITOR_ID)
}

/// Scrolls the note body just enough to show its last line, and only if that
/// line has gone below the visible area. Runs against the real layout, so it
/// sees the text height after the edit that triggered it.
pub fn reveal_last_line() -> iced::Task<Message> {
    iced::advanced::widget::operate(RevealLastLine(Id::new(BODY_SCROLL_ID)))
}

struct RevealLastLine(Id);

impl<T> Operation<T> for RevealLastLine {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        if id != Some(&self.0) {
            return;
        }
        let caret_bottom = content_bounds.height - CARET_MARGIN;
        let visible_bottom = translation.y + bounds.height;
        if caret_bottom > visible_bottom {
            state.scroll_to(AbsoluteOffset {
                x: None,
                y: Some(caret_bottom - bounds.height),
            });
        }
    }
}
/// The body editor's vertical padding; its layout adds this on top of its
/// minimum height.
const EDITOR_PADDING_TOP: f32 = 6.0;
const EDITOR_PADDING_BOTTOM: f32 = 14.0;
const EDITOR_PADDING_Y: f32 = EDITOR_PADDING_TOP + EDITOR_PADDING_BOTTOM;
/// Gap between the note's edges and the body's scrollable, so the editor's
/// focus ring stays clear of the rounded corners. The body padding makes up
/// for it: the text sits 18 px in from the edges.
const BODY_INSET: f32 = 8.0;
const BODY_INSET_BOTTOM: f32 = 4.0;
/// Height of the grip along the top edge, under the adhesive band.
const GRIP_HEIGHT: f32 = 16.0;

pub struct PostIt<'a> {
    pub note: &'a Note,
    pub theme: theme::Theme,
    pub palette: &'a [NoteColor],
    /// How much lighter (negative) or darker than its bar the paper is.
    pub paper_tint: f32,
    /// Header controls stay this faint until the note is hovered.
    pub idle_control_alpha: f32,
    pub content: &'a text_editor::Content,
    /// The body shows the editor; otherwise it shows `doc` rendered.
    pub editing: bool,
    pub doc: &'a Doc,
    /// Folder the note's `images/` references resolve against.
    pub data_dir: &'a Path,
    /// Image references in `doc` that can't be shown.
    pub broken_images: &'a HashSet<String>,
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
    /// The note was just copied; the copy button shows a check.
    pub copied: bool,
    /// The note (its stack's top) is pinned; the pin button draws at full ink.
    pub pinned: bool,
    pub color_picker_open: bool,
    /// The color bubble's hex field, as typed.
    pub color_hex: &'a str,
    /// The toolbar's text color grid is open.
    pub text_color_picker_open: bool,
    pub hovered: bool,
    pub dragging: bool,
    /// Progress (0..=1) of the fade after switching edit/render mode.
    pub mode_fade: f32,
    /// What the title says about a reminder, for the header label.
    pub reminder: reminder::Status,
    /// Open reminder calendar draft, if any.
    pub reminder_draft: Option<&'a ReminderDraft>,
}

/// A button whose background darkens while pressed and whose label sinks
/// 1 px. `padding` goes around the label, inside the press area.
pub(crate) fn pressable<'a>(
    label: impl Into<Element<'a, Message>>,
    padding: Padding,
    message: Message,
) -> button::Button<'a, Message> {
    button(press_shift(container(label).padding(padding)))
        .padding(0)
        .on_press(message)
}

/// The header button's look: a faint ink wash when hovered, a darker one
/// when pressed, and a persistent wash at full ink while `active`.
fn header_button_style(
    theme: theme::Theme,
    alpha: f32,
    active: bool,
    status: button::Status,
) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered => Some(theme.ink(0.12 * alpha).into()),
            button::Status::Pressed => Some(theme.ink(0.22 * alpha).into()),
            _ if active => Some(theme.ink(0.10 * alpha).into()),
            _ => None,
        },
        text_color: theme.ink(if active { alpha } else { 0.65 * alpha }),
        border: border::rounded(RADIUS_CONTROL),
        ..Default::default()
    }
}

/// A header control: a Lucide icon on a faint ink wash when hovered, a
/// darker one when pressed. `active` keeps the wash on, for a toggle that is on.
fn header_button<'a>(
    glyph: Icon,
    message: Message,
    active: bool,
    theme: theme::Theme,
    alpha: f32,
) -> Element<'a, Message> {
    pressable(
        icon(glyph, TEXT_SM),
        Padding::new(space(1)).left(space(2)).right(space(2)),
        message,
    )
    .style(move |_theme: &Theme, status| header_button_style(theme, alpha, active, status))
    .into()
}

/// The header's pin toggle: washed and at full ink while pinned.
fn pin_button<'a>(pinned: bool, theme: theme::Theme, alpha: f32) -> Element<'a, Message> {
    let tip = if pinned { "Unpin" } else { "Pin to top" };
    tooltip(
        header_button(Icon::Pin, Message::TogglePin, pinned, theme, alpha),
        tip_label(tip, theme),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// A tooltip's card.
fn tip_label<'a>(tip: impl text::IntoFragment<'a>, theme: theme::Theme) -> Element<'a, Message> {
    container(text(tip).size(TEXT_SM).font(theme::BODY_FONT))
        .padding(Padding::new(space(1)).left(space(2)).right(space(2)))
        .style(move |_theme: &Theme| container::Style {
            background: Some(theme.card().into()),
            text_color: Some(theme.ink(1.0)),
            border: border::rounded(RADIUS_CONTROL),
            shadow: theme.shadows(1.0)[0],
            ..Default::default()
        })
        .into()
}

/// The title field's placeholder.
const TITLE_PLACEHOLDER: &str = "Title \u{2014} @15:00 adds a reminder";

/// Notes narrower than this show the reminder as a bell alone.
const REMINDER_LABEL_MIN_WIDTH: f32 = 420.0;

/// What the header shows for a reminder status.
#[derive(Debug, Clone, PartialEq)]
struct ReminderLabel {
    bell: bool,
    text: Option<String>,
    ink: f32,
    /// Shown on hover when the text is left out.
    tooltip: Option<String>,
}

/// The header's reminder label for `status` in a note `width` wide; `None`
/// shows nothing. A fired reminder is dimmed. A narrow note keeps just the
/// bell, with the text as its tooltip.
/// What the title field shows: the title without its reminder tag, which
/// only appears as the header's rendered date and time.
pub fn title_field_text(note: &Note) -> String {
    reminder::visible_title(&note.title)
}

fn reminder_label(status: &reminder::Status, width: f32) -> Option<ReminderLabel> {
    let (text, ink, bell) = match status {
        reminder::Status::None => return None,
        reminder::Status::Pending(label) => (label.clone(), 0.65, true),
        reminder::Status::Fired(label) => (label.clone(), 0.45, true),
        reminder::Status::Invalid => ("not a reminder".to_string(), 0.45, false),
    };
    Some(if width < REMINDER_LABEL_MIN_WIDTH {
        ReminderLabel {
            bell: true,
            text: None,
            ink,
            tooltip: Some(text),
        }
    } else {
        ReminderLabel {
            bell,
            text: Some(text),
            ink,
            tooltip: None,
        }
    })
}

/// Padding around the body text, the same in edit and rendered mode so the
/// text doesn't jump when switching. With `BODY_INSET` it puts the text
/// 18 px in from the note's edges.
fn body_padding() -> Padding {
    Padding::new(EDITOR_PADDING_TOP)
        .left(18.0 - BODY_INSET)
        .right(18.0 - BODY_INSET)
        .bottom(EDITOR_PADDING_BOTTOM)
}

/// Width of the body's scrollbar: slim while idle, wider while the pointer
/// is over the note.
fn scroller_width(active: bool) -> f32 {
    if active {
        6.0
    } else {
        3.0
    }
}

/// The note body's scrollable, with a slim inked scrollbar.
fn body_scrollable<'a>(
    content: impl Into<Element<'a, Message>>,
    theme: theme::Theme,
    hovered: bool,
    a: f32,
) -> Element<'a, Message> {
    let width = scroller_width(hovered);
    scrollable(content)
        .id(BODY_SCROLL_ID)
        .height(Fill)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(width)
                .scroller_width(width)
                .margin(1),
        ))
        .style(move |_theme: &Theme, status| {
            let active = matches!(
                status,
                scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
            );
            let rail = scrollable::Rail {
                background: Some(theme.ink(0.06 * a).into()),
                border: border::rounded(width / 2.0),
                scroller: scrollable::Scroller {
                    background: Color {
                        a: theme.scrollbar(active).a * a,
                        ..theme.scrollbar(active)
                    }
                    .into(),
                    border: border::rounded(width / 2.0),
                },
            };
            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: None,
                auto_scroll: scrollable::AutoScroll {
                    background: theme.ink(0.1 * a).into(),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    icon: theme.ink(a),
                },
            }
        })
        .into()
}

/// Holds the body's scrollable `BODY_INSET` in from the note's edges.
fn body_inset<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding(
            Padding::ZERO
                .left(BODY_INSET)
                .right(BODY_INSET)
                .bottom(BODY_INSET_BOTTOM),
        )
        .into()
}

/// Minimum editor height that makes it, padding included, exactly fill a
/// body `available` px tall: short text then needs no scrollbar.
fn editor_min_height(available: f32) -> f32 {
    (available - EDITOR_PADDING_Y).max(0.0)
}

/// The paper while the note grows out of its bar: the bar's color at
/// `progress` 0, so the morph has no seam where it meets the bar, easing
/// into the theme's paper at 1.
pub(crate) fn morph_paper(
    theme: &theme::Theme,
    color: NoteColor,
    tint: f32,
    progress: f32,
) -> Color {
    let [r, g, b, _] = color.rgba;
    let bar = Color::from_rgb(r, g, b);
    theme::mix(bar, theme.paper(color, tint), ease_out_cubic(progress))
}

pub fn post_it(p: PostIt<'_>) -> Element<'_, Message> {
    let PostIt {
        note,
        theme,
        palette,
        paper_tint,
        idle_control_alpha,
        content,
        editing,
        doc,
        data_dir,
        broken_images,
        size,
        morph_progress,
        content_alpha: a,
        copied,
        pinned,
        color_picker_open,
        color_hex,
        text_color_picker_open,
        hovered,
        dragging,
        mode_fade,
        reminder,
        reminder_draft,
    } = p;
    let mode_eased = ease_out_cubic(mode_fade);
    // The body that just appeared fades in after a mode switch.
    let body_a = a * mode_eased;
    // Space the sliding toolbar takes from the top of the editor below it,
    // so the text stays put.
    let borrowed = if editing {
        slide_padding(mode_eased).borrowed()
    } else {
        0.0
    };
    let controls = if hovered || dragging || color_picker_open {
        a
    } else {
        a * idle_control_alpha
    };

    let paper = morph_paper(&theme, note.color, paper_tint, morph_progress);
    let [contact, ambient] = theme.shadows(morph_progress);

    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let title = text_input(TITLE_PLACEHOLDER, &title_field_text(note))
            .id(TITLE_ID)
            .on_input(Message::TitleEdited)
            .size(TEXT_MD)
            .padding(Padding::new(2.0).left(4).right(4))
            .font(theme::TITLE_FONT)
            .style(move |_theme: &Theme, _status| text_input::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                icon: theme.ink(a),
                placeholder: theme.ink(0.35 * a),
                value: theme.ink(a),
                selection: theme.ink(0.18 * a),
            });

        let swatch_color = {
            let [r, g, b, _] = note.color.rgba;
            Color::from_rgba(r, g, b, controls)
        };
        let swatch = container(Space::new().width(12).height(12)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(swatch_color.into()),
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: theme.ink(0.35 * controls),
                },
                ..Default::default()
            }
        });
        let color_btn = pressable(swatch, Padding::new(5.0), Message::ToggleColorPicker).style(
            move |_theme: &Theme, status| button::Style {
                background: matches!(status, button::Status::Pressed)
                    .then(|| theme.ink(0.12 * controls).into()),
                border: border::rounded(RADIUS_CONTROL),
                ..Default::default()
            },
        );
        // The color bubble opens above the button on its own layer, so the
        // header and body stay put. Not while the note unfolds: an overlay
        // doesn't fade or move with it.
        let color_btn = color_bubble(
            color_btn,
            (color_picker_open && morph_progress >= 1.0)
                .then(|| color_picker(note.color, palette, color_hex, theme)),
            theme,
        );

        // The reminder's state right of the title: bell and time, or why
        // the tag isn't one. Its width hugs the text so the title keeps the row.
        // Pending/Fired labels open the calendar picker; while not editing the
        // bubble anchors here (toolbar hosts it in edit mode).
        let can_clear_reminder = matches!(
            reminder,
            reminder::Status::Pending(_) | reminder::Status::Fired(_)
        );
        let reminder_tag = reminder_label(&reminder, size.width).map(|l| {
            let ink = theme.ink(l.ink * a);
            let tag = row![
                l.bell.then(|| icon(Icon::Bell, TEXT_SM).color(ink)),
                l.text.map(|label| text(label)
                    .size(TEXT_SM)
                    .font(theme::BODY_FONT)
                    .color(ink)
                    .wrapping(text::Wrapping::None))
            ]
            .spacing(space(1))
            .align_y(iced::Alignment::Center)
            .width(Length::Shrink);
            let tag: Element<'_, Message> = match l.tooltip {
                Some(tip) => tooltip(tag, tip_label(tip, theme), tooltip::Position::Bottom)
                    .gap(4)
                    .into(),
                None => tag.into(),
            };
            let clickable = can_clear_reminder;
            let tag = if clickable {
                pressable(tag, Padding::new(2.0), Message::OpenReminderPicker)
                    .style(move |_theme: &Theme, status| button::Style {
                        background: match status {
                            button::Status::Hovered => Some(theme.ink(0.1 * a).into()),
                            button::Status::Pressed => Some(theme.ink(0.18 * a).into()),
                            _ => None,
                        },
                        border: border::rounded(RADIUS_CONTROL),
                        ..Default::default()
                    })
                    .into()
            } else {
                tag
            };
            if clickable && !editing {
                color_bubble(
                    tag,
                    reminder_draft
                        .map(|draft| reminder_picker::view(draft, can_clear_reminder, theme)),
                    theme,
                )
                .into()
            } else {
                tag
            }
        });

        // Any press on the header, title and buttons included, leaves edit
        // mode; the press still reaches them.
        let header = press_through(
            container(
                row![
                    // Cmd shortcuts reach the app instead of typing.
                    command_passthrough(title),
                    reminder_tag,
                    color_btn,
                    pin_button(pinned, theme, controls),
                    header_button(
                        if copied { Icon::Check } else { Icon::Copy },
                        Message::CopyNote,
                        false,
                        theme,
                        controls
                    ),
                    header_button(
                        Icon::Trash,
                        Message::DeleteNote(note.id),
                        false,
                        theme,
                        controls
                    ),
                    header_button(Icon::Close, Message::ClosePanel, false, theme, controls),
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .padding(Padding::new(2.0).left(14).right(10).bottom(6)),
            Message::EditorBlurred,
        );

        // Grip along the top edge: drag to move the note, double-click to
        // send it back next to the dock. It sits on the adhesive band, under
        // the paper's 1 px top highlight (kept clear of the rounded corners).
        let pill = container(Space::new().width(36).height(4)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(theme.ink(0.3 * controls).into()),
                border: border::rounded(2),
                ..Default::default()
            }
        });
        let highlight = container(container(Space::new().width(Fill).height(1)).style(
            move |_theme: &Theme| container::Style {
                background: Some(theme.highlight().into()),
                ..Default::default()
            },
        ))
        .padding(Padding::ZERO.left(RADIUS_SURFACE).right(RADIUS_SURFACE));
        let grip = mouse_area(
            container(column![
                highlight,
                container(pill)
                    .width(Fill)
                    .height(Fill)
                    .align_x(iced::Alignment::Center)
                    .align_y(iced::Alignment::Center),
            ])
            .width(Fill)
            .height(GRIP_HEIGHT)
            .style(move |_theme: &Theme| container::Style {
                background: Some(theme.band().into()),
                border: border::rounded(border::top(RADIUS_SURFACE)),
                ..Default::default()
            }),
        )
        .on_press(Message::NoteDragStart)
        .on_double_click(Message::NoteResetPosition)
        .interaction(if dragging {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::Grab
        });

        // The editor grows with its text inside a scrollable, so a scrollbar
        // appears only once the text no longer fits. Its minimum height
        // fills exactly the space the body gets (measured, not estimated),
        // so clicks below short text still land in the editor.
        // The editor styles the Markdown it shows; see `rich_highlight`.
        let highlight_settings = rich_highlight::HighlightSettings {
            palette: palette.to_vec(),
            text: if editing {
                content.text()
            } else {
                String::new()
            },
            mode: theme.mode,
        };
        let body: Element<'_, Message> = if editing {
            body_inset(responsive(move |available| {
                let editor = text_editor(content)
                    .id(BODY_EDITOR_ID)
                    .highlight_with::<rich_highlight::Highlighter>(
                        highlight_settings.clone(),
                        rich_highlight::format,
                    )
                    .placeholder(PLACEHOLDER)
                    .on_action(Message::NoteEdited)
                    .key_binding(body_key_binding)
                    .min_height(editor_min_height(available.height) + borrowed)
                    .size(TEXT_SM)
                    .line_height(text::LineHeight::Relative(theme::BODY_LINE_HEIGHT))
                    .padding(body_padding().top(EDITOR_PADDING_TOP - borrowed))
                    .style(move |_theme: &Theme, _status| text_editor::Style {
                        background: Color::TRANSPARENT.into(),
                        border: Border::default(),
                        placeholder: theme.ink(0.35 * body_a),
                        value: theme.ink(0.9 * body_a),
                        selection: theme.ink(0.18 * body_a),
                    });
                body_scrollable(pass_wheel(editor), theme, hovered, body_a)
            }))
        } else {
            // Content inside a scrollable can't fill its height, so the
            // click target for the space below the text sits behind it.
            let rendered = container(rich_view::view(doc, data_dir, broken_images, theme, body_a))
                .padding(body_padding());
            stack![
                mouse_area(Space::new().width(Fill).height(Fill))
                    .on_press(Message::BodyClicked(None)),
                body_inset(body_scrollable(rendered, theme, hovered, body_a)),
            ]
            .width(Fill)
            .height(Fill)
            .into()
        };

        // Divider: a faint inked hairline with a short soft shadow fading
        // below it, as if the header sheet rests slightly on the body.
        let hairline =
            container(Space::new().width(Fill).height(1)).style(move |_theme: &Theme| {
                container::Style {
                    background: Some(theme.ink(0.12 * a).into()),
                    ..Default::default()
                }
            });
        let fade = container(Space::new().width(Fill).height(6)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(
                    gradient::Linear::new(std::f32::consts::PI)
                        .add_stop(0.0, theme.ink(0.07 * a))
                        .add_stop(1.0, theme.ink(0.0))
                        .into(),
                ),
                ..Default::default()
            }
        });
        let divider = container(column![hairline, fade]).padding(Padding::ZERO.left(14).right(14));

        let mut col = column![grip, header, divider];
        if editing {
            col = col.push(toolbar(
                palette,
                text_color_picker_open,
                controls,
                mode_eased,
                reminder_draft,
                can_clear_reminder,
                theme,
            ));
        }
        col = col.push(body);

        col.into()
    };

    // Gradient quads draw no shadow, so each shadow sits on its own solid
    // paper layer under the gradient one: ambient outside, contact inside.
    let sheet = container(inner)
        .width(Length::Fixed(size.width))
        .height(Length::Fixed(size.height))
        .clip(true)
        .style(move |_theme: &Theme| container::Style {
            background: Some(theme.paper_gradient(paper, 1.0).into()),
            border: border::rounded(RADIUS_SURFACE),
            ..Default::default()
        });
    paper_layer(paper_layer(sheet, paper, contact), paper, ambient).into()
}

/// A solid paper quad under `content` that casts `shadow`.
fn paper_layer<'a>(
    content: impl Into<Element<'a, Message>>,
    paper: Color,
    shadow: Shadow,
) -> container::Container<'a, Message> {
    container(content).style(move |_theme: &Theme| container::Style {
        background: Some(paper.into()),
        border: border::rounded(RADIUS_SURFACE),
        shadow,
        ..Default::default()
    })
}

/// Whether the note's title field has keyboard focus.
pub(crate) fn title_focused() -> iced::Task<bool> {
    iced::widget::operation::is_focused(TITLE_ID)
}

/// `Undo` for Cmd/Ctrl+Z, `Redo` for Cmd/Ctrl+Shift+Z (and Ctrl+Y off
/// macOS), given a key's shortcut letter (see
/// [`crate::command_passthrough::shortcut_char`]) and the held modifiers.
pub(crate) fn history_key(c: char, modifiers: keyboard::Modifiers) -> Option<Message> {
    if !modifiers.command() {
        return None;
    }
    match c {
        'z' if modifiers.shift() => Some(Message::Redo),
        'z' => Some(Message::Undo),
        'y' if cfg!(not(target_os = "macos")) => Some(Message::Redo),
        _ => None,
    }
}

/// The body editor's key bindings while it has focus: Cmd/Ctrl+V asks the
/// app to paste (text or image), and the undo/redo keys step its history;
/// other command shortcuts (Cmd+F, Cmd+N, Cmd+,) pass through to the app
/// instead of typing their letter, unless Alt is held too off macOS;
/// everything else is iced's default.
fn body_key_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return text_editor::Binding::from_key_press(press);
    }
    // Found by the key's Latin letter, so they work on any layout.
    let shortcut = crate::command_passthrough::shortcut_char(&press.key, press.physical_key);
    if press.modifiers.command() && shortcut == Some('v') {
        return Some(text_editor::Binding::Custom(Message::PasteRequested));
    }
    if let Some(step) = shortcut.and_then(|c| history_key(c, press.modifiers)) {
        return Some(text_editor::Binding::Custom(step));
    }
    // Off macOS, Ctrl+Alt is AltGr on Windows layouts and types characters
    // such as `{` and `@`; macOS has no AltGr, so Cmd+Opt+letter stays a
    // shortcut there.
    let command =
        press.modifiers.command() && (cfg!(target_os = "macos") || !press.modifiers.alt());
    // iced binds only c/x/v/a under command; any other letter would be
    // inserted (macOS reports it as text).
    match text_editor::Binding::from_key_press(press) {
        Some(text_editor::Binding::Insert(_)) if command => None,
        binding => binding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(ch: &str, command: bool, status: text_editor::Status) -> text_editor::KeyPress {
        let key = keyboard::Key::Character(ch.into());
        text_editor::KeyPress {
            modified_key: key.clone(),
            key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            modifiers: if command {
                keyboard::Modifiers::COMMAND
            } else {
                keyboard::Modifiers::empty()
            },
            text: Some(ch.into()),
            status,
        }
    }

    #[test]
    fn command_shortcuts_pass_through_the_editor() {
        let focused = text_editor::Status::Focused { is_hovered: false };
        for key in ["f", "n", ","] {
            assert!(
                body_key_binding(press(key, true, focused)).is_none(),
                "{key}"
            );
        }
        assert!(matches!(
            body_key_binding(press("c", true, focused)),
            Some(text_editor::Binding::Copy)
        ));
        assert!(matches!(
            body_key_binding(press("f", false, focused)),
            Some(text_editor::Binding::Insert('f'))
        ));
    }

    fn press_on(
        ch: &str,
        code: keyboard::key::Code,
        modifiers: keyboard::Modifiers,
    ) -> text_editor::KeyPress {
        let key = keyboard::Key::Character(ch.into());
        text_editor::KeyPress {
            modified_key: key.clone(),
            key,
            physical_key: keyboard::key::Physical::Code(code),
            modifiers,
            text: Some(ch.into()),
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    #[test]
    fn non_latin_layout_keeps_editor_shortcuts() {
        use keyboard::key::Code;
        let cmd = keyboard::Modifiers::COMMAND;
        // Russian layout: V types "м", Z types "я".
        assert!(matches!(
            body_key_binding(press_on("м", Code::KeyV, cmd)),
            Some(text_editor::Binding::Custom(Message::PasteRequested))
        ));
        assert!(matches!(
            body_key_binding(press_on("я", Code::KeyZ, cmd)),
            Some(text_editor::Binding::Custom(Message::Undo))
        ));
        assert!(matches!(
            body_key_binding(press_on("Я", Code::KeyZ, cmd | keyboard::Modifiers::SHIFT)),
            Some(text_editor::Binding::Custom(Message::Redo))
        ));
    }

    #[test]
    fn ctrl_alt_characters_still_type() {
        // LCtrl+LAlt is AltGr on Windows layouts, which types `{`, `}`, `@`.
        // macOS has no AltGr: Cmd+Opt+letter stays a shortcut there.
        let focused = text_editor::Status::Focused { is_hovered: false };
        let mut brace = press("{", true, focused);
        brace.modifiers |= keyboard::Modifiers::ALT;
        let binding = body_key_binding(brace);
        if cfg!(target_os = "macos") {
            assert!(binding.is_none());
        } else {
            assert!(matches!(binding, Some(text_editor::Binding::Insert('{'))));
        }
        let mut cmd_opt_f = press("ƒ", true, focused);
        cmd_opt_f.key = keyboard::Key::Character("f".into());
        cmd_opt_f.modifiers |= keyboard::Modifiers::ALT;
        let binding = body_key_binding(cmd_opt_f);
        if cfg!(target_os = "macos") {
            assert!(binding.is_none(), "Cmd+Opt+F must not insert ƒ");
        } else {
            assert!(matches!(binding, Some(text_editor::Binding::Insert('ƒ'))));
        }
    }

    #[test]
    fn undo_and_redo_bindings_when_focused() {
        let focused = text_editor::Status::Focused { is_hovered: false };
        assert!(matches!(
            body_key_binding(press("z", true, focused)),
            Some(text_editor::Binding::Custom(Message::Undo))
        ));
        let mut shift_z = press("Z", true, focused);
        shift_z.modifiers |= keyboard::Modifiers::SHIFT;
        assert!(matches!(
            body_key_binding(shift_z),
            Some(text_editor::Binding::Custom(Message::Redo))
        ));
        assert!(body_key_binding(press("z", true, text_editor::Status::Active)).is_none());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ctrl_y_redoes_off_macos() {
        let focused = text_editor::Status::Focused { is_hovered: false };
        assert!(matches!(
            body_key_binding(press("y", true, focused)),
            Some(text_editor::Binding::Custom(Message::Redo))
        ));
    }

    #[test]
    fn paste_binding_only_when_focused() {
        let focused = text_editor::Status::Focused { is_hovered: false };
        assert!(matches!(
            body_key_binding(press("v", true, focused)),
            Some(text_editor::Binding::Custom(Message::PasteRequested))
        ));
        assert!(body_key_binding(press("v", true, text_editor::Status::Active)).is_none());
    }

    #[test]
    fn other_keys_use_default_bindings() {
        let focused = text_editor::Status::Focused { is_hovered: false };
        assert!(matches!(
            body_key_binding(press("x", false, focused)),
            Some(text_editor::Binding::Insert('x'))
        ));
    }

    use iced::advanced::widget::operation::scrollable::RelativeOffset;
    use iced::{Point, Size};

    #[test]
    fn empty_placeholder_text() {
        assert_eq!(PLACEHOLDER, "…");
    }

    #[test]
    fn paper_starts_as_the_bar() {
        let close = |a: Color, b: Color| {
            [a.r - b.r, a.g - b.g, a.b - b.b, a.a - b.a]
                .iter()
                .all(|d| d.abs() < 1e-5)
        };
        for mode in [theme::Mode::Light, theme::Mode::Dark] {
            let theme = theme::Theme::new(mode);
            for color in crate::note::PALETTE {
                let [r, g, b, _] = color.rgba;
                let bar = Color::from_rgb(r, g, b);
                assert!(close(morph_paper(&theme, color, 0.2, 0.0), bar));
                assert!(close(
                    morph_paper(&theme, color, 0.2, 1.0),
                    theme.paper(color, 0.2)
                ));
            }
        }
    }

    #[test]
    fn scrollbar_widths() {
        assert_eq!(scroller_width(true), 6.0);
        assert_eq!(scroller_width(false), 3.0);
    }

    #[test]
    fn editor_fills_body_without_overflow() {
        for available in [0.0, 10.0, 264.8, 600.0] {
            let total = editor_min_height(available) + EDITOR_PADDING_Y;
            assert!(total <= available.max(EDITOR_PADDING_Y), "{available}");
        }
        assert_eq!(editor_min_height(264.8) + EDITOR_PADDING_Y, 264.8);
    }

    /// Lays out the open note at `top` in an 800 × 900 window, with the
    /// color bubble open or not. Returns every node's bounds and the
    /// bubble's card, if any shows.
    fn note_layout(top: f32, bubble_open: bool) -> (Vec<Rectangle>, Option<Rectangle>) {
        note_layout_at(top, bubble_open, 1.0)
    }

    /// `note_layout` with the note unfolded `morph_progress` of the way.
    fn note_layout_at(
        top: f32,
        bubble_open: bool,
        morph_progress: f32,
    ) -> (Vec<Rectangle>, Option<Rectangle>) {
        use iced::advanced::layout::{self, Layout};
        use iced::advanced::widget::Tree;

        let window = Size::new(800.0, 900.0);
        let renderer = iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(TEXT_SM),
        ));
        let note = Note::new(crate::note::PALETTE[0]);
        let content = text_editor::Content::with_text("hello");
        let doc = crate::rich::parse("hello", &crate::note::DEFAULT_PALETTE);
        let broken = HashSet::new();
        let view = post_it(PostIt {
            note: &note,
            theme: theme::Theme::default(),
            palette: &crate::note::DEFAULT_PALETTE,
            paper_tint: 0.0,
            idle_control_alpha: 0.3,
            content: &content,
            editing: false,
            doc: &doc,
            data_dir: Path::new("."),
            broken_images: &broken,
            size: Size::new(420.0, 320.0),
            morph_progress,
            content_alpha: 1.0,
            copied: false,
            pinned: false,
            color_picker_open: bubble_open,
            color_hex: "#FF6B6B",
            text_color_picker_open: false,
            hovered: true,
            dragging: false,
            mode_fade: 1.0,
            reminder: reminder::status(&note),
            reminder_draft: None,
        });
        let mut root: Element<'_, Message> =
            iced::widget::column![Space::new().height(top), view].into();
        let mut tree = Tree::new(&root);
        let node = root.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, window),
        );
        fn collect(layout: Layout<'_>, out: &mut Vec<Rectangle>) {
            out.push(layout.bounds());
            for child in layout.children() {
                collect(child, out);
            }
        }
        let mut rects = Vec::new();
        collect(Layout::new(&node), &mut rects);
        let card = root
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &Rectangle::with_size(window),
                Vector::ZERO,
            )
            .map(|mut overlay| {
                // Overlay groups span the window; the card is inside.
                let node = overlay.as_overlay_mut().layout(&renderer, window);
                let mut layout = Layout::new(&node);
                while layout.bounds().size() == window {
                    layout = layout.children().next().expect("a card");
                }
                layout.bounds()
            });
        (rects, card)
    }

    #[test]
    fn bubble_leaves_the_note_layout_alone() {
        let (closed, no_card) = note_layout(400.0, false);
        let (open, card) = note_layout(400.0, true);
        assert_eq!(no_card, None);
        assert_eq!(open, closed, "header and body must not move");
        let card = card.expect("open bubble shows a card");
        // The color button: 12 px swatch + 5 px padding each side.
        let button = *closed
            .iter()
            .find(|r| r.width == 22.0 && r.height == 22.0)
            .expect("color button");
        let tail = crate::color_bubble::TAIL + crate::color_bubble::GAP;
        assert!(
            (card.y + card.height + tail - button.y).abs() < 1e-3,
            "{card:?}"
        );
        assert!((card.center_x() - button.center_x()).abs() < 1e-3);
    }

    #[test]
    fn bubble_waits_for_the_note_to_unfold() {
        assert_eq!(note_layout_at(400.0, true, 0.6).1, None);
        assert!(note_layout_at(400.0, true, 1.0).1.is_some());
    }

    #[test]
    fn bubble_flips_below_at_the_window_top() {
        let (closed, card) = note_layout(0.0, true);
        let card = card.expect("open bubble shows a card");
        let button = closed
            .iter()
            .find(|r| r.width == 22.0 && r.height == 22.0)
            .expect("color button");
        assert!(card.y > button.y + button.height, "{card:?}");
    }

    #[derive(Default)]
    struct FakeScrollable {
        scrolled_to: Option<f32>,
    }

    impl Scrollable for FakeScrollable {
        fn snap_to(&mut self, _offset: RelativeOffset<Option<f32>>) {}

        fn scroll_to(&mut self, offset: AbsoluteOffset<Option<f32>>) {
            self.scrolled_to = offset.y;
        }

        fn scroll_by(&mut self, _: AbsoluteOffset, _: Rectangle, _: Rectangle) {}
    }

    /// Runs the operation on a body `viewport` px tall showing `content` px of
    /// text, currently scrolled down by `offset`.
    fn run(viewport: f32, content: f32, offset: f32) -> Option<f32> {
        let id = Id::new(BODY_SCROLL_ID);
        let mut state = FakeScrollable::default();
        Operation::<()>::scrollable(
            &mut RevealLastLine(id.clone()),
            Some(&id),
            Rectangle::new(Point::ORIGIN, Size::new(300.0, viewport)),
            Rectangle::new(Point::ORIGIN, Size::new(300.0, content)),
            Vector::new(0.0, offset),
            &mut state,
        );
        state.scrolled_to
    }

    #[test]
    fn no_scroll_while_text_fits() {
        assert_eq!(run(250.0, 120.0, 0.0), None);
        assert_eq!(run(250.0, 250.0 + CARET_MARGIN, 0.0), None);
    }

    #[test]
    fn scrolls_just_enough_once_text_passes_bottom() {
        let to = run(250.0, 300.0, 0.0).expect("should scroll");
        assert!((to - (300.0 - CARET_MARGIN - 250.0)).abs() < 0.01);
    }

    #[test]
    fn no_scroll_when_last_line_already_visible() {
        assert_eq!(run(250.0, 300.0, 60.0), None);
    }

    #[test]
    fn ignores_other_scrollables() {
        let mut state = FakeScrollable::default();
        Operation::<()>::scrollable(
            &mut RevealLastLine(Id::new(BODY_SCROLL_ID)),
            Some(&Id::new("something-else")),
            Rectangle::new(Point::ORIGIN, Size::new(300.0, 100.0)),
            Rectangle::new(Point::ORIGIN, Size::new(300.0, 900.0)),
            Vector::ZERO,
            &mut state,
        );
        assert_eq!(state.scrolled_to, None);
    }

    #[test]
    fn title_placeholder_mentions_reminders() {
        assert_eq!(TITLE_PLACEHOLDER, "Title \u{2014} @15:00 adds a reminder");
    }

    #[test]
    fn reminder_label_text_for_each_status() {
        use reminder::Status;
        let wide = 500.0;
        assert_eq!(reminder_label(&Status::None, wide), None);
        assert_eq!(
            reminder_label(&Status::Pending("Fri 09:00".into()), wide),
            Some(ReminderLabel {
                bell: true,
                text: Some("Fri 09:00".into()),
                ink: 0.65,
                tooltip: None
            })
        );
        assert_eq!(
            reminder_label(&Status::Invalid, wide),
            Some(ReminderLabel {
                bell: false,
                text: Some("not a reminder".into()),
                ink: 0.45,
                tooltip: None
            })
        );
    }

    #[test]
    fn fired_label_is_dimmed_without_suffix() {
        let l = reminder_label(&reminder::Status::Fired("Fri 09:00".into()), 500.0).unwrap();
        assert!(l.bell);
        assert_eq!(l.text.as_deref(), Some("Fri 09:00"));
        assert_eq!(l.ink, 0.45);
    }

    #[test]
    fn reminder_label_narrow_shows_bell_only() {
        use reminder::Status;
        let narrow = 419.0;
        let p = reminder_label(&Status::Pending("Fri 09:00".into()), narrow).unwrap();
        assert!(p.bell && p.text.is_none() && p.ink == 0.65);
        assert_eq!(p.tooltip.as_deref(), Some("Fri 09:00"));
        let f = reminder_label(&Status::Fired("Fri 09:00".into()), narrow).unwrap();
        assert!(f.bell && f.text.is_none() && f.ink == 0.45);
        assert_eq!(f.tooltip.as_deref(), Some("Fri 09:00"));
        let i = reminder_label(&Status::Invalid, narrow).unwrap();
        assert!(i.bell && i.text.is_none() && i.ink == 0.45);
        assert_eq!(i.tooltip.as_deref(), Some("not a reminder"));
        let wide = reminder_label(&Status::Pending("x".into()), 420.0).unwrap();
        assert!(wide.text.is_some());
    }

    #[test]
    fn active_header_button_has_wash() {
        let theme = theme::Theme::default();
        let idle = header_button_style(theme, 1.0, false, button::Status::Active);
        assert_eq!(idle.background, None);
        assert_eq!(idle.text_color, theme.ink(0.65));
        let on = header_button_style(theme, 1.0, true, button::Status::Active);
        assert_eq!(on.background, Some(theme.ink(0.10).into()));
        assert_eq!(on.text_color, theme.ink(1.0));
    }
}
