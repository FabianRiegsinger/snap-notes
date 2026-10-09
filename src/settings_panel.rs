//! The settings card: live sliders per group and the palette editor. It
//! morphs out of the gear slot in the strip like a note out of its bar.

use crate::app::Message;
use crate::color_picker::swatch;
use crate::edge::Edge;
use crate::icons::{icon, Icon};
use crate::note_panel::pressable;
use crate::settings::SettingToggle;
use crate::settings::{
    ReminderAlertStyle, SettingKey, Settings, SettingsGroup, SettingsTab, PRESETS,
};
use crate::theme::{self, space, RADIUS_CONTROL, RADIUS_SURFACE, TEXT_MD, TEXT_SM, TEXT_XS};

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, slider, text, toggler, Space,
};
use iced::{border, mouse, Color, Element, Fill, Length, Padding, Shadow, Size, Theme};

pub const PANEL_WIDTH: f32 = 400.0;
pub const PANEL_MAX_HEIGHT: f32 = 600.0;
/// The adhesive band across the card's top, as tall as the note's.
const BAND_HEIGHT: f32 = 16.0;
/// Drag grip height inside the settings card (matches the note grip).
const GRIP_HEIGHT: f32 = 16.0;

pub struct SettingsView<'a> {
    pub theme: theme::Theme,
    pub settings: &'a Settings,
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
    /// The tab whose groups the card shows.
    pub tab: SettingsTab,
    pub selected_slot: Option<usize>,
    /// The menu bar / tray icon exists. Without it the Dock icon is the
    /// only way back into the app, so it can't be turned off.
    pub tray_ok: bool,
    /// The Dock icon is shown regardless of the setting (no tray icon).
    pub dock_forced: bool,
    /// Why the global hotkey couldn't be registered, if it couldn't.
    pub hotkey_error: Option<&'a str>,
    /// The panel is being dragged from its grip.
    pub dragging: bool,
}

/// A quiet text or icon button: an ink wash while hovered or pressed.
pub(crate) fn text_button<'a>(
    label: impl Into<Element<'a, Message>>,
    message: Message,
    theme: theme::Theme,
    alpha: f32,
) -> Element<'a, Message> {
    pressable(
        label,
        Padding::new(2.0).left(space(2)).right(space(2)),
        message,
    )
    .style(move |_theme: &Theme, status| button::Style {
        background: match status {
            button::Status::Hovered => Some(theme.ink(0.1 * alpha).into()),
            button::Status::Pressed => Some(theme.ink(0.2 * alpha).into()),
            _ => None,
        },
        text_color: theme.ink(0.6 * alpha),
        border: border::rounded(RADIUS_CONTROL),
        ..Default::default()
    })
    .into()
}

fn group_header<'a>(group: SettingsGroup, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let mut header = row![
        text(group.label())
            .size(TEXT_SM)
            .font(theme::TITLE_FONT)
            .color(theme.ink(0.9 * a)),
        Space::new().width(Fill),
    ]
    .align_y(iced::Alignment::Center);
    if group.resettable() {
        header = header.push(text_button(
            text("Reset").size(TEXT_XS),
            Message::ResetGroup(group),
            theme,
            a,
        ));
    }
    header.into()
}

/// The Data group: the button that opens the export panel in place of the
/// settings.
fn data_section<'a>(theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let label = row![icon(Icon::Download, TEXT_SM), text("Export…").size(TEXT_XS)]
        .spacing(space(2))
        .align_y(iced::Alignment::Center);
    column![
        group_header(SettingsGroup::Data, theme, a),
        text_button(label, Message::ToggleExport, theme, a),
    ]
    .spacing(space(2))
    .into()
}

fn slider_row<'a>(
    settings: &Settings,
    key: SettingKey,
    theme: theme::Theme,
    a: f32,
) -> Element<'a, Message> {
    let value = settings.get(key);
    column![
        row![
            text(key.label()).size(TEXT_XS).color(theme.ink(0.75 * a)),
            Space::new().width(Fill),
            text(key.format(value))
                .size(TEXT_XS)
                .color(theme.ink(0.55 * a)),
        ],
        slider(key.range(), value, move |v| Message::SettingChanged(key, v)).step(key.step()),
    ]
    .spacing(space(1))
    .into()
}

