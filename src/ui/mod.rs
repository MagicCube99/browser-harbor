//! egui application shell.

mod localization;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Layout, Margin, Stroke, TextStyle,
};

use localization::{
    LocalizedResponse as _, LocalizedUi as _, RichText, localized, localized_button, select_label,
    set_language,
};

use crate::{
    app::{
        AppState,
        operation::{
            CachedReleaseMetadata, MetadataSource, OperationController, OperationMode,
            OperationPhase, StartOperationRequest, UpstreamCheck, start_operation,
        },
    },
    domain::{
        release::{CpuBuild, ProviderKind, ReleaseChannel},
        settings::{Language, ThemeMode, WindowPosition},
        target::{BrowserFamily, ChromePlusMode, SupportLevel, TargetId},
        version::{LocalVersion, VersionRelation, VersionSource, compare_versions},
    },
    infra::windows::{
        folder_picker, process::running_process_count_in_directory,
        shortcut::create_browser_shortcut,
    },
    services::{
        download::{clear_download_cache, download_cache_usage},
        health::{
            HealthAspect, HealthReport, HealthState, ProgramIdentity, inspect,
            inspect_program_identity, relocate, repair_chromeplus_paths, repair_portable_paths,
        },
        version_probe::{VersionProbeError, locate_executable, probe_local_version},
    },
};

pub const APP_TITLE: &str = "Browser Harbor";
pub const BODY_FONT_SIZE: f32 = 12.5;
const FIELD_LABEL_WIDTH: f32 = 116.0;
const RELEASES_URL: &str = "https://github.com/MagicCube99/browser-harbor/releases";
const CHROME_PLUS_SOURCE_URL: &str = "https://github.com/Bush2021/chrome_plus";

#[derive(Clone, Copy)]
struct ThemePalette {
    canvas: Color32,
    panel: Color32,
    surface: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    control: Color32,
    hover: Color32,
    accent: Color32,
    accent_soft: Color32,
    accent_text: Color32,
    success: Color32,
    success_soft: Color32,
    warning: Color32,
    warning_soft: Color32,
    error: Color32,
    error_soft: Color32,
}

const LIGHT_PALETTE: ThemePalette = ThemePalette {
    canvas: Color32::from_rgb(0xF3, 0xF4, 0xF6),
    panel: Color32::from_rgb(0xF8, 0xF9, 0xFB),
    surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    border: Color32::from_rgb(0xDF, 0xE3, 0xE8),
    text: Color32::from_rgb(0x1F, 0x23, 0x28),
    muted: Color32::from_rgb(0x63, 0x6B, 0x75),
    control: Color32::from_rgb(0xFC, 0xFD, 0xFE),
    hover: Color32::from_rgb(0xEE, 0xF4, 0xFA),
    accent: Color32::from_rgb(0x00, 0x67, 0xC0),
    accent_soft: Color32::from_rgb(0xDF, 0xEF, 0xFC),
    accent_text: Color32::from_rgb(0x00, 0x3E, 0x73),
    success: Color32::from_rgb(0x0F, 0x7B, 0x0F),
    success_soft: Color32::from_rgb(0xE8, 0xF7, 0xE8),
    warning: Color32::from_rgb(0x8A, 0x4B, 0x00),
    warning_soft: Color32::from_rgb(0xFF, 0xF4, 0xD6),
    error: Color32::from_rgb(0xB4, 0x23, 0x18),
    error_soft: Color32::from_rgb(0xFD, 0xEC, 0xEA),
};

const DARK_PALETTE: ThemePalette = ThemePalette {
    canvas: Color32::from_rgb(0x1E, 0x1F, 0x22),
    panel: Color32::from_rgb(0x25, 0x26, 0x2A),
    surface: Color32::from_rgb(0x2B, 0x2D, 0x31),
    border: Color32::from_rgb(0x3A, 0x3D, 0x42),
    text: Color32::from_rgb(0xF2, 0xF3, 0xF5),
    muted: Color32::from_rgb(0xA6, 0xAC, 0xB5),
    control: Color32::from_rgb(0x30, 0x33, 0x39),
    hover: Color32::from_rgb(0x38, 0x3E, 0x46),
    accent: Color32::from_rgb(0x4C, 0xC2, 0xFF),
    accent_soft: Color32::from_rgb(0x17, 0x3B, 0x56),
    accent_text: Color32::from_rgb(0xBC, 0xEB, 0xFF),
    success: Color32::from_rgb(0x6C, 0xCB, 0x5F),
    success_soft: Color32::from_rgb(0x18, 0x35, 0x1E),
    warning: Color32::from_rgb(0xF3, 0xC6, 0x77),
    warning_soft: Color32::from_rgb(0x3B, 0x2F, 0x1A),
    error: Color32::from_rgb(0xFF, 0xB4, 0xAB),
    error_soft: Color32::from_rgb(0x3F, 0x23, 0x24),
};

