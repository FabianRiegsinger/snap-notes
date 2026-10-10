//! The global hotkey (by default Cmd+Shift+Space on macOS, Ctrl+Shift+Space
//! elsewhere) that adds a note from any app. The OS hook must be made on the
//! thread running the event loop, which is the thread `App::update` runs on.

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::fmt;

/// Keys a combo may end in besides letters and digits, by their W3C code
/// name (as iced and the hotkey crate spell them).
const NAMED_KEYS: [&str; 39] = [
    "Space",
    "Enter",
    "Tab",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "Minus",
    "Equal",
    "Comma",
    "Period",
    "Slash",
    "Backslash",
    "Semicolon",
    "Quote",
    "Backquote",
    "BracketLeft",
    "BracketRight",
    "NumpadAdd",
    "NumpadSubtract",
];

/// The modifiers of a combo. `primary` is Cmd on macOS and Ctrl elsewhere;
/// `ctrl` is the Control key on macOS (elsewhere it is the same key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub primary: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// A global hotkey: modifiers plus one key, saved in a readable form such
/// as "CmdOrCtrl+Shift+Space".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    mods: Mods,
    /// Canonical key name: "Q", "7", "Space", "F5", ...
    key: String,
}

impl Default for Combo {
    fn default() -> Self {
        Self {
            mods: Mods {
                primary: true,
                shift: true,
                ..Mods::default()
            },
            key: "Space".to_string(),
        }
    }
}

/// `name` as a combo's key ("KeyQ" and "q" are "Q", "Digit7" is "7"), if
/// it is one a hotkey can use.
fn canonical_key(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let short = lower
        .strip_prefix("key")
        .or_else(|| lower.strip_prefix("digit"))
        .unwrap_or(&lower);
    let mut chars = short.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
    }
    NAMED_KEYS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(name))
        .map(|k| k.to_string())
}

impl Combo {
    /// A key pressed with `mods`, if it makes a usable hotkey: at least one
    /// modifier and a supported key (not a modifier key itself, not Esc).
    pub fn from_press(mods: Mods, key: &str) -> Option<Combo> {
        if mods == Mods::default() {
            return None;
        }
        Some(Combo {
            mods,
            key: canonical_key(key)?,
        })
    }

    /// Reads a saved combo: modifiers then one key, joined by "+", in any
    /// case and spacing. `None` when it isn't a usable hotkey.
    pub fn parse(s: &str) -> Option<Combo> {
        let mut tokens: Vec<&str> = s.split('+').map(str::trim).collect();
        let key = tokens.pop()?;
        let mut mods = Mods::default();
        for token in tokens {
            let flag = match token.to_ascii_lowercase().as_str() {
                "cmdorctrl" | "cmdorcontrol" | "commandorctrl" | "commandorcontrol" => {
                    &mut mods.primary
                }
                "ctrl" | "control" => &mut mods.ctrl,
                "alt" | "option" => &mut mods.alt,
                "shift" => &mut mods.shift,
                _ => return None,
            };
            *flag = true;
        }
        Combo::from_press(mods, key)
    }

    /// The saved combo, or the default when it is missing or unusable.
    pub fn parse_or_default(s: &str) -> Combo {
        Combo::parse(s).unwrap_or_default()
    }

    /// How Settings shows it: symbols on macOS ("⌘⇧Space"), words elsewhere
    /// ("Ctrl+Shift+Space").
    pub fn label(&self, mac: bool) -> String {
        let m = self.mods;
        let names: [(bool, &str, &str); 4] = [
            (m.primary, "⌘", "Ctrl"),
            (m.ctrl, "⌃", "Ctrl"),
            (m.alt, "⌥", "Alt"),
            (m.shift, "⇧", "Shift"),
        ];
        let held = names.iter().filter(|(on, ..)| *on);
        if mac {
            let symbols: String = held.map(|(_, symbol, _)| *symbol).collect();
            format!("{symbols}{}", self.key)
        } else {
            let mut parts: Vec<&str> = held.map(|(.., word)| *word).collect();
            parts.push(&self.key);
            parts.join("+")
        }
    }

    /// The combo for the hotkey crate.
    pub fn hotkey(&self) -> Result<HotKey, String> {
        self.to_string().parse().map_err(|e| format!("{e}"))
    }
}

/// The saved form, e.g. "CmdOrCtrl+Shift+Space".
impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.mods;
        for (on, name) in [
            (m.primary, "CmdOrCtrl"),
            (m.ctrl, "Ctrl"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&self.key)
    }
}

/// The registered hotkey. Dropping it unregisters the hotkey.
pub struct Manager {
    _manager: GlobalHotKeyManager,
    /// The combo it holds.
    pub combo: Combo,
}