/// The Window group's toggle, on every platform.
pub const AUTO_HIDE_LABEL: &str = "Auto-hide";

/// The Window group's three-way screen edge control.
pub const SCREEN_EDGE_LABEL: &str = "Screen edge";

/// The Motion group's Jump / Pulse control for fired reminders.
pub const REMINDER_ALERT_LABEL: &str = "Reminder alert";

fn edge_label(edge: Edge) -> &'static str {
    match edge {
        Edge::Right => "Right",
        Edge::Left => "Left",
        Edge::Top => "Top",
    }
}

/// One side of a segmented switch: an ink wash marks the chosen option.
fn segment_button<'a>(
    label: &'static str,
    selected: bool,
    on_press: Message,
    theme: theme::Theme,
    a: f32,
) -> Element<'a, Message> {
    pressable(
        text(label).size(TEXT_XS),
        Padding::new(2.0).left(space(2)).right(space(2)),
        on_press,
    )
    .style(move |_theme: &Theme, status| button::Style {
        background: match status {
            button::Status::Pressed => Some(theme.ink(0.24 * a).into()),
            button::Status::Hovered => {
                Some(theme.ink(if selected { 0.18 } else { 0.1 } * a).into())
            }
            _ if selected => Some(theme.ink(0.14 * a).into()),
            _ => None,
        },
        text_color: theme.ink(if selected { 0.9 } else { 0.6 } * a),
        border: border::rounded(RADIUS_CONTROL),
        ..Default::default()
    })
    .into()
}

fn edge_button<'a>(edge: Edge, chosen: Edge, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    segment_button(
        edge_label(edge),
        edge == chosen,
        Message::EdgeChosen(edge),
        theme,
        a,
    )
}

fn edge_row<'a>(chosen: Edge, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let mut buttons = row![].spacing(space(1));
    for edge in Edge::ALL {
        buttons = buttons.push(edge_button(edge, chosen, theme, a));
    }
    row![
        text(SCREEN_EDGE_LABEL)
            .size(TEXT_XS)
            .color(theme.ink(0.75 * a)),
        Space::new().width(Fill),
        buttons,
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn reminder_alert_row<'a>(
    chosen: ReminderAlertStyle,
    theme: theme::Theme,
    a: f32,
) -> Element<'a, Message> {
    let mut buttons = row![].spacing(space(1));
    for style in ReminderAlertStyle::ALL {
        buttons = buttons.push(segment_button(
            style.label(),
            style == chosen,
            Message::ReminderAlertChosen(style),
            theme,
            a,
        ));
    }
    row![
        text(REMINDER_ALERT_LABEL)
            .size(TEXT_XS)
            .color(theme.ink(0.75 * a)),
        Space::new().width(Fill),
        buttons,
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

/// A labelled switch in the panel's text style, showing `on`.
fn switch<'a>(
    on: bool,
    label: &'a str,
    theme: theme::Theme,
    a: f32,
) -> toggler::Toggler<'a, Message> {
    toggler(on)
        .label(label)
        .text_size(TEXT_XS)
        .style(move |iced_theme: &Theme, status| {
            let style = toggler::default(iced_theme, status);
            toggler::Style {
                text_color: Some(theme.ink(0.75 * a)),
                ..style
            }
        })
}

/// Whether the user may flip `toggle` right now.
#[cfg(any(windows, target_os = "macos", test))]
pub fn toggle_enabled(settings: &Settings, toggle: SettingToggle, tray_ok: bool) -> bool {
    let dock_is_lifeline = toggle == SettingToggle::DockIcon && settings.is_on(toggle) && !tray_ok;
    settings.can_toggle(toggle) && !dock_is_lifeline
}

#[cfg(any(windows, target_os = "macos"))]
const HOTKEY_LABEL: &str = "Global hotkey (Cmd/Ctrl+Shift+Space)";

#[cfg(any(windows, target_os = "macos"))]
fn hotkey_error_label(reason: &str) -> String {
    format!("Couldn't register: {reason}")
}