pub fn configure_context(ctx: &egui::Context, theme_mode: ThemeMode) {
    configure_fonts(ctx);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(15.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(BODY_FONT_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(BODY_FONT_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(10.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(7.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size = egui::vec2(28.0, 24.0);
        style.spacing.window_margin = Margin::same(8);
        style.spacing.menu_margin = Margin::same(6);
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating_width = 4.0;
        style.visuals = windows_11_visuals(theme);
        ctx.set_style_of(theme, style);
    }
    apply_theme_preference(ctx, theme_mode);
}

pub fn apply_theme_preference(ctx: &egui::Context, theme: ThemeMode) {
    ctx.set_theme(match theme {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
    });
}

fn palette(ui: &egui::Ui) -> ThemePalette {
    match ui.ctx().theme() {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    }
}

fn theme_label(theme: ThemeMode) -> &'static str {
    match theme {
        ThemeMode::System => "跟随系统",
        ThemeMode::Light => "浅色",
        ThemeMode::Dark => "深色",
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::empty();
    let font_dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    let mut proportional = Vec::new();
    if let Some(bytes) = load_first_font(&font_dir, &["SegUIVar.ttf", "segoeui.ttf"]) {
        fonts
            .font_data
            .insert("Segoe UI".to_owned(), Arc::new(FontData::from_owned(bytes)));
        proportional.push("Segoe UI".to_owned());
    }
    if let Some(bytes) = load_first_font(&font_dir, &["msyh.ttc", "msyh.ttf", "simsun.ttc"]) {
        fonts.font_data.insert(
            "Microsoft YaHei".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        proportional.push("Microsoft YaHei".to_owned());
    }
    if proportional.is_empty()
        && let Some(bytes) = load_first_font(&font_dir, &["arial.ttf", "tahoma.ttf"])
    {
        fonts.font_data.insert(
            "Windows UI".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        proportional.push("Windows UI".to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Proportional, proportional.clone());

    let mut monospace = Vec::new();
    if let Some(bytes) = load_first_font(&font_dir, &["consola.ttf"]) {
        fonts
            .font_data
            .insert("Consolas".to_owned(), Arc::new(FontData::from_owned(bytes)));
        monospace.push("Consolas".to_owned());
    }
    monospace.extend(proportional);
    fonts.families.insert(FontFamily::Monospace, monospace);
    ctx.set_fonts(fonts);
}

fn load_first_font(font_dir: &std::path::Path, names: &[&str]) -> Option<Vec<u8>> {
    names
        .iter()
        .find_map(|name| std::fs::read(font_dir.join(name)).ok())
}

fn windows_11_visuals(theme: egui::Theme) -> egui::Visuals {
    let palette = match theme {
        egui::Theme::Light => LIGHT_PALETTE,
        egui::Theme::Dark => DARK_PALETTE,
    };
    let mut visuals = match theme {
        egui::Theme::Light => egui::Visuals::light(),
        egui::Theme::Dark => egui::Visuals::dark(),
    };
    let radius = CornerRadius::same(6);
    visuals.weak_text_color = Some(palette.muted);
    visuals.panel_fill = palette.canvas;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.extreme_bg_color = palette.control;
    visuals.text_edit_bg_color = Some(palette.surface);
    visuals.faint_bg_color = palette.panel;
    visuals.warn_fg_color = palette.warning;
    visuals.error_fg_color = palette.error;
    visuals.hyperlink_color = palette.accent;
    visuals.selection.bg_fill = palette.accent_soft;
    visuals.selection.stroke = Stroke::new(1.0, palette.accent_text);
    visuals.interact_cursor = None;
    visuals.disabled_alpha = 0.42;
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;

    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.noninteractive.weak_bg_fill = palette.surface;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = palette.control;
    visuals.widgets.inactive.weak_bg_fill = palette.control;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = palette.hover;
    visuals.widgets.hovered.weak_bg_fill = palette.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = palette.accent_soft;
    visuals.widgets.active.weak_bg_fill = palette.accent_soft;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.accent_text);
    visuals.widgets.active.corner_radius = radius;

    visuals.widgets.open = visuals.widgets.active;
    visuals
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum DetailTab {
    #[default]
    Overview,
    Network,
    ChromePlus,
}

pub struct BrowserHarborApp {
    state: AppState,
    active_tab: DetailTab,
    startup_error: Option<String>,
    status_message: Option<String>,
    latest_versions: BTreeMap<(TargetId, ReleaseChannel, CpuBuild), UpstreamCheck>,
    checked_releases: BTreeMap<(TargetId, ReleaseChannel, CpuBuild), CachedReleaseMetadata>,
    auto_checked_keys: BTreeSet<(TargetId, ReleaseChannel, CpuBuild)>,
    auto_check_enabled: bool,
    chrome_plus_latest: BTreeMap<TargetId, String>,
    chrome_plus_versions: BTreeMap<TargetId, Option<String>>,
    chrome_plus_ini_loaded: BTreeSet<TargetId>,
    local_versions: BTreeMap<TargetId, LocalVersionState>,
    health_dialog: Option<HealthDialog>,
    same_version_prompt: Option<SameVersionPrompt>,
    chrome_plus_update_prompt: Option<ChromePlusUpdatePrompt>,
    browser_upgrade_prompt: Option<BrowserUpgradePrompt>,
    program_identity: ProgramIdentity,
    identity_prompt: Option<IdentityPrompt>,
    operation: Option<OperationController>,
    operation_phase: Option<OperationPhase>,
    accelerator_input: String,
    observed_window_position: Option<WindowPosition>,
    pending_window_position: Option<WindowPosition>,
    window_position_changed_at: Option<Instant>,
}

#[derive(Debug, Clone)]
enum LocalVersionState {
    Detected(LocalVersion),
    MissingDirectory(PathBuf),
    Incomplete(PathBuf),
    Unavailable(String),
}

#[derive(Debug, Clone)]
struct SameVersionPrompt {
    current_version: String,
    upstream: UpstreamCheck,
}

#[derive(Debug, Clone)]
struct ChromePlusUpdatePrompt {
    target: TargetId,
    current_version: Option<String>,
    latest_version: String,
}

#[derive(Debug, Clone)]
struct BrowserUpgradePrompt {
    target: TargetId,
    current_browser: String,
    upstream_browser: String,
    chrome_plus_current: String,
    chrome_plus_latest: String,
    replace_same_version: bool,
}

#[derive(Debug, Clone)]
struct IdentityPrompt {
    target: TargetId,
    actual: TargetId,
    product_name: String,
    replace_same_version: bool,
}

#[derive(Debug, Clone)]
struct HealthDialog {
    target: TargetId,
    report: HealthReport,
    pending_relocation: Option<crate::domain::settings::TargetConfig>,
    relocation_warning: Option<String>,
    confirming_repair: bool,
}

impl BrowserHarborApp {
    pub fn new(state: AppState) -> Self {
        let observed_window_position = state.draft().window_position;
        let mut app = Self {
            state,
            active_tab: DetailTab::Overview,
            startup_error: None,
            status_message: None,
            latest_versions: BTreeMap::new(),
            checked_releases: BTreeMap::new(),
            auto_checked_keys: BTreeSet::new(),
            auto_check_enabled: !cfg!(test),
            chrome_plus_latest: BTreeMap::new(),
            chrome_plus_versions: BTreeMap::new(),
            chrome_plus_ini_loaded: BTreeSet::new(),
            local_versions: BTreeMap::new(),
            health_dialog: None,
            same_version_prompt: None,
            chrome_plus_update_prompt: None,
            browser_upgrade_prompt: None,
            program_identity: ProgramIdentity::NoExecutable,
            identity_prompt: None,
            operation: None,
            operation_phase: None,
            accelerator_input: String::new(),
            observed_window_position,
            pending_window_position: None,
            window_position_changed_at: None,
        };
        let target = app.state.selected_target();
        app.refresh_local_version(target);
        app.refresh_program_identity(target);
        app
    }

    pub fn with_startup_error(state: AppState, error: impl Into<String>) -> Self {
        Self {
            startup_error: Some(error.into()),
            ..Self::new(state)
        }
    }

    #[cfg(test)]
    pub fn state(&self) -> &AppState {
        &self.state
    }

    fn refresh_local_version(&mut self, target: TargetId) {
        let config = self.state.draft().targets[&target].clone();
        let state = match self.state.resolve_operation_paths(config) {
            Err(error) => LocalVersionState::Unavailable(error.to_string()),
            Ok(config) => {
                let Some(root) = config.browser_root.as_deref() else {
                    self.local_versions.insert(
                        target,
                        LocalVersionState::Unavailable("浏览器根目录未配置".to_owned()),
                    );
                    return;
                };
                if !root.is_dir() {
                    LocalVersionState::MissingDirectory(root.to_path_buf())
                } else if let Some(program) = config.browser_directory.as_deref() {
                    if !program.is_dir() {
                        LocalVersionState::MissingDirectory(program.to_path_buf())
                    } else {
                        match probe_local_version(target, program) {
                            Ok(probe) => LocalVersionState::Detected(LocalVersion {
                                version: probe.version,
                                source: probe.source,
                            }),
                            Err(VersionProbeError::ExecutableNotFound(path)) => {
                                LocalVersionState::Incomplete(path)
                            }
                            Err(error) => LocalVersionState::Unavailable(error.to_string()),
                        }
                    }
                } else {
                    LocalVersionState::Unavailable("程序目录未配置".to_owned())
                }
            }
        };
        self.local_versions.insert(target, state);
    }

    fn refresh_program_identity(&mut self, target: TargetId) {
        let config = self.state.draft().targets[&target].clone();
        self.program_identity = self
            .state
            .resolve_operation_paths(config)
            .ok()
            .and_then(|config| config.browser_directory)
            .map_or(ProgramIdentity::NoExecutable, |program| {
                inspect_program_identity(target, &program)
            });
    }

    fn open_health_dialog(&mut self) {
        let target = self.state.selected_target();
        let config = self.state.draft().targets[&target].clone();
        let resolved = match self.state.resolve_operation_paths(config) {
            Ok(config) => config,
            Err(error) => {
                self.status_message = Some(format!("无法检查配置：{error}"));
                return;
            }
        };
        self.health_dialog = Some(HealthDialog {
            target,
            report: inspect(target, &resolved),
            pending_relocation: None,
            relocation_warning: None,
            confirming_repair: false,
        });
    }

    fn repair_current_chromeplus_paths(&mut self) {
        let target = self.state.selected_target();
        let config = self.state.draft().targets[&target].clone();
        let resolved = match self.state.resolve_operation_paths(config) {
            Ok(config) => config,
            Err(error) => {
                self.status_message = Some(format!("无法修复 Chrome++ 路径：{error}"));
                return;
            }
        };
        match repair_chromeplus_paths(target, &resolved) {
            Ok(repair) => {
                self.status_message = Some(format!(
                    "Chrome++ 路径已修复；原配置备份到 {}",
                    repair.backup_path.display()
                ));
                self.open_health_dialog();
            }
            Err(error) => {
                self.status_message = Some(format!("修复 Chrome++ 路径失败：{error}"));
            }
        }
    }

    fn repair_current_portable_paths(&mut self) {
        let target = self.state.selected_target();
        let config = self.state.draft().targets[&target].clone();
        let resolved = match self.state.resolve_operation_paths(config) {
            Ok(config) => config,
            Err(error) => {
                self.status_message = Some(format!("无法修复便携配置：{error}"));
                return;
            }
        };
        match repair_portable_paths(target, &resolved) {
            Ok(repair) => {
                self.status_message = Some(format!(
                    "便携配置已修复；原配置备份到 {}",
                    repair.backup_path.display()
                ));
                self.open_health_dialog();
            }
            Err(error) => {
                self.status_message = Some(format!("修复便携配置失败：{error}"));
            }
        }
    }

    fn stage_relocation(&mut self, new_root: PathBuf) {
        let target = self.state.selected_target();
        let config = self.state.draft().targets[&target].clone();
        let resolved = match self.state.resolve_operation_paths(config) {
            Ok(config) => config,
            Err(error) => {
                self.status_message = Some(format!("无法重新定位：{error}"));
                return;
            }
        };
        match relocate(target, &resolved, &new_root) {
            Ok(config) => {
                let identity = config
                    .browser_directory
                    .as_deref()
                    .map_or(ProgramIdentity::NoExecutable, |program| {
                        inspect_program_identity(target, program)
                    });
                let warning = (!matches!(identity, ProgramIdentity::Match { .. }))
                    .then(|| "无法确认该程序属于当前浏览器；应用前请确认目录选择正确。".to_owned());
                if let Some(dialog) = &mut self.health_dialog {
                    dialog.pending_relocation = Some(config);
                    dialog.relocation_warning = warning;
                }
                self.status_message = Some("请确认重新定位后的路径".to_owned());
            }
            Err(error) => {
                self.status_message = Some(format!("重新定位失败：{error}"));
            }
        }
    }

    fn apply_staged_relocation(&mut self) {
        let Some(dialog) = self.health_dialog.clone() else {
            return;
        };
        let Some(config) = dialog.pending_relocation else {
            return;
        };
        let new_root = config.browser_root.clone().unwrap_or_default();
        self.update_target_config(dialog.target, config.clone());
        self.open_health_dialog();
        self.status_message = Some(format!("已重新定位到：{}", new_root.display()));
        self.refresh_local_version(dialog.target);
    }

    fn detected_local_version(&self, target: TargetId) -> Option<&LocalVersion> {
        match self.local_versions.get(&target) {
            Some(LocalVersionState::Detected(version)) => Some(version),
            _ => None,
        }
    }

    fn chrome_plus_running_process_count(&self, target: TargetId) -> Result<usize, String> {
        let config = self.state.draft().targets[&target].clone();
        let config = self
            .state
            .resolve_operation_paths(config)
            .map_err(|error| error.to_string())?;
        let program = config
            .browser_directory
            .as_deref()
            .ok_or_else(|| "程序目录未配置".to_owned())?;
        running_process_count_in_directory(program).map_err(|error| error.to_string())
    }

    fn chrome_plus_pending_update(
        &self,
        target: TargetId,
        config: &crate::domain::settings::TargetConfig,
    ) -> Option<(String, String)> {
        chrome_plus_ini_path(target, config, self.state.tool_root())?;
        let current = self
            .chrome_plus_versions
            .get(&target)
            .cloned()
            .flatten()
            .or_else(|| config.chrome_plus_version.clone())?;
        let latest = self.chrome_plus_latest.get(&target)?.clone();
        (compare_versions(&current, &latest) == VersionRelation::UpstreamNewer)
            .then_some((current, latest))
    }

    fn request_chrome_plus_update(
        &mut self,
        target: TargetId,
        current_version: Option<String>,
        latest_version: String,
    ) {
        match self.chrome_plus_running_process_count(target) {
            Ok(0) => {
                self.chrome_plus_update_prompt = Some(ChromePlusUpdatePrompt {
                    target,
                    current_version,
                    latest_version,
                });
                self.status_message = None;
            }
            Ok(count) => {
                self.status_message = Some(chrome_plus_running_message(
                    self.state.draft().language,
                    target,
                    count,
                ));
            }
            Err(error) => {
                self.status_message = Some(format!("无法检查浏览器运行状态：{error}"));
            }
        }
    }

    fn confirm_chrome_plus_update(&mut self) {
        let Some(prompt) = self.chrome_plus_update_prompt.clone() else {
            return;
        };
        match self.chrome_plus_running_process_count(prompt.target) {
            Ok(0) => {
                self.chrome_plus_update_prompt = None;
                self.state.select(prompt.target);
                self.start_operation_checked_with_expected(
                    OperationMode::ChromePlusInstall,
                    false,
                    false,
                    Some(prompt.latest_version),
                );
            }
            Ok(count) => {
                self.chrome_plus_update_prompt = None;
                self.status_message = Some(chrome_plus_running_message(
                    self.state.draft().language,
                    prompt.target,
                    count,
                ));
            }
            Err(error) => {
                self.chrome_plus_update_prompt = None;
                self.status_message = Some(format!("无法检查浏览器运行状态：{error}"));
            }
        }
    }

    fn latest_check(
        &self,
        target: TargetId,
        config: &crate::domain::settings::TargetConfig,
    ) -> Option<&UpstreamCheck> {
        self.latest_versions
            .get(&(target, config.channel, config.cpu_build))
    }

    fn cached_release(
        &self,
        target: TargetId,
        config: &crate::domain::settings::TargetConfig,
    ) -> Option<CachedReleaseMetadata> {
        self.checked_releases
            .get(&(target, config.channel, config.cpu_build))
            .filter(|cached| {
                cached.is_fresh()
                    && cached.release.target == target
                    && cached.release.channel == config.channel
                    && cached.release.cpu_build == config.cpu_build
            })
            .cloned()
    }

    fn maybe_auto_check(
        &mut self,
        target: TargetId,
        config: &crate::domain::settings::TargetConfig,
    ) {
        if !self.auto_check_enabled || self.startup_error.is_some() || self.operation.is_some() {
            return;
        }
        let key = (target, config.channel, config.cpu_build);
        if self.latest_versions.contains_key(&key) || !self.auto_checked_keys.insert(key) {
            return;
        }
        self.start_operation(OperationMode::Check, false);
    }

    fn record_checked_release(
        &mut self,
        target: TargetId,
        check: UpstreamCheck,
        release: crate::domain::release::ReleaseDescriptor,
    ) {
        let key = (target, check.channel, check.cpu_build);
        self.latest_versions.insert(key, check.clone());
        self.checked_releases.insert(
            key,
            CachedReleaseMetadata {
                check,
                release,
                observed_at: unix_timestamp(),
            },
        );
    }

    fn request_install_action(&mut self) {
        let target = self.state.selected_target();
        self.refresh_local_version(target);
        let config = self.state.draft().targets[&target].clone();
        let current = self.detected_local_version(target).cloned();
        let upstream = self.latest_check(target, &config).cloned();
        let chrome_plus_update = self.chrome_plus_pending_update(target, &config);
        if let (Some(current), Some(upstream)) = (current, upstream)
            && compare_versions(&current.version, &upstream.version) == VersionRelation::Same
        {
            if let Some((chrome_plus_current, chrome_plus_latest)) = chrome_plus_update {
                self.status_message = Some("浏览器版本相同；替换操作还会更新 Chrome++".to_owned());
                self.browser_upgrade_prompt = Some(BrowserUpgradePrompt {
                    target,
                    current_browser: current.version.clone(),
                    upstream_browser: upstream.version.clone(),
                    chrome_plus_current,
                    chrome_plus_latest,
                    replace_same_version: true,
                });
                return;
            }
            self.status_message = Some("本地版本与上游版本相同，等待确认替换".to_owned());
            self.same_version_prompt = Some(SameVersionPrompt {
                current_version: current.version,
                upstream,
            });
            return;
        }
        if let Some((chrome_plus_current, chrome_plus_latest)) = chrome_plus_update {
            self.status_message = Some("浏览器升级还会更新 Chrome++，等待确认".to_owned());
            self.browser_upgrade_prompt = Some(BrowserUpgradePrompt {
                target,
                current_browser: self
                    .detected_local_version(target)
                    .map_or_else(|| "—".to_owned(), |version| version.version.clone()),
                upstream_browser: self
                    .latest_check(target, &config)
                    .map_or_else(|| "—".to_owned(), |check| check.version.clone()),
                chrome_plus_current,
                chrome_plus_latest,
                replace_same_version: false,
            });
            return;
        }
        self.start_operation(OperationMode::Install, false);
    }

    fn auto_save(&mut self) -> bool {
        if self.startup_error.is_some() {
            return false;
        }
        match self.state.save_if_dirty() {
            Ok(_) => true,
            Err(error) => {
                self.status_message = Some(format!("自动保存失败：{error}"));
                false
            }
        }
    }

    fn track_window_position(&mut self, ctx: &egui::Context, position: Option<WindowPosition>) {
        let Some(position) = position else {
            return;
        };
        self.observed_window_position = Some(position);
        if self.state.draft().window_position == Some(position) {
            self.pending_window_position = None;
            self.window_position_changed_at = None;
            return;
        }
        if self.pending_window_position != Some(position) {
            self.pending_window_position = Some(position);
            self.window_position_changed_at = Some(Instant::now());
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }
        if self
            .window_position_changed_at
            .is_some_and(|changed_at| changed_at.elapsed() >= Duration::from_millis(500))
        {
            self.state
                .update_draft(|settings| settings.window_position = Some(position));
            self.auto_save();
            self.pending_window_position = None;
            self.window_position_changed_at = None;
        }
    }

    fn flush_window_position(&mut self) {
        if let Some(position) = self.observed_window_position
            && self.state.draft().window_position != Some(position)
        {
            self.state
                .update_draft(|settings| settings.window_position = Some(position));
            self.auto_save();
        }
    }

    fn start_operation(&mut self, mode: OperationMode, replace_same_version: bool) {
        self.start_operation_checked(mode, replace_same_version, false);
    }

    fn start_operation_checked(
        &mut self,
        mode: OperationMode,
        replace_same_version: bool,
        identity_confirmed: bool,
    ) {
        self.start_operation_checked_with_expected(
            mode,
            replace_same_version,
            identity_confirmed,
            None,
        );
    }

    fn start_operation_checked_with_expected(
        &mut self,
        mode: OperationMode,
        replace_same_version: bool,
        identity_confirmed: bool,
        chrome_plus_expected_version: Option<String>,
    ) {
        if self.operation.is_some() {
            return;
        }
        if self.state.draft() != self.state.saved() && !self.auto_save() {
            return;
        }
        let target = self.state.selected_target();
        self.refresh_local_version(target);
        let mut config = self.state.draft().targets[&target].clone();
        let current_version = self
            .detected_local_version(target)
            .map(|version| version.version.clone());
        if matches!(
            mode,
            OperationMode::Install | OperationMode::ChromePlusInstall
        ) && config.browser_root.is_none()
        {
            self.status_message = Some("请先设置浏览器根目录".to_owned());
            return;
        }
        if matches!(
            mode,
            OperationMode::Install | OperationMode::ChromePlusInstall
        ) {
            config = match self.state.resolve_operation_paths(config) {
                Ok(config) => config,
                Err(error) => {
                    self.status_message = Some(error.to_string());
                    return;
                }
            };
        }
        if mode == OperationMode::Install
            && !identity_confirmed
            && let ProgramIdentity::Mismatch {
                actual,
                product_name,
            } = self.program_identity.clone()
        {
            self.identity_prompt = Some(IdentityPrompt {
                target,
                actual,
                product_name,
                replace_same_version,
            });
            self.status_message = Some("浏览器身份不一致，等待确认".to_owned());
            return;
        }
        if let Err(error) = crate::services::http::UrlRouter::validate_accelerators(
            &self.state.draft().network.accelerators,
        ) {
            self.status_message = Some(error);
            return;
        }
        let cached_release = (mode == OperationMode::Install)
            .then(|| self.cached_release(target, &config))
            .flatten();
        self.status_message = None;
        self.operation_phase = Some(OperationPhase::Preparing);
        self.operation = Some(start_operation(StartOperationRequest {
            target,
            mode,
            target_config: config,
            network: self.state.draft().network.clone(),
            cache_dir: self.state.cache_dir(),
            current_version,
            chrome_plus_expected_version,
            replace_same_version,
            cached_release,
        }));
    }

    fn poll_operation(&mut self) {
        let events = self.operation.as_ref().map_or_else(Vec::new, |operation| {
            operation.receiver.try_iter().collect::<Vec<_>>()
        });
        for event in events {
            if event.phase.is_terminal() {
                self.operation = None;
            }
            match &event.phase {
                OperationPhase::Checked { check, release } => {
                    self.record_checked_release(event.target, check.clone(), release.clone());
                    let source = localized(match check.source {
                        MetadataSource::Official => "上游版本",
                        MetadataSource::SignedManifest => "签名发布清单",
                    });
                    self.status_message = Some(format!(
                        "{} {source}：{}（{}）",
                        event.target.display_name(),
                        check.version,
                        checked_age(check.checked_at)
                    ));
                }
                OperationPhase::SameVersion {
                    current_version,
                    upstream,
                    release,
                } => {
                    self.record_checked_release(event.target, upstream.clone(), release.clone());
                    self.status_message = Some("本地版本与上游版本相同，等待确认替换".to_owned());
                    self.same_version_prompt = Some(SameVersionPrompt {
                        current_version: current_version.clone(),
                        upstream: upstream.clone(),
                    });
                }
                OperationPhase::Installed { receipt } => {
                    let previous_chrome_plus = self
                        .state
                        .draft()
                        .targets
                        .get(&event.target)
                        .and_then(|config| config.chrome_plus_version.clone());
                    if let Some(config) = self.state.draft().targets.get(&event.target) {
                        let key = (event.target, config.channel, config.cpu_build);
                        self.latest_versions.insert(
                            key,
                            UpstreamCheck::fresh(receipt.version.clone(), key.1, key.2),
                        );
                        self.checked_releases.remove(&key);
                    }
                    self.chrome_plus_versions
                        .insert(event.target, receipt.chrome_plus_version.clone());
                    self.chrome_plus_ini_loaded.remove(&event.target);
                    let mut message = match self.state.record_install_success(event.target, receipt)
                    {
                        Ok(()) => match (
                            previous_chrome_plus.as_deref(),
                            receipt.chrome_plus_version.as_deref(),
                        ) {
                            (Some(previous), Some(current)) if previous != current => format!(
                                "{} {} 已安装；Chrome++ 已从 {previous} 更新到 {current}，请重新检查配置",
                                event.target.display_name(),
                                receipt.version
                            ),
                            _ => format!(
                                "{} {} 已安装",
                                event.target.display_name(),
                                receipt.version
                            ),
                        },
                        Err(error) => format!("安装完成，但保存状态失败：{error}"),
                    };
                    if let Some(config) = self.state.draft().targets.get(&event.target)
                        && config.auto_create_shortcut
                        && let Some(root) = config
                            .browser_root
                            .as_deref()
                            .and_then(|path| resolve_preview_path(self.state.tool_root(), path))
                    {
                        match create_auto_shortcut(
                            event.target,
                            config.auto_create_shortcut,
                            &root,
                            &receipt.browser_directory,
                        ) {
                            Ok(Some(path)) => {
                                message.push_str(&format!("；快捷方式已创建：{}", path.display()));
                            }
                            Ok(None) => {}
                            Err(error) => {
                                message.push_str(&format!("；自动创建快捷方式失败：{error}"));
                            }
                        }
                    }
                    self.status_message = Some(message);
                    self.refresh_local_version(event.target);
                }
                OperationPhase::ChromePlusChecked {
                    current_version,
                    latest_version,
                } => {
                    for target in TargetId::ALL
                        .into_iter()
                        .filter(|target| target.chrome_plus_mode() != ChromePlusMode::Disabled)
                    {
                        self.chrome_plus_latest
                            .insert(target, latest_version.clone());
                    }
                    self.status_message = Some(match current_version.as_deref() {
                        Some(current)
                            if compare_versions(current, latest_version)
                                == VersionRelation::Same =>
                        {
                            format!("Chrome++ 已是最新版本：{latest_version}")
                        }
                        Some(_) => format!("Chrome++ 发现上游版本：{latest_version}"),
                        None => format!("Chrome++ 最新版本：{latest_version}"),
                    });
                }
                OperationPhase::ChromePlusInstalled { version } => {
                    let mut config = self.state.draft().targets[&event.target].clone();
                    config.chrome_plus_version = Some(version.clone());
                    let saved = self.update_target_config(event.target, config);
                    self.chrome_plus_versions
                        .insert(event.target, Some(version.clone()));
                    self.chrome_plus_ini_loaded.remove(&event.target);
                    self.chrome_plus_latest
                        .insert(event.target, version.clone());
                    if saved {
                        self.status_message = Some(format!(
                            "Chrome++ 已更新到 {version}，浏览器数据和配置已保留"
                        ));
                    }
                }
                OperationPhase::Cancelled => {
                    self.status_message = Some("任务已取消".to_owned());
                }
                OperationPhase::Failed { .. } => {
                    self.status_message = None;
                }
                OperationPhase::Notice(message) => {
                    self.status_message = Some(message.clone());
                }
                _ => {}
            }
            self.operation_phase = Some(event.phase);
        }
    }

    fn select_target(&mut self, target: TargetId) {
        let previous = self.state.selected_target();
        if self.state.draft() != self.state.saved() && !self.auto_save() {
            return;
        }
        self.state.select(target);
        self.health_dialog = None;
        if !self.auto_save() {
            self.state.select(previous);
            return;
        }
        self.status_message = None;
        self.operation_phase = None;
        self.identity_prompt = None;
        self.chrome_plus_update_prompt = None;
        self.browser_upgrade_prompt = None;
        self.refresh_local_version(target);
        self.refresh_program_identity(target);
        if target.chrome_plus_mode() == ChromePlusMode::Disabled
            && self.active_tab == DetailTab::ChromePlus
        {
            self.active_tab = DetailTab::Overview;
        }
    }

    fn show_tabs(&mut self, ui: &mut egui::Ui) {
        let target = self.state.selected_target();
        let chrome_plus_mode = target.chrome_plus_mode();
        let mut requested_tab = None;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if ui
                .add_sized(
                    [54.0, 24.0],
                    egui::Button::selectable(
                        self.active_tab == DetailTab::Overview,
                        localized("浏览器"),
                    ),
                )
                .clicked()
            {
                requested_tab = Some(DetailTab::Overview);
            }
            let chrome_plus = ui
                .add_enabled_ui(chrome_plus_mode != ChromePlusMode::Disabled, |ui| {
                    ui.add_sized(
                        [76.0, 24.0],
                        egui::Button::selectable(
                            self.active_tab == DetailTab::ChromePlus,
                            "Chrome++",
                        ),
                    )
                })
                .inner;
            if chrome_plus.clicked() {
                requested_tab = Some(DetailTab::ChromePlus);
            }
            if self.active_tab == DetailTab::ChromePlus {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.localized_label(
                        RichText::new(format!("当前浏览器：{}", target.display_name()))
                            .size(11.0)
                            .color(palette(ui).muted),
                    );
                });
            }
        });

        if let Some(tab) = requested_tab {
            self.active_tab = tab;
        }
    }

    fn show_tab_content(&mut self, ui: &mut egui::Ui) {
        match self.active_tab {
            DetailTab::Overview => self.show_overview(ui),
            DetailTab::Network => self.show_network(ui),
            DetailTab::ChromePlus => self.show_chrome_plus(ui),
        }
    }

    fn show_overview(&mut self, ui: &mut egui::Ui) {
        let palette = palette(ui);
        let target = self.state.selected_target();
        let spec = target.spec();
        let mut config = self.state.draft().targets[&target].clone();
        self.maybe_auto_check(target, &config);
        let detected_version = self.detected_local_version(target).cloned();
        let displayed_version = detected_version.clone();
        let missing_directory = matches!(
            self.local_versions.get(&target),
            Some(LocalVersionState::MissingDirectory(_))
        );
        let incomplete_install = matches!(
            self.local_versions.get(&target),
            Some(LocalVersionState::Incomplete(_))
        );
        let (current_version, current_hover, current_color) = match self.local_versions.get(&target)
        {
            Some(LocalVersionState::Detected(version)) => (
                version.version.clone(),
                format!(
                    "版本来源：{}",
                    localized(version_source_label(version.source))
                ),
                palette.text,
            ),
            Some(LocalVersionState::MissingDirectory(path)) => (
                localized("目录不存在"),
                format!("目录不存在：{}", path.display()),
                palette.warning,
            ),
            Some(LocalVersionState::Incomplete(path)) => (
                localized("安装不完整"),
                format!("未找到浏览器程序：{}", path.display()),
                palette.error,
            ),
            Some(LocalVersionState::Unavailable(error)) => {
                (localized("版本未知"), error.clone(), palette.warning)
            }
            None => (
                localized("未检测到"),
                "未检测到本地浏览器版本".to_owned(),
                palette.error,
            ),
        };
        let current_hover = config
            .installed_version
            .as_ref()
            .map_or(current_hover.clone(), |version| {
                format!("{current_hover}；上次安装记录：{version}")
            });
        let upstream = self.latest_check(target, &config).cloned();
        let latest_version = upstream
            .as_ref()
            .map_or_else(|| "—".to_owned(), |check| check.version.clone());
        let latest_checked = upstream.as_ref().map(|check| checked_age(check.checked_at));
        let relation = match (detected_version.as_ref(), upstream.as_ref()) {
            (Some(current), Some(upstream)) => {
                compare_versions(&current.version, &upstream.version)
            }
            _ => VersionRelation::Unknown,
        };
        let relation_text = if missing_directory {
            Some(("未安装", StatusTone::Neutral))
        } else if incomplete_install {
            Some(("无法比较", StatusTone::Error))
        } else {
            relation_status(detected_version.is_some(), upstream.is_some(), relation)
        };
        let install_label = match relation {
            VersionRelation::Same => "重新下载并替换",
            VersionRelation::UpstreamNewer => "下载并升级",
            VersionRelation::UpstreamOlder | VersionRelation::Different => "下载上游版本",
            VersionRelation::Unknown if incomplete_install => "重新下载并安装",
            VersionRelation::Unknown if displayed_version.is_some() => "检查并安装/升级",
            VersionRelation::Unknown => "下载并安装",
        };
        let can_start = self.operation.is_none()
            && self.startup_error.is_none()
            && self.state.draft() == self.state.saved();
        let path_valid = config.browser_root.is_some()
            && self.state.resolve_operation_paths(config.clone()).is_ok();
        let can_install = can_start && path_valid;
        let shortcut_executable = local_browser_executable(target, &config, self.state.tool_root());
        let mut requested_action = None;
        let mut requested_target = None;
        let mut create_shortcut = false;
        let upstream_color = if upstream.is_some() {
            palette.text
        } else {
            palette.muted
        };

        let mut changed = section(ui, "浏览器程序", |ui| {
            let release_hint = format!(
                "更新来源：{}；{}；Chrome++：{}",
                localized(provider_label(spec.provider)),
                localized(if spec.display_beta {
                    "BETA，用户确认后安装"
                } else {
                    "官方发布，用户确认后安装"
                }),
                localized(chrome_plus_label(target.chrome_plus_mode()))
            );
            let mut metadata_changed = false;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if let Some(selected) = browser_selector(ui, target, 126.0) {
                    requested_target = Some(selected);
                }
                ui.add_space(8.0);

                let channel_label = ui.localized_label(RichText::new("渠道").color(palette.muted));
                let channel_response = if spec.channels.len() == 1 {
                    ui.localized_label(config.channel.display_name())
                } else {
                    egui::ComboBox::from_id_salt(format!("overview_channel_{}", target.sort_key()))
                        .selected_text(config.channel.display_name())
                        .width(76.0)
                        .show_ui(ui, |ui| {
                            for channel in spec.channels {
                                metadata_changed |= ui
                                    .selectable_value(
                                        &mut config.channel,
                                        *channel,
                                        channel.display_name(),
                                    )
                                    .changed();
                            }
                        })
                        .response
                };
                channel_response
                    .labelled_by(channel_label.id)
                    .localized_hover(&release_hint);
                ui.add_space(10.0);

                let build_label = ui.localized_label(RichText::new("构建").color(palette.muted));
                let build_response = if spec.cpu_builds.len() == 1 {
                    ui.localized_label(config.cpu_build.display_name())
                } else {
                    egui::ComboBox::from_id_salt(format!("overview_build_{}", target.sort_key()))
                        .selected_text(config.cpu_build.display_name())
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            for cpu_build in spec.cpu_builds {
                                metadata_changed |= ui
                                    .selectable_value(
                                        &mut config.cpu_build,
                                        *cpu_build,
                                        cpu_build.display_name(),
                                    )
                                    .changed();
                            }
                        })
                        .response
                };
                let build_hint =
                    if target == TargetId::Thorium && config.cpu_build == CpuBuild::Avx512 {
                        "AVX512 构建需要 CPU 支持 AVX512"
                    } else {
                        "选择下载包对应的处理器构建"
                    };
                build_response
                    .labelled_by(build_label.id)
                    .localized_hover(build_hint);
                ui.add_space(10.0);

                ui.localized_label(RichText::new("来源").color(palette.muted));
                ui.localized_label(provider_label(spec.provider))
                    .localized_hover(&release_hint);
            });
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.localized_label(RichText::new("当前版本").color(palette.muted));
                ui.add(
                    egui::Label::new(
                        RichText::new(current_version)
                            .monospace()
                            .strong()
                            .color(current_color),
                    )
                    .extend(),
                )
                .localized_hover(current_hover.clone());
                ui.add_space(18.0);

                ui.localized_label(RichText::new("上游版本").color(palette.muted));
                let response = ui.add(
                    egui::Label::new(
                        RichText::new(latest_version)
                            .monospace()
                            .strong()
                            .color(upstream_color),
                    )
                    .extend(),
                );
                if let Some(age) = latest_checked.as_deref() {
                    response.localized_hover(format!(
                        "{}检查时间：{age}",
                        match upstream.as_ref().map(|check| check.source) {
                            Some(MetadataSource::SignedManifest) => "签名发布清单；",
                            Some(MetadataSource::Official) | None => "",
                        }
                    ));
                }
                if let Some((text, tone)) = relation_text {
                    status_chip(ui, text, tone).localized_hover(current_hover.clone());
                }
            });
            ui.add_space(4.0);
            let mut local_package_changed = false;
            let mut pick_local_package = false;
            let mut clear_local_package = false;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_start, localized_button("检查更新"))
                    .clicked()
                {
                    requested_action = Some(OperationMode::Check);
                }
                if ui
                    .add_enabled(
                        can_install,
                        if relation == VersionRelation::UpstreamNewer {
                            localized_button(
                                RichText::new(install_label)
                                    .strong()
                                    .color(palette.accent_text),
                            )
                            .fill(palette.accent_soft)
                            .stroke(Stroke::new(1.0, palette.accent))
                        } else {
                            localized_button(install_label)
                        },
                    )
                    .clicked()
                {
                    requested_action = Some(OperationMode::Install);
                }
                let package_label = if config.local_package.is_some() {
                    "更换本地包"
                } else {
                    "载入本地包"
                };
                let package_hint = config.local_package.as_ref().map_or_else(
                    || "选择本地安装包（可选）".to_owned(),
                    |path| format!("当前本地包：{}", path.display()),
                );
                let package = ui
                    .localized_button(package_label)
                    .localized_hover(package_hint);
                if package.clicked() {
                    pick_local_package = true;
                }
                package.context_menu(|ui| {
                    if config.local_package.is_some() && ui.localized_button("清除本地包").clicked()
                    {
                        clear_local_package = true;
                        ui.close();
                    }
                });
            });
            if pick_local_package
                && let Some(selected) = folder_picker::pick_file(
                    &localized("选择本地安装包"),
                    config.local_package.as_deref(),
                )
            {
                config.local_package = Some(selected);
                local_package_changed = true;
            }
            if clear_local_package {
                config.local_package = None;
                local_package_changed = true;
            }
            metadata_changed | local_package_changed
        });

        ui.add_space(4.0);
        let (path_changed, check_configuration) =
            section_with_action(ui, "路径与用户数据", "检查配置", |ui| {
                let shortcut_toggle_label = if config.auto_create_shortcut {
                    "快捷方式：开"
                } else {
                    "快捷方式：关"
                };
                let (root_changed, shortcut_response) = path_field_with_action(
                    ui,
                    "浏览器根目录",
                    &mut config.browser_root,
                    r"例如 .\Helium 或 D:\Tools\Browser\Helium",
                    shortcut_toggle_label,
                    true,
                );
                if root_changed {
                    config.derive_paths_from_root(target);
                }
                let mut changed = root_changed;
                if let Some(response) = shortcut_response {
                    let hint = if config.auto_create_shortcut {
                        "安装后自动在浏览器根目录创建快捷方式；点击关闭"
                    } else {
                        "已关闭自动创建；点击开启"
                    };
                    if response.localized_hover(hint).clicked() {
                        config.auto_create_shortcut = !config.auto_create_shortcut;
                        changed = true;
                        create_shortcut =
                            config.auto_create_shortcut && shortcut_executable.is_some();
                    }
                }
                changed |= path_field(
                    ui,
                    "程序目录",
                    &mut config.browser_directory,
                    r"例如 .\Chrome\App",
                );
                ui.separator();
                let data_changed = path_field(
                    ui,
                    "用户数据目录",
                    &mut config.data_directory,
                    "书签、历史、Cookie、登录状态和浏览器设置所在的总目录",
                );
                if data_changed && target.spec().layout.data == target.spec().layout.profile {
                    config.profile_directory = config.data_directory.clone();
                }
                changed |= data_changed;
                if target.spec().layout.data != target.spec().layout.profile {
                    changed |= path_field(
                        ui,
                        "Profile 位置",
                        &mut config.profile_directory,
                        "该浏览器发行包使用独立 Profile 位置；通常无需修改",
                    );
                }
                if target.supports_custom_cache() {
                    let cache_hint = if target.family() == BrowserFamily::Firefox {
                        "缓存根；留空跟随 Profile；修改后运行“检查配置”应用到现有安装"
                    } else {
                        "缓存根，最终通常为 <此目录>\\<Profile>\\Cache；修改后运行“检查配置”应用"
                    };
                    changed |= path_field(ui, "缓存目录", &mut config.cache_directory, cache_hint);
                } else {
                    config.cache_directory = None;
                    readonly_field(
                        ui,
                        "缓存目录",
                        if target == TargetId::SkyKakapo {
                            "跟随 PortableData"
                        } else {
                            "跟随 Profile"
                        },
                    );
                }
                changed
            });
        changed |= path_changed;
        if check_configuration {
            self.open_health_dialog();
        }

        if let Some(selected) = requested_target {
            self.select_target(selected);
            return;
        }
        if create_shortcut
            && let (Some(root), Some(executable)) = (
                config
                    .browser_root
                    .as_deref()
                    .and_then(|path| resolve_preview_path(self.state.tool_root(), path)),
                shortcut_executable.as_deref(),
            )
        {
            self.status_message = Some(
                match create_browser_shortcut(&root, target.display_name(), executable) {
                    Ok(path) => format!("快捷方式已创建：{}", path.display()),
                    Err(error) => format!("创建快捷方式失败：{error}"),
                },
            );
        }
        if changed {
            self.update_target_config(target, config);
        }
        if let Some(mode) = requested_action {
            if mode == OperationMode::Install {
                self.request_install_action();
            } else {
                self.start_operation(mode, false);
            }
        }
    }

    fn show_network(&mut self, ui: &mut egui::Ui) {
        let palette = palette(ui);
        let cache_dir = self.state.cache_dir();
        let cache_usage = download_cache_usage(&cache_dir).ok();
        let mut network = self.state.draft().network.clone();
        let mut accelerator_input = std::mem::take(&mut self.accelerator_input);
        let mut changed = false;
        let mut add_requested = false;
        let mut validation_error = None;
        let mut clear_cache = false;
        let mut requested_config_location = None;

        changed |= section(ui, "下载加速", |ui| {
            let mut section_changed = ui
                .localized_checkbox(&mut network.accelerators_enabled, "启用内置下载加速站")
                .changed();
            ui.localized_label(
                RichText::new("添加额外的自定义加速站（可选）")
                    .size(11.0)
                    .color(palette.muted),
            );
            ui.horizontal(|ui| {
                let label = field_label(ui, "HTTPS 前缀", FIELD_LABEL_WIDTH);
                let response = ui
                    .add_sized(
                        [(ui.available_width() - 52.0).max(80.0), 24.0],
                        egui::TextEdit::singleline(&mut accelerator_input)
                            .hint_text("https://example.com/"),
                    )
                    .labelled_by(label.id);
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    add_requested = true;
                }
                add_requested |= ui
                    .add_sized([46.0, 24.0], localized_button("添加"))
                    .clicked();
            });
            let mut remove = None;
            let mut move_up = None;
            let mut move_down = None;
            for index in 0..network.accelerators.len() {
                let prefix = network.accelerators[index].prefix.clone();
                ui.horizontal(|ui| {
                    ui.localized_checkbox(
                        &mut network.accelerators[index].enabled,
                        RichText::new(prefix).size(11.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.localized_button("删除").clicked() {
                            remove = Some(index);
                        }
                        if ui
                            .add_enabled(
                                index + 1 < network.accelerators.len(),
                                localized_button("下移"),
                            )
                            .clicked()
                        {
                            move_down = Some(index);
                        }
                        if ui
                            .add_enabled(index > 0, localized_button("上移"))
                            .clicked()
                        {
                            move_up = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove {
                network.accelerators.remove(index);
                section_changed = true;
            } else if let Some(index) = move_up {
                network.accelerators.swap(index, index - 1);
                section_changed = true;
            } else if let Some(index) = move_down {
                network.accelerators.swap(index, index + 1);
                section_changed = true;
            }
            if network.accelerators.is_empty() {
                ui.localized_label(
                    RichText::new("未添加自定义加速站；仍会尝试内置 GitHub 加速入口。")
                        .size(11.0)
                        .color(palette.muted),
                );
            }
            ui.localized_label(
                RichText::new("加速站只改变 GitHub 传输路径，不改变版本、长度或摘要校验。")
                    .size(11.0)
                    .color(palette.muted),
            );
            section_changed
        });

        if add_requested {
            let accelerator = crate::domain::settings::Accelerator {
                prefix: accelerator_input.trim().to_owned(),
                enabled: true,
            };
            if network.accelerators.len() >= 20 {
                validation_error = Some("最多添加 20 个加速站".to_owned());
            } else if let Err(error) = accelerator.validate() {
                validation_error = Some(error);
            } else if network.accelerators.iter().any(|existing| {
                existing
                    .prefix
                    .trim()
                    .eq_ignore_ascii_case(&accelerator.prefix)
            }) {
                validation_error = Some("加速站已经存在".to_owned());
            } else {
                network.accelerators.push(accelerator);
                accelerator_input.clear();
                changed = true;
            }
        }
        if let Some(error) = &validation_error {
            ui.colored_label(palette.error, localized(error));
        }

        ui.add_space(7.0);
        changed |= section(ui, "代理与超时", |ui| {
            let mut section_changed = false;
            let mut proxy = network.proxy.clone().unwrap_or_default();
            let proxy_changed =
                text_field(ui, "代理地址", &mut proxy, "留空时使用系统代理").changed();
            if proxy_changed {
                network.proxy = (!proxy.trim().is_empty()).then(|| proxy.trim().to_owned());
                section_changed = true;
            }
            let label = field_label(ui, "超时", FIELD_LABEL_WIDTH);
            section_changed |= ui
                .add_sized(
                    [110.0, 24.0],
                    egui::DragValue::new(&mut network.timeout_seconds)
                        .range(5..=300)
                        .speed(1.0)
                        .suffix(format!(" {}", localized("秒"))),
                )
                .labelled_by(label.id)
                .changed();
            section_changed
        });

        ui.add_space(7.0);
        section(ui, "下载缓存", |ui| {
            ui.horizontal(|ui| {
                let usage = cache_usage.map_or_else(
                    || "无法读取缓存目录".to_owned(),
                    |usage| {
                        format!(
                            "{} {}，{}",
                            usage.files,
                            localized("个文件"),
                            format_bytes(usage.bytes)
                        )
                    },
                );
                ui.localized_label(RichText::new(usage).size(11.0).color(palette.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    clear_cache = ui
                        .add_enabled(
                            self.operation.is_none()
                                && cache_usage.is_some_and(|usage| usage.files > 0),
                            localized_button("清理下载缓存"),
                        )
                        .localized_hover(cache_dir.display().to_string())
                        .clicked();
                });
            });
            ui.localized_label(
                RichText::new("仅删除可重新下载的安装包，不影响浏览器数据。")
                    .size(10.5)
                    .color(palette.muted),
            );
        });

        ui.add_space(7.0);
        section(ui, "配置位置", |ui| {
            let mut portable = self.state.is_portable_config();
            let user = ui.radio_value(&mut portable, false, localized("系统用户目录（默认）"));
            let user_changed = user.changed();
            user.localized_hover("保存在 %LOCALAPPDATA%\\browser-harbor\\manager.json");
            let portable_response =
                ui.radio_value(&mut portable, true, localized("程序目录（便携）"));
            let portable_changed = portable_response.changed();
            portable_response
                .localized_hover("将 manager.json 保存在 Browser Harbor EXE 旁，可随程序一起带走");
            if user_changed || portable_changed {
                requested_config_location = Some(portable);
            }
            let path = self.state.config_path().display().to_string();
            ui.add(
                egui::Label::new(
                    RichText::new(&path)
                        .size(10.5)
                        .monospace()
                        .color(palette.muted),
                )
                .truncate(),
            )
            .localized_hover(path);
            ui.localized_label(
                RichText::new(
                    "便携模式要求 EXE 所在目录可写；浏览器绝对路径在新电脑上仍需重新定位。",
                )
                .size(10.5)
                .color(palette.muted),
            );
        });

        self.accelerator_input = accelerator_input;
        if clear_cache {
            self.status_message = Some(match clear_download_cache(&cache_dir) {
                Ok(removed) => {
                    format!("已清理下载缓存：{}", format_bytes(removed.bytes))
                }
                Err(error) => format!("清理下载缓存失败：{error}"),
            });
        }
        if changed {
            self.status_message = None;
            self.operation_phase = None;
            self.state
                .update_draft(|settings| settings.network = network);
            self.auto_save();
        }
        if let Some(portable) = requested_config_location {
            match self.state.switch_config_location(portable) {
                Ok(()) => {
                    self.status_message = Some(format!(
                        "{}：{}",
                        localized("配置位置已切换"),
                        self.state.config_path().display()
                    ));
                }
                Err(error) => {
                    self.status_message =
                        Some(format!("{}：{error}", localized("切换配置位置失败")));
                }
            }
        }
    }

    fn show_chrome_plus(&mut self, ui: &mut egui::Ui) {
        let palette = palette(ui);
        let target = self.state.selected_target();
        let mode = target.chrome_plus_mode();
        let mut config = self.state.draft().targets[&target].clone();
        let ini_path = chrome_plus_ini_path(target, &config, self.state.tool_root());
        if !self.chrome_plus_versions.contains_key(&target) {
            let detected = chrome_plus_program_directory(target, &config, self.state.tool_root())
                .and_then(|directory| {
                    crate::runtime::chromeplus::installed_version(&directory).ok()
                });
            self.chrome_plus_versions.insert(target, detected);
        }
        let detected_version = self.chrome_plus_versions.get(&target).cloned().flatten();
        let installed = ini_path.is_some();
        let installed_version = if installed {
            detected_version.or_else(|| config.chrome_plus_version.clone())
        } else {
            None
        };
        let stored_chrome_plus_version = config.chrome_plus_version.clone();
        if installed_version.is_some() {
            config.chrome_plus_version = installed_version.clone();
        }
        let backup_dir = config
            .browser_root
            .as_deref()
            .and_then(|root| resolve_preview_path(self.state.tool_root(), root))
            .map(|root| root.join("backups"));
        let latest_version = self.chrome_plus_latest.get(&target).cloned();
        let update_label = chrome_plus_update_button_label(
            installed_version.as_deref(),
            latest_version.as_deref(),
        );
        let mut changed = config.chrome_plus_version != stored_chrome_plus_version;
        let mut ini_changed = false;
        let mut pending_status = None;
        if !self.chrome_plus_ini_loaded.contains(&target)
            && let Some(path) = &ini_path
        {
            match crate::runtime::chromeplus::read_config_from_ini(path, &config.chrome_plus) {
                Ok(loaded) => {
                    changed |= loaded != config.chrome_plus;
                    config.chrome_plus = loaded;
                    self.chrome_plus_ini_loaded.insert(target);
                }
                Err(error) => {
                    pending_status = Some(format!("读取 Chrome++ 配置失败：{error}"));
                }
            }
        }
        let mut open_ini = false;
        let mut open_ini_folder = false;
        let mut backup_ini = false;
        let mut restore_defaults = false;
        let mut check_update = false;
        let mut request_update = false;
        let mut load_keymapping = false;
        let mut load_ini = false;

        ui.columns(2, |columns| {
            section_with_header(
                &mut columns[0],
                "Chrome++ 集成",
                |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(86.0, 24.0),
                        Layout::right_to_left(Align::Center),
                        |ui| {
                            let update_available = update_label == "可更新";
                            let update_hover = if update_available {
                                chrome_plus_update_hover(
                                    self.state.draft().language,
                                    installed_version.as_deref().unwrap_or("—"),
                                    latest_version.as_deref().unwrap_or("—"),
                                )
                            } else {
                                latest_version.as_deref().map_or_else(
                                    || "检查 Bush2021/chrome_plus 的最新正式版本".to_owned(),
                                    |latest| format!("最新版本：{latest}；点击重新检查"),
                                )
                            };
                            let update = ui
                                .add_enabled(
                                    self.operation.is_none(),
                                    localized_button("")
                                        .min_size(egui::vec2(24.0, 24.0))
                                        .frame(false),
                                )
                                .localized_hover(update_hover);
                            let hovered = update.hovered() || update.has_focus();
                            ui.painter().rect_filled(
                                update.rect,
                                CornerRadius::same(5),
                                if hovered {
                                    palette.hover
                                } else {
                                    Color32::TRANSPARENT
                                },
                            );
                            if update.has_focus() {
                                ui.painter().rect_stroke(
                                    update.rect,
                                    CornerRadius::same(5),
                                    Stroke::new(1.0, palette.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let update_color = match update_label.as_str() {
                                "已最新" => palette.success,
                                "可更新" => palette.accent_text,
                                _ => palette.text,
                            };
                            let icon_color = if hovered {
                                palette.accent_text
                            } else {
                                update_color
                            };
                            if update_available {
                                paint_update_icon(ui.painter(), update.rect.center(), icon_color);
                            } else {
                                paint_refresh_icon(ui.painter(), update.rect.center(), icon_color);
                            }
                            let accessible_label = if update_available {
                                chrome_plus_update_action_label(
                                    self.state.draft().language,
                                    latest_version.as_deref().unwrap_or("—"),
                                )
                            } else {
                                localized(update_label.as_str())
                            };
                            update.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::Button,
                                    self.operation.is_none(),
                                    accessible_label.clone(),
                                )
                            });
                            if update.clicked() {
                                if update_available {
                                    request_update = true;
                                } else {
                                    check_update = true;
                                }
                            }

                            ui.add_space(3.0);
                            let version_text = installed_version
                                .as_deref()
                                .map_or_else(|| "—".to_owned(), |version| format!("v{version}"));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(version_text).monospace().strong().color(
                                        if installed_version.is_some() {
                                            palette.text
                                        } else {
                                            palette.muted
                                        },
                                    ),
                                )
                                .show_tooltip_when_elided(false)
                                .truncate(),
                            )
                            .localized_hover(if installed {
                                "已安装的 Chrome++ 版本"
                            } else {
                                "Chrome++ 尚未安装"
                            });
                        },
                    );
                },
                |ui| {
                    ui.horizontal(|ui| {
                        metadata_label(ui, "模式", 72.0);
                        ui.localized_label(RichText::new(chrome_plus_label(mode)).strong().color(
                            if mode == ChromePlusMode::DataOnly {
                                palette.warning
                            } else {
                                palette.text
                            },
                        ));
                    });
                    ui.horizontal(|ui| {
                        metadata_label(ui, "状态", 72.0);
                        ui.localized_label(if installed {
                            "已安装"
                        } else {
                            "尚未安装"
                        })
                        .localized_hover(if installed {
                            "Chrome++ 文件位于浏览器程序目录"
                        } else {
                            "浏览器安装或升级时会自动下载、校验并放置 Chrome++，无需手工下载"
                        });
                    });
                    ui.horizontal(|ui| {
                        metadata_label(ui, "来源", 72.0);
                        ui.hyperlink_to(
                            localized("Chrome++ 源码与 GPL-3.0 许可证"),
                            CHROME_PLUS_SOURCE_URL,
                        )
                        .on_hover_text(CHROME_PLUS_SOURCE_URL);
                    });
                },
            );
            section_with_header(
                &mut columns[1],
                "配置文件",
                |_| {},
                |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let button_width = ui.available_width();
                    load_ini = ui
                        .add_enabled(
                            ini_path.is_some(),
                            localized_button("从 INI 载入")
                                .min_size(egui::vec2(button_width, 24.0)),
                        )
                        .clicked();
                    open_ini = ui
                        .add_enabled(
                            ini_path.is_some(),
                            localized_button("打开配置文件")
                                .min_size(egui::vec2(button_width, 24.0)),
                        )
                        .clicked();
                    let menu = egui::containers::menu::MenuButton::from_button(
                        localized_button("更多操作…").min_size(egui::vec2(button_width, 24.0)),
                    );
                    menu.ui(ui, |ui| {
                        if ui
                            .add_enabled(ini_path.is_some(), localized_button("打开所在文件夹"))
                            .clicked()
                        {
                            open_ini_folder = true;
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                ini_path.is_some() && backup_dir.is_some(),
                                localized_button("备份当前 INI"),
                            )
                            .clicked()
                        {
                            backup_ini = true;
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                ini_path.is_some() && backup_dir.is_some(),
                                localized_button("恢复默认配置"),
                            )
                            .localized_hover("先备份当前 INI，再将可编辑配置恢复为默认值")
                            .clicked()
                        {
                            restore_defaults = true;
                            ui.close();
                        }
                    });
                },
            );
        });
        if target.support_level() == SupportLevel::Experimental {
            ui.add_space(5.0);
            ui.localized_label(
                RichText::new(
                    "该浏览器的 Chrome++ 兼容性仍为实验性；完整功能已开放，若启动异常请关闭相关选项。",
                )
                .size(11.0)
                .color(palette.warning),
            );
        }
        ui.add_space(4.0);
        if mode == ChromePlusMode::Full {
            ui.localized_label(
                RichText::new("完整模式：以下选项对应 Chrome++ 的全部公开配置。")
                    .size(11.0)
                    .color(palette.muted),
            );
        } else if mode == ChromePlusMode::DataOnly {
            ui.localized_label(
                RichText::new("实验性 DataOnly 只写入用户数据与缓存路径，不开放标签或策略补丁。")
                    .size(11.5)
                    .color(palette.muted),
            );
        }

        if mode == ChromePlusMode::Full {
            ui.add_space(7.0);
            let section_changed = section(ui, "启动与快捷键", |ui| {
                let mut section_changed = text_field(
                    ui,
                    "附加参数",
                    &mut config.chrome_plus.command_line,
                    "例如 --disable-features=OutdatedBuildDetector",
                )
                .changed();
                section_changed |= text_field(
                    ui,
                    "启动命令",
                    &mut config.chrome_plus.launch_on_startup,
                    "浏览器启动后执行；不需要请留空",
                )
                .changed();
                section_changed |= text_field(
                    ui,
                    "退出命令",
                    &mut config.chrome_plus.launch_on_exit,
                    r#"多条命令用 ; 分隔，例如 "D:\App\clean.bat";"#,
                )
                .changed();
                ui.columns(2, |columns| {
                    section_changed |= text_field(
                        &mut columns[0],
                        "老板键",
                        &mut config.chrome_plus.boss_key,
                        "例如 Ctrl+Alt+B",
                    )
                    .changed();
                    section_changed |= text_field(
                        &mut columns[1],
                        "翻译键",
                        &mut config.chrome_plus.translate_key,
                        "例如 Ctrl+Alt+T",
                    )
                    .changed();
                });
                ui.localized_label(
                    RichText::new("启动和退出命令会由 Chrome++ 执行，请只填写可信命令。")
                        .size(10.5)
                        .color(palette.warning),
                );
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;

            ui.add_space(7.0);
            let section_changed = section(ui, "标签页与书签", |ui| {
                let mut section_changed = false;
                ui.columns(2, |columns| {
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.double_click_close,
                            "双击关闭标签",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.right_click_close,
                            "右键关闭标签",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.keep_last_tab,
                            "保留最后一个标签",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(&mut config.chrome_plus.wheel_tab, "标签栏滚轮切换")
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.wheel_tab_when_press_rbutton,
                            "按住右键滚轮切换",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.open_url_new_tab,
                            "地址栏新标签打开",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.open_bookmark_new_tab,
                            "书签在新标签打开",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.new_tab_disable,
                            "禁用指定新标签页",
                        )
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(&mut config.chrome_plus.hover_tab, "悬停自动切换标签")
                        .changed();
                });
                section_changed |= text_field(
                    ui,
                    "新标签匹配",
                    &mut config.chrome_plus.new_tab_disable_name,
                    "\"about:blank\",\"新建标签\"",
                )
                .changed();
                ui.horizontal(|ui| {
                    field_label(ui, "悬停延迟", FIELD_LABEL_WIDTH);
                    section_changed |= ui
                        .add_enabled(
                            config.chrome_plus.hover_tab,
                            egui::DragValue::new(&mut config.chrome_plus.hover_tab_delay)
                                .range(0..=10_000)
                                .suffix(" ms"),
                        )
                        .changed();
                });
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;

            ui.add_space(7.0);
            let section_changed = section(ui, "兼容与策略", |ui| {
                let mut section_changed = false;
                ui.columns(2, |columns| {
                    section_changed |= columns[0]
                        .localized_checkbox(
                            &mut config.chrome_plus.show_password,
                            "免系统验证查看密码",
                        )
                        .changed();
                    section_changed |= columns[0]
                        .localized_checkbox(&mut config.chrome_plus.win32k, "启用 win32k")
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(&mut config.chrome_plus.ignore_policies, "忽略企业策略")
                        .changed();
                    section_changed |= columns[1]
                        .localized_checkbox(
                            &mut config.chrome_plus.suppress_false_upgrade_notification,
                            "抑制错误升级提示",
                        )
                        .changed();
                });
                ui.localized_label(
                    RichText::new(
                        "win32k 仅在 Chrome++ 导致启动崩溃时启用；免验证查看密码会降低本机账户保护。",
                    )
                    .size(10.5)
                    .color(palette.warning),
                );
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;

            ui.add_space(7.0);
            let section_changed = section(ui, "按键映射", |ui| {
                let mut section_changed = false;
                if let Some(mapping) = &mut config.chrome_plus.keymapping {
                    section_changed |= ui
                        .add_sized(
                            [ui.available_width(), 68.0],
                            egui::TextEdit::multiline(mapping)
                                .hint_text(localized("每行一个映射，例如 F2=Ctrl+PageUp")),
                        )
                        .changed();
                    if ui.localized_button("移除全部按键映射").clicked() {
                        mapping.clear();
                        section_changed = true;
                    }
                    ui.localized_label(
                        RichText::new("支持 SourceKey=TargetKey 或 SourceKey=command:ID。")
                            .size(10.5)
                            .color(palette.muted),
                    );
                } else {
                    ui.localized_label(
                        RichText::new("未编辑时保留 INI 中已有的按键映射。")
                            .size(11.0)
                            .color(palette.muted),
                    );
                    load_keymapping = ui
                        .add_enabled(ini_path.is_some(), localized_button("载入现有映射"))
                        .clicked();
                }
                section_changed
            });
            changed |= section_changed;
            ini_changed |= section_changed;
        }

        if load_keymapping && let Some(path) = &ini_path {
            match crate::runtime::chromeplus::keymapping_text(path) {
                Ok(mapping) => {
                    config.chrome_plus.keymapping = Some(mapping);
                    changed = true;
                }
                Err(error) => {
                    self.status_message = Some(format!("读取按键映射失败：{error}"));
                }
            }
        }
        if load_ini && let Some(path) = &ini_path {
            match crate::runtime::chromeplus::read_config_from_ini(path, &config.chrome_plus) {
                Ok(loaded) => {
                    config.chrome_plus = loaded;
                    changed = true;
                }
                Err(error) => {
                    self.status_message = Some(format!("读取 Chrome++ 配置失败：{error}"));
                }
            }
        }
        if open_ini_folder
            && let Some(path) = &ini_path
            && let Err(error) = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn()
        {
            pending_status = Some(format!("无法打开配置文件所在目录：{error}"));
        }
        if backup_ini && let (Some(path), Some(directory)) = (&ini_path, &backup_dir) {
            pending_status = Some(
                match crate::runtime::chromeplus::backup_ini(path, directory, "manual") {
                    Ok(backup) => format!("INI 已备份：{}", backup.display()),
                    Err(error) => format!("备份 INI 失败：{error}"),
                },
            );
        }
        if restore_defaults && let (Some(path), Some(directory)) = (&ini_path, &backup_dir) {
            pending_status = Some(
                match crate::runtime::chromeplus::restore_default_config(path, directory) {
                    Ok(backup) => {
                        config.chrome_plus = crate::domain::runtime::ChromePlusConfig::default();
                        changed = true;
                        format!("已恢复默认配置；原文件备份到 {}", backup.display())
                    }
                    Err(error) => format!("恢复默认配置失败：{error}"),
                },
            );
        }
        if changed {
            self.update_target_config(target, config.clone());
        }
        if ini_changed
            && mode == ChromePlusMode::Full
            && let Some(path) = &ini_path
        {
            self.status_message = Some(
                match crate::runtime::chromeplus::update_existing_ini(path, &config.chrome_plus) {
                    Ok(()) => "Chrome++ 配置已自动写入".to_owned(),
                    Err(error) => format!("自动写入 Chrome++ 配置失败：{error}"),
                },
            );
        }
        if let Some(message) = pending_status {
            self.status_message = Some(message);
        }
        if open_ini
            && let Some(path) = &ini_path
            && let Err(error) = std::process::Command::new("notepad.exe").arg(path).spawn()
        {
            self.status_message = Some(format!("无法打开配置文件：{error}"));
        }
        if request_update && let Some(latest) = latest_version {
            self.request_chrome_plus_update(target, installed_version, latest);
        }
        if check_update {
            self.start_operation(OperationMode::ChromePlusCheck, false);
        }
    }

    fn update_target_config(
        &mut self,
        target: TargetId,
        config: crate::domain::settings::TargetConfig,
    ) -> bool {
        let program_path_changed =
            self.state
                .draft()
                .targets
                .get(&target)
                .is_some_and(|previous| {
                    previous.browser_root != config.browser_root
                        || previous.browser_directory != config.browser_directory
                });
        self.status_message = None;
        self.operation_phase = None;
        self.state.update_draft(|settings| {
            settings.targets.insert(target, config);
        });
        self.refresh_program_identity(target);
        if program_path_changed {
            self.local_versions.remove(&target);
            self.chrome_plus_versions.remove(&target);
            self.chrome_plus_ini_loaded.remove(&target);
        }
        self.auto_save()
    }

    fn show_health_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.health_dialog.clone() else {
            return;
        };
        let palette = match ctx.theme() {
            egui::Theme::Light => LIGHT_PALETTE,
            egui::Theme::Dark => DARK_PALETTE,
        };
        let mut close = false;
        let mut relocate = false;
        let mut apply_relocation = false;
        let mut cancel_relocation = false;
        let mut request_repair = false;
        let mut apply_repair = false;
        let mut cancel_repair = false;
        egui::Modal::new(egui::Id::unique("configuration_health"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(430.0);
                ui.heading("配置检查");
                ui.localized_label(
                    RichText::new(dialog.target.display_name())
                        .size(11.0)
                        .color(palette.muted),
                );
                ui.localized_label(
                    RichText::new(format!(
                        "检查结果：{}",
                        localized(health_state_label(dialog.report.state))
                    ))
                    .strong()
                    .color(health_state_color(dialog.report.state, palette)),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("configuration_health_items")
                    .max_height(230.0)
                    .show(ui, |ui| {
                        for item in &dialog.report.items {
                            ui.horizontal_wrapped(|ui| {
                                ui.localized_label(
                                    RichText::new(health_aspect_label(item.aspect))
                                        .strong()
                                        .color(palette.text),
                                );
                                ui.localized_label(
                                    RichText::new(health_state_label(item.state))
                                        .color(health_state_color(item.state, palette)),
                                );
                                ui.localized_label(
                                    RichText::new(&item.detail).color(palette.muted),
                                );
                            });
                        }
                    });
                let repair_label = if dialog.target.chrome_plus_mode() != ChromePlusMode::Disabled {
                    Some("修复 Chrome++ 路径")
                } else if matches!(
                    dialog.target,
                    TargetId::Firefox | TargetId::Zen | TargetId::Iceweasel | TargetId::SkyKakapo
                ) {
                    Some("修复便携配置")
                } else {
                    None
                };
                let repairing_portable =
                    dialog.target.chrome_plus_mode() == ChromePlusMode::Disabled;
                let can_repair = repair_label.is_some()
                    && dialog.report.items.iter().any(|item| {
                        item.aspect == HealthAspect::PortableConfig
                            && item.state >= HealthState::Warning
                    });
                if dialog.confirming_repair {
                    ui.separator();
                    ui.localized_label(
                        RichText::new(format!(
                            "将备份 {} 并只修复路径键。",
                            if dialog.target == TargetId::SkyKakapo {
                                "tmemutil.ini"
                            } else if repairing_portable {
                                "portable.ini"
                            } else {
                                "chrome++.ini"
                            }
                        ))
                        .strong()
                        .color(palette.warning),
                    );
                    ui.localized_label(
                        RichText::new(if repairing_portable {
                            "请先关闭浏览器；其他便携参数和注释会保留。"
                        } else {
                            "请先关闭浏览器；其他 Chrome++ 参数和注释会保留。"
                        })
                        .size(11.0)
                        .color(palette.muted),
                    );
                    ui.horizontal(|ui| {
                        if ui.localized_button("取消修复").clicked() {
                            cancel_repair = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.localized_button("确认修复").clicked() {
                                apply_repair = true;
                            }
                        });
                    });
                } else if can_repair
                    && repair_label.is_some_and(|label| ui.localized_button(label).clicked())
                {
                    request_repair = true;
                }
                if let Some(pending) = &dialog.pending_relocation {
                    ui.separator();
                    ui.localized_label(
                        RichText::new("将应用以下路径").strong().color(palette.text),
                    );
                    for (label, path) in [
                        ("浏览器根目录", pending.browser_root.as_deref()),
                        ("程序目录", pending.browser_directory.as_deref()),
                        ("用户数据目录", pending.data_directory.as_deref()),
                        ("Profile 目录", pending.profile_directory.as_deref()),
                        ("缓存目录", pending.cache_directory.as_deref()),
                    ] {
                        if let Some(path) = path {
                            ui.localized_label(
                                RichText::new(format!("{label}：{}", path.display()))
                                    .size(11.0)
                                    .color(palette.muted),
                            );
                        }
                    }
                    if let Some(warning) = &dialog.relocation_warning {
                        ui.localized_label(
                            RichText::new(warning).size(11.0).color(palette.warning),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ui.localized_button("取消重新定位").clicked() {
                            cancel_relocation = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.localized_button("应用").clicked() {
                                apply_relocation = true;
                            }
                        });
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.localized_button("关闭").clicked() {
                        close = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.localized_button("重新定位…").clicked() {
                            relocate = true;
                        }
                    });
                });
            });

        if close {
            self.health_dialog = None;
        }
        if cancel_relocation && let Some(dialog) = &mut self.health_dialog {
            dialog.pending_relocation = None;
            dialog.relocation_warning = None;
        }
        if request_repair && let Some(dialog) = &mut self.health_dialog {
            dialog.confirming_repair = true;
        }
        if cancel_repair && let Some(dialog) = &mut self.health_dialog {
            dialog.confirming_repair = false;
        }
        if apply_repair {
            let target = self.state.selected_target();
            if target.chrome_plus_mode() == ChromePlusMode::Disabled {
                self.repair_current_portable_paths();
            } else {
                self.repair_current_chromeplus_paths();
            }
        }
        if apply_relocation {
            self.apply_staged_relocation();
        }
        if relocate {
            let target = self.state.selected_target();
            let current = self.state.draft().targets[&target]
                .browser_root
                .as_deref()
                .and_then(|path| resolve_preview_path(self.state.tool_root(), path));
            if let Some(root) = folder_picker::pick_folder(
                &localized("选择移动后的浏览器根目录"),
                current.as_deref(),
            ) {
                self.stage_relocation(root);
            }
        }
    }

    fn show_same_version_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.same_version_prompt.clone() else {
            return;
        };
        let palette = match ctx.theme() {
            egui::Theme::Light => LIGHT_PALETTE,
            egui::Theme::Dark => DARK_PALETTE,
        };
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("same_version_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.heading("版本与上游相同");
                ui.add_space(7.0);
                egui::Grid::new("same_version_meta")
                    .num_columns(2)
                    .spacing([14.0, 6.0])
                    .show(ui, |ui| {
                        ui.localized_label("当前版本");
                        ui.localized_label(&prompt.current_version);
                        ui.end_row();
                        ui.localized_label(match prompt.upstream.source {
                            MetadataSource::Official => "上游版本",
                            MetadataSource::SignedManifest => "签名发布清单",
                        });
                        ui.localized_label(format!(
                            "{}（{}）",
                            prompt.upstream.version,
                            checked_age(prompt.upstream.checked_at)
                        ));
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.localized_label(
                    RichText::new(
                        "继续将重新下载并替换浏览器程序目录。用户数据、缓存和可按规则恢复的 Chrome++ 配置会保留。",
                    )
                    .color(palette.muted),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button("继续替换").clicked();
                    cancel = ui.localized_button("保留当前版本").clicked();
                });
            });
        if response.should_close() {
            cancel = true;
        }
        if confirm {
            self.same_version_prompt = None;
            self.start_operation(OperationMode::Install, true);
        } else if cancel {
            self.same_version_prompt = None;
            self.operation_phase = None;
            self.status_message = Some("已取消同版本替换".to_owned());
        }
    }

    fn show_chrome_plus_update_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.chrome_plus_update_prompt.clone() else {
            return;
        };
        if prompt.target != self.state.selected_target() {
            self.chrome_plus_update_prompt = None;
            return;
        }
        let palette = match ctx.theme() {
            egui::Theme::Light => LIGHT_PALETTE,
            egui::Theme::Dark => DARK_PALETTE,
        };
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("chromeplus_update_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.heading("更新 Chrome++");
                ui.add_space(7.0);
                ui.localized_label(
                    RichText::new(format!(
                        "Chrome++ {} → {}",
                        prompt.current_version.as_deref().unwrap_or("—"),
                        prompt.latest_version
                    ))
                    .strong()
                    .color(palette.text),
                );
                ui.localized_label(format!("当前浏览器：{}", prompt.target.display_name()));
                ui.add_space(8.0);
                ui.localized_label(
                    RichText::new(
                        "将只替换 Chrome++ 文件，保留浏览器程序、用户数据和缓存；旧 INI 会先备份。",
                    )
                    .color(palette.muted),
                );
                ui.localized_label(
                    RichText::new(
                        "请保持所有当前浏览器实例关闭。点击更新后会再次检测；仍在运行的实例不会被强制结束。",
                    )
                    .color(palette.warning),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button("更新 Chrome++").clicked();
                    cancel = ui.localized_button("取消").clicked();
                });
            });
        if response.should_close() {
            cancel = true;
        }
        if confirm {
            self.confirm_chrome_plus_update();
        } else if cancel {
            self.chrome_plus_update_prompt = None;
            self.status_message = Some("已取消 Chrome++ 更新".to_owned());
        }
    }

    fn show_browser_upgrade_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.browser_upgrade_prompt.clone() else {
            return;
        };
        if prompt.target != self.state.selected_target() {
            self.browser_upgrade_prompt = None;
            return;
        }
        let palette = match ctx.theme() {
            egui::Theme::Light => LIGHT_PALETTE,
            egui::Theme::Dark => DARK_PALETTE,
        };
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("browser_upgrade_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.heading("同时更新 Chrome++");
                ui.add_space(7.0);
                ui.localized_label(format!(
                    "浏览器：{} → {}",
                    prompt.current_browser, prompt.upstream_browser
                ));
                ui.localized_label(
                    RichText::new(format!(
                        "Chrome++：{} → {}",
                        prompt.chrome_plus_current, prompt.chrome_plus_latest
                    ))
                    .strong()
                    .color(palette.accent_text),
                );
                ui.add_space(8.0);
                ui.localized_label(
                    RichText::new(
                        if prompt.replace_same_version {
                            "浏览器版本相同，继续会重新下载并替换浏览器，同时更新 Chrome++。"
                        } else {
                            "浏览器升级会替换程序目录，因此本次操作也会安装已检测到的 Chrome++ 新版本。"
                        },
                    )
                    .color(palette.muted),
                );
                ui.localized_label(
                    RichText::new(
                        "用户数据、缓存和可恢复的 Chrome++ 配置会保留；请先关闭当前浏览器的所有实例。",
                    )
                    .color(palette.warning),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button("继续浏览器操作").clicked();
                    cancel = ui.localized_button("取消").clicked();
                });
            });
        if response.should_close() {
            cancel = true;
        }
        if confirm {
            self.browser_upgrade_prompt = None;
            self.start_operation_checked_with_expected(
                OperationMode::Install,
                prompt.replace_same_version,
                false,
                Some(prompt.chrome_plus_latest),
            );
        } else if cancel {
            self.browser_upgrade_prompt = None;
            self.status_message = Some("已取消浏览器操作".to_owned());
        }
    }

    fn show_identity_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.identity_prompt.clone() else {
            return;
        };
        if prompt.target != self.state.selected_target() {
            self.identity_prompt = None;
            return;
        }
        let palette = match ctx.theme() {
            egui::Theme::Light => LIGHT_PALETTE,
            egui::Theme::Dark => DARK_PALETTE,
        };
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("browser_identity_confirmation"))
            .frame(
                Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.heading("浏览器身份不一致");
                ui.add_space(7.0);
                ui.localized_label(format!(
                    "当前目标：{}",
                    prompt.target.display_name()
                ));
                ui.localized_label(format!(
                    "目录中的程序：{}（{}）",
                    prompt.actual.display_name(),
                    prompt.product_name
                ));
                ui.add_space(8.0);
                ui.localized_label(
                    RichText::new(
                        "继续安装或替换可能覆盖另一个浏览器的程序文件。建议重新选择目录；只有确认这是自定义构建时才继续。",
                    )
                    .color(palette.warning),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirm = ui.localized_button("仍然继续").clicked();
                    cancel = ui.localized_button("重新选择目录").clicked();
                });
            });
        if response.should_close() {
            cancel = true;
        }
        if confirm {
            self.identity_prompt = None;
            self.start_operation_checked(OperationMode::Install, prompt.replace_same_version, true);
        } else if cancel {
            self.identity_prompt = None;
            self.operation_phase = None;
            self.status_message = Some("已取消浏览器安装".to_owned());
        }
    }

    fn show_footer(&mut self, ui: &mut egui::Ui) {
        let palette = palette(ui);
        let operation_active = self.operation.is_some();
        let current_theme = self.state.draft().theme;
        let current_language = self.state.draft().language;
        let mut requested_theme = None;
        let mut requested_language = None;
        let mut requested_tab = None;
        let error = self
            .startup_error
            .as_deref()
            .map(|error| format!("配置加载失败：{error}"))
            .or_else(|| {
                self.state
                    .last_error()
                    .map(|error| format!("保存失败：{error}"))
            });
        let identity_warning =
            identity_warning(&self.program_identity, self.state.selected_target());
        let status = if let Some(error) = &error {
            Some((error.clone(), palette.error))
        } else if let Some(warning) = &identity_warning {
            Some((warning.clone(), palette.warning))
        } else if operation_active {
            self.operation_phase
                .as_ref()
                .map(|phase| (operation_status(phase), palette.muted))
        } else if let Some(message) = &self.status_message {
            Some((message.clone(), palette.muted))
        } else if let Some(OperationPhase::Failed { message }) = self.operation_phase.as_ref() {
            Some((message.clone(), palette.error))
        } else {
            None
        };
        let mut cancel_operation = false;

        egui::Panel::bottom("footer")
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(10, 7))
                    .stroke(Stroke::new(1.0, palette.border)),
            )
            .show(ui, |ui| {
                if let Some(phase) = self.operation_phase.as_ref()
                    && operation_active
                    && let Some((downloaded, total)) = operation_progress(phase)
                {
                    let fraction = total
                        .filter(|total| *total > 0)
                        .map_or(0.0, |total| downloaded as f32 / total as f32);
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(ui.available_width())
                            .show_percentage(),
                    );
                    ui.add_space(3.0);
                }
                egui::Sides::new()
                    .height(24.0)
                    .spacing(8.0)
                    .shrink_left()
                    .truncate()
                    .show(
                        ui,
                        |ui| {
                            ui.localized_label(
                                RichText::new("自动保存已开启")
                                    .size(11.0)
                                    .color(palette.muted),
                            );
                            if let Some((text, color)) = &status {
                                ui.separator();
                                let display_text = localized(text);
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(display_text).size(11.5).color(*color),
                                    )
                                    .show_tooltip_when_elided(false)
                                    .truncate(),
                                )
                                .localized_hover(text);
                            }
                        },
                        |ui| {
                            ui.hyperlink_to(
                                RichText::new(format!("ver {}", env!("CARGO_PKG_VERSION")))
                                    .size(10.5)
                                    .color(palette.muted),
                                RELEASES_URL,
                            )
                            .localized_hover("查看发布版本");
                            ui.add_space(5.0);

                            let language_menu = egui::containers::menu::MenuButton::from_button(
                                localized_button(if current_language == Language::English {
                                    "EN"
                                } else {
                                    "中"
                                })
                                .min_size(egui::vec2(34.0, 24.0)),
                            );
                            let (language, _) = language_menu.ui(ui, |ui| {
                                for (language, label) in
                                    [(Language::Chinese, "中文"), (Language::English, "English")]
                                {
                                    if ui
                                        .selectable_label(language == current_language, label)
                                        .clicked()
                                    {
                                        requested_language = Some(language);
                                        ui.close();
                                    }
                                }
                            });
                            language.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::Button,
                                    true,
                                    format!(
                                        "{}: {}",
                                        localized("界面语言"),
                                        if current_language == Language::English {
                                            "English"
                                        } else {
                                            "中文"
                                        }
                                    ),
                                )
                            });
                            language.localized_hover("切换界面语言");

                            let theme = ui
                                .menu_button("   ", |ui| {
                                    ui.horizontal(|ui| {
                                        for theme in
                                            [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark]
                                        {
                                            if theme_icon_button(ui, theme, theme == current_theme)
                                                .clicked()
                                            {
                                                requested_theme = Some(theme);
                                                ui.close();
                                            }
                                        }
                                    });
                                })
                                .response;
                            let visuals = ui.style().interact(&theme);
                            paint_theme_icon(
                                ui.painter(),
                                current_theme,
                                theme.rect.left_center() + egui::vec2(13.0, 0.0),
                                6.5,
                                if theme.hovered() {
                                    palette.accent_text
                                } else {
                                    palette.text
                                },
                                visuals.weak_bg_fill,
                            );
                            theme.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::Button,
                                    true,
                                    localized(format!(
                                        "外观：{}",
                                        localized(theme_label(current_theme))
                                    )),
                                )
                            });
                            theme.localized_hover(format!(
                                "外观：{}（点击切换）",
                                localized(theme_label(current_theme))
                            ));

                            let network =
                                ui.add_sized([30.0, 24.0], localized_button("").frame(false));
                            let network_active = self.active_tab == DetailTab::Network;
                            let network_background = if network_active {
                                palette.accent_soft
                            } else if network.hovered() || network.has_focus() {
                                palette.hover
                            } else {
                                ui.visuals().window_fill
                            };
                            ui.painter().rect_filled(
                                network.rect,
                                CornerRadius::same(5),
                                network_background,
                            );
                            if network_active || network.has_focus() {
                                ui.painter().rect_stroke(
                                    network.rect,
                                    CornerRadius::same(5),
                                    Stroke::new(1.0, palette.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            paint_settings_icon(
                                ui.painter(),
                                network.rect.center(),
                                if network.hovered() || network_active {
                                    palette.accent_text
                                } else {
                                    palette.text
                                },
                            );
                            network.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::Button,
                                    true,
                                    network_active,
                                    localized("全局设置"),
                                )
                            });
                            if network
                                .localized_hover(if network_active {
                                    "再次点击返回浏览器设置"
                                } else {
                                    "全局设置：下载加速、代理、超时与缓存清理"
                                })
                                .clicked()
                            {
                                requested_tab = Some(if network_active {
                                    DetailTab::Overview
                                } else {
                                    DetailTab::Network
                                });
                            }

                            if operation_active {
                                let cancelable = self
                                    .operation_phase
                                    .as_ref()
                                    .is_some_and(operation_cancelable);
                                cancel_operation = ui
                                    .add_enabled(
                                        cancelable,
                                        localized_button("取消任务")
                                            .fill(palette.control)
                                            .stroke(Stroke::new(1.0, palette.border))
                                            .min_size(egui::vec2(82.0, 26.0)),
                                    )
                                    .clicked();
                            }
                        },
                    );
            });

        if cancel_operation && let Some(operation) = &self.operation {
            operation.cancel();
        }
        if let Some(theme) = requested_theme {
            self.state.update_draft(|settings| settings.theme = theme);
            apply_theme_preference(ui.ctx(), theme);
            ui.ctx().request_repaint();
            self.operation_phase = None;
            if self.auto_save() {
                self.status_message = None;
            }
        }
        if let Some(language) = requested_language {
            self.state
                .update_draft(|settings| settings.language = language);
            set_language(language);
            ui.ctx().request_repaint();
            self.operation_phase = None;
            if self.auto_save() {
                self.status_message = None;
            }
        }
        if let Some(tab) = requested_tab {
            self.active_tab = tab;
        }
    }
}

