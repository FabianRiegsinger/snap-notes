//! Start at login: a per-user OS entry that launches this executable. macOS
//! uses a LaunchAgent, Windows the `Run` registry key and Linux an XDG
//! autostart entry. The entry names the running executable, so re-applying
//! it after an update or a move keeps it pointing at the right file.

use std::io;
use std::path::{Path, PathBuf};

/// Adds or removes the login entry for the running executable.
pub fn set(on: bool) -> io::Result<()> {
    // Tests flip the setting; they must not touch the real login items.
    if cfg!(test) {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    #[cfg(windows)]
    {
        set_run_key(on, &exe)
    }
    #[cfg(not(windows))]
    {
        let entry = entry_path(|key| std::env::var_os(key))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
        set_file(on, &entry, &entry_contents(&exe))
    }
}

/// Writes `contents` to `entry`, or removes it; a missing entry is already off.
#[cfg_attr(windows, allow(dead_code))]
fn set_file(on: bool, entry: &Path, contents: &str) -> io::Result<()> {
    if on {
        if let Some(dir) = entry.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(entry, contents)
    } else {
        match std::fs::remove_file(entry) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

/// Where this platform looks for the login entry, read from `env`.
#[cfg_attr(windows, allow(dead_code))]
fn entry_path(env: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(PathBuf::from(env("HOME")?).join(format!("Library/LaunchAgents/{LABEL}.plist")))
    } else {
        let base = env("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("autostart/snap-notes.desktop"))
    }
}

#[cfg_attr(windows, allow(dead_code))]
const LABEL: &str = "io.github.fabianriegsinger.snap-notes";

/// The LaunchAgent plist (macOS) or autostart desktop entry (Linux).
#[cfg_attr(windows, allow(dead_code))]
fn entry_contents(exe: &Path) -> String {
    let exe = exe.to_string_lossy();
    if cfg!(target_os = "macos") {
        let exe = exe
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#
        )
    } else {
        // Desktop entry Exec quoting: these four are backslash-escaped.
        let mut quoted = String::new();
        for c in exe.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        format!(
            "[Desktop Entry]\nType=Application\nName=Snap Notes\nExec=\"{quoted}\"\nIcon=snap-notes\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        )
    }
}

#[cfg(windows)]
fn set_run_key(on: bool, exe: &Path) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
    };

    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide("Snap Notes");
    // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that outlives the call.
    let status = unsafe {
        if on {
            let value = wide(&format!("\"{}\"", exe.display()));
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr())
        }
    };
    match status {
        ERROR_SUCCESS => Ok(()),
        ERROR_FILE_NOT_FOUND if !on => Ok(()),
        code => Err(io::Error::from_raw_os_error(code as i32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_file_is_written_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("autostart/snap-notes.desktop");
        set_file(true, &entry, "x").unwrap();
        assert_eq!(std::fs::read_to_string(&entry).unwrap(), "x");
        set_file(false, &entry, "x").unwrap();
        assert!(!entry.exists());
        // Turning off twice is fine.
        set_file(false, &entry, "x").unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_agent_escapes_the_path() {
        let plist = entry_contents(Path::new(
            "/Applications/A & B.app/Contents/MacOS/snap-notes",
        ));
        assert!(plist
            .contains("<string>/Applications/A &amp; B.app/Contents/MacOS/snap-notes</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        let path = entry_path(|k| (k == "HOME").then(|| "/Users/me".into())).unwrap();
        assert_eq!(
            path,
            Path::new("/Users/me/Library/LaunchAgents/io.github.fabianriegsinger.snap-notes.plist")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn autostart_entry_quotes_the_path() {
        let entry = entry_contents(Path::new("/opt/my \"notes\"/snap-notes"));
        assert!(entry.contains("Exec=\"/opt/my \\\"notes\\\"/snap-notes\"\n"));
        let path = entry_path(|k| (k == "HOME").then(|| "/home/me".into())).unwrap();
        assert_eq!(
            path,
            Path::new("/home/me/.config/autostart/snap-notes.desktop")
        );
    }
}