#[cfg(target_os = "macos")]
fn toggle_label(toggle: SettingToggle) -> &'static str {
    match toggle {
        SettingToggle::MenuBarIcon => "Show menu bar icon",
        SettingToggle::DockIcon => "Show Dock icon",
        SettingToggle::GlobalHotkey => HOTKEY_LABEL,
        SettingToggle::AutoHide => AUTO_HIDE_LABEL,
    }
}

#[cfg(windows)]
fn toggle_label(toggle: SettingToggle) -> &'static str {
    match toggle {
        SettingToggle::MenuBarIcon => "Show tray icon",
        SettingToggle::DockIcon => "Show taskbar button",
        SettingToggle::GlobalHotkey => HOTKEY_LABEL,
        SettingToggle::AutoHide => AUTO_HIDE_LABEL,
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn app_section<'a>(
    settings: &Settings,
    tray_ok: bool,
    dock_forced: bool,
    hotkey_error: Option<&str>,
    theme: theme::Theme,
    a: f32,
) -> Element<'a, Message> {
    let mut col = column![group_header(SettingsGroup::App, theme, a)].spacing(space(2));
    for t in SettingToggle::ALL {
        let forced = t == SettingToggle::DockIcon && dock_forced;
        let mut switch = switch(settings.is_on(t) || forced, toggle_label(t), theme, a);
        if toggle_enabled(settings, t, tray_ok) && !forced {
            switch = switch.on_toggle(move |_| Message::SettingToggled(t));
        }
        col = col.push(switch);
        if let (SettingToggle::GlobalHotkey, Some(reason)) = (t, hotkey_error) {
            col = col.push(
                text(hotkey_error_label(reason))
                    .size(TEXT_XS)
                    .color(theme.danger(a)),
            );
        }
    }
    col.into()
}

fn palette_section<'a>(
    settings: &Settings,
    selected: Option<usize>,
    theme: theme::Theme,
    a: f32,
) -> Element<'a, Message> {
    const PER_ROW: usize = 6;
    let mut col = column![group_header(SettingsGroup::Palette, theme, a)].spacing(space(2));
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
                theme,
            )
        });
        col = col.push(row(slots).spacing(6.0));
    }
    if selected.is_some() {
        col = col.push(
            text("Replace with")
                .size(TEXT_XS)
                .color(theme.ink(0.55 * a)),
        );
        for chunk in PRESETS.chunks(12) {
            col = col.push(
                row(chunk
                    .iter()
                    .map(|c| swatch(*c, 18.0, false, Message::PaletteColorChosen(*c), theme)))
                .spacing(0),
            );
        }
    }
    col.into()
}

/// The tabs under the card's title, splitting its width: the active one in
/// full ink over a solid underline, the others muted over a faint one.
fn tab_bar<'a>(active: SettingsTab, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let tabs = SettingsTab::ALL.map(|tab| {
        let on = tab == active;
        let label = text(tab.label())
            .size(TEXT_SM)
            .font(if on {
                theme::TITLE_FONT
            } else {
                theme::BODY_FONT
            })
            .color(theme.ink(if on { a } else { 0.55 * a }))
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Center);
        let underline = container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(if on { 2.0 } else { 1.0 })
            .style(move |_theme: &Theme| container::Style {
                background: Some(theme.ink(if on { 0.8 * a } else { 0.12 * a }).into()),
                border: border::rounded(1.0),
                ..Default::default()
            });
        let tab_column = column![label, underline]
            .spacing(space(2))
            .width(Fill)
            .align_x(iced::Alignment::Center);
        pressable(
            tab_column,
            Padding::new(0.0).top(space(1)),
            Message::SettingsTabSelected(tab),
        )
        .width(Fill)
        .style(move |_theme: &Theme, status| button::Style {
            background: match status {
                button::Status::Hovered if !on => Some(theme.ink(0.06 * a).into()),
                button::Status::Pressed => Some(theme.ink(0.12 * a).into()),
                _ => None,
            },
            border: border::rounded(border::top(RADIUS_CONTROL)),
            ..Default::default()
        })
        .into()
    });
    row(tabs).into()
}

