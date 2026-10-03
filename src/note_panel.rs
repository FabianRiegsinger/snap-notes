use crate::app::Message;
use crate::color_picker::color_picker;
use crate::note::{Note, NoteColor};
use crate::pass_wheel::pass_wheel;

use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, Scrollable};
use iced::advanced::widget::{Id, Operation};
use iced::Rectangle;

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, text, text_editor, text_input, Space,
};
use iced::{
    font, gradient, Background, Border, Color, Element, Fill, Font, Length, Padding, Shadow, Size,
};
use iced::{mouse, Theme, Vector};

/// Named explicitly: with the generic family, the bold face can fall back to a
/// monospace font.
#[cfg(target_os = "macos")]
const TITLE_FAMILY: font::Family = font::Family::Name("Helvetica Neue");
#[cfg(windows)]
const TITLE_FAMILY: font::Family = font::Family::Name("Segoe UI");
#[cfg(not(any(target_os = "macos", windows)))]
const TITLE_FAMILY: font::Family = font::Family::SansSerif;
pub(crate) const TITLE_FONT: Font = Font {
    family: TITLE_FAMILY,
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};

const BODY_SCROLL_ID: &str = "note-body-scroll";
const BODY_EDITOR_ID: &str = "note-body-editor";
/// Space kept below the last line when following the caret (part of the
/// editor's bottom padding).
const CARET_MARGIN: f32 = 12.0;

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
/// Approximate heights of everything above/below the body, used to size the
/// editor so it fills the note without forcing a scrollbar.
const CHROME_HEIGHT: f32 = 72.0;
const PICKER_HEIGHT: f32 = 148.0;
const CONFIRM_HEIGHT: f32 = 46.0;

/// How much lighter than its bar a note's paper is.
pub(crate) const PAPER_LIGHTEN: f32 = -0.12;

const INK: [f32; 3] = [0.13, 0.12, 0.10];
const RADIUS: f32 = 1.0;
/// Header controls stay faint until the note is hovered, like plain paper.
const IDLE_CONTROL_ALPHA: f32 = 0.3;

pub struct PostIt<'a> {
    pub note: &'a Note,
    pub palette: &'a [NoteColor],
    pub content: &'a text_editor::Content,
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
    pub confirm_delete: bool,
    pub expanded: bool,
    pub color_picker_open: bool,
    pub hovered: bool,
    pub dragging: bool,
}

pub(crate) fn ink(alpha: f32) -> Color {
    Color::from_rgba(INK[0], INK[1], INK[2], alpha)
}

/// Mixes the note color toward black (`amount > 0`) or white (`amount < 0`).
pub(crate) fn shade(note: &Note, amount: f32, alpha: f32) -> Color {
    let [r, g, b, _] = note.color.rgba;
    let mix = |c: f32| {
        if amount >= 0.0 {
            c * (1.0 - amount)
        } else {
            c + (1.0 - c) * -amount
        }
    };
    Color::from_rgba(mix(r), mix(g), mix(b), alpha)
}

fn icon_button<'a>(label: &'a str, message: Message, alpha: f32) -> Element<'a, Message> {
    button(text(label).size(14))
        .on_press(message)
        .padding(Padding::new(3.0).left(7).right(7))
        .style(move |_theme: &Theme, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(ink(0.12 * alpha).into()),
                _ => None,
            },
            text_color: ink(0.65 * alpha),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

