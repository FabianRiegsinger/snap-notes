//! Small calendar popover for setting a note reminder as an absolute title tag.

use crate::app::Message;
use crate::theme::{self, space, RADIUS_CONTROL, TEXT_SM, TEXT_XS};

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, Timelike};
use iced::widget::{button, column, container, row, text, Space};
use iced::{border, Alignment, Element, Length, Padding, Theme};

/// Draft datetime while the reminder picker is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderDraft {
    pub date: NaiveDate,
    pub hour: u32,
    pub minute: u32,
    /// First day of the month shown in the calendar grid.
    pub view_month: NaiveDate,
}

impl ReminderDraft {
    /// Prefill from a resolved reminder time.
    pub fn from_at(at: chrono::DateTime<Local>) -> Self {
        let date = at.date_naive();
        Self {
            date,
            hour: at.hour(),
            minute: at.minute(),
            view_month: first_of_month(date),
        }
    }

    /// Prefill with [`crate::reminder::default_at`].
    pub fn from_now(now: chrono::DateTime<Local>) -> Self {
        Self::from_at(crate::reminder::default_at(now))
    }

    /// Local datetime for Done, if the clock fields are valid.
    pub fn at(&self) -> Option<chrono::DateTime<Local>> {
        let time = NaiveTime::from_hms_opt(self.hour, self.minute, 0)?;
        crate::reminder::to_local(self.date.and_time(time))
    }
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("valid month")
}

/// Width of the popover content.
const WIDTH: f32 = 252.0;
const DAY: f32 = 32.0;

/// Calendar + time steppers + Clear / Done.
pub fn view<'a>(
    draft: &'a ReminderDraft,
    can_clear: bool,
    theme: theme::Theme,
) -> Element<'a, Message> {
    let month_label = text(draft.view_month.format("%B %Y").to_string())
        .size(TEXT_SM)
        .color(theme.ink(0.85));
    let nav = row![
        step_btn("‹", Message::ReminderPickerMonth(-1), theme),
        container(month_label)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        step_btn("›", Message::ReminderPickerMonth(1), theme),
    ]
    .spacing(space(1))
    .align_y(Alignment::Center)
    .width(WIDTH);

    let weekdays = row(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|d| {
        container(text(d).size(TEXT_XS).color(theme.ink(0.45)).center())
            .width(DAY)
            .height(22.0)
            .into()
    }))
    .spacing(2);

    let grid = month_grid(draft, theme);

    let time_row = row![
        text("Time").size(TEXT_XS).color(theme.ink(0.55)),
        Space::new().width(Length::Fill),
        step_btn("−", Message::ReminderPickerHour(-1), theme),
        text(format!("{:02}", draft.hour))
            .size(TEXT_SM)
            .color(theme.ink(0.85))
            .width(Length::Fixed(28.0))
            .center(),
        step_btn("+", Message::ReminderPickerHour(1), theme),
        text(":").size(TEXT_SM).color(theme.ink(0.45)),
        step_btn("−", Message::ReminderPickerMinute(-5), theme),
        text(format!("{:02}", draft.minute))
            .size(TEXT_SM)
            .color(theme.ink(0.85))
            .width(Length::Fixed(28.0))
            .center(),
        step_btn("+", Message::ReminderPickerMinute(5), theme),
    ]
    .spacing(space(1))
    .align_y(Alignment::Center)
    .width(WIDTH);

    let mut actions = row![].spacing(space(2));
    if can_clear {
        actions = actions.push(action_btn(
            "Clear",
            Message::ReminderPickerClear,
            false,
            theme,
        ));
    }
    actions = actions.push(Space::new().width(Length::Fill));
    actions = actions.push(action_btn("Done", Message::ReminderPickerDone, true, theme));

    column![nav, weekdays, grid, time_row, actions]
        .spacing(space(2))
        .width(WIDTH)
        .into()
}