/// A solid quad under `content` that casts `shadow`.
fn solid_layer<'a>(
    content: impl Into<Element<'a, Message>>,
    color: Color,
    radius: f32,
    shadow: Shadow,
) -> container::Container<'a, Message> {
    container(content).style(move |_theme: &Theme| container::Style {
        background: Some(color.into()),
        border: border::rounded(radius),
        shadow,
        ..Default::default()
    })
}

pub fn settings_panel(v: SettingsView<'_>) -> Element<'_, Message> {
    let SettingsView {
        theme,
        settings,
        size,
        morph_progress: t,
        content_alpha: a,
        tab,
        selected_slot,
        tray_ok,
        dock_forced,
        hotkey_error,
        dragging,
    } = v;
    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let pill = container(Space::new().width(36).height(4)).style(move |_theme: &Theme| {
            container::Style {
                background: Some(theme.ink(0.3 * a).into()),
                border: border::rounded(2),
                ..Default::default()
            }
        });
        let grip = mouse_area(
            container(pill)
                .width(Fill)
                .height(GRIP_HEIGHT)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center),
        )
        .on_press(Message::SettingsDragStart)
        .on_double_click(Message::SettingsResetPosition)
        .interaction(if dragging {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::Grab
        });

        let header = row![
            text("Settings")
                .size(TEXT_MD)
                .font(theme::TITLE_FONT)
                .color(theme.ink(a)),
            Space::new().width(Fill),
            text_button(icon(Icon::Close, TEXT_SM), Message::CloseSettings, theme, a),
        ]
        .align_y(iced::Alignment::Center);

        let mut body = column![]
            .spacing(space(4))
            .padding(Padding::ZERO.right(space(5)));
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = (tray_ok, dock_forced, hotkey_error);
        for group in SettingsGroup::ALL.into_iter().filter(|g| g.tab() == tab) {
            let section = match group {
                // Only macOS and Windows have the menu bar / tray icon.
                #[cfg(any(windows, target_os = "macos"))]
                SettingsGroup::App => {
                    app_section(settings, tray_ok, dock_forced, hotkey_error, theme, a)
                }
                #[cfg(not(any(windows, target_os = "macos")))]
                SettingsGroup::App => continue,
                SettingsGroup::Palette => palette_section(settings, selected_slot, theme, a),
                SettingsGroup::Data => data_section(theme, a),
                _ => {
                    let mut section = column![group_header(group, theme, a)].spacing(space(2));
                    for key in SettingKey::ALL.into_iter().filter(|k| k.group() == group) {
                        section = section.push(slider_row(settings, key, theme, a));
                    }
                    if group == SettingsGroup::Motion {
                        section = section.push(reminder_alert_row(
                            settings.motion.reminder_alert,
                            theme,
                            a,
                        ));
                    }
                    if group == SettingsGroup::Window {
                        section = section.push(edge_row(settings.window.edge, theme, a));
                        let t = SettingToggle::AutoHide;
                        section = section.push(
                            switch(settings.is_on(t), AUTO_HIDE_LABEL, theme, a)
                                .on_toggle(move |_| Message::SettingToggled(t)),
                        );
                    }
                    section.into()
                }
            };
            body = body.push(section);
        }

        column![
            grip,
            header,
            tab_bar(tab, theme, a),
            scrollable(body).height(Fill)
        ]
        .spacing(space(3))
        .padding(Padding::new(space(4)).top(space(1)).right(space(1)))
        .into()
    };
    paper_card(inner, size, theme, t)
}