impl eframe::App for BrowserHarborApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        set_language(self.state.draft().language);
        self.poll_operation();
        let window_position = frame.winit_window().and_then(|window| {
            let position = window.outer_position().ok()?;
            let position = position.to_logical::<f32>(window.scale_factor());
            Some(WindowPosition {
                x: position.x.round() as i32,
                y: position.y.round() as i32,
            })
        });
        self.track_window_position(ui.ctx(), window_position);
        if self.operation.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let palette = palette(ui);
        self.show_footer(ui);

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(palette.canvas)
                    .inner_margin(Margin::symmetric(12, 6)),
            )
            .show(ui, |ui| {
                let forms_enabled = self.operation.is_none();
                ui.add_enabled_ui(forms_enabled, |ui| {
                    if self.active_tab != DetailTab::Network {
                        self.show_tabs(ui);
                        ui.add_space(3.0);
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("settings")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.show_tab_content(ui);
                        });
                });
            });
        self.show_same_version_prompt(ui.ctx());
        self.show_chrome_plus_update_prompt(ui.ctx());
        self.show_browser_upgrade_prompt(ui.ctx());
        self.show_identity_prompt(ui.ctx());
        self.show_health_dialog(ui.ctx());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_window_position();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusTone {
    Neutral,
    Info,
    Success,
    Warning,
    Error,
}

