//! Primary-monitor placement for the first application launch.

use windows_sys::Win32::{
    UI::HiDpi::GetDpiForSystem,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
        WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
    },
};

use crate::domain::settings::WindowPosition;

pub fn centered_window_position(inner_size: [f32; 2]) -> WindowPosition {
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let scale = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32 / scale;
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32 / scale;
    unsafe {
        SetThreadDpiAwarenessContext(previous);
    }
    WindowPosition {
        x: ((screen_width - inner_size[0]) / 2.0).round() as i32,
        y: ((screen_height - inner_size[1]) / 2.0).round() as i32,
    }
}
