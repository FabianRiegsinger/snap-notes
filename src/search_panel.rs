//! The search card: a query field over the matching notes. It morphs out of
//! the search slot in the strip like Settings out of the gear.

use crate::app::Message;
use crate::icons::{icon, Icon, ICON_FONT};
use crate::note_panel::pressable;
use crate::search::Hit;
use crate::settings_panel::{paper_card, text_button};
use crate::theme::{self, space, RADIUS_CONTROL, TEXT_MD, TEXT_SM, TEXT_XS};

use iced::widget::{button, column, rich_text, row, scrollable, text, text_input, Space};
use iced::{border, font, Border, Element, Fill, Font, Padding, Size, Theme};
use std::ops::Range;

const FIELD_ID: &str = "search-field";

/// Puts the keyboard focus into the search field.
pub fn focus_field() -> iced::Task<Message> {
    iced::widget::operation::focus(FIELD_ID)
}

pub struct SearchView<'a> {
    pub theme: theme::Theme,
    pub query: &'a str,
    pub hits: &'a [Hit],
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
}

/// The snippet split around its match: before, match, after. A range that
/// doesn't fit the snippet (or splits a char) leaves the snippet unbolded.
fn snippet_parts(snippet: &str, highlight: Range<usize>) -> (&str, &str, &str) {
    let end = highlight.end.min(snippet.len());
    let start = highlight.start.min(end);
    if !snippet.is_char_boundary(start) || !snippet.is_char_boundary(end) {
        return (snippet, "", "");
    }
    (&snippet[..start], &snippet[start..end], &snippet[end..])
}

fn result_row<'a>(hit: &Hit, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let title = if hit.title.trim().is_empty() {
        "Untitled".to_string()
    } else {
        hit.title.clone()
    };
    let (before, matched, after) = snippet_parts(&hit.snippet, hit.highlight.clone());
    let bold = Font {
        weight: font::Weight::Bold,
        ..theme::BODY_FONT
    };
    let spans: Vec<text::Span<'a, ()>> = vec![
        text::Span::new(before.to_string()),
        text::Span::new(matched.to_string())
            .font(bold)
            .color(theme.ink(0.9 * a)),
        text::Span::new(after.to_string()),
    ];
    let label = column![
        text(title)
            .size(TEXT_SM)
            .font(theme::TITLE_FONT)
            .color(theme.ink(0.9 * a)),
        rich_text(spans).size(TEXT_XS).color(theme.ink(0.65 * a)),
    ]
    .spacing(2.0)
    .width(Fill);
    pressable(
        label,
        Padding::new(space(2)),
        Message::SearchResultPicked(hit.note_id),
    )
    .width(Fill)
    .style(move |_theme: &Theme, status| button::Style {
        background: match status {
            button::Status::Hovered => Some(theme.ink(0.08 * a).into()),
            button::Status::Pressed => Some(theme.ink(0.16 * a).into()),
            _ => None,
        },
        border: border::rounded(RADIUS_CONTROL),
        ..Default::default()
    })
    .into()
}

pub fn search_panel(v: SearchView<'_>) -> Element<'_, Message> {
    let SearchView {
        theme,
        query,
        hits,
        size,
        morph_progress: t,
        content_alpha: a,
    } = v;

    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let header = row![
            text("Search")
                .size(TEXT_MD)
                .font(theme::TITLE_FONT)
                .color(theme.ink(a)),
            Space::new().width(Fill),
            text_button(icon(Icon::Close, TEXT_SM), Message::CloseSearch, theme, a),
        ]
        .align_y(iced::Alignment::Center);

        let field = text_input("Search notes", query)
            .id(FIELD_ID)
            .on_input(Message::SearchChanged)
            .size(TEXT_SM)
            .padding(Padding::new(space(1)).left(space(2)).right(space(2)))
            .icon(text_input::Icon {
                font: ICON_FONT,
                code_point: Icon::Search.codepoint(),
                size: Some(TEXT_SM.into()),
                spacing: space(2),
                side: text_input::Side::Left,
            })
            .style(move |_theme: &Theme, status| text_input::Style {
                background: theme.ink(0.06 * a).into(),
                border: Border {
                    color: theme.focus_ring().scale_alpha(a),
                    width: if matches!(status, text_input::Status::Focused { .. }) {
                        1.5
                    } else {
                        0.0
                    },
                    radius: RADIUS_CONTROL.into(),
                },
                icon: theme.ink(0.55 * a),
                placeholder: theme.ink(0.4 * a),
                value: theme.ink(a),
                selection: theme.ink(0.18 * a),
            });

        let mut list = column![]
            .spacing(space(1))
            .padding(Padding::ZERO.right(space(3)));
        if hits.is_empty() && !query.trim().is_empty() {
            list = list.push(
                text("No matching notes")
                    .size(TEXT_XS)
                    .color(theme.ink(0.55 * a)),
            );
        }
        for hit in hits {
            list = list.push(result_row(hit, theme, a));
        }

        column![header, field, scrollable(list).height(Fill)]
            .spacing(space(3))
            .padding(Padding::new(space(4)).right(space(1)))
            .into()
    };
    paper_card(inner, size, theme, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_splits_around_the_match() {
        assert_eq!(snippet_parts("a café b", 2..7), ("a ", "café", " b"));
        assert_eq!(snippet_parts("plain", 0..0), ("", "", "plain"));
    }

    #[test]
    fn out_of_range_highlight_is_clamped() {
        assert_eq!(snippet_parts("abc", 1..99), ("a", "bc", ""));
        assert_eq!(snippet_parts("abc", 50..99), ("abc", "", ""));
        #[allow(clippy::reversed_empty_ranges)]
        let reversed = 2..1;
        assert_eq!(snippet_parts("abc", reversed), ("a", "", "bc"));
        // Inside the two-byte `é`: shown unbolded rather than panicking.
        assert_eq!(snippet_parts("é", 1..2), ("é", "", ""));
    }
}