fn health_state_label(state: HealthState) -> &'static str {
    match state {
        HealthState::NotInstalled => "未安装",
        HealthState::Healthy => "正常",
        HealthState::Warning => "有警告",
        HealthState::Error => "有错误",
    }
}

fn health_state_color(state: HealthState, palette: ThemePalette) -> Color32 {
    match state {
        HealthState::NotInstalled => palette.muted,
        HealthState::Healthy => palette.success,
        HealthState::Warning => palette.warning,
        HealthState::Error => palette.error,
    }
}

fn health_aspect_label(aspect: HealthAspect) -> &'static str {
    match aspect {
        HealthAspect::BrowserRoot => "浏览器根目录",
        HealthAspect::Program => "程序",
        HealthAspect::Version => "版本",
        HealthAspect::Runtime => "便携运行时",
        HealthAspect::Data => "用户数据目录",
        HealthAspect::Profile => "Profile 目录",
        HealthAspect::Cache => "缓存目录",
        HealthAspect::PortableConfig => "便携配置",
    }
}

fn theme_icon_button(ui: &mut egui::Ui, theme: ThemeMode, selected: bool) -> egui::Response {
    let response = ui.add_sized([28.0, 25.0], localized_button("").frame(false));
    let palette = palette(ui);
    let visuals = ui.style().interact(&response);
    let background = if selected {
        palette.accent_soft
    } else if response.hovered() || response.has_focus() {
        palette.hover
    } else {
        ui.visuals().window_fill
    };
    ui.painter()
        .rect_filled(response.rect, CornerRadius::same(5), background);
    if selected || response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(5),
            Stroke::new(1.0, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    let color = if selected {
        palette.accent_text
    } else if response.hovered() {
        palette.text
    } else {
        visuals.fg_stroke.color
    };
    paint_theme_icon(
        ui.painter(),
        theme,
        response.rect.center(),
        6.5,
        color,
        background,
    );
    let label = theme_label(theme);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    response.localized_hover(label)
}

fn paint_settings_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    for (offset, knob) in [(-5.0, -2.2), (0.0, 2.2), (5.0, -2.2)] {
        let y = center.y + offset;
        painter.line_segment(
            [egui::pos2(center.x - 6.0, y), egui::pos2(center.x + 6.0, y)],
            stroke,
        );
        painter.circle_filled(egui::pos2(center.x + knob, y), 2.0, color);
    }
}

