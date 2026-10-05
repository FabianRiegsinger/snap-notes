//! The global hotkey (Cmd+Shift+Space on macOS, Ctrl+Shift+Space elsewhere)
//! that adds a note from any app. The OS hook must be made on the thread
//! running the event loop, which is the thread `App::update` runs on.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// The registered hotkey. Dropping it unregisters the hotkey.
pub struct Manager {
    _manager: GlobalHotKeyManager,
}

fn hotkey() -> HotKey {
    let command = if cfg!(target_os = "macos") {
        Modifiers::SUPER
    } else {
        Modifiers::CONTROL
    };
    HotKey::new(Some(command | Modifiers::SHIFT), Code::Space)
}

/// Registers the hotkey. On failure (another app holds it, or there is no
/// X11 display on Linux) the error goes to stderr and nothing else changes.
pub fn register() -> Option<Manager> {
    // The Linux backend only speaks X11; without a display it would fail
    // silently on its own thread.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("global hotkey unavailable: it needs an X11 display");
        return None;
    }
    let result = GlobalHotKeyManager::new().and_then(|manager| {
        manager.register(hotkey())?;
        Ok(manager)
    });
    match result {
        Ok(manager) => Some(Manager { _manager: manager }),
        Err(e) => {
            eprintln!("could not register the global hotkey: {e}");
            None
        }
    }
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
