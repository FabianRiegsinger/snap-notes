use crate::animation::ease_out_cubic;
use crate::app::Message;
use crate::color_picker::color_picker;
use crate::icons::{icon, Icon};
use crate::note::{Note, NoteColor};
use crate::pass_wheel::pass_wheel;
use crate::press_shift::press_shift;
use crate::press_through::press_through;
use crate::rich::Doc;
use crate::rich_view;
use crate::theme::{self, space, RADIUS_CONTROL, RADIUS_SURFACE, TEXT_MD, TEXT_SM};
use crate::toolbar::toolbar;

use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, Scrollable};
use iced::advanced::widget::{Id, Operation};
use iced::Rectangle;
use std::collections::HashSet;
use std::path::Path;

use iced::widget::{
    button, column, container, mouse_area, responsive, row, scrollable, stack, text, text_editor,
    text_input, Space,
};
use iced::{border, gradient, Border, Color, Element, Fill, Length, Padding, Shadow, Size};
use iced::{keyboard, mouse, Theme, Vector};

/// Shown in an empty note, in the editor and in the formatted view.
pub(crate) const PLACEHOLDER: &str = "Start typing… Markdown works.";

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
/// Width of a focus ring around the title and the editor.
const FOCUS_RING: f32 = 1.5;
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
    pub confirm_delete: bool,
    pub color_picker_open: bool,
    /// The toolbar's text color grid is open.
    pub text_color_picker_open: bool,
    pub hovered: bool,
    pub dragging: bool,
    /// Progress (0..=1) of the fade after switching edit/render mode.
    pub mode_fade: f32,
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

/// A header control: a Lucide icon on a faint ink wash when hovered, a
/// darker one when pressed.
fn header_button<'a>(
    glyph: Icon,
    message: Message,
    theme: theme::Theme,
    alpha: f32,
) -> Element<'a, Message> {
    pressable(
        icon(glyph, TEXT_SM),
        Padding::new(space(1)).left(space(2)).right(space(2)),
        message,
    )
    .style(move |_theme: &Theme, status| button::Style {
        background: match status {
            button::Status::Hovered => Some(theme.ink(0.12 * alpha).into()),
            button::Status::Pressed => Some(theme.ink(0.22 * alpha).into()),
            _ => None,
        },
        text_color: theme.ink(0.65 * alpha),
        border: border::rounded(RADIUS_CONTROL),
        ..Default::default()
    })
    .into()
}