fn paint_refresh_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = Stroke::new(1.25, color);
    let radius = 5.5;
    let start = -0.65 * std::f32::consts::PI;
    let sweep = 1.72 * std::f32::consts::PI;
    let points = (0..=22)
        .map(|index| {
            let angle = start + sweep * index as f32 / 22.0;
            center + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));

    let end = start + sweep;
    let tip = center + egui::vec2(end.cos(), end.sin()) * radius;
    let tangent = egui::vec2(-end.sin(), end.cos());
    let radial = egui::vec2(end.cos(), end.sin());
    let back = tip - tangent * 3.4;
    painter.line_segment([tip, back + radial * 2.1], stroke);
    painter.line_segment([tip, back - radial * 2.1], stroke);
}

fn paint_update_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.line_segment(
        [
            center + egui::vec2(0.0, -6.0),
            center + egui::vec2(0.0, 3.5),
        ],
        stroke,
    );
    painter.line_segment(
        [
            center + egui::vec2(-3.5, 0.0),
            center + egui::vec2(0.0, 3.5),
        ],
        stroke,
    );
    painter.line_segment(
        [center + egui::vec2(3.5, 0.0), center + egui::vec2(0.0, 3.5)],
        stroke,
    );
    painter.line_segment(
        [
            center + egui::vec2(-6.0, 6.0),
            center + egui::vec2(6.0, 6.0),
        ],
        stroke,
    );
}

fn browser_selector(ui: &mut egui::Ui, selected: TargetId, width: f32) -> Option<TargetId> {
    let palette = palette(ui);
    let mut requested = None;
    let label = ui.localized_label(RichText::new("浏览器").color(palette.muted));
    egui::ComboBox::from_id_salt("browser_target")
        .selected_text(selected.display_name())
        .width(width)
        .height(260.0)
        .show_ui(ui, |ui| {
            ui.set_min_width(350.0);
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.columns(BrowserFamily::ALL.len(), |columns| {
                for (column, family) in columns.iter_mut().zip(BrowserFamily::ALL) {
                    column.label(
                        RichText::new(family_label(family))
                            .size(10.5)
                            .color(palette.muted),
                    );
                    for target in TargetId::ALL
                        .into_iter()
                        .filter(|target| target.family() == family)
                    {
                        let response = column
                            .push_id(target.sort_key(), |ui| {
                                ui.selectable_label(target == selected, target.display_name())
                            })
                            .inner;
                        if response.clicked() {
                            requested = Some(target);
                        }
                    }
                }
            });
        })
        .response
        .labelled_by(label.id);
    requested
}

fn paint_theme_icon(
    painter: &egui::Painter,
    theme: ThemeMode,
    center: egui::Pos2,
    radius: f32,
    color: Color32,
    background: Color32,
) {
    let stroke = Stroke::new(1.35, color);
    match theme {
        ThemeMode::System => {
            let mut points = Vec::with_capacity(14);
            for index in 0..=12 {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * index as f32 / 12.0;
                points.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
            }
            points.push(center);
            painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
            painter.circle_stroke(center, radius, stroke);
        }
        ThemeMode::Light => {
            painter.circle_stroke(center, radius * 0.42, stroke);
            for index in 0..8 {
                let angle = std::f32::consts::FRAC_PI_4 * index as f32;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [
                        center + direction * radius * 0.70,
                        center + direction * radius * 1.08,
                    ],
                    stroke,
                );
            }
        }
        ThemeMode::Dark => {
            painter.circle_filled(center, radius, color);
            painter.circle_filled(
                center + egui::vec2(radius * 0.48, -radius * 0.28),
                radius * 0.88,
                background,
            );
        }
    }
}

fn relation_status(
    has_local: bool,
    has_upstream: bool,
    relation: VersionRelation,
) -> Option<(&'static str, StatusTone)> {
    match (has_local, has_upstream) {
        (false, _) => Some(("无法比较", StatusTone::Error)),
        (true, false) => Some(("尚未检查上游版本", StatusTone::Neutral)),
        (true, true) => Some(match relation {
            VersionRelation::Same => ("与上游版本相同", StatusTone::Success),
            VersionRelation::UpstreamNewer => ("上游有新版本", StatusTone::Info),
            VersionRelation::UpstreamOlder => ("上游版本较旧", StatusTone::Warning),
            VersionRelation::Different => ("上游版本不同", StatusTone::Warning),
            VersionRelation::Unknown => ("版本无法可靠比较", StatusTone::Warning),
        }),
    }
}

fn status_chip(ui: &mut egui::Ui, text: &str, tone: StatusTone) -> egui::Response {
    let palette = palette(ui);
    let (foreground, background) = match tone {
        StatusTone::Neutral => (palette.muted, palette.control),
        StatusTone::Info => (palette.accent_text, palette.accent_soft),
        StatusTone::Success => (palette.success, palette.success_soft),
        StatusTone::Warning => (palette.warning, palette.warning_soft),
        StatusTone::Error => (palette.error, palette.error_soft),
    };
    let border =
        Color32::from_rgba_unmultiplied(foreground.r(), foreground.g(), foreground.b(), 90);
    Frame::new()
        .fill(background)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(text).size(10.5).strong().color(foreground))
                    .extend(),
            )
        })
        .inner
}

fn family_label(family: BrowserFamily) -> &'static str {
    match family {
        BrowserFamily::Chromium => "Chromium 系",
        BrowserFamily::Firefox => "Firefox 系",
    }
}

fn version_source_label(source: VersionSource) -> &'static str {
    match source {
        VersionSource::ExecutableResource => "程序版本资源",
        VersionSource::ExecutableFileVersion => "程序文件版本",
        VersionSource::BraveProductVersion => "Brave 产品版本",
        VersionSource::ApplicationIni => "application.ini",
        VersionSource::InstallReceipt => "上次安装记录",
    }
}

fn chrome_plus_label(mode: ChromePlusMode) -> &'static str {
    match mode {
        ChromePlusMode::Full => "完整集成",
        ChromePlusMode::DataOnly => "实验性 DataOnly",
        ChromePlusMode::Disabled => "不使用",
    }
}

fn chrome_plus_update_button_label(current: Option<&str>, latest: Option<&str>) -> String {
    match (current, latest) {
        (Some(current), Some(latest))
            if compare_versions(current, latest) == VersionRelation::Same =>
        {
            "已最新".to_owned()
        }
        (Some(current), Some(latest))
            if compare_versions(current, latest) == VersionRelation::UpstreamNewer =>
        {
            "可更新".to_owned()
        }
        _ => "检查更新".to_owned(),
    }
}

