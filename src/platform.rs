#[cfg(target_os = "windows")]
pub fn enforce_single_instance() {
    let instance = single_instance::SingleInstance::new("work-notes-a1b2c3d4").unwrap();
    if !instance.is_single() {
        std::process::exit(0);
    }
    std::mem::forget(instance);
}

#[cfg(not(target_os = "windows"))]
pub fn enforce_single_instance() {}

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
pub fn cursor_in_window(window: &dyn iced::window::Window) -> Option<iced::Point> {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};

    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    let hwnd = win32.hwnd.get() as HWND;
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
