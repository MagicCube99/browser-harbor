//! Portable runtime identity and user-editable Chrome++ settings.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::release::Artifact;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    ChromiumPackage,
    ChromePlusFull,
    ChromePlusDataOnly,
    Libportable,
    IceweaselNative,
    SkyKakapoNative,
    CatsxpLauncher,
    FloorpLauncher,
    LibreWolfLauncher,
    TurboLauncher,
}

impl RuntimeKind {
    pub const fn uses_chrome_plus(self) -> bool {
        matches!(self, Self::ChromePlusFull | Self::ChromePlusDataOnly)
    }

    pub const fn uses_libportable(self) -> bool {
        matches!(self, Self::Libportable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChromePlusConfig {
    pub command_line: String,
    pub launch_on_startup: String,
    pub launch_on_exit: String,
    pub boss_key: String,
    pub translate_key: String,
    pub show_password: bool,
    pub win32k: bool,
    pub ignore_policies: bool,
    pub suppress_false_upgrade_notification: bool,
    pub double_click_close: bool,
    pub right_click_close: bool,
    pub keep_last_tab: bool,
    pub wheel_tab: bool,
    pub wheel_tab_when_press_rbutton: bool,
    pub open_url_new_tab: bool,
    pub open_bookmark_new_tab: bool,
    pub new_tab_disable: bool,
    pub new_tab_disable_name: String,
    pub hover_tab: bool,
    pub hover_tab_delay: u32,
    pub keymapping: Option<String>,
}

impl Default for ChromePlusConfig {
    fn default() -> Self {
        Self {
            command_line: String::new(),
            launch_on_startup: String::new(),
            launch_on_exit: String::new(),
            boss_key: String::new(),
            translate_key: String::new(),
            show_password: true,
            win32k: false,
            ignore_policies: false,
            suppress_false_upgrade_notification: false,
            double_click_close: true,
            right_click_close: false,
            keep_last_tab: true,
            wheel_tab: true,
            wheel_tab_when_press_rbutton: true,
            open_url_new_tab: false,
            open_bookmark_new_tab: false,
            new_tab_disable: true,
            new_tab_disable_name: "\"about:blank\",\"新建标签\"".to_owned(),
            hover_tab: false,
            hover_tab_delay: 400,
            keymapping: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromePlusRelease {
    pub version: String,
    pub artifact: Artifact,
    pub setdll_artifact: Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromePlusInstallation {
    pub version: String,
    pub dll_sha256: String,
    pub ini_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibportableRelease {
    pub version: String,
    pub artifact: Artifact,
}