fn chrome_plus_running_message(language: Language, target: TargetId, count: usize) -> String {
    let name = target.display_name();
    match language {
        Language::Chinese => {
            format!("检测到 {count} 个 {name} 进程；请手工关闭所有 {name} 实例后再更新 Chrome++")
        }
        Language::English => format!(
            "Detected {count} running {name} process(es); close every {name} instance manually before updating Chrome++"
        ),
    }
}

fn chrome_plus_update_action_label(language: Language, latest: &str) -> String {
    match language {
        Language::Chinese => format!("更新 Chrome++ 到 {latest}"),
        Language::English => format!("Update Chrome++ to {latest}"),
    }
}

fn chrome_plus_update_hover(language: Language, current: &str, latest: &str) -> String {
    match language {
        Language::Chinese => {
            format!(
                "发现 {current} → {latest}；点击后才会下载并替换，完成后版本才会变化；请先手工关闭所有当前浏览器实例"
            )
        }
        Language::English => format!(
            "Found {current} → {latest}; the version changes only after the confirmed download and replacement finishes. Close every instance of the current browser first"
        ),
    }
}

fn identity_warning(identity: &ProgramIdentity, target: TargetId) -> Option<String> {
    let ProgramIdentity::Mismatch {
        actual,
        product_name,
    } = identity
    else {
        return None;
    };
    Some(format!(
        "目录中的程序是 {}（{product_name}），当前目标是 {}；安装前会再次确认",
        actual.display_name(),
        target.display_name()
    ))
}

fn provider_label(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ChromeOmaha => "Google Omaha",
        ProviderKind::EdgeEnterprise => "Microsoft Edge Updates",
        ProviderKind::Mozilla => "Mozilla 官方",
        ProviderKind::GitHubRelease => "GitHub Release",
        ProviderKind::LibreWolf => "LibreWolf 官方",
        ProviderKind::Catsxp => "Catsxp 官方",
        ProviderKind::SkyKakapo => "SkyKakapo 官方",
        ProviderKind::Turbo => "Turbo 官方",
    }
}

fn chrome_plus_ini_path(
    target: TargetId,
    config: &crate::domain::settings::TargetConfig,
    tool_root: &std::path::Path,
) -> Option<PathBuf> {
    chrome_plus_program_directory(target, config, tool_root).and_then(|program| {
        let ini = program.join("chrome++.ini");
        ini.is_file().then_some(ini)
    })
}

fn chrome_plus_program_directory(
    target: TargetId,
    config: &crate::domain::settings::TargetConfig,
    tool_root: &std::path::Path,
) -> Option<PathBuf> {
    let configured = config
        .browser_directory
        .as_deref()
        .and_then(|path| resolve_preview_path(tool_root, path))
        .or_else(|| {
            config
                .browser_root
                .as_deref()
                .and_then(|root| resolve_preview_path(tool_root, root))
                .and_then(|root| {
                    target
                        .spec()
                        .layout
                        .program
                        .map(|program| root.join(program))
                })
        })?;
    locate_executable(target, &configured)
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
}

fn local_browser_executable(
    target: TargetId,
    config: &crate::domain::settings::TargetConfig,
    tool_root: &Path,
) -> Option<PathBuf> {
    let program = config
        .browser_directory
        .as_deref()
        .and_then(|path| resolve_preview_path(tool_root, path))
        .or_else(|| {
            config
                .browser_root
                .as_deref()
                .and_then(|root| resolve_preview_path(tool_root, root))
                .map(|root| root.join("App"))
        })?;
    locate_executable(target, &program).ok()
}

fn resolve_preview_path(root: &std::path::Path, path: &std::path::Path) -> Option<PathBuf> {
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else if path.has_root() {
        None
    } else {
        Some(root.join(path))
    }
}

fn operation_status(phase: &OperationPhase) -> String {
    match phase {
        OperationPhase::Preparing => "正在准备任务…".to_owned(),
        OperationPhase::Resolving => "正在检查官方发布…".to_owned(),
        OperationPhase::Notice(message) => message.clone(),
        OperationPhase::Downloading {
            resource,
            downloaded,
            total,
        } => match total {
            Some(total) => format!(
                "正在下载 {resource}：{} / {}",
                format_bytes(*downloaded),
                format_bytes(*total)
            ),
            None => format!("正在下载 {resource}：{}", format_bytes(*downloaded)),
        },
        OperationPhase::Verifying => "正在校验下载文件…".to_owned(),
        OperationPhase::Extracting => "正在解压…".to_owned(),
        OperationPhase::Installing => "正在替换程序目录…".to_owned(),
        OperationPhase::Checked { check, .. } => format!("检查完成：{}", check.version),
        OperationPhase::SameVersion {
            current_version,
            upstream,
            ..
        } => format!(
            "本地版本 {current_version} 与上游 {} 相同，等待确认",
            upstream.version
        ),
        OperationPhase::Installed { receipt } => format!("已安装 {}", receipt.version),
        OperationPhase::ChromePlusChecked { latest_version, .. } => {
            format!("Chrome++ 检查完成：{latest_version}")
        }
        OperationPhase::ChromePlusInstalled { version } => {
            format!("Chrome++ 已更新到 {version}")
        }
        OperationPhase::Cancelled => "任务已取消".to_owned(),
        OperationPhase::Failed { message } => message.clone(),
    }
}

fn operation_progress(phase: &OperationPhase) -> Option<(u64, Option<u64>)> {
    match phase {
        OperationPhase::Downloading {
            downloaded, total, ..
        } => Some((*downloaded, *total)),
        _ => None,
    }
}

fn operation_cancelable(phase: &OperationPhase) -> bool {
    matches!(
        phase,
        OperationPhase::Preparing | OperationPhase::Resolving | OperationPhase::Downloading { .. }
    )
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn checked_age(checked_at: u64) -> String {
    let now = unix_timestamp();
    let seconds = now.saturating_sub(checked_at);
    match seconds {
        0..=10 => "刚刚".to_owned(),
        11..=59 => "不足 1 分钟前".to_owned(),
        60..=3599 => format!("{} 分钟前", seconds / 60),
        3600..=86_399 => format!("{} 小时前", seconds / 3600),
        _ => format!("{} 天前", seconds / 86_400),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn section<R>(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let palette = palette(ui);
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.localized_label(RichText::new(title).size(12.0).strong().color(palette.text));
            ui.add_space(1.0);
            add_contents(ui)
        })
        .inner
}

fn section_with_header<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_header: impl FnOnce(&mut egui::Ui),
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let palette = palette(ui);
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Sides::new()
                .height(24.0)
                .spacing(6.0)
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        ui.localized_label(
                            RichText::new(title).size(12.0).strong().color(palette.text),
                        );
                    },
                    add_header,
                );
            ui.add_space(1.0);
            add_contents(ui)
        })
        .inner
}

fn section_with_action<R>(
    ui: &mut egui::Ui,
    title: &str,
    action: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    let palette = palette(ui);
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut clicked = false;
            ui.horizontal(|ui| {
                ui.localized_label(RichText::new(title).size(12.0).strong().color(palette.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    clicked = ui.small_button(localized(action)).clicked();
                });
            });
            ui.add_space(1.0);
            (add_contents(ui), clicked)
        })
        .inner
}

fn metadata_label(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 18.0),
        Layout::left_to_right(Align::Center),
        |ui| ui.localized_label(RichText::new(text).color(palette(ui).muted)),
    )
    .inner
}

fn field_label(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::hover());
    let mut label_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    label_ui.add(
        egui::Label::new(RichText::new(text).color(palette(&label_ui).text))
            .truncate()
            .halign(Align::LEFT),
    )
}

fn path_field(ui: &mut egui::Ui, label: &str, path: &mut Option<PathBuf>, hint: &str) -> bool {
    path_field_with_picker(ui, label, path, hint, folder_picker::pick_folder)
}

fn create_auto_shortcut(
    target: TargetId,
    enabled: bool,
    browser_root: &Path,
    program_directory: &Path,
) -> Result<Option<PathBuf>, String> {
    if !enabled {
        return Ok(None);
    }
    let executable =
        locate_executable(target, program_directory).map_err(|error| error.to_string())?;
    create_browser_shortcut(browser_root, target.display_name(), &executable)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn path_field_with_picker(
    ui: &mut egui::Ui,
    label: &str,
    path: &mut Option<PathBuf>,
    hint: &str,
    picker: fn(&str, Option<&Path>) -> Option<PathBuf>,
) -> bool {
    path_field_impl(ui, label, path, hint, picker, None).0
}

fn path_field_with_action(
    ui: &mut egui::Ui,
    label: &str,
    path: &mut Option<PathBuf>,
    hint: &str,
    action: &str,
    action_enabled: bool,
) -> (bool, Option<egui::Response>) {
    path_field_impl(
        ui,
        label,
        path,
        hint,
        folder_picker::pick_folder,
        Some((action, action_enabled)),
    )
}

fn path_field_impl(
    ui: &mut egui::Ui,
    label: &str,
    path: &mut Option<PathBuf>,
    hint: &str,
    picker: fn(&str, Option<&Path>) -> Option<PathBuf>,
    action: Option<(&str, bool)>,
) -> (bool, Option<egui::Response>) {
    let mut text = path
        .as_ref()
        .map_or_else(String::new, |path| path.display().to_string());
    let mut changed = false;
    let action_response = ui.horizontal(|ui| {
        let field_label =
            field_label(ui, label, FIELD_LABEL_WIDTH).localized_hover(localized(hint));
        let action_width = action.map_or(0.0, |_| 104.0 + ui.spacing().item_spacing.x);
        let text_width = (ui.available_width() - action_width).max(100.0);
        let response = ui
            .add_sized(
                [text_width, 24.0],
                egui::TextEdit::singleline(&mut text)
                    .hint_text(localized(hint))
                    .margin(Margin {
                        left: 4,
                        right: 30,
                        top: 2,
                        bottom: 2,
                    })
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(field_label.id);
        changed |= response.changed();
        let browse_rect = egui::Rect::from_min_size(
            response.rect.right_top() + egui::vec2(-28.0, 2.0),
            egui::vec2(24.0, 20.0),
        );
        let browse = ui
            .interact(
                browse_rect,
                field_label.id.with("browse"),
                egui::Sense::click(),
            )
            .localized_hover(select_label(label));
        if browse.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            ui.painter()
                .rect_filled(browse_rect, CornerRadius::same(3), palette(ui).hover);
        }
        ui.painter().text(
            browse_rect.center(),
            egui::Align2::CENTER_CENTER,
            "…",
            FontId::new(12.0, FontFamily::Proportional),
            palette(ui).muted,
        );
        browse.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, select_label(label))
        });
        if browse.clicked()
            && let Some(selected) = picker(&select_label(label), path.as_deref())
        {
            text = selected.display().to_string();
            changed = true;
        }
        action.map(|(label, enabled)| {
            ui.add_enabled(
                enabled,
                localized_button(label).min_size(egui::vec2(104.0, 24.0)),
            )
        })
    });
    if changed {
        *path = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
    }
    (changed, action_response.inner)
}

fn text_field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let label = field_label(ui, label, FIELD_LABEL_WIDTH);
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(value)
                .hint_text(localized(hint))
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id)
    })
    .inner
}

