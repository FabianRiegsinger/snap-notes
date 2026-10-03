use crate::app::Message;
use crate::color_picker::color_picker;
use crate::note::Note;

use iced::widget::{button, column, container, row, text, text_editor, text_input, Space};
use iced::{font, Background, Border, Color, Element, Fill, Font, Length, Padding, Shadow, Size};
use iced::{Theme, Vector};

const INK: [f32; 3] = [0.13, 0.12, 0.10];
const RADIUS: f32 = 1.0;
/// Header controls stay faint until the note is hovered, like plain paper.
const IDLE_CONTROL_ALPHA: f32 = 0.3;

pub struct PostIt<'a> {
    pub note: &'a Note,
    pub content: &'a text_editor::Content,
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
    pub confirm_delete: bool,
    pub expanded: bool,
    pub color_picker_open: bool,
    pub hovered: bool,
}

fn ink(alpha: f32) -> Color {
    Color::from_rgba(INK[0], INK[1], INK[2], alpha)
}

/// Mixes the note color toward black (`amount > 0`) or white (`amount < 0`).
fn shade(note: &Note, amount: f32, alpha: f32) -> Color {
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
        content,
        size,
        morph_progress,
        content_alpha: a,
        confirm_delete,
        expanded,
        color_picker_open,
        hovered,
    } = p;
    let controls = if hovered || color_picker_open || confirm_delete {
        a
    } else {
        a * IDLE_CONTROL_ALPHA
    };

    let paper = shade(note, -0.12, 1.0);
    let shadow_alpha = 0.28 * morph_progress;

    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let title = text_input("Title", &note.title)
            .on_input(Message::TitleEdited)
            .size(16)
            .padding(0)
            .font(Font {
                weight: font::Weight::Bold,
                ..Font::default()
            })
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
        .padding(Padding::new(12.0).left(18).right(10).bottom(6));

        let body = text_editor(content)
            .placeholder("Write something…")
            .on_action(Message::NoteEdited)
            .height(Fill)
            .size(14)
            .padding(Padding::new(6.0).left(18).right(18).bottom(18))
            .style(move |_theme: &Theme, _status| text_editor::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                placeholder: ink(0.35 * a),
                value: ink(0.9 * a),
                selection: ink(0.18 * a),
            });

        let mut col = column![header];
        if color_picker_open {
            col = col.push(
                container(color_picker(&note.color)).padding(Padding::ZERO.left(14).bottom(6)),
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
