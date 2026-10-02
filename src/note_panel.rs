use crate::app::Message;
use crate::note::Note;

use iced::widget::{
    button, column, container, row, scrollable, text, text_editor, Space,
};
use iced::{Color, Element, Fill, Length, Padding, Theme};

pub fn note_panel<'a>(
    note: &'a Note,
    content: &'a text_editor::Content,
    slide_progress: f32,
    confirm_delete: bool,
    expanded: bool,
    panel_width: f32,
) -> Element<'a, Message> {
    let width = panel_width * slide_progress;
    if width < 1.0 {
        return Space::new().width(0).height(0).into();
    }

    let color_indicator = container(Space::new().width(16).height(16))
        .style(move |_theme: &Theme| container::Style {
            background: Some(
                Color::from_rgba(
                    note.color.rgba[0],
                    note.color.rgba[1],
                    note.color.rgba[2],
                    1.0,
                )
                .into(),
            ),
            border: iced::Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let color_btn = button(color_indicator)
        .on_press(Message::ToggleColorPicker)
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            ..Default::default()
        })
        .padding(4);

    let (expand_icon, expand_msg) = if expanded {
        ("⤡", Message::ShrinkNote)
    } else {
        ("⤢", Message::ExpandNote)
    };
    let expand_btn = button(text(expand_icon).size(16))
        .on_press(expand_msg)
        .padding(4)
        .style(|_theme: &Theme, _status| button::Style {
            background: Some(Color::from_rgba(0.3, 0.3, 0.3, 0.6).into()),
            text_color: Color::WHITE,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let close_btn = button(text("✕").size(14))
        .on_press(Message::DeleteRequested)
        .padding(4)
        .style(|_theme: &Theme, _status| button::Style {
            background: Some(Color::from_rgba(0.8, 0.2, 0.2, 0.7).into()),
            text_color: Color::WHITE,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let top_bar = row![
        color_btn,
        Space::new().width(Fill),
        expand_btn,
        Space::new().width(4),
        close_btn,
    ]
    .padding(4)
    .align_y(iced::Alignment::Center);

    let editor = text_editor(content).on_action(Message::NoteEdited);

    let body = scrollable(container(editor).padding(8)).height(Fill);

    let mut panel_content = column![top_bar, body].spacing(4);

    if confirm_delete {
        let dialog = container(
            column![
                text("Delete this note?").size(14),
                row![
                    button(text("Yes").size(13))
                        .on_press(Message::ConfirmDelete(true))
                        .padding(Padding::new(4.0).left(12).right(12))
                        .style(|_theme: &Theme, _status| button::Style {
                            background: Some(
                                Color::from_rgba(0.8, 0.2, 0.2, 0.9).into(),
                            ),
                            text_color: Color::WHITE,
                            border: iced::Border {
                                radius: 4.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    Space::new().width(8),
                    button(text("No").size(13))
                        .on_press(Message::ConfirmDelete(false))
                        .padding(Padding::new(4.0).left(12).right(12))
                        .style(|_theme: &Theme, _status| button::Style {
                            background: Some(
                                Color::from_rgba(0.4, 0.4, 0.4, 0.9).into(),
                            ),
                            text_color: Color::WHITE,
                            border: iced::Border {
                                radius: 4.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                ]
                .spacing(8)
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .padding(12)
        .style(|_theme: &Theme| container::Style {
            background: Some(Color::from_rgba(0.15, 0.15, 0.15, 0.95).into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: Color::from_rgba(0.4, 0.4, 0.4, 0.5),
            },
            ..Default::default()
        });

        panel_content = panel_content.push(
            container(dialog)
                .width(Fill)
                .align_x(iced::Alignment::Center)
                .padding(Padding::ZERO.bottom(8)),
        );
    }

    container(panel_content)
        .width(Length::Fixed(width))
        .height(Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(Color::from_rgba(0.12, 0.12, 0.14, 0.95).into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: Color::from_rgba(0.3, 0.3, 0.3, 0.5),
            },
            ..Default::default()
        })
        .into()
}
