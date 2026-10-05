//! System notifications.

/// Shows a reminder notification for `body` (the note's title) without
/// blocking the caller. Failures are logged. Tests never show one.
pub fn reminder(body: &str) {
    #[cfg(not(test))]
    {
        let body = if body.trim().is_empty() {
            "Reminder".to_string()
        } else {
            body.to_string()
        };
        std::thread::spawn(move || {
            if let Err(e) = notify_rust::Notification::new()
                .summary("Snap Notes")
                .body(&body)
                .show()
            {
                eprintln!("could not show the reminder notification: {e}");
            }
        });
    }
    #[cfg(test)]
    let _ = body;
}
