//! Primary-monitor placement for the first application launch.

use windows_sys::Win32::{
    UI::HiDpi::GetDpiForSystem,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext},
        WindowsAndMessaging::{
            GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
            SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        },
    },
};

use crate::domain::settings::WindowPosition;

#[derive(Debug, Clone, Copy)]
struct VirtualScreen {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

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

pub fn visible_window_position(position: WindowPosition, inner_size: [i32; 2]) -> WindowPosition {
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let scale = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
    let screen = VirtualScreen {
        left: (unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } as f32 / scale).round() as i32,
        top: (unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) } as f32 / scale).round() as i32,
        width: (unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) } as f32 / scale).round() as i32,
        height: (unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) } as f32 / scale).round() as i32,
    };
    unsafe {
        SetThreadDpiAwarenessContext(previous);
    }
    clamp_to_screen(position, inner_size, screen)
}

fn clamp_to_screen(
    position: WindowPosition,
    inner_size: [i32; 2],
    screen: VirtualScreen,
) -> WindowPosition {
    if screen.width <= 0 || screen.height <= 0 {
        return position;
    }
    let max_x = (screen.left + screen.width - inner_size[0]).max(screen.left);
    let max_y = (screen.top + screen.height - inner_size[1]).max(screen.top);
    WindowPosition {
        x: position.x.clamp(screen.left, max_x),
        y: position.y.clamp(screen.top, max_y),
    }
}

#[cfg(test)]
mod tests {
    use super::{VirtualScreen, clamp_to_screen};
    use crate::domain::settings::WindowPosition;

    #[test]
    fn saved_window_position_is_clamped_into_the_virtual_screen() {
        let position = WindowPosition { x: 5000, y: -4000 };
        let screen = VirtualScreen {
            left: -1920,
            top: 0,
            width: 3840,
            height: 1080,
        };

        let clamped = clamp_to_screen(position, [560, 420], screen);

        assert_eq!(clamped, WindowPosition { x: 1360, y: 0 });
    }

    #[test]
    fn visible_window_position_is_preserved() {
        let position = WindowPosition { x: -800, y: 100 };
        let screen = VirtualScreen {
            left: -1920,
            top: 0,
            width: 3840,
            height: 1080,
        };

        assert_eq!(clamp_to_screen(position, [560, 420], screen), position);
    }
}