/// The focus ring's border while `focused`, otherwise a plain rounded edge.
fn focus_border(theme: theme::Theme, focused: bool, alpha: f32) -> Border {
    let ring = theme.focus_ring();
    Border {
        color: Color {
            a: ring.a * alpha,
            ..ring
        },
        width: if focused { FOCUS_RING } else { 0.0 },
        radius: RADIUS_CONTROL.into(),
    }
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
/// While `ring`, it draws the focus ring around the scroll area; it stays
/// put while the text scrolls.
fn body_inset<'a>(
    content: impl Into<Element<'a, Message>>,
    theme: theme::Theme,
    ring: bool,
    a: f32,
) -> Element<'a, Message> {
    let frame = container(content).style(move |_theme: &Theme| container::Style {
        border: focus_border(theme, ring, a),
        ..Default::default()
    });
    container(frame)
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
        confirm_delete,
        color_picker_open,
        text_color_picker_open,
        hovered,
        dragging,
        mode_fade,
    } = p;
    let mode_eased = ease_out_cubic(mode_fade);
    // The body that just appeared fades in after a mode switch.
    let body_a = a * mode_eased;
    let controls = if hovered || dragging || color_picker_open || confirm_delete {
        a
    } else {
        a * idle_control_alpha
    };

    let paper = morph_paper(&theme, note.color, paper_tint, morph_progress);
    let [contact, ambient] = theme.shadows(morph_progress);

    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let title = text_input("Title", &note.title)
            .id(TITLE_ID)
            .on_input(Message::TitleEdited)
            .size(TEXT_MD)
            .padding(Padding::new(2.0).left(4).right(4))
            .font(theme::TITLE_FONT)
            .style(move |_theme: &Theme, status| text_input::Style {
                background: Color::TRANSPARENT.into(),
                border: focus_border(
                    theme,
                    matches!(status, text_input::Status::Focused { .. }),
                    a,
                ),
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

        // Any press on the header, title and buttons included, leaves edit
        // mode; the press still reaches them.
        let header = press_through(
            container(
                row![
                    title,
                    color_btn,
                    header_button(Icon::Trash, Message::DeleteRequested, theme, controls),
                    header_button(Icon::Close, Message::ClosePanel, theme, controls),
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
        let body: Element<'_, Message> = if editing {
            body_inset(
                responsive(move |available| {
                    let editor = text_editor(content)
                        .id(BODY_EDITOR_ID)
                        .placeholder(PLACEHOLDER)
                        .on_action(Message::NoteEdited)
                        .key_binding(body_key_binding)
                        .min_height(editor_min_height(available.height))
                        .size(TEXT_SM)
                        .line_height(text::LineHeight::Relative(theme::BODY_LINE_HEIGHT))
                        .padding(body_padding())
                        .style(move |_theme: &Theme, _status| text_editor::Style {
                            background: Color::TRANSPARENT.into(),
                            border: Border::default(),
                            placeholder: theme.ink(0.35 * body_a),
                            value: theme.ink(0.9 * body_a),
                            selection: theme.ink(0.18 * body_a),
                        });
                    body_scrollable(pass_wheel(editor), theme, hovered, body_a)
                }),
                theme,
                true,
                body_a,
            )
        } else {
            // Content inside a scrollable can't fill its height, so the
            // click target for the space below the text sits behind it.
            let rendered = container(rich_view::view(doc, data_dir, broken_images, theme, body_a))
                .padding(body_padding());
            stack![
                mouse_area(Space::new().width(Fill).height(Fill))
                    .on_press(Message::BodyClicked(None)),
                body_inset(
                    body_scrollable(rendered, theme, hovered, body_a),
                    theme,
                    false,
                    body_a
                ),
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
        if color_picker_open {
            col = col.push(
                container(color_picker(&note.color, palette, theme))
                    .padding(Padding::ZERO.left(14).bottom(6)),
            );
        }
        if editing {
            col = col.push(toolbar(
                palette,
                text_color_picker_open,
                controls,
                mode_eased,
                theme,
            ));
        }
        col = col.push(body);

        if confirm_delete {
            let choice = |label: &'static str, msg: Message, danger: bool| {
                pressable(
                    text(label).size(TEXT_SM),
                    Padding::new(space(1)).left(space(3)).right(space(3)),
                    msg,
                )
                .style(move |_theme: &Theme, status| {
                    let pressed = matches!(status, button::Status::Pressed);
                    button::Style {
                        background: Some(if danger {
                            let fill = theme.danger(0.95 * a);
                            if pressed {
                                theme::mix(fill, Color::BLACK, 0.18)
                            } else {
                                fill
                            }
                            .into()
                        } else {
                            theme.ink(if pressed { 0.22 } else { 0.12 } * a).into()
                        }),
                        text_color: if danger { Color::WHITE } else { theme.ink(a) },
                        border: border::rounded(RADIUS_CONTROL),
                        ..Default::default()
                    }
                })
            };
            col = col.push(
                container(
                    row![
                        text("Delete this note?").size(TEXT_SM).color(theme.ink(a)),
                        Space::new().width(Fill),
                        choice("Cancel", Message::ConfirmDelete(false), false),
                        choice("Delete", Message::ConfirmDelete(true), true),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding(Padding::new(10.0).left(18))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(theme.ink(0.06 * a).into()),
                    border: border::rounded(border::bottom(RADIUS_SURFACE)),
                    ..Default::default()
                }),
            );
        }
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
/// macOS), given a key's character and the held modifiers.
pub(crate) fn history_key(key: &str, modifiers: keyboard::Modifiers) -> Option<Message> {
    if !modifiers.command() {
        return None;
    }
    match key {
        "z" | "Z" if modifiers.shift() => Some(Message::Redo),
        "z" | "Z" => Some(Message::Undo),
        "y" | "Y" if cfg!(not(target_os = "macos")) => Some(Message::Redo),
        _ => None,
    }
}

/// The body editor's key bindings while it has focus: Cmd/Ctrl+V asks the
/// app to paste (text or image), and the undo/redo keys step its history;
/// everything else is iced's default.
fn body_key_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return text_editor::Binding::from_key_press(press);
    }
    match press.key.as_ref() {
        keyboard::Key::Character("v") if press.modifiers.command() => {
            Some(text_editor::Binding::Custom(Message::PasteRequested))
        }
        keyboard::Key::Character(c) => match history_key(c, press.modifiers) {
            Some(step) => Some(text_editor::Binding::Custom(step)),
            None => text_editor::Binding::from_key_press(press),
        },
        _ => text_editor::Binding::from_key_press(press),
    }
}

#[cfg(test)]
mod tests {
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

    use super::*;
    use iced::advanced::widget::operation::scrollable::RelativeOffset;
    use iced::{Point, Size};

    #[test]
    fn empty_placeholder_text() {
        assert_eq!(PLACEHOLDER, "Start typing… Markdown works.");
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
}