fn readonly_field(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        field_label(ui, label, FIELD_LABEL_WIDTH);
        ui.localized_label(RichText::new(value).color(palette(ui).muted));
    });
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        ops::{Deref, DerefMut},
        path::PathBuf,
        sync::Arc,
    };

    use eframe::egui::accesskit::Role;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use egui_system_font_provider::SystemFontProvider;
    use tempfile::tempdir;

    use super::{
        BODY_FONT_SIZE, BrowserHarborApp, DetailTab, LIGHT_PALETTE, ProgramIdentity, StatusTone,
        chrome_plus_update_button_label, chrome_plus_update_hover, configure_context,
        create_auto_shortcut, relation_status, set_language,
    };
    use crate::{
        app::{
            AppState,
            operation::{MetadataSource, UpstreamCheck},
        },
        config::ConfigStore,
        domain::{
            release::{CpuBuild, ReleaseChannel},
            settings::{Language, Settings, ThemeMode},
            target::TargetId,
        },
        infra::ini::{IniEncoding, read_ini, write_ini},
    };

    fn harness_for(app: BrowserHarborApp) -> Harness<'static, BrowserHarborApp> {
        Harness::builder()
            .with_size(eframe::egui::vec2(560.0, 420.0))
            .with_context_setup(|ctx| {
                ctx.add_font_provider(Arc::new(SystemFontProvider::new()));
                configure_context(ctx, ThemeMode::System);
            })
            .build_eframe(move |_cc| app)
    }

    struct TestHarness {
        harness: Harness<'static, BrowserHarborApp>,
        _directory: tempfile::TempDir,
    }

    impl Deref for TestHarness {
        type Target = Harness<'static, BrowserHarborApp>;

        fn deref(&self) -> &Self::Target {
            &self.harness
        }
    }

    impl DerefMut for TestHarness {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.harness
        }
    }

    fn harness() -> TestHarness {
        let directory = tempdir().unwrap();
        let state = AppState::new(
            Settings::default(),
            ConfigStore::new(directory.path().join("manager.json")),
        );
        TestHarness {
            harness: harness_for(BrowserHarborApp::new(state)),
            _directory: directory,
        }
    }

    #[test]
    fn ui_harness_removes_auto_saved_config_after_drop() {
        let marker = format!(
            r"D:\browser-harbor-tempdir-lifecycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        {
            let mut harness = harness();
            harness.run();
            harness
                .get_by_role_and_label(Role::TextInput, "浏览器根目录")
                .set_text_value_accesskit(&marker);
            harness.run();
        }

        let leaked = fs::read_dir(std::env::temp_dir())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
            .filter_map(|entry| fs::read(entry.path().join("manager.json")).ok())
            .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .any(|value| {
                value
                    .pointer("/targets/firefox/browserRoot")
                    .and_then(serde_json::Value::as_str)
                    == Some(marker.as_str())
            });

        assert!(
            !leaked,
            "UI test config survived after its TempDir owner was dropped"
        );
    }

    fn firefox_harness_with_versions(
        local_version: &str,
        upstream_version: &str,
    ) -> (tempfile::TempDir, Harness<'static, BrowserHarborApp>) {
        let directory = tempdir().unwrap();
        let browser = directory.path().join("Firefox");
        let program = browser.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::write(program.join("firefox.exe"), b"fixture").unwrap();
        fs::write(
            program.join("application.ini"),
            format!("[App]\r\nName=Firefox\r\nVersion={local_version}\r\n"),
        )
        .unwrap();

        let mut settings = Settings {
            selected_browser: TargetId::Firefox,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Firefox).unwrap();
        config.browser_root = Some(browser);
        config.browser_directory = Some(program);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.state_mut().latest_versions.insert(
            (TargetId::Firefox, ReleaseChannel::Stable, CpuBuild::X64),
            UpstreamCheck {
                version: upstream_version.to_owned(),
                channel: ReleaseChannel::Stable,
                cpu_build: CpuBuild::X64,
                source: MetadataSource::Official,
                checked_at: 0,
            },
        );
        (directory, harness)
    }

    #[test]
    fn browser_combo_uses_two_ordered_groups_with_sorted_members() {
        let mut harness = harness();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "浏览器")
            .click();
        harness.run();

        let list = harness.get_by_role(Role::ListBox);
        let chromium = list.get_by_label("Chromium 系");
        let firefox = list.get_by_label("Firefox 系");
        assert_eq!(chromium.accesskit_node().role(), Role::Label);
        assert_eq!(firefox.accesskit_node().role(), Role::Label);
        assert!(chromium.rect().min.x < firefox.rect().min.x);

        let chromium_members = [
            "Brave",
            "Catsxp",
            "Chrome",
            "Edge",
            "Helium",
            "Thorium",
            "Turbo",
            "Ungoogled Chromium",
        ];
        let firefox_members = [
            "Firefox",
            "Floorp",
            "Iceweasel",
            "LibreWolf",
            "SkyKakapo",
            "Zen",
        ];
        let mut previous_y = chromium.rect().min.y;
        for member in chromium_members {
            let y = list.get_by_label(member).rect().min.y;
            assert!(y > previous_y, "{member} is out of order");
            previous_y = y;
        }
        let mut previous_y = firefox.rect().min.y;
        for member in firefox_members {
            let y = list.get_by_label(member).rect().min.y;
            assert!(y > previous_y, "{member} is out of order");
            previous_y = y;
        }
    }

    #[test]
    fn selecting_chrome_changes_the_configured_target() {
        let mut harness = harness();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "浏览器")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Chrome")
            .click();

        harness.run();

        assert_eq!(harness.state().state().selected_target(), TargetId::Chrome);
        assert_eq!(
            harness.state().state().saved().selected_browser,
            TargetId::Chrome
        );
        harness.get_by_label("Chrome++");
    }

    #[test]
    fn switching_targets_keeps_each_browser_paths_isolated() {
        let directory = tempdir().unwrap();
        let tool_root = directory.path().join("Browser");
        let thorium_root = tool_root.join("Thorium_AVX512");
        let mut settings = Settings {
            selected_browser: TargetId::Thorium,
            ..Settings::default()
        };
        let thorium = settings.targets.get_mut(&TargetId::Thorium).unwrap();
        thorium.browser_root = Some(thorium_root.clone());
        thorium.browser_directory = Some(thorium_root.join("BIN"));
        thorium.data_directory = Some(thorium_root.join("USER_DATA"));
        thorium.profile_directory = Some(thorium_root.join("USER_DATA"));
        thorium.cache_directory = None;
        let state = AppState::with_tool_root(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
            tool_root.clone(),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        for (target, expected_name) in [
            (TargetId::Iceweasel, "Iceweasel"),
            (TargetId::SkyKakapo, "SkyKakapo"),
        ] {
            harness
                .get_by_role_and_label(Role::ComboBox, "浏览器")
                .click();
            harness.run();
            harness
                .get_by_role(Role::ListBox)
                .get_by_label(expected_name)
                .click();
            harness.run();

            assert_eq!(harness.state().state().saved().selected_browser, target);
            let config = &harness.state().state().saved().targets[&target];
            assert_eq!(
                config.browser_root,
                Some(PathBuf::from(format!(r".\{expected_name}")))
            );
            assert_eq!(
                config.browser_directory,
                Some(PathBuf::from(format!(r".\{expected_name}\App")))
            );
            assert_eq!(
                harness
                    .get_by_role_and_label(Role::TextInput, "浏览器根目录")
                    .accesskit_node()
                    .value(),
                Some(format!(r".\{expected_name}"))
            );
            assert_eq!(
                harness
                    .get_by_role_and_label(Role::TextInput, "程序目录")
                    .accesskit_node()
                    .value(),
                Some(format!(r".\{expected_name}\App"))
            );
        }

        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Thorium].browser_root,
            Some(thorium_root)
        );
    }

    #[test]
    fn failed_target_switch_keeps_the_previous_browser_selected() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("manager.json");
        fs::create_dir(directory.path().join("manager.json.tmp")).unwrap();
        let state = AppState::new(Settings::default(), ConfigStore::new(config_path));
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness.get_by_role(Role::ComboBox).click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Chrome")
            .click();
        harness.run();

        assert_eq!(harness.state().state().selected_target(), TargetId::Firefox);
        assert_eq!(
            harness.state().state().saved().selected_browser,
            TargetId::Firefox
        );
        assert!(harness.state().state().last_error().is_some());
    }

    #[test]
    fn mismatched_browser_identity_requires_install_confirmation() {
        let mut harness = harness();
        harness.run();
        harness.state_mut().program_identity = ProgramIdentity::Mismatch {
            actual: TargetId::Helium,
            product_name: "Helium".to_owned(),
        };
        harness.run();

        harness.get_by_label("下载并安装").click();
        harness.run();

        harness.get_by_label("浏览器身份不一致");
        harness.get_by_label("仍然继续");
        harness.get_by_label("重新选择目录");
        assert!(harness.state().identity_prompt.is_some());
    }

    #[test]
    fn chromeplus_tab_is_disabled_for_firefox_and_enabled_for_chrome() {
        let mut harness = harness();
        harness.run();
        harness.get_by_role_and_label(Role::Button, "浏览器");
        harness.get_by_label("全局设置");
        harness.get_by_label("自动保存已开启");
        assert!(harness.query_by_label("高级").is_none());
        assert!(harness.query_by_label("支持级别").is_none());
        assert!(
            harness
                .get_by_label("Chrome++")
                .accesskit_node()
                .is_disabled()
        );

        harness.get_by_role(Role::ComboBox).click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Chrome")
            .click();
        harness.run();

        assert!(
            !harness
                .get_by_label("Chrome++")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn overview_integrates_channel_and_build_selectors() {
        let mut harness = harness();
        harness.run();
        harness.get_by_role(Role::ComboBox).click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Iceweasel")
            .click();
        harness.run();

        harness
            .get_by_role_and_label(Role::ComboBox, "渠道")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Release")
            .click();
        harness.run();
        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Iceweasel].channel,
            ReleaseChannel::Release
        );

        harness
            .get_by_role_and_label(Role::ComboBox, "浏览器")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Thorium")
            .click();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "构建")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("AVX512")
            .click();
        harness.run();
        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Thorium].cpu_build,
            CpuBuild::Avx512
        );
    }

    #[test]
    fn overview_uses_inline_metadata_and_action_rows() {
        let mut harness = harness();
        harness.run();

        let y = |label: &str| harness.get_by_label(label).rect().center().y;
        let x = |label: &str| harness.get_by_label(label).rect().min.x;
        let browser = harness
            .get_by_role_and_label(Role::ComboBox, "浏览器")
            .rect();
        assert!((browser.center().y - y("渠道")).abs() < 1.0);
        assert!((y("渠道") - y("构建")).abs() < 1.0);
        assert!((y("构建") - y("来源")).abs() < 1.0);
        assert!(browser.min.x < x("渠道") && x("渠道") < x("构建") && x("构建") < x("来源"));
        assert!((y("当前版本") - y("上游版本")).abs() < 1.0);
        assert!(y("当前版本") > browser.center().y);
        assert!(harness.query_by_label("内核").is_none());
        assert!(harness.query_by_label("配置").is_none());
        assert!(harness.query_by_label("比较").is_none());

        let check = harness.get_by_label("检查更新").rect();
        let install = harness.get_by_label("下载并安装").rect();
        let local = harness.get_by_label("载入本地包").rect();
        assert!((check.center().y - install.center().y).abs() < 1.0);
        assert!((install.center().y - local.center().y).abs() < 1.0);
        assert!(check.max.x < install.min.x && install.max.x < local.min.x);
    }

    #[test]
    fn overview_has_no_scroll_bar() {
        let mut harness = harness();
        harness.run();

        assert!(harness.query_by_role(Role::ScrollBar).is_none());
    }

    #[test]
    fn footer_shows_version_link_at_the_right_edge() {
        assert_eq!(
            super::RELEASES_URL,
            "https://github.com/MagicCube99/browser-harbor/releases"
        );
        let mut harness = harness();
        harness.run();

        let version_label = format!("ver {}", env!("CARGO_PKG_VERSION"));
        let version = harness.get_by_role_and_label(Role::Link, &version_label);
        let network = harness.get_by_label("全局设置");
        let theme = harness.get_by_label("外观：跟随系统");

        assert!(version.rect().min.x > network.rect().max.x);
        assert!(version.rect().min.x > theme.rect().max.x);
    }

    #[test]
    fn footer_truncates_long_status_before_right_controls() {
        let mut harness = harness();
        harness.run();
        let message = format!(
            "Firefox 158.0.1 已安装；快捷方式已创建到 {}；这条状态信息故意很长以验证省略显示",
            r"D:\Tools\Browser\Firefox\Firefox.lnk"
        );
        harness.state_mut().status_message = Some(message.clone());
        harness.run();

        let status = harness.get_by_label(&message);
        let network = harness.get_by_label("全局设置");
        let theme = harness.get_by_label("外观：跟随系统");
        let language = harness.get_by_role_and_label(Role::Button, "界面语言: 中文");
        let version_label = format!("ver {}", env!("CARGO_PKG_VERSION"));
        let version = harness.get_by_role_and_label(Role::Link, &version_label);

        assert!(status.rect().max.x < network.rect().min.x);
        assert!(status.rect().max.x < theme.rect().min.x);
        assert!(status.rect().max.x < language.rect().min.x);
        assert!(status.rect().max.x < version.rect().min.x);
        let gap = network.rect().min.x - status.rect().max.x;
        assert!(
            gap <= 12.0,
            "truncated footer status should use the available width; gap was {gap}"
        );
    }

    #[test]
    fn local_package_has_an_explicit_action() {
        let mut harness = harness();
        harness.run();

        harness.get_by_label("载入本地包");
    }

    #[test]
    fn install_action_is_available_with_the_default_browser_root() {
        let mut harness = harness();
        harness.run();

        assert!(
            !harness
                .get_by_label("下载并安装")
                .accesskit_node()
                .is_disabled()
        );
        harness.get_by_label("检查更新");

        harness
            .get_by_role_and_label(Role::TextInput, "浏览器根目录")
            .set_text_value_accesskit(r"D:\Browsers\Firefox");
        harness.run();

        assert!(
            !harness
                .get_by_label("下载并安装")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn version_status_uses_semantic_tone_and_keeps_missing_state_distinct() {
        use crate::domain::version::VersionRelation;

        assert_eq!(
            relation_status(false, true, VersionRelation::Unknown),
            Some(("无法比较", StatusTone::Error))
        );
        assert_eq!(
            relation_status(true, true, VersionRelation::Same),
            Some(("与上游版本相同", StatusTone::Success))
        );
        assert_eq!(
            relation_status(true, true, VersionRelation::UpstreamNewer),
            Some(("上游有新版本", StatusTone::Info))
        );
        assert_eq!(
            relation_status(true, true, VersionRelation::UpstreamOlder),
            Some(("上游版本较旧", StatusTone::Warning))
        );
    }

    #[test]
    fn detected_same_version_uses_replace_label_and_requires_confirmation() {
        let (_directory, mut harness) = firefox_harness_with_versions("158.0.1", "158.0.1");
        harness.run();

        harness.get_by_label("与上游版本相同");
        harness.get_by_label("重新下载并替换").click();
        harness.run();
        harness.get_by_label("版本与上游相同");
        harness.get_by_label("继续替换");
        harness.get_by_label("保留当前版本").click();
        harness.run();

        assert!(harness.query_by_label("版本与上游相同").is_none());
        harness.get_by_label("已取消同版本替换");
    }

    #[test]
    fn upstream_direction_drives_upgrade_and_replacement_labels() {
        let (_directory, mut newer) = firefox_harness_with_versions("158.0.1", "159.0.0");
        newer.run();
        newer.get_by_label("上游有新版本");
        newer.get_by_label("下载并升级");

        let (_directory, mut older) = firefox_harness_with_versions("158.0.1", "157.0.9");
        older.run();
        older.get_by_label("上游版本较旧");
        older.get_by_label("下载上游版本");
    }

    #[test]
    fn upstream_check_is_scoped_by_release_channel() {
        let directory = tempdir().unwrap();
        let browser = directory.path().join("Iceweasel");
        let program = browser.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::write(program.join("iceweasel.exe"), b"fixture").unwrap();
        fs::write(
            program.join("application.ini"),
            "[App]\r\nName=Iceweasel\r\nVersion=158.0.1\r\n",
        )
        .unwrap();
        let mut settings = Settings {
            selected_browser: TargetId::Iceweasel,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Iceweasel).unwrap();
        config.browser_root = Some(browser);
        config.browser_directory = Some(program);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.state_mut().latest_versions.insert(
            (TargetId::Iceweasel, ReleaseChannel::Release, CpuBuild::X64),
            UpstreamCheck {
                version: "158.0.1".to_owned(),
                channel: ReleaseChannel::Release,
                cpu_build: CpuBuild::X64,
                source: MetadataSource::Official,
                checked_at: 0,
            },
        );
        harness.run();

        harness.get_by_label("尚未检查上游版本");
        harness.get_by_label("检查并安装/升级");

        harness.state_mut().state.update_draft(|settings| {
            settings
                .targets
                .get_mut(&TargetId::Iceweasel)
                .unwrap()
                .channel = ReleaseChannel::Release;
        });
        harness.run();

        harness.get_by_label("与上游版本相同");
    }

    #[test]
    fn missing_browser_directory_does_not_show_receipt_as_current_version() {
        let directory = tempdir().unwrap();
        let mut settings = Settings::default();
        let config = settings.targets.get_mut(&TargetId::Firefox).unwrap();
        let root = directory.path().join("deleted-firefox");
        config.browser_root = Some(root.clone());
        config.browser_directory = Some(root.join("App"));
        config.installed_version = Some("157.0.1".to_owned());
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.state_mut().latest_versions.insert(
            (TargetId::Firefox, ReleaseChannel::Stable, CpuBuild::X64),
            UpstreamCheck {
                version: "157.0.1".to_owned(),
                channel: ReleaseChannel::Stable,
                cpu_build: CpuBuild::X64,
                source: MetadataSource::Official,
                checked_at: 0,
            },
        );
        harness.run();

        harness.get_by_label("目录不存在");
        harness.get_by_label("未安装");
        harness.get_by_label("下载并安装");
        assert!(harness.query_by_label("157.0.1（上次安装）").is_none());
    }

    #[test]
    fn existing_program_directory_without_browser_exe_is_marked_incomplete() {
        let directory = tempdir().unwrap();
        let mut settings = Settings::default();
        let config = settings.targets.get_mut(&TargetId::Firefox).unwrap();
        let root = directory.path().join("broken-firefox");
        let program = root.join("App");
        fs::create_dir_all(&program).unwrap();
        config.browser_root = Some(root);
        config.browser_directory = Some(program);
        config.installed_version = Some("157.0.1".to_owned());
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness.get_by_label("安装不完整");
        harness.get_by_label("无法比较");
        assert!(harness.query_by_label("157.0.1（上次安装）").is_none());
    }

    #[test]
    fn network_editor_adds_a_valid_https_accelerator() {
        let mut harness = harness();
        harness.run();
        let network = harness.get_by_label("全局设置");
        let overview = harness.get_by_role_and_label(Role::Button, "浏览器").rect();
        assert!(network.rect().min.y > overview.max.y);
        assert!(network.rect().max.x > overview.max.x);
        network.click();
        harness.run();

        assert!(harness.query_by_label("浏览器").is_none());
        assert!(
            harness
                .query_by_label("以下网络设置应用于全部浏览器，不属于当前选中的单个浏览器。")
                .is_none()
        );
        let accelerators = harness.get_by_label("启用内置下载加速站").rect();
        let custom = harness
            .get_by_role_and_label(Role::TextInput, "HTTPS 前缀")
            .rect();
        let proxy = harness
            .get_by_role_and_label(Role::TextInput, "代理地址")
            .rect();
        assert!(accelerators.min.y < custom.min.y);
        assert!(custom.min.y < proxy.min.y);
        harness.get_by_label("下载缓存");
        harness.get_by_label("清理下载缓存");
        harness
            .get_by_role_and_label(Role::TextInput, "HTTPS 前缀")
            .set_text_value_accesskit("https://mirror.example/");
        harness.run();
        harness.get_by_label("添加").click();
        harness.run();

        let network = &harness.state().state().draft().network;
        assert_eq!(network.accelerators.len(), 1);
        assert_eq!(network.accelerators[0].prefix, "https://mirror.example/");
        assert_eq!(
            harness.state().state().saved().network.accelerators,
            network.accelerators
        );

        harness.get_by_label("全局设置").click();
        harness.run();
        harness.get_by_role_and_label(Role::Button, "浏览器");
        harness.get_by_label("Chrome++");
    }

    #[test]
    fn global_settings_switches_manager_config_between_user_and_program_directories() {
        let directory = tempdir().unwrap();
        let tool_root = directory.path().join("Tool");
        fs::create_dir_all(&tool_root).unwrap();
        let user_path = directory.path().join("Local/browser-harbor/manager.json");
        let portable_path = tool_root.join("manager.json");
        let state = AppState::with_config_paths(
            Settings::default(),
            ConfigStore::new(user_path.clone()),
            tool_root,
            user_path.clone(),
            portable_path.clone(),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();
        harness.get_by_label("全局设置").click();
        harness.run();

        harness
            .get_by_role_and_label(Role::RadioButton, "程序目录（便携）")
            .scroll_to_me();
        harness.run();
        harness
            .get_by_role_and_label(Role::RadioButton, "程序目录（便携）")
            .click();
        harness.run();

        assert!(portable_path.is_file());
        assert!(harness.state().state().is_portable_config());
        harness.get_by_label_contains("配置位置已切换");

        harness
            .get_by_role_and_label(Role::RadioButton, "系统用户目录（默认）")
            .scroll_to_me();
        harness.run();
        harness
            .get_by_role_and_label(Role::RadioButton, "系统用户目录（默认）")
            .click();
        harness.run();

        assert!(user_path.is_file());
        assert!(!portable_path.exists());
        assert!(!harness.state().state().is_portable_config());
    }

    #[test]
    fn chromeplus_full_exposes_common_ini_controls() {
        let mut harness = harness();
        harness.run();
        harness.get_by_role(Role::ComboBox).click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Chrome")
            .click();
        harness.run();

        harness.get_by_label("Chrome++").click();
        harness.run();

        harness.get_by_label("当前浏览器：Chrome");
        assert!(
            harness
                .query_by_role_and_label(Role::ComboBox, "浏览器")
                .is_none()
        );
        harness.get_by_label("双击关闭标签");
        harness.get_by_label("右键关闭标签");
        harness.get_by_label("保留最后一个标签");
        harness.get_by_label("标签栏滚轮切换");
        harness.get_by_label("按住右键滚轮切换");
        harness.get_by_label("地址栏新标签打开");
        harness.get_by_label("书签在新标签打开");
        harness.get_by_label("禁用指定新标签页");
        harness.get_by_label("悬停自动切换标签");
        harness.get_by_label("新标签匹配");
        harness.get_by_label("启动命令");
        harness.get_by_label("退出命令");
        harness.get_by_label("老板键");
        harness.get_by_label("翻译键");
        harness.get_by_label("免系统验证查看密码");
        harness.get_by_label("启用 win32k");
        harness.get_by_label("忽略企业策略");
        harness.get_by_label("抑制错误升级提示");
        harness.get_by_label("按键映射");
        let integration = harness.get_by_label("Chrome++ 集成").rect();
        let configuration = harness.get_by_label("配置文件").rect();
        harness.get_by_label("Chrome++ 源码与 GPL-3.0 许可证");
        let mode_row = harness.get_by_label("模式").rect();
        let status_row = harness.get_by_label("状态").rect();
        let source_row = harness.get_by_label("来源").rect();
        assert!((mode_row.min.x - status_row.min.x).abs() < 1.0);
        assert!((mode_row.min.x - source_row.min.x).abs() < 1.0);
        assert!(mode_row.center().y < status_row.center().y);
        assert!(status_row.center().y < source_row.center().y);
        assert!((integration.center().y - configuration.center().y).abs() < 1.0);
        let version = harness.get_by_label("—").rect();
        let check = harness.get_by_label("检查更新").rect();
        assert!(integration.max.x < version.min.x && version.max.x < check.min.x);
        assert!((integration.center().y - version.center().y).abs() < 1.0);
        assert!((integration.center().y - check.center().y).abs() < 1.0);
        let load = harness.get_by_label("从 INI 载入").rect();
        let open = harness.get_by_label("打开配置文件").rect();
        let more = harness.get_by_label("更多操作…").rect();
        let window_right = 560.0;
        assert!(load.min.x > integration.max.x);
        assert!(load.width() > 180.0);
        assert!((load.min.x - open.min.x).abs() < 1.0);
        assert!((load.min.x - more.min.x).abs() < 1.0);
        assert!(load.min.y < open.min.y && open.min.y < more.min.y);
        assert!(more.max.x <= window_right);
        harness.get_by_label("更多操作…").click();
        harness.run();
        harness.get_by_label("打开所在文件夹");
        harness.get_by_label("备份当前 INI");
        harness.get_by_label("恢复默认配置");
    }

    #[test]
    fn chromeplus_header_fits_in_english() {
        let directory = tempdir().unwrap();
        let settings = Settings {
            selected_browser: TargetId::Chrome,
            language: Language::English,
            ..Settings::default()
        };
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();
        harness.get_by_label("Chrome++").click();
        harness.run();

        let integration = harness.get_by_label("Chrome++ integration").rect();
        let configuration = harness.get_by_label("Configuration file").rect();
        let version = harness.get_by_label("—").rect();
        let check = harness.get_by_label("Check for updates").rect();
        assert!(integration.width() > 100.0);
        assert!(integration.max.x < version.min.x && version.max.x < check.min.x);
        assert!((integration.center().y - version.center().y).abs() < 1.0);
        assert!((integration.center().y - check.center().y).abs() < 1.0);
        assert!((integration.center().y - configuration.center().y).abs() < 1.0);

        set_language(Language::Chinese);
    }

    #[test]
    fn uninstalled_chromeplus_does_not_show_stored_version() {
        let directory = tempdir().unwrap();
        let browser = directory.path().join("Chrome");
        let program = browser.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(browser);
        config.browser_directory = Some(program);
        config.chrome_plus_version = Some("1.18.2".to_owned());
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.state_mut().active_tab = DetailTab::ChromePlus;
        harness
            .state_mut()
            .chrome_plus_latest
            .insert(TargetId::Chrome, "1.18.3".to_owned());
        harness.run();

        harness.get_by_label("尚未安装");
        harness.get_by_label("—");
        assert!(harness.query_by_label("v1.18.2").is_none());
        assert!(harness.query_by_label("更新 Chrome++ 到 1.18.3").is_none());
        let config = &harness.state().state().draft().targets[&TargetId::Chrome];
        assert!(
            harness
                .state()
                .chrome_plus_pending_update(TargetId::Chrome, config)
                .is_none()
        );
    }

    #[test]
    fn chromeplus_update_blocks_when_multiple_target_instances_are_running() {
        use std::process::{Child, Command, Stdio};

        struct Children(Vec<Child>);
        impl Drop for Children {
            fn drop(&mut self) {
                for child in &mut self.0 {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }

        let directory = tempdir().unwrap();
        let browser = directory.path().join("Chrome");
        let program = browser.join("App");
        fs::create_dir_all(&program).unwrap();
        let executable = program.join("chrome.exe");
        fs::copy(std::env::var_os("COMSPEC").unwrap(), &executable).unwrap();
        write_ini(
            &program.join("chrome++.ini"),
            "[general]\r\ncommand_line=\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();
        let _children = Children(
            (0..2)
                .map(|_| {
                    Command::new(&executable)
                        .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .unwrap()
                })
                .collect(),
        );
        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(browser);
        config.browser_directory = Some(program);
        config.data_directory = Some(r"D:\Browser\Chrome\Profiles".into());
        config.profile_directory = Some(r"D:\Browser\Chrome\Profiles".into());
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        let app = harness.state_mut();
        app.active_tab = DetailTab::ChromePlus;
        app.chrome_plus_versions
            .insert(TargetId::Chrome, Some("1.18.2".to_owned()));
        app.chrome_plus_latest
            .insert(TargetId::Chrome, "1.18.3".to_owned());
        harness.run();

        harness.get_by_label("更新 Chrome++ 到 1.18.3").click();
        harness.run();

        harness
            .get_by_label("检测到 2 个 Chrome 进程；请手工关闭所有 Chrome 实例后再更新 Chrome++");
        assert!(harness.state().operation.is_none());
    }

    #[test]
    fn chromeplus_update_ignores_other_browser_programs() {
        use std::process::{Child, Command, Stdio};

        struct ChildGuard(Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let directory = tempdir().unwrap();
        let browser = directory.path().join("Chrome");
        let program = browser.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        write_ini(
            &program.join("chrome++.ini"),
            "[general]\r\ncommand_line=\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();
        let other = directory.path().join("Helium/App");
        fs::create_dir_all(&other).unwrap();
        let other_executable = other.join("helium.exe");
        fs::copy(std::env::var_os("COMSPEC").unwrap(), &other_executable).unwrap();
        let _other_child = ChildGuard(
            Command::new(&other_executable)
                .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(browser);
        config.browser_directory = Some(program);
        config.data_directory = Some(r"D:\Browser\Chrome\Profiles".into());
        config.profile_directory = Some(r"D:\Browser\Chrome\Profiles".into());
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        let app = harness.state_mut();
        app.active_tab = DetailTab::ChromePlus;
        app.chrome_plus_versions
            .insert(TargetId::Chrome, Some("1.18.2".to_owned()));
        app.chrome_plus_latest
            .insert(TargetId::Chrome, "1.18.3".to_owned());
        harness.run();

        harness.get_by_label("更新 Chrome++ 到 1.18.3").click();
        harness.run();

        harness.get_by_label("Chrome++ 1.18.2 → 1.18.3");
        assert!(harness.state().operation.is_none());
    }

    #[test]
    fn browser_upgrade_prompt_names_both_version_transitions() {
        let mut harness = harness();
        harness.state_mut().browser_upgrade_prompt = Some(super::BrowserUpgradePrompt {
            target: TargetId::Firefox,
            current_browser: "159.0".to_owned(),
            upstream_browser: "160.0".to_owned(),
            chrome_plus_current: "1.18.2".to_owned(),
            chrome_plus_latest: "1.18.3".to_owned(),
            replace_same_version: false,
        });
        harness.run();

        harness.get_by_label("同时更新 Chrome++");
        harness.get_by_label("浏览器：159.0 → 160.0");
        harness.get_by_label("Chrome++：1.18.2 → 1.18.3");
    }

    #[test]
    fn chromeplus_edits_autosave_manager_state_and_ini() {
        let directory = tempdir().unwrap();
        let browser = directory.path().join("Chrome");
        let program = browser.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        let ini = program.join("chrome++.ini");
        write_ini(&ini, "[general]\r\ncommand_line=\r\n", IniEncoding::Utf16Le).unwrap();
        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(browser);
        config.browser_directory = Some(program);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();
        harness.get_by_label("Chrome++").click();
        harness.run();

        harness
            .get_by_role_and_label(Role::TextInput, "附加参数")
            .set_text_value_accesskit("--autosave-test");
        harness.run();

        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Chrome]
                .chrome_plus
                .command_line,
            "--autosave-test"
        );
        let (ini_text, _) = read_ini(&ini).unwrap();
        assert!(ini_text.contains("--autosave-test"));
    }

    #[test]
    fn chromeplus_installed_version_is_probed_from_version_dll() {
        let directory = tempdir().unwrap();
        let browser = directory.path().join("Chrome");
        let program = browser.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            program.join("version.dll"),
        )
        .unwrap();
        write_ini(
            &program.join("chrome++.ini"),
            "[general]\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();
        let expected = crate::runtime::chromeplus::installed_version(&program).unwrap();
        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(browser);
        config.browser_directory = Some(program);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();
        harness.get_by_label("Chrome++").click();
        harness.run();

        harness.get_by_label(&format!("v{expected}"));
        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Chrome]
                .chrome_plus_version
                .as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn experimental_chromium_targets_expose_full_chromeplus() {
        for (browser, target) in [
            ("Brave", TargetId::Brave),
            ("Helium", TargetId::Helium),
            ("Ungoogled Chromium", TargetId::UngoogledChromium),
        ] {
            let mut harness = harness();
            harness.run();
            harness.get_by_role(Role::ComboBox).click();
            harness.run();
            harness
                .get_by_role(Role::ListBox)
                .get_by_label(browser)
                .click();
            harness.run();
            assert_eq!(harness.state().state().selected_target(), target);
            harness.get_by_label("Chrome++").click();
            harness.run();

            harness.get_by_label("启动命令");
            harness.get_by_label("禁用指定新标签页");
            harness.get_by_label("免系统验证查看密码");
            harness.get_by_label("按键映射");
        }
    }

    #[test]
    fn chrome_plus_update_button_reflects_the_last_check() {
        assert_eq!(chrome_plus_update_button_label(None, None), "检查更新");
        assert_eq!(
            chrome_plus_update_button_label(Some("1.18.2"), Some("1.18.2")),
            "已最新"
        );
        assert_eq!(
            chrome_plus_update_button_label(Some("1.18.1"), Some("1.18.2")),
            "可更新"
        );
        assert_eq!(
            chrome_plus_update_button_label(Some("1.18.2"), Some("1.18.2-beta")),
            "检查更新"
        );
    }

    #[test]
    fn chromeplus_update_hover_separates_detection_from_installation() {
        let hover = chrome_plus_update_hover(Language::Chinese, "1.18.2", "1.18.3");
        assert!(hover.contains("点击后才会下载并替换"));
        assert!(hover.contains("完成后版本才会变化"));
    }

    #[test]
    fn every_directory_field_has_a_folder_picker() {
        let mut harness = harness();
        harness.run();

        let labels = ["浏览器根目录", "程序目录", "用户数据目录", "缓存目录"];
        for label in labels {
            assert_eq!(
                harness
                    .get_by_role_and_label(Role::Button, &format!("选择{label}"))
                    .accesskit_node()
                    .role(),
                Role::Button
            );
        }
        let label_left = labels.map(|label| {
            harness
                .get_by_role_and_label(Role::Label, label)
                .rect()
                .min
                .x
        });
        let input_left = labels.map(|label| {
            harness
                .get_by_role_and_label(Role::TextInput, label)
                .rect()
                .min
                .x
        });
        assert!(
            label_left
                .iter()
                .all(|left| (left - label_left[0]).abs() < 0.5)
        );
        assert!(
            input_left
                .iter()
                .all(|left| (left - input_left[0]).abs() < 0.5),
            "{input_left:?}"
        );
        assert!(harness.query_by_label("Profile 位置").is_none());
        let shortcut = harness.get_by_label("快捷方式：开");
        assert!(!shortcut.accesskit_node().is_disabled());
        let input_right = labels.map(|label| {
            harness
                .get_by_role_and_label(Role::TextInput, label)
                .rect()
                .max
                .x
        });
        assert!(
            input_right[1..]
                .iter()
                .all(|right| (right - shortcut.rect().max.x).abs() < 1.0),
            "shortcut action must share lower directory fields' right edge: shortcut={}, inputs={input_right:?}",
            shortcut.rect().max.x
        );
    }

    #[test]
    fn separate_profile_location_is_shown_only_for_targets_that_need_it() {
        let directory = tempdir().unwrap();
        let settings = Settings {
            selected_browser: TargetId::Floorp,
            ..Settings::default()
        };
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();
        harness.get_by_role_and_label(Role::TextInput, "Profile 位置");
        assert!(harness.query_by_label("Profile 目录").is_none());
    }

    #[test]
    fn shared_user_data_edit_updates_the_internal_profile_path() {
        let directory = tempdir().unwrap();
        let settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "用户数据目录")
            .set_text_value_accesskit(r"D:\ChromeData");
        harness.run();
        let config = &harness.state().state().saved().targets[&TargetId::Chrome];
        assert_eq!(config.data_directory, Some(r"D:\ChromeData".into()));
        assert_eq!(config.profile_directory, Some(r"D:\ChromeData".into()));
    }

    #[test]
    fn shortcut_auto_creation_is_on_by_default_and_can_be_disabled() {
        let mut harness = harness();
        harness.run();

        harness.get_by_label("快捷方式：开").click();
        harness.run();

        harness.get_by_label("快捷方式：关");
        assert!(!harness.state().state().saved().targets[&TargetId::Firefox].auto_create_shortcut);
    }

    #[test]
    fn skykakapo_uses_app_and_shows_profile_followed_cache() {
        let mut harness = harness();
        harness.run();
        harness.get_by_role(Role::ComboBox).click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("SkyKakapo")
            .click();
        harness.run();

        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "程序目录")
                .accesskit_node()
                .value(),
            Some(".\\SkyKakapo\\App".to_owned())
        );
        assert!(
            harness
                .query_by_role_and_label(Role::TextInput, "缓存目录")
                .is_none()
        );
        harness.get_by_label("跟随 PortableData");
    }

    #[test]
    fn auto_shortcut_creation_respects_the_enabled_flag() {
        let root = tempdir().unwrap();
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let program = root.path().join("App");
        fs::create_dir_all(&program).unwrap();
        let executable = program.join("firefox.exe");
        fs::copy(
            PathBuf::from(windows).join(r"System32\notepad.exe"),
            &executable,
        )
        .unwrap();

        assert_eq!(
            create_auto_shortcut(TargetId::Firefox, false, root.path(), &program).unwrap(),
            None
        );
        assert!(!root.path().join("Firefox.lnk").exists());

        let shortcut =
            create_auto_shortcut(TargetId::Firefox, true, root.path(), &program).unwrap();
        assert_eq!(shortcut, Some(root.path().join("Firefox.lnk")));
    }

    #[test]
    fn browser_root_change_is_saved_immediately() {
        let mut harness = harness();
        harness.run();
        let field = harness.get_by_role_and_label(Role::TextInput, "浏览器根目录");
        field.set_text_value_accesskit(r"D:\draft");
        harness.run();
        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Firefox].browser_root,
            Some(PathBuf::from(r"D:\draft"))
        );
        let config = &harness.state().state().saved().targets[&TargetId::Firefox];
        assert_eq!(
            config.browser_directory,
            Some(PathBuf::from(r"D:\draft\App"))
        );
        assert_eq!(
            config.data_directory,
            Some(PathBuf::from(r"D:\draft\Profiles"))
        );
        assert_eq!(
            config.profile_directory,
            Some(PathBuf::from(r"D:\draft\Profiles"))
        );
        assert_eq!(config.cache_directory, None);
        assert_eq!(
            harness.state().state().draft(),
            harness.state().state().saved()
        );
    }

    #[test]
    fn configuration_diagnostics_open_from_path_section() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("Chrome");
        let program = root.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::create_dir_all(root.join("Profiles")).unwrap();
        fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            program.join("version.dll"),
        )
        .unwrap();
        fs::write(
            program.join("chrome++.ini"),
            "[general]\r\ndata_dir=%app%\\..\\Profiles\r\ncache_dir=none\r\n",
        )
        .unwrap();

        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(root);
        config.derive_paths_from_root(TargetId::Chrome);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness
            .get_by_role_and_label(Role::Button, "检查配置")
            .click();
        harness.run();

        assert!(harness.query_by_label("配置检查").is_some());
        assert!(harness.query_by_label("检查结果：正常").is_some());
    }

    #[test]
    fn chromeplus_path_repair_requires_confirmation_and_creates_backup() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("Chrome");
        let program = root.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::create_dir_all(root.join("Profiles")).unwrap();
        fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            program.join("version.dll"),
        )
        .unwrap();
        fs::write(
            program.join("chrome++.ini"),
            format!(
                "[general]\r\ndata_dir={}\r\ncache_dir=none\r\n",
                root.join("Profiles").display()
            ),
        )
        .unwrap();
        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Chrome);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness
            .get_by_role_and_label(Role::Button, "检查配置")
            .click();
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "修复 Chrome++ 路径")
            .click();
        harness.run();

        assert!(
            harness
                .query_by_label("将备份 chrome++.ini 并只修复路径键。")
                .is_some()
        );
        harness
            .get_by_role_and_label(Role::Button, "确认修复")
            .click();
        harness.run();

        assert!(harness.query_by_label("检查结果：正常").is_some());
        let ini = fs::read_to_string(program.join("chrome++.ini")).unwrap();
        assert!(ini.contains(r"data_dir=%app%\..\Profiles"));
        assert!(
            fs::read_dir(root.join("backups"))
                .unwrap()
                .any(|entry| entry.unwrap().file_type().unwrap().is_file())
        );
    }

    #[test]
    fn iceweasel_portable_paths_can_be_repaired_from_health_dialog() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("Iceweasel");
        let program = root.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::create_dir_all(root.join("Profiles")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            program.join("iceweasel.exe"),
        )
        .unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            program.join("portable64.dll"),
        )
        .unwrap();
        fs::write(
            program.join("application.ini"),
            "[App]\r\nVersion=159.0\r\n",
        )
        .unwrap();
        fs::write(
            program.join("portable.ini"),
            format!(
                "[General]\r\nPortable=1\r\nPortableDataPath={}\r\n[Env]\r\nTmpDataPath=\r\n",
                root.join("Profiles").display()
            ),
        )
        .unwrap();
        let mut settings = Settings {
            selected_browser: TargetId::Iceweasel,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Iceweasel).unwrap();
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Iceweasel);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness
            .get_by_role_and_label(Role::Button, "检查配置")
            .click();
        harness.run();
        harness
            .get_by_role_and_label(Role::Button, "修复便携配置")
            .click();
        harness.run();

        assert!(
            harness
                .query_by_label("将备份 portable.ini 并只修复路径键。")
                .is_some()
        );
        harness
            .get_by_role_and_label(Role::Button, "确认修复")
            .click();
        harness.run();

        assert!(harness.query_by_label("检查结果：正常").is_some());
        let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\Profiles"));
        assert!(
            fs::read_dir(root.join("backups"))
                .unwrap()
                .any(|entry| entry.unwrap().file_type().unwrap().is_file())
        );
    }

    #[test]
    fn relocation_preview_does_not_save_until_applied() {
        let directory = tempdir().unwrap();
        let old_root = directory.path().join("OldChrome");
        let new_root = directory.path().join("NewChrome");
        fs::create_dir_all(new_root.join("App")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            new_root.join("App/chrome.exe"),
        )
        .unwrap();
        let mut settings = Settings {
            selected_browser: TargetId::Chrome,
            ..Settings::default()
        };
        let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
        config.browser_root = Some(old_root.clone());
        config.derive_paths_from_root(TargetId::Chrome);
        let state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness.state_mut().open_health_dialog();
        harness.state_mut().stage_relocation(new_root.clone());

        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Chrome].browser_root,
            Some(old_root)
        );
        assert!(
            harness
                .state()
                .health_dialog
                .as_ref()
                .and_then(|dialog| dialog.relocation_warning.as_deref())
                .is_some()
        );

        harness.state_mut().apply_staged_relocation();

        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Chrome].browser_root,
            Some(new_root)
        );
    }

    #[test]
    fn derived_paths_survive_configuration_reload() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("manager.json");
        let state = AppState::new(Settings::default(), ConfigStore::new(config_path.clone()));
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness
            .get_by_role_and_label(Role::TextInput, "浏览器根目录")
            .set_text_value_accesskit(r"D:\Persisted\Firefox");
        harness.run();
        drop(harness);

        let reloaded = ConfigStore::new(config_path).load().unwrap();
        let config = &reloaded.targets[&TargetId::Firefox];
        assert_eq!(
            config.browser_root,
            Some(PathBuf::from(r"D:\Persisted\Firefox"))
        );
        assert_eq!(
            config.browser_directory,
            Some(PathBuf::from(r"D:\Persisted\Firefox\App"))
        );
        assert_eq!(
            config.data_directory,
            Some(PathBuf::from(r"D:\Persisted\Firefox\Profiles"))
        );
        assert_eq!(
            config.profile_directory,
            Some(PathBuf::from(r"D:\Persisted\Firefox\Profiles"))
        );
        assert_eq!(config.cache_directory, None);
    }

    #[test]
    fn every_edit_is_saved_without_a_save_button() {
        let directory = tempdir().unwrap();
        let state = AppState::new(
            Settings::default(),
            ConfigStore::new(directory.path().join("manager.json")),
        );
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "浏览器根目录")
            .set_text_value_accesskit(r"D:\saved");
        harness.run();
        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Firefox].browser_root,
            Some(PathBuf::from(r"D:\saved"))
        );

        harness
            .get_by_role_and_label(Role::TextInput, "浏览器根目录")
            .set_text_value_accesskit(r"D:\changed");
        harness.run();

        assert_eq!(
            harness.state().state().saved().targets[&TargetId::Firefox].browser_root,
            Some(PathBuf::from(r"D:\changed"))
        );
        assert_eq!(
            harness.state().state().draft(),
            harness.state().state().saved()
        );
        assert!(harness.query_by_label("保存").is_none());
    }

    #[test]
    fn malformed_config_is_visible_and_autosave_is_blocked() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("manager.json");
        fs::write(&path, b"{broken").unwrap();
        let store = ConfigStore::new(path);
        let error = store.load().unwrap_err().to_string();
        let state = AppState::new(Settings::default(), store);
        let mut harness = harness_for(BrowserHarborApp::with_startup_error(state, error));

        harness.run();

        harness.get_by_label_contains("配置加载失败");
        assert!(harness.query_by_label("保存").is_none());
        let field = harness.get_by_role_and_label(Role::TextInput, "浏览器根目录");
        field.set_text_value_accesskit(r"D:\must-not-overwrite");
        harness.run();
        assert_eq!(
            fs::read(directory.path().join("manager.json")).unwrap(),
            b"{broken"
        );
    }

    #[test]
    fn theme_choice_is_saved_immediately() {
        let mut harness = harness();
        harness.run();

        harness.get_by_label("外观：跟随系统").click();
        harness.run();
        harness.get_by_label("深色").click();
        harness.run();

        assert_eq!(harness.state().state().draft().theme, ThemeMode::Dark);
        assert_eq!(harness.state().state().saved().theme, ThemeMode::Dark);
        harness.get_by_label("外观：深色");
    }

    #[test]
    fn language_switch_localizes_ui_and_persists() {
        let directory = tempdir().unwrap();
        let config_path = directory.path().join("manager.json");
        let state = AppState::new(Settings::default(), ConfigStore::new(config_path.clone()));
        let mut harness = harness_for(BrowserHarborApp::new(state));
        harness.run();

        harness
            .get_by_role_and_label(Role::Button, "界面语言: 中文")
            .click();
        harness.run();
        harness.get_by_label("English").click();
        harness.run();

        assert_eq!(harness.state().state().saved().language, Language::English);
        harness.get_by_role_and_label(Role::Button, "Browser");
        harness.get_by_role_and_label(Role::ComboBox, "Browser");
        harness.get_by_role_and_label(Role::TextInput, "Browser root");
        harness.get_by_label("Appearance: Follow system");
        assert!(harness.query_by_label("浏览器根目录").is_none());

        let labels = [
            "Browser root",
            "Program directory",
            "User data directory",
            "Cache directory",
        ];
        let label_left = labels.map(|label| {
            harness
                .get_by_role_and_label(Role::Label, label)
                .rect()
                .min
                .x
        });
        let input_left = labels.map(|label| {
            harness
                .get_by_role_and_label(Role::TextInput, label)
                .rect()
                .min
                .x
        });
        assert!(
            label_left
                .iter()
                .all(|left| (left - label_left[0]).abs() < 0.5)
        );
        assert!(
            input_left
                .iter()
                .all(|left| (left - input_left[0]).abs() < 0.5),
            "{input_left:?}"
        );

        let reloaded = ConfigStore::new(config_path).load().unwrap();
        assert_eq!(reloaded.language, Language::English);
        set_language(Language::Chinese);
    }

    #[test]
    fn theme_configuration_keeps_system_default_and_styles_both_appearances() {
        let ctx = eframe::egui::Context::default();
        configure_context(&ctx, ThemeMode::System);

        assert_eq!(
            ctx.options(|options| options.theme_preference),
            eframe::egui::ThemePreference::System
        );
        let style = ctx.style_of(eframe::egui::Theme::Light);
        assert_eq!(
            style.text_styles[&eframe::egui::TextStyle::Body].size,
            BODY_FONT_SIZE
        );
        assert_eq!(style.visuals.selection.bg_fill, LIGHT_PALETTE.accent_soft);
        assert!(!style.visuals.dark_mode);
        assert!(ctx.style_of(eframe::egui::Theme::Dark).visuals.dark_mode);
    }

    #[test]
    fn egui_default_fonts_are_not_embedded() {
        assert!(
            eframe::egui::FontDefinitions::builtin_font_names().is_empty(),
            "default font assets would regress the release size budget"
        );
    }
}
