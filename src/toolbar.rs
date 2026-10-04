use crate::app::Message;
use crate::color_picker::swatch;
use crate::icons::{icon, Icon, ICON_FONT};
use crate::note::NoteColor;
use crate::note_panel::pressable;
use crate::rich::Format;
use crate::theme::{self, space, RADIUS_CONTROL, TEXT_SM};

use iced::widget::{button, column, container, pick_list, row, text};
use iced::{border, Element, Padding, Theme};
use std::fmt;

/// A font size offered by the size list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SizeChoice(&'static str, f32);

const SIZES: [SizeChoice; 4] = [
    SizeChoice("small", 12.0),
    SizeChoice("normal", 14.0),
    SizeChoice("large", 20.0),
    SizeChoice("huge", 28.0),
];

impl fmt::Display for SizeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The toolbar's icons, in button order.
fn button_icons() -> [Icon; 9] {
    [
        Icon::Bold,
        Icon::Italic,
        Icon::Strike,
        Icon::Code,
        Icon::Palette,
        Icon::Highlight,
        Icon::Size,
        Icon::Link,
        Icon::Image,
    ]
}

/// The formatting row shown above the editor, with the text color grid
/// below it while `color_open`. `alpha` fades it like the header controls.
pub fn toolbar<'a>(
    palette: &'a [NoteColor],
    color_open: bool,
    alpha: f32,
    theme: theme::Theme,
) -> Element<'a, Message> {
    let tool = |glyph: Icon, message: Message| -> Element<'a, Message> {
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
    };
    let apply = |glyph: Icon, format| tool(glyph, Message::FormatApplied(format));
    let sizes = pick_list(SIZES, None::<SizeChoice>, |choice: SizeChoice| {
        Message::FormatApplied(Format::Size(choice.1))
    })
    .placeholder("size")
    .text_size(13)
    .padding(Padding::new(2.0).left(6).right(6))
    .handle(pick_list::Handle::Static(pick_list::Icon {
        font: ICON_FONT,
        code_point: Icon::Size.codepoint(),
        size: Some(TEXT_SM.into()),
        line_height: text::LineHeight::default(),
        shaping: text::Shaping::Basic,
    }))
    .style(move |_theme: &Theme, _status| pick_list::Style {
        text_color: theme.ink(0.65 * alpha),
        placeholder_color: theme.ink(0.65 * alpha),
        handle_color: theme.ink(0.5 * alpha),
        background: theme.ink(0.06 * alpha).into(),
        border: border::rounded(RADIUS_CONTROL),
    });

    let [bold, italic, strike, code, palette_icon, highlight, _size, link, image] = button_icons();
    let buttons = row![
        apply(bold, Format::Bold),
        apply(italic, Format::Italic),
        apply(strike, Format::Strike),
        apply(code, Format::Code),
        tool(palette_icon, Message::ToggleTextColorPicker),
        apply(highlight, Format::Highlight),
        sizes,
        apply(link, Format::Link),
        tool(image, Message::PickImage),
    ]
    .spacing(2)
    .align_y(iced::Alignment::Center)
    // Narrow notes wrap the row, so the link and image buttons stay visible.
    .wrap()
    .vertical_spacing(2);

    let mut col = column![buttons].spacing(4);
    if color_open {
        let rows: Vec<Element<'a, Message>> = palette
            .chunks(10)
            .enumerate()
            .map(|(r, chunk)| {
                row(chunk.iter().enumerate().map(|(i, color)| {
                    swatch(
                        *color,
                        18.0,
                        false,
                        Message::FormatApplied(Format::Color(r * 10 + i)),
                    )
                }))
                .spacing(2)
                .into()
            })
            .collect();
        col = col.push(container(column(rows).spacing(2)));
    }
    container(col)
        .padding(Padding::new(4.0).left(14).right(14))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_buttons_use_icons() {
        assert_eq!(
            button_icons(),
            [
                Icon::Bold,
                Icon::Italic,
                Icon::Strike,
                Icon::Code,
                Icon::Palette,
                Icon::Highlight,
                Icon::Size,
                Icon::Link,
                Icon::Image,
            ]
        );
    }
}
