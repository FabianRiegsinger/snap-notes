#[cfg(target_os = "windows")]
pub fn enforce_single_instance() {
    let instance = single_instance::SingleInstance::new("snap-notes-a1b2c3d4").unwrap();
    if !instance.is_single() {
        std::process::exit(0);
    }
    std::mem::forget(instance);
}

#[cfg(not(target_os = "windows"))]
pub fn enforce_single_instance() {}

/// Picks a renderer setup that can draw a transparent window. On Windows only
/// DX12 with a DirectComposition swapchain can; the default HWND swapchain and
/// Vulkan paint the transparent parts black. Explicit settings in the
/// environment win.
#[cfg(target_os = "windows")]
pub fn prepare_graphics() {
    for (key, value) in [
        ("WGPU_BACKEND", "dx12"),
        ("WGPU_DX12_PRESENTATION_SYSTEM", "Visual"),
    ] {
        if std::env::var_os(key).is_none() {
            // Runs first in `main`, before any other thread exists.
            std::env::set_var(key, value);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn prepare_graphics() {}

/// Whether the window can let clicks fall through to the apps behind it while
/// still finding the cursor (see [`cursor_in_window`]). Where it can, the
/// window keeps one fixed size and never resizes, which avoids flicker.
pub const SUPPORTS_PASSTHROUGH: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(target_os = "macos")]
fn with_ns_window<R>(
    window: &dyn iced::window::Window,
    f: impl FnOnce(&objc2_app_kit::NSWindow) -> R,
) -> Option<R> {
    use raw_window_handle::RawWindowHandle;

    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: the handle comes from a live winit window, and `window::run`
    // callbacks execute on the main thread.
    let view: &objc2_app_kit::NSView = unsafe { appkit.ns_view.cast().as_ref() };
    view.window().map(|ns_window| f(&ns_window))
}

/// macOS draws its own shadow around the visible pixels of a transparent
/// window, which outlines the note's soft shadow with a dark frame and lags
/// behind the open/close morph. The app draws its own shadows, so turn it off.
#[cfg(target_os = "macos")]
pub fn disable_native_shadow(window: &dyn iced::window::Window) {
    with_ns_window(window, |ns_window| ns_window.setHasShadow(false));
}

#[cfg(not(target_os = "macos"))]
pub fn disable_native_shadow(_window: &dyn iced::window::Window) {}

/// Shows or hides the app's Dock icon. Hiding makes it an accessory app
/// (no Dock icon, no app menu); the window is brought back to the front
/// because switching the policy can push it behind other apps.
#[cfg(target_os = "macos")]
pub fn set_dock_icon_visible(window: &dyn iced::window::Window, visible: bool) {
    set_dock_policy(visible);
    with_ns_window(window, |ns_window| ns_window.orderFrontRegardless());
}

/// Switches the app between a regular app and an accessory app (no Dock
/// icon). Needs no window, so it can run at startup; off the main thread
/// it does nothing.
#[cfg(target_os = "macos")]
pub fn set_dock_policy(visible: bool) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
}

/// The taskbar button belongs to the window, so there is nothing to do
/// before it exists.
#[cfg(not(target_os = "macos"))]
pub fn set_dock_policy(_visible: bool) {}

/// Shows or hides the window's taskbar button. A tool window has none; the
/// window is hidden around the style change so the taskbar notices it.
#[cfg(windows)]
pub fn set_dock_icon_visible(window: &dyn iced::window::Window, visible: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
        SW_SHOWNOACTIVATE, WS_EX_TOOLWINDOW,
    };

    let Some(hwnd) = hwnd(window) else {
        return;
    };
    // SAFETY: style queries and updates on a live window handle, on the
    // thread that owns it.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let tool = WS_EX_TOOLWINDOW as isize;
        let wanted = if visible { style & !tool } else { style | tool };
        if wanted == style {
            return;
        }
        ShowWindow(hwnd, SW_HIDE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_dock_icon_visible(_window: &dyn iced::window::Window, _visible: bool) {}

/// Cursor position relative to the window's top-left corner in logical pixels,
/// read from the OS so it also works while mouse passthrough is on.
#[cfg(target_os = "macos")]
pub fn cursor_in_window(window: &dyn iced::window::Window) -> Option<iced::Point> {
    with_ns_window(window, |ns_window| {
        let height = ns_window.frame().size.height;
        let p = ns_window.mouseLocationOutsideOfEventStream();
        iced::Point::new(p.x as f32, (height - p.y) as f32)
    })
}

#[cfg(windows)]
fn hwnd(window: &dyn iced::window::Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::RawWindowHandle;

    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    Some(win32.hwnd.get() as windows_sys::Win32::Foundation::HWND)
}

#[cfg(windows)]
pub fn cursor_in_window(window: &dyn iced::window::Window) -> Option<iced::Point> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};

    let hwnd = hwnd(window)?;
    let mut cursor = POINT { x: 0, y: 0 };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: read-only Win32 queries on a live window handle.
    unsafe {
        if GetCursorPos(&mut cursor) == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let scale = GetDpiForWindow(hwnd).max(1) as f32 / 96.0;
        Some(iced::Point::new(
            (cursor.x - rect.left) as f32 / scale,
            (cursor.y - rect.top) as f32 / scale,
        ))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn cursor_in_window(_window: &dyn iced::window::Window) -> Option<iced::Point> {
    None
}

/// Hands `url` to the system's default opener, as a single argument and
/// never through a shell; failures are ignored.
#[cfg(target_os = "macos")]
pub fn open_url(url: &str) {
    spawn_and_reap(std::process::Command::new("open").arg(url));
}

/// Starts `command` and waits for it on a thread, so it never lingers as a
/// zombie; failures are ignored.
#[cfg(not(windows))]
fn spawn_and_reap(command: &mut std::process::Command) {
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Hands `url` to the system's default opener via `ShellExecuteW`, never
/// through `cmd`; failures are ignored.
#[cfg(windows)]
pub fn open_url(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (operation, file) = (wide("open"), wide(url));
    // SAFETY: both strings are NUL-terminated and outlive the call; the
    // other pointers may be null.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Hands `url` to the system's default opener, as a single argument and
/// never through a shell; failures are ignored.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn open_url(url: &str) {
    spawn_and_reap(std::process::Command::new("xdg-open").arg(url));
}