pub fn post_it(p: PostIt<'_>) -> Element<'_, Message> {
    let PostIt {
        note,
        palette,
        content,
        size,
        morph_progress,
        content_alpha: a,
        confirm_delete,
        expanded,
        color_picker_open,
        hovered,
        dragging,
    } = p;
    let controls = if hovered || dragging || color_picker_open || confirm_delete {
        a
    } else {
        a * IDLE_CONTROL_ALPHA
    };

    // Starts as the bar color so the morph has no seam where it meets the bar.
    let paper = shade(note, PAPER_LIGHTEN * morph_progress, 1.0);
    let shadow_alpha = 0.28 * morph_progress;

    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let title = text_input("Title", &note.title)
            .on_input(Message::TitleEdited)
            .size(16)
            .padding(0)
            .font(TITLE_FONT)
            .style(move |_theme: &Theme, _status| text_input::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                icon: ink(a),
                placeholder: ink(0.35 * a),
                value: ink(a),
                selection: ink(0.18 * a),
            });

        let swatch = container(Space::new().width(12).height(12)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(shade(note, 0.0, controls).into()),
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: ink(0.35 * controls),
                },
                ..Default::default()
            }
        });
        let color_btn = button(swatch)
            .on_press(Message::ToggleColorPicker)
            .padding(5)
            .style(|_theme: &Theme, _status| button::Style {
                background: None,
                ..Default::default()
            });

        let (expand_icon, expand_msg) = if expanded {
            ("⤡", Message::ShrinkNote)
        } else {
            ("⤢", Message::ExpandNote)
        };

        let header = container(
            row![
                title,
                color_btn,
                icon_button(expand_icon, expand_msg, controls),
                icon_button("🗑", Message::DeleteRequested, controls),
                icon_button("✕", Message::ClosePanel, controls),
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::new(2.0).left(18).right(10).bottom(6));

        // Grip along the top edge: drag to move the note, double-click to
        // send it back next to the dock.
        let pill = container(Space::new().width(36).height(4)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(ink(0.3 * controls).into()),
                border: Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });
        let grip = mouse_area(
            container(pill)
                .width(Fill)
                .height(16)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center),
        )
        .on_press(Message::NoteDragStart)
        .on_double_click(Message::NoteResetPosition)
        .interaction(if dragging {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::Grab
        });

        // The editor grows with its text inside a scrollable, so a scrollbar
        // appears (and stays) only when the text no longer fits. Its minimum
        // height roughly fills the visible body so clicks below short text
        // still land in the editor.
        let chrome = CHROME_HEIGHT
            + if color_picker_open {
                PICKER_HEIGHT
            } else {
                0.0
            }
            + if confirm_delete { CONFIRM_HEIGHT } else { 0.0 };
        let editor = text_editor(content)
            .id(BODY_EDITOR_ID)
            .placeholder("Write something…")
            .on_action(Message::NoteEdited)
            .min_height((size.height - chrome).max(0.0))
            .size(14)
            .padding(Padding::new(6.0).left(18).right(18).bottom(18))
            .style(move |_theme: &Theme, _status| text_editor::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                placeholder: ink(0.35 * a),
                value: ink(0.9 * a),
                selection: ink(0.18 * a),
            });
        let body = scrollable(pass_wheel(editor))
            .id(BODY_SCROLL_ID)
            .height(Fill)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(6)
                    .scroller_width(6)
                    .margin(5),
            ))
            .style(move |_theme: &Theme, status| {
                let active = matches!(
                    status,
                    scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
                );
                let rail = scrollable::Rail {
                    background: Some(ink(0.06 * a).into()),
                    border: Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    scroller: scrollable::Scroller {
                        background: ink(if active { 0.45 } else { 0.28 } * a).into(),
                        border: Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                    },
                };
                scrollable::Style {
                    container: container::Style::default(),
                    vertical_rail: rail,
                    horizontal_rail: rail,
                    gap: None,
                    auto_scroll: scrollable::AutoScroll {
                        background: ink(0.1 * a).into(),
                        border: Border::default(),
                        shadow: Shadow::default(),
                        icon: ink(a),
                    },
                }
            });

        // Divider: a faint inked hairline with a short soft shadow fading
        // below it, as if the header sheet rests slightly on the body.
        let hairline =
            container(Space::new().width(Fill).height(1)).style(move |_theme: &Theme| {
                container::Style {
                    background: Some(ink(0.12 * a).into()),
                    ..Default::default()
                }
            });
        let fade = container(Space::new().width(Fill).height(6)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(
                    gradient::Linear::new(std::f32::consts::PI)
                        .add_stop(0.0, ink(0.07 * a))
                        .add_stop(1.0, ink(0.0))
                        .into(),
                ),
                ..Default::default()
            }
        });
        let divider = container(column![hairline, fade]).padding(Padding::ZERO.left(14).right(14));

        let mut col = column![grip, header, divider];
        if color_picker_open {
            col = col.push(
                container(color_picker(&note.color, palette))
                    .padding(Padding::ZERO.left(14).bottom(6)),
            );
        }
        col = col.push(body);

        if confirm_delete {
            let choice = |label: &'static str, msg: Message, danger: bool| {
                button(text(label).size(13))
                    .on_press(msg)
                    .padding(Padding::new(4.0).left(12).right(12))
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(if danger {
                            Color::from_rgba(0.75, 0.18, 0.18, 0.95 * a).into()
                        } else {
                            ink(0.12 * a).into()
                        }),
                        text_color: if danger { Color::WHITE } else { ink(a) },
                        border: Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
            };
            col = col.push(
                container(
                    row![
                        text("Delete this note?").size(13).color(ink(a)),
                        Space::new().width(Fill),
                        choice("Cancel", Message::ConfirmDelete(false), false),
                        choice("Delete", Message::ConfirmDelete(true), true),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding(Padding::new(10.0).left(18))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(shade(note, 0.08, a).into()),
                    ..Default::default()
                }),
            );
        }
        col.into()
    };

    container(inner)
        .width(Length::Fixed(size.width))
        .height(Length::Fixed(size.height))
        .clip(true)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(paper)),
            border: Border {
                radius: RADIUS.into(),
                ..Default::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, shadow_alpha),
                offset: Vector::new(4.0, 10.0),
                blur_radius: 22.0,
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::widget::operation::scrollable::RelativeOffset;
    use iced::{Point, Size};

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