fn month_grid<'a>(draft: &'a ReminderDraft, theme: theme::Theme) -> Element<'a, Message> {
    let start = draft.view_month;
    let weekday_offset = start.weekday().number_from_monday() as i64 - 1;
    let first_cell = start - Duration::days(weekday_offset);
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for week in 0..6 {
        let mut cells: Vec<Element<'a, Message>> = Vec::new();
        for day in 0..7 {
            let date = first_cell + Duration::days(week * 7 + day);
            let in_month = date.month() == start.month();
            let selected = date == draft.date;
            let label = text(format!("{}", date.day()))
                .size(TEXT_XS)
                .color(theme.ink(if selected {
                    0.95
                } else if in_month {
                    0.75
                } else {
                    0.35
                }))
                .center();
            let cell = button(label)
                .padding(0)
                .width(DAY)
                .height(DAY)
                .on_press(Message::ReminderPickerDay(date))
                .style(move |_theme: &Theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: if selected {
                            Some(theme.ink(0.22).into())
                        } else if hovered {
                            Some(theme.ink(0.1).into())
                        } else {
                            None
                        },
                        text_color: theme.ink(0.85),
                        border: border::rounded(RADIUS_CONTROL),
                        ..Default::default()
                    }
                });
            cells.push(cell.into());
        }
        rows.push(row(cells).spacing(2).into());
    }
    column(rows).spacing(2).into()
}

fn step_btn<'a>(label: &'a str, message: Message, theme: theme::Theme) -> Element<'a, Message> {
    button(text(label).size(TEXT_SM).color(theme.ink(0.7)))
        .padding(Padding::new(4.0).left(8.0).right(8.0))
        .on_press(message)
        .style(move |_theme: &Theme, status| button::Style {
            background: match status {
                button::Status::Hovered => Some(theme.ink(0.12).into()),
                button::Status::Pressed => Some(theme.ink(0.2).into()),
                _ => Some(theme.ink(0.06).into()),
            },
            text_color: theme.ink(0.75),
            border: border::rounded(RADIUS_CONTROL),
            ..Default::default()
        })
        .into()
}

fn action_btn<'a>(
    label: &'a str,
    message: Message,
    primary: bool,
    theme: theme::Theme,
) -> Element<'a, Message> {
    button(text(label).size(TEXT_SM))
        .padding(Padding::new(6.0).left(12.0).right(12.0))
        .on_press(message)
        .style(move |_theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    theme
                        .ink(if primary {
                            if hovered {
                                0.28
                            } else {
                                0.2
                            }
                        } else if hovered {
                            0.14
                        } else {
                            0.08
                        })
                        .into(),
                ),
                text_color: theme.ink(0.9),
                border: border::rounded(RADIUS_CONTROL),
                ..Default::default()
            }
        })
        .into()
}

/// Shift `hour`/`minute` by the given deltas, wrapping within the day.
pub fn adjust_time(draft: &mut ReminderDraft, hour_delta: i32, minute_delta: i32) {
    let mut minutes = draft.hour as i32 * 60 + draft.minute as i32 + hour_delta * 60 + minute_delta;
    minutes = minutes.rem_euclid(24 * 60);
    draft.hour = (minutes / 60) as u32;
    draft.minute = (minutes % 60) as u32;
}

/// Move the visible calendar month by `delta` months; keeps the selected day
/// when it exists in the new month.
pub fn shift_month(draft: &mut ReminderDraft, delta: i32) {
    let (y, m) = (
        draft.view_month.year(),
        draft.view_month.month() as i32 + delta,
    );
    let mut year = y;
    let mut month = m;
    while month < 1 {
        month += 12;
        year -= 1;
    }
    while month > 12 {
        month -= 12;
        year += 1;
    }
    draft.view_month = NaiveDate::from_ymd_opt(year, month as u32, 1).expect("valid month");
    if let Some(date) = NaiveDate::from_ymd_opt(year, month as u32, draft.date.day()) {
        draft.date = date;
    } else {
        // Clamp to last day of month (e.g. Jan 31 → Feb 28).
        let last = last_day_of_month(year, month as u32);
        draft.date = NaiveDate::from_ymd_opt(year, month as u32, last).expect("valid day");
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let first_next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .expect("valid month");
    (first_next - Duration::days(1)).day()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn adjust_time_wraps_within_day() {
        let mut d =
            ReminderDraft::from_at(Local.with_ymd_and_hms(2026, 10, 15, 23, 55, 0).unwrap());
        adjust_time(&mut d, 0, 10);
        assert_eq!((d.hour, d.minute), (0, 5));
        adjust_time(&mut d, -1, 0);
        assert_eq!((d.hour, d.minute), (23, 5));
    }

    #[test]
    fn shift_month_clamps_day() {
        let mut d = ReminderDraft::from_at(Local.with_ymd_and_hms(2026, 1, 31, 10, 0, 0).unwrap());
        shift_month(&mut d, 1);
        assert_eq!(d.view_month, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert_eq!(d.date, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }
}