/// Registers `combo`. On failure (another app holds it, or there is no
/// X11 display on Linux) the reason goes to stderr and is returned, so
/// Settings can show it.
pub fn register(combo: &Combo) -> Result<Manager, String> {
    // The Linux backend only speaks X11; without a display it would fail
    // silently on its own thread.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() {
        let reason = "it needs an X11 display".to_string();
        eprintln!("global hotkey unavailable: {reason}");
        return Err(reason);
    }
    let hotkey = combo.hotkey()?;
    GlobalHotKeyManager::new()
        .and_then(|manager| {
            manager.register(hotkey)?;
            Ok(manager)
        })
        .map(|manager| Manager {
            _manager: manager,
            combo: combo.clone(),
        })
        .map_err(|e| {
            eprintln!("could not register the global hotkey {combo}: {e}");
            e.to_string()
        })
}

/// One item per hotkey press, pushed as it happens so the app can sleep
/// while idle instead of polling.
pub fn events() -> iced::Subscription<()> {
    iced::Subscription::run(event_stream)
}

fn event_stream() -> impl iced::futures::Stream<Item = ()> {
    iced::stream::channel(4, async |sender| {
        let sender = std::sync::Mutex::new(sender);
        // The crate keeps only the first handler it is given, so this
        // subscription must stay alive for the whole session.
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                if let Ok(mut sender) = sender.lock() {
                    let _ = sender.try_send(());
                }
            }
        }));
        // The handler feeds the channel; keep the stream alive.
        std::future::pending::<()>().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_cmd_or_ctrl_shift_space() {
        let combo = Combo::default();
        assert_eq!(combo.to_string(), "CmdOrCtrl+Shift+Space");
        assert_eq!(combo.label(true), "⌘⇧Space");
        assert_eq!(combo.label(false), "Ctrl+Shift+Space");
    }

    #[test]
    fn parse_and_format_round_trip() {
        for s in [
            "CmdOrCtrl+Shift+Space",
            "Ctrl+Alt+Q",
            "CmdOrCtrl+Ctrl+Alt+Shift+F5",
            "Alt+1",
            "Shift+ArrowUp",
            "CmdOrCtrl+Backquote",
        ] {
            let combo = Combo::parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(combo.to_string(), s);
        }
    }

    #[test]
    fn parse_is_lenient_about_case_spacing_and_aliases() {
        let combo = Combo::parse(" shift + commandorcontrol + keyq ").unwrap();
        assert_eq!(combo.to_string(), "CmdOrCtrl+Shift+Q");
        let combo = Combo::parse("Option+Control+digit7").unwrap();
        assert_eq!(combo.to_string(), "Ctrl+Alt+7");
    }

    #[test]
    fn invalid_combos_fall_back_to_the_default() {
        for s in [
            "",
            "Space",
            "Shift",
            "Shift+",
            "CmdOrCtrl+Q+W",
            "CmdOrCtrl+Bogus",
            "Hyper+Q",
            "CmdOrCtrl+Escape",
        ] {
            assert_eq!(Combo::parse(s), None, "{s:?}");
            assert_eq!(Combo::parse_or_default(s), Combo::default(), "{s:?}");
        }
    }

    #[test]
    fn mac_labels_use_symbols_and_others_use_words() {
        let combo = Combo::parse("CmdOrCtrl+Ctrl+Alt+Shift+K").unwrap();
        assert_eq!(combo.label(true), "⌘⌃⌥⇧K");
        assert_eq!(combo.label(false), "Ctrl+Ctrl+Alt+Shift+K");
        let combo = Combo::parse("Alt+F2").unwrap();
        assert_eq!(combo.label(true), "⌥F2");
        assert_eq!(combo.label(false), "Alt+F2");
    }

    #[test]
    fn a_press_needs_a_modifier_and_a_supported_key() {
        let mods = Mods {
            primary: true,
            shift: true,
            ..Mods::default()
        };
        let combo = Combo::from_press(mods, "KeyQ").unwrap();
        assert_eq!(combo.to_string(), "CmdOrCtrl+Shift+Q");
        assert_eq!(Combo::from_press(Mods::default(), "KeyQ"), None);
        assert_eq!(Combo::from_press(mods, "ShiftLeft"), None);
        assert_eq!(Combo::from_press(mods, "Escape"), None);
    }

    #[test]
    fn every_combo_maps_to_a_global_hotkey() {
        for s in [
            "CmdOrCtrl+Shift+Space",
            "Ctrl+Alt+Q",
            "Alt+Shift+Backslash",
            "Shift+F12",
        ] {
            let combo = Combo::parse(s).unwrap();
            assert!(combo.hotkey().is_ok(), "{s}");
        }
        use global_hotkey::hotkey::{Code, Modifiers};
        let command = if cfg!(target_os = "macos") {
            Modifiers::SUPER
        } else {
            Modifiers::CONTROL
        };
        let space = HotKey::new(Some(command | Modifiers::SHIFT), Code::Space);
        assert_eq!(Combo::default().hotkey().unwrap(), space);
    }
}
