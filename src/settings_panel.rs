//! The settings card: live sliders per group and the palette editor. It
//! morphs out of the gear slot in the strip like a note out of its bar.

use crate::app::Message;
use crate::color_picker::swatch;
use crate::settings::{SettingKey, Settings, SettingsGroup, PRESETS};

use iced::widget::{button, column, container, row, scrollable, slider, text, Space};
use iced::{
    Background, Border, Color, Element, Fill, Length, Padding, Shadow, Size, Theme, Vector,
};

pub const PANEL_WIDTH: f32 = 360.0;
pub const PANEL_MAX_HEIGHT: f32 = 600.0;

/// Card color while open; it starts as the gear slot's gray.
const CARD: [f32; 3] = [0.13, 0.13, 0.15];
const SLOT: [f32; 3] = [0.45, 0.46, 0.5];

pub struct SettingsView<'a> {
    pub settings: &'a Settings,
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
    pub selected_slot: Option<usize>,
}

fn white(alpha: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, alpha)
}

fn text_button<'a>(label: &'a str, message: Message, alpha: f32) -> Element<'a, Message> {
    button(text(label).size(12))
        .on_press(message)
        .padding(Padding::new(2.0).left(8).right(8))
        .style(move |_theme: &Theme, status| button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(white(0.1 * alpha).into())
                }
                _ => None,
            },
            text_color: white(0.6 * alpha),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn group_header<'a>(group: SettingsGroup, a: f32) -> Element<'a, Message> {
    row![
        text(group.label()).size(13).color(white(0.9 * a)),
        Space::new().width(Fill),
        text_button("Reset", Message::ResetGroup(group), a),
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn slider_row<'a>(settings: &Settings, key: SettingKey, a: f32) -> Element<'a, Message> {
    let value = settings.get(key);
    column![
        row![
            text(key.label()).size(12).color(white(0.75 * a)),
            Space::new().width(Fill),
            text(key.format(value)).size(12).color(white(0.55 * a)),
        ],
        slider(key.range(), value, move |v| Message::SettingChanged(key, v)).step(key.step()),
    ]
    .spacing(4)
    .into()
}

fn palette_section<'a>(
    settings: &Settings,
    selected: Option<usize>,
    a: f32,
) -> Element<'a, Message> {
    const PER_ROW: usize = 5;
    let mut col = column![group_header(SettingsGroup::Palette, a)].spacing(8);
    for (r, chunk) in settings.palette.chunks(PER_ROW).enumerate() {
        let slots = chunk.iter().enumerate().map(|(j, color)| {
            let i = r * PER_ROW + j;
            let is_selected = selected == Some(i);
            // Clicking the selected slot again closes the preset grid.
            let next = (!is_selected).then_some(i);
            swatch(
                *color,
                28.0,
                is_selected,
                Message::PaletteSlotSelected(next),
            )
        });
        col = col.push(row(slots).spacing(6));
    }
    if selected.is_some() {
        col = col.push(text("Replace with").size(12).color(white(0.55 * a)));
        for chunk in PRESETS.chunks(12) {
            col = col.push(
                row(chunk
                    .iter()
                    .map(|c| swatch(*c, 18.0, false, Message::PaletteColorChosen(*c))))
                .spacing(0),
            );
        }
    }
    col.into()
}

pub fn settings_panel(v: SettingsView<'_>) -> Element<'_, Message> {
    let SettingsView {
        settings,
        size,
        morph_progress: t,
        content_alpha: a,
        selected_slot,
    } = v;
    let mix = |i: usize| SLOT[i] + (CARD[i] - SLOT[i]) * t;
    let card = Color::from_rgb(mix(0), mix(1), mix(2));

    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let header = row![
            text("Settings").size(16).color(white(a)),
            Space::new().width(Fill),
            text_button("✕", Message::CloseSettings, a),
        ]
        .align_y(iced::Alignment::Center);

        let mut body = column![].spacing(18).padding(Padding::ZERO.right(14));
        for group in SettingsGroup::SLIDERS {
            let mut section = column![group_header(group, a)].spacing(10);
            for key in SettingKey::ALL.into_iter().filter(|k| k.group() == group) {
                section = section.push(slider_row(settings, key, a));
            }
            body = body.push(section);
        }
        body = body.push(palette_section(settings, selected_slot, a));

        column![header, scrollable(body).height(Fill)]
            .spacing(12)
            .padding(Padding::new(16.0).right(4))
            .into()
    };

    container(inner)
        .width(Length::Fixed(size.width))
        .height(Length::Fixed(size.height))
        .clip(true)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(card)),
            border: Border {
                radius: (3.0 + 7.0 * t).into(),
                ..Default::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.28 * t),
                offset: Vector::new(4.0, 10.0),
                blur_radius: 22.0,
            },
            ..Default::default()
        })
        .into()
}
