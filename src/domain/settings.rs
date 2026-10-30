//! Persisted application settings.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    release::{CpuBuild, ReleaseChannel},
    runtime::ChromePlusConfig,
    target::TargetId,
};

pub const CURRENT_SCHEMA_VERSION: u32 = 11;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    pub selected_browser: TargetId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_position: Option<WindowPosition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub targets: BTreeMap<TargetId, TargetConfig>,
    #[serde(default, skip_serializing_if = "NetworkSettings::is_default")]
    pub network: NetworkSettings,
    #[serde(default, skip_serializing_if = "ThemeMode::is_system")]
    pub theme: ThemeMode,
    #[serde(default, skip_serializing_if = "Language::is_chinese")]
    pub language: Language,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            selected_browser: TargetId::Firefox,
            window_position: None,
            targets: TargetId::ALL
                .into_iter()
                .map(|target| (target, TargetConfig::for_target(target)))
                .collect(),
            network: NetworkSettings::default(),
            theme: ThemeMode::System,
            language: Language::Chinese,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    fn is_system(&self) -> bool {
        *self == Self::System
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Chinese,
    English,
}

impl Language {
    pub fn is_english(self) -> bool {
        self == Self::English
    }

    fn is_chinese(&self) -> bool {
        *self == Self::Chinese
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TargetConfig {
    pub browser_root: Option<PathBuf>,
    pub browser_directory: Option<PathBuf>,
    pub data_directory: Option<PathBuf>,
    pub profile_directory: Option<PathBuf>,
    pub cache_directory: Option<PathBuf>,
    pub auto_create_shortcut: bool,
    pub channel: ReleaseChannel,
    pub cpu_build: CpuBuild,
    pub installed_version: Option<String>,
    pub local_package: Option<PathBuf>,
    pub chrome_plus: ChromePlusConfig,
    pub chrome_plus_version: Option<String>,
}

impl TargetConfig {
    pub fn for_target(target: TargetId) -> Self {
        let spec = target.spec();
        let mut config = Self {
            channel: spec.default_channel,
            cpu_build: spec.default_cpu_build,
            ..Self::default()
        };
        config.fill_default_paths(target);
        config
    }

    pub fn fill_default_paths(&mut self, target: TargetId) {
        let root = self
            .browser_root
            .get_or_insert_with(|| PathBuf::from(".").join(target.display_name()))
            .clone();
        let layout = target.spec().layout;
        if self.browser_directory.is_none() {
            self.browser_directory = Some(
                layout
                    .program
                    .map_or_else(|| root.clone(), |program| root.join(program)),
            );
        }
        if self.data_directory.is_none() {
            self.data_directory = Some(root.join(layout.data));
        }
        if self.profile_directory.is_none() {
            self.profile_directory = Some(root.join(layout.profile));
        }
        if self.cache_directory.is_none() && !target.cache_follows_profile() {
            self.cache_directory = Some(root.join(layout.cache));
        }
    }

    pub fn derive_paths_from_root(&mut self, target: TargetId) {
        let Some(root) = self.browser_root.clone() else {
            self.browser_directory = None;
            self.data_directory = None;
            self.profile_directory = None;
            self.cache_directory = None;
            return;
        };
        let layout = target.spec().layout;
        self.browser_directory = Some(
            layout
                .program
                .map_or_else(|| root.clone(), |program| root.join(program)),
        );
        self.data_directory = Some(root.join(layout.data));
        self.profile_directory = Some(root.join(layout.profile));
        self.cache_directory = (!target.cache_follows_profile()).then(|| root.join(layout.cache));
    }

    pub fn effective_data_dir(&self, target: TargetId) -> Option<&std::path::Path> {
        if target.spec().layout.data == target.spec().layout.profile {
            self.data_directory
                .as_deref()
                .or(self.profile_directory.as_deref())
        } else {
            self.data_directory.as_deref()
        }
    }

    pub fn effective_profile_dir(&self, target: TargetId) -> Option<&std::path::Path> {
        if target.spec().layout.data == target.spec().layout.profile {
            self.data_directory
                .as_deref()
                .or(self.profile_directory.as_deref())
        } else {
            self.profile_directory.as_deref()
        }
    }

    pub fn set_user_data_dir(&mut self, target: TargetId, path: Option<std::path::PathBuf>) {
        if target.spec().layout.data == target.spec().layout.profile {
            self.profile_directory = path.clone();
        }
        self.data_directory = path;
    }

    pub fn align_single_directory_paths(&mut self, target: TargetId) {
        if target.spec().layout.data == target.spec().layout.profile {
            if let Some(data) = self.data_directory.clone() {
                self.profile_directory = Some(data);
            } else if let Some(profile) = self.profile_directory.clone() {
                self.data_directory = Some(profile);
            }
        }
    }
}

impl Default for TargetConfig {
    fn default() -> Self {
        Self {
            browser_root: None,
            browser_directory: None,
            data_directory: None,
            profile_directory: None,
            cache_directory: None,
            auto_create_shortcut: true,
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            installed_version: None,
            local_package: None,
            chrome_plus: ChromePlusConfig::default(),
            chrome_plus_version: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NetworkSettings {
    pub proxy: Option<String>,
    pub timeout_seconds: u64,
    pub accelerators_enabled: bool,
    pub accelerators: Vec<Accelerator>,
}

impl NetworkSettings {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accelerator {
    pub prefix: String,
    pub enabled: bool,
}

impl Accelerator {
    pub fn validate(&self) -> Result<(), String> {
        let prefix = self.prefix.trim();
        let lower = prefix.to_ascii_lowercase();
        if !lower.starts_with("https://") {
            return Err("加速站必须使用 HTTPS".to_owned());
        }
        let parsed = url::Url::parse(prefix).map_err(|_| "加速站地址无效".to_owned())?;
        if parsed.username() != "" || parsed.password().is_some() {
            return Err("加速站地址不得包含凭据".to_owned());
        }
        if parsed.host_str().is_none() {
            return Err("加速站地址缺少主机名".to_owned());
        }
        Ok(())
    }
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            proxy: None,
            timeout_seconds: 30,
            accelerators_enabled: true,
            accelerators: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Accelerator, NetworkSettings, TargetConfig};
    use crate::domain::{
        release::{CpuBuild, ReleaseChannel},
        target::{BrowserFamily, TargetId},
    };

    #[test]
    fn target_specific_defaults_follow_the_specification() {
        let iceweasel = TargetConfig::for_target(TargetId::Iceweasel);
        assert_eq!(iceweasel.channel, ReleaseChannel::Esr);
        assert_eq!(iceweasel.cpu_build, CpuBuild::X64);

        let thorium = TargetConfig::for_target(TargetId::Thorium);
        assert_eq!(thorium.channel, ReleaseChannel::Stable);
        assert_eq!(thorium.cpu_build, CpuBuild::Sse3);
    }

    #[test]
    fn browser_roots_and_portable_directories_have_defaults() {
        let helium = TargetConfig::for_target(TargetId::Helium);
        assert_eq!(helium.browser_root, Some(r".\Helium".into()));
        assert_eq!(helium.browser_directory, Some(r".\Helium\App".into()));
        assert_eq!(helium.data_directory, Some(r".\Helium\Profiles".into()));
        assert_eq!(helium.profile_directory, Some(r".\Helium\Profiles".into()));
        assert_eq!(helium.cache_directory, None);
        assert!(helium.auto_create_shortcut);

        let catsxp = TargetConfig::for_target(TargetId::Catsxp);
        assert_eq!(catsxp.browser_directory, Some(r".\Catsxp\App".into()));
        assert_eq!(catsxp.data_directory, Some(r".\Catsxp\Profiles".into()));

        for target in [
            TargetId::Firefox,
            TargetId::Floorp,
            TargetId::Iceweasel,
            TargetId::LibreWolf,
            TargetId::SkyKakapo,
            TargetId::Zen,
        ] {
            assert_eq!(
                TargetConfig::for_target(target).cache_directory,
                None,
                "{}",
                target.sort_key()
            );
        }

        let librewolf = TargetConfig::for_target(TargetId::LibreWolf);
        assert_eq!(
            librewolf.browser_directory,
            Some(r".\LibreWolf\LibreWolf".into())
        );
        assert_eq!(
            librewolf.data_directory,
            Some(r".\LibreWolf\Profiles".into())
        );
        assert_eq!(
            librewolf.profile_directory,
            Some(r".\LibreWolf\Profiles\Default".into())
        );

        let floorp = TargetConfig::for_target(TargetId::Floorp);
        assert_eq!(floorp.browser_directory, Some(r".\Floorp\app".into()));
        assert_eq!(floorp.data_directory, Some(r".\Floorp\data".into()));
        assert_eq!(
            floorp.profile_directory,
            Some(r".\Floorp\data\profile\default".into())
        );

        let thorium = TargetConfig::for_target(TargetId::Thorium);
        assert_eq!(thorium.browser_root, Some(r".\Thorium".into()));
        assert_eq!(thorium.browser_directory, Some(r".\Thorium\BIN".into()));
        assert_eq!(thorium.data_directory, Some(r".\Thorium\USER_DATA".into()));
        assert_eq!(
            thorium.profile_directory,
            Some(r".\Thorium\USER_DATA".into())
        );
        assert_eq!(thorium.cache_directory, None);

        let turbo = TargetConfig::for_target(TargetId::Turbo);
        assert_eq!(turbo.browser_directory, Some(r".\Turbo".into()));
        assert_eq!(turbo.data_directory, Some(r".\Turbo\User Data".into()));
        assert_eq!(turbo.profile_directory, Some(r".\Turbo\User Data".into()));
        assert_eq!(turbo.cache_directory, Some(r".\Turbo\User Data".into()));
    }

    #[test]
    fn changing_browser_root_rederives_every_child_path() {
        for target in TargetId::ALL {
            let root = PathBuf::from(r".\CustomRoot");
            let layout = target.spec().layout;
            let mut config = TargetConfig::for_target(target);
            config.browser_root = Some(root.clone());

            config.derive_paths_from_root(target);

            assert_eq!(
                config.browser_directory,
                Some(
                    layout
                        .program
                        .map_or_else(|| root.clone(), |program| root.join(program))
                ),
                "{}",
                target.sort_key()
            );
            assert_eq!(config.data_directory, Some(root.join(layout.data)));
            assert_eq!(config.profile_directory, Some(root.join(layout.profile)));
            let expected_cache = (target.family() == BrowserFamily::Chromium
                && !target.spec().runtime.uses_chrome_plus())
            .then(|| root.join(layout.cache));
            assert_eq!(
                config.cache_directory,
                expected_cache,
                "{}",
                target.sort_key()
            );
        }
    }

    #[test]
    fn accelerator_requires_https_and_rejects_embedded_credentials() {
        assert!(
            Accelerator {
                prefix: "http://mirror.example/".to_owned(),
                enabled: true,
            }
            .validate()
            .is_err()
        );
        assert!(
            Accelerator {
                prefix: "https://user:secret@mirror.example/".to_owned(),
                enabled: true,
            }
            .validate()
            .is_err()
        );
        assert!(
            Accelerator {
                prefix: "https://mirror.example/".to_owned(),
                enabled: true,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn network_defaults_do_not_add_user_accelerators() {
        assert!(NetworkSettings::default().accelerators.is_empty());
    }
}