/// The card a panel's `inner` content sits on, `t` of the way through its
/// morph out of a strip slot: it starts as the slot's ink wash and settles
/// into paper with the note's band, highlight and shadows.
pub(crate) fn paper_card<'a>(
    inner: Element<'a, Message>,
    size: Size,
    theme: theme::Theme,
    t: f32,
) -> Element<'a, Message> {
    let paper = theme.card();
    let card = theme::mix(theme::over(theme.ink(0.8), paper), paper, t);
    let [contact, ambient] = theme.shadows(t);

    // Gradient quads draw no shadow, so each shadow sits on its own solid
    // layer under the gradient one, as on the open note.
    let radius = 3.0 + (RADIUS_SURFACE - 3.0) * t;
    let sheet = container(inner)
        .width(Length::Fixed(size.width))
        .height(Length::Fixed(size.height))
        .clip(true)
        .style(move |_theme: &Theme| container::Style {
            background: Some(theme.paper_gradient(card, 1.0).into()),
            border: border::rounded(radius),
            ..Default::default()
        });
    let highlight = container(container(Space::new().width(Fill).height(1)).style(
        move |_theme: &Theme| {
            container::Style {
                background: Some(
                    Color {
                        a: theme.highlight().a * t,
                        ..theme.highlight()
                    }
                    .into(),
                ),
                ..Default::default()
            }
        },
    ))
    .padding(Padding::ZERO.left(radius).right(radius))
    .width(Length::Fixed(size.width));
    // The adhesive band across the top, as on the note; it fades in with
    // the morph like the highlight.
    let band = container(Space::new().width(Fill).height(Fill))
        .width(Length::Fixed(size.width))
        .height(BAND_HEIGHT)
        .style(move |_theme: &Theme| container::Style {
            background: Some(
                Color {
                    a: theme.band().a * t,
                    ..theme.band()
                }
                .into(),
            ),
            border: border::rounded(border::top(radius)),
            ..Default::default()
        });
    let top = iced::widget::stack![sheet, band, highlight];
    solid_layer(
        solid_layer(top, card, radius, contact),
        card,
        radius,
        ambient,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{self, contrast, Mode};

    #[test]
    fn settings_text_is_readable_in_both_modes() {
        for mode in [Mode::Light, Mode::Dark] {
            let theme = theme::Theme::new(mode);
            let card = theme.card();
            assert!(contrast(theme.ink(1.0), card) >= 7.0, "{mode:?} ink");
            let muted = theme::over(theme.ink(0.55), card);
            assert!(contrast(muted, card) > 3.0, "{mode:?} muted");
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn hotkey_error_label_names_the_reason() {
        assert_eq!(
            hotkey_error_label("HotKey already registered"),
            "Couldn't register: HotKey already registered"
        );
    }

    #[test]
    fn danger_text_is_readable_in_both_modes() {
        for mode in [Mode::Light, Mode::Dark] {
            let theme = theme::Theme::new(mode);
            let ratio = contrast(theme.danger(1.0), theme.card());
            assert!(ratio >= 4.5, "{mode:?} danger: {ratio}");
        }
    }

    #[test]
    fn auto_hide_toggle_is_labelled_and_always_enabled() {
        assert_eq!(AUTO_HIDE_LABEL, "Auto-hide");
        let s = Settings::default();
        assert!(toggle_enabled(&s, SettingToggle::AutoHide, false));
    }

    #[test]
    fn window_group_shows_screen_edge_and_strip_length() {
        assert_eq!(SCREEN_EDGE_LABEL, "Screen edge");
        let labels: Vec<_> = Edge::ALL.into_iter().map(edge_label).collect();
        assert_eq!(labels, ["Right", "Left", "Top"]);
        assert_eq!(SettingKey::HeightFraction.label(), "Strip length");
        assert_eq!(SettingKey::HeightFraction.group(), SettingsGroup::Window);
    }

    #[test]
    fn dock_toggle_locked_without_tray() {
        let mut s = Settings::default();
        s.app.show_dock_icon = true;
        assert!(!toggle_enabled(&s, SettingToggle::DockIcon, false));
        assert!(toggle_enabled(&s, SettingToggle::MenuBarIcon, false));
        assert!(toggle_enabled(&s, SettingToggle::DockIcon, true));
    }

    #[test]
    fn last_icon_toggle_disabled() {
        let s = Settings::default();
        assert!(!s.app.show_dock_icon);
        assert!(!toggle_enabled(&s, SettingToggle::MenuBarIcon, true));
        assert!(toggle_enabled(&s, SettingToggle::DockIcon, true));
    }

    #[test]
    fn settings_data_group_has_export_button() {
        assert!(SettingsGroup::ALL.contains(&SettingsGroup::Data));
        assert_eq!(SettingsGroup::Data.label(), "Data");
        // Data holds nothing to reset.
        assert!(!SettingsGroup::Data.resettable());
        assert!(SettingsGroup::ALL
            .iter()
            .filter(|g| **g != SettingsGroup::Data)
            .all(|g| g.resettable()));
    }
}
