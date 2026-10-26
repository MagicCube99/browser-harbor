#![cfg_attr(not(test), windows_subsystem = "windows")]

use std::path::PathBuf;

use browser_harbor::{
    app::AppState,
    config::ConfigStore,
    domain::settings::{Settings, WindowPosition},
    infra::{
        storage,
        windows::{
            single_instance::{SingleInstance, focus_window, show_error},
            window_position::{centered_window_position, visible_window_position},
        },
    },
    ui::{self, BrowserHarborApp},
};

fn main() -> eframe::Result {
    let _single_instance = match SingleInstance::acquire(r"Local\BrowserHarbor.SingleInstance") {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            focus_window(ui::APP_TITLE);
            return Ok(());
        }
        Err(error) => {
            show_error(
                ui::APP_TITLE,
                &format!("无法创建单实例保护，程序不会启动。\n\n{error}"),
            );
            return Ok(());
        }
    };
    let executable =
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("browser-harbor.exe"));
    let app_storage = storage::resolve(&executable, std::env::var_os("LOCALAPPDATA").as_deref());
    let migration_error = storage::migrate_legacy(&app_storage)
        .err()
        .map(|error| format!("迁移旧配置失败：{error}"));
    let store = ConfigStore::new(app_storage.config_path);
    let (settings, load_error) = match store.load() {
        Ok(settings) => (settings, None),
        Err(error) => (Settings::default(), Some(error.to_string())),
    };
    let compact_error = load_error
        .is_none()
        .then(|| store.compact_if_needed(&settings).err())
        .flatten()
        .map(|error| format!("整理旧配置失败：{error}"));
    let startup_error = migration_error.or(load_error).or(compact_error);
    let initial_theme = settings.theme;
    let initial_position = settings
        .window_position
        .map(|position| visible_window_position(position, [560, 420]))
        .unwrap_or_else(|| centered_window_position([560.0, 420.0]));
    let tool_root = executable
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let state = AppState::with_tool_root(settings, store, tool_root);
    let app = match startup_error {
        Some(error) => BrowserHarborApp::with_startup_error(state, error),
        None => BrowserHarborApp::new(state),
    };

    eframe::run_native(
        ui::APP_TITLE,
        app_native_options(initial_position),
        Box::new(move |cc| {
            ui::configure_context(&cc.egui_ctx, initial_theme);
            Ok(Box::new(app))
        }),
    )
}

fn app_native_options(position: WindowPosition) -> eframe::NativeOptions {
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_title(ui::APP_TITLE)
        .with_inner_size([560.0, 420.0])
        .with_min_inner_size([560.0, 420.0])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_icon(app_icon());
    let viewport = viewport.with_position([position.x as f32, position.y as f32]);
    eframe::NativeOptions {
        viewport,
        ..Default::default()
    }
}

fn app_icon() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::app_native_options;
    use browser_harbor::domain::settings::WindowPosition;
    use eframe::egui::{pos2, vec2};

    #[test]
    fn window_defaults_are_compact_and_fixed() {
        let options = app_native_options(WindowPosition { x: 120, y: 80 });

        assert_eq!(options.viewport.inner_size, Some(vec2(560.0, 420.0)));
        assert_eq!(options.viewport.min_inner_size, Some(vec2(560.0, 420.0)));
        assert_eq!(options.viewport.resizable, Some(false));
        assert_eq!(options.viewport.maximize_button, Some(false));
        let icon = options.viewport.icon.expect("application icon");
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        let (pixels, _) = icon.rgba.as_chunks::<4>();
        assert!(pixels.iter().any(|pixel| pixel[3] > 0));
        assert!(pixels.iter().any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn window_uses_provided_position() {
        let options = app_native_options(WindowPosition { x: 120, y: 80 });

        assert_eq!(options.viewport.position, Some(pos2(120.0, 80.0)));
    }
}
