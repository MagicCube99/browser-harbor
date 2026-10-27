use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::domain::{
    settings::{CURRENT_SCHEMA_VERSION, Settings, TargetConfig},
    target::TargetId,
};

#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Settings, ConfigError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(Settings::default());
            }
            Err(source) => {
                return Err(ConfigError::Read {
                    path: self.path.clone(),
                    source,
                });
            }
        };

        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|source| ConfigError::InvalidJson {
                path: self.path.clone(),
                source,
            })?;
        let found = value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| ConfigError::InvalidSchemaVersion {
                path: self.path.clone(),
            })?;
        if !(1..=u64::from(CURRENT_SCHEMA_VERSION)).contains(&found) {
            return Err(ConfigError::UnsupportedVersion {
                path: self.path.clone(),
                found,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }

        let mut settings: Settings =
            serde_json::from_value(value).map_err(|source| ConfigError::InvalidJson {
                path: self.path.clone(),
                source,
            })?;
        if found == 1 {
            settings.theme = crate::domain::settings::ThemeMode::System;
        }
        if found < 3 {
            for (target, config) in &mut settings.targets {
                let spec = target.spec();
                config.channel = spec.default_channel;
                config.cpu_build = spec.default_cpu_build;
            }
        }
        settings.schema_version = CURRENT_SCHEMA_VERSION;
        if found < 5 {
            for target in TargetId::ALL {
                if !settings.targets.contains_key(&target) {
                    return Err(ConfigError::MissingTarget {
                        path: self.path.clone(),
                        target,
                    });
                }
            }
        }
        for target in TargetId::ALL {
            settings
                .targets
                .entry(target)
                .or_insert_with(|| TargetConfig::for_target(target));
        }
        if found < 6 {
            for (target, config) in &mut settings.targets {
                if !target.spec().runtime.uses_chrome_plus() {
                    continue;
                }
                let default_cache = config
                    .browser_root
                    .as_ref()
                    .map(|root| root.join(target.spec().layout.cache));
                let profile_relative_cache =
                    config.profile_directory.as_ref().and_then(|profile| {
                        std::path::Path::new(target.spec().layout.cache)
                            .strip_prefix(target.spec().layout.profile)
                            .ok()
                            .map(|relative| profile.join(relative))
                    });
                if config.cache_directory == default_cache
                    || config.cache_directory == profile_relative_cache
                {
                    config.cache_directory = None;
                }
            }
        }
        if found < 7 {
            for (target, config) in &mut settings.targets {
                let profile_relative_cache =
                    config.profile_directory.as_ref().and_then(|profile| {
                        Path::new(target.spec().layout.cache)
                            .strip_prefix(target.spec().layout.profile)
                            .ok()
                            .map(|relative| profile.join(relative))
                    });
                let root_relative_cache = config
                    .browser_root
                    .as_ref()
                    .map(|root| root.join(target.spec().layout.cache));
                if target.cache_follows_profile()
                    && (config.cache_directory == profile_relative_cache
                        || config.cache_directory == root_relative_cache)
                {
                    config.cache_directory = None;
                }
                if !target.supports_custom_cache() {
                    config.cache_directory = None;
                }

                let old_program = match target {
                    TargetId::Brave
                    | TargetId::Catsxp
                    | TargetId::Helium
                    | TargetId::SkyKakapo
                    | TargetId::UngoogledChromium => Some(""),
                    TargetId::Iceweasel => Some("App"),
                    TargetId::Zen => Some("core"),
                    _ => None,
                };
                let Some(old_program) = old_program else {
                    continue;
                };
                let inferred_root = infer_browser_root(config, *target);
                let old_absolute = inferred_root
                    .as_ref()
                    .map(|root| root.join(old_program))
                    .or_else(|| {
                        config
                            .browser_root
                            .as_ref()
                            .map(|root| root.join(old_program))
                    });
                let old_configured = config
                    .browser_root
                    .as_ref()
                    .map(|root| root.join(old_program));
                if config.browser_directory == old_absolute
                    || config.browser_directory == old_configured
                {
                    let root = inferred_root.or_else(|| config.browser_root.clone());
                    if let Some(root) = root
                        && let Some(program) = target.spec().layout.program
                    {
                        config.browser_directory = Some(root.join(program));
                    }
                }
            }
        }
        if found < 8 {
            repair_known_cross_target_paths(&mut settings);
        }
        if found < 9
            && let Some(config) = settings.targets.get_mut(&TargetId::Edge)
            && let Some(program) = config.browser_directory.as_deref()
            && let Some(application) = edge_application_parent(program)
        {
            config.browser_directory = Some(application);
        }
        if found < 11 {
            if let Some(config) = settings.targets.get_mut(&TargetId::Floorp)
                && let Some(root) = config.browser_root.as_ref()
                && config.profile_directory.as_ref() == Some(&root.join("data"))
            {
                config.profile_directory = Some(root.join(r"data\profile\default"));
            }
            if let Some(config) = settings.targets.get_mut(&TargetId::LibreWolf)
                && let Some(root) = config.browser_root.as_ref()
                && config.profile_directory.as_ref() == Some(&root.join("Profiles"))
            {
                config.profile_directory = Some(root.join(r"Profiles\Default"));
            }
        }
        for (target, config) in &mut settings.targets {
            config.fill_default_paths(*target);
        }
        Ok(settings)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn save(&self, settings: &Settings) -> Result<(), ConfigError> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let temp_path = self.temp_path();
        let bytes = self.serialized(settings)?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)
            .map_err(|source| ConfigError::Write {
                path: temp_path.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|source| ConfigError::Write {
                path: temp_path.clone(),
                source,
            })?;
        drop(file);

        fs::rename(&temp_path, &self.path).map_err(|source| {
            let _ = fs::remove_file(&temp_path);
            ConfigError::Write {
                path: self.path.clone(),
                source,
            }
        })
    }

    pub fn compact_if_needed(&self, settings: &Settings) -> Result<bool, ConfigError> {
        if !self.path.is_file() {
            return Ok(false);
        }
        let bytes = self.serialized(settings)?;
        let current = fs::read(&self.path).map_err(|source| ConfigError::Read {
            path: self.path.clone(),
            source,
        })?;
        if current == bytes {
            return Ok(false);
        }
        self.save(settings)?;
        Ok(true)
    }

    fn serialized(&self, settings: &Settings) -> Result<Vec<u8>, ConfigError> {
        let mut persisted = settings.clone();
        persisted.schema_version = CURRENT_SCHEMA_VERSION;
        persisted
            .targets
            .retain(|target, config| *config != TargetConfig::for_target(*target));
        let mut bytes = serde_json::to_vec_pretty(&persisted).map_err(ConfigError::Serialize)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn temp_path(&self) -> PathBuf {
        let file_name = self.path.file_name().unwrap_or_default().to_string_lossy();
        self.path.with_file_name(format!("{file_name}.tmp"))
    }
}

fn infer_browser_root(config: &TargetConfig, target: TargetId) -> Option<PathBuf> {
    let layout = target.spec().layout;
    for (directory, suffix) in [
        (config.profile_directory.as_deref(), layout.profile),
        (config.data_directory.as_deref(), layout.data),
    ] {
        let Some(directory) = directory else {
            continue;
        };
        if !directory.ends_with(suffix) {
            continue;
        }
        let mut root = directory.to_path_buf();
        let mut valid = true;
        for _ in Path::new(suffix).components() {
            valid &= root.pop();
        }
        if valid {
            return Some(root);
        }
    }
    config
        .browser_root
        .clone()
        .filter(|root| root.is_absolute())
}

fn repair_known_cross_target_paths(settings: &mut Settings) {
    for target in [TargetId::Iceweasel, TargetId::SkyKakapo] {
        let Some(config) = settings.targets.get_mut(&target) else {
            continue;
        };
        let Some(root) = config.browser_root.as_deref() else {
            continue;
        };
        if !root
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("Thorium"))
        {
            continue;
        }
        let layout = target.spec().layout;
        if !same_path(config.browser_directory.as_deref(), root, layout.program)
            || !same_path(config.data_directory.as_deref(), root, Some(layout.data))
            || !same_path(
                config.profile_directory.as_deref(),
                root,
                Some(layout.profile),
            )
        {
            continue;
        }
        let defaults = TargetConfig::for_target(target);
        config.browser_root = defaults.browser_root;
        config.browser_directory = defaults.browser_directory;
        config.data_directory = defaults.data_directory;
        config.profile_directory = defaults.profile_directory;
        config.cache_directory = defaults.cache_directory;
    }
}

fn same_path(path: Option<&Path>, root: &Path, suffix: Option<&str>) -> bool {
    let expected = suffix.map_or_else(|| root.to_path_buf(), |suffix| root.join(suffix));
    path.is_some_and(|path| {
        path.to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
    })
}

fn edge_application_parent(program: &Path) -> Option<PathBuf> {
    let version = program.file_name()?.to_str()?;
    if !version
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.')
        || !version.chars().any(|character| character.is_ascii_digit())
    {
        return None;
    }
    let parent = program.parent()?;
    parent
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("Application"))
        .then(|| parent.to_path_buf())
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration contains an invalid relative path: {path}")]
    InvalidPath { path: PathBuf },
    #[error("failed to read configuration at {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("configuration at {path} is not valid JSON")]
    InvalidJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("configuration at {path} has no valid schemaVersion")]
    InvalidSchemaVersion { path: PathBuf },
    #[error("configuration at {path} uses unsupported schema {found}; supported: {supported}")]
    UnsupportedVersion {
        path: PathBuf,
        found: u64,
        supported: u32,
    },
    #[error("configuration at {path} is missing target {target:?}")]
    MissingTarget { path: PathBuf, target: TargetId },
    #[error("failed to serialize configuration")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to write configuration at {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{ConfigError, ConfigStore};
    use crate::domain::settings::{Language, Settings, TargetConfig, ThemeMode, WindowPosition};
    use crate::domain::target::TargetId;

    fn temp_path(directory: &tempfile::TempDir) -> std::path::PathBuf {
        directory.path().join("manager.json")
    }

    #[test]
    fn default_settings_include_all_targets_with_default_paths() {
        let settings = Settings::default();

        assert_eq!(settings.schema_version, 11);
        assert_eq!(settings.selected_browser, TargetId::Firefox);
        assert_eq!(settings.theme, ThemeMode::System);
        assert_eq!(settings.language, Language::Chinese);
        assert_eq!(settings.targets.len(), TargetId::ALL.len());
        assert!(
            settings
                .targets
                .iter()
                .all(|(target, config)| *config == TargetConfig::for_target(*target))
        );
        assert_eq!(
            settings.targets[&TargetId::Helium].browser_root,
            Some(std::path::PathBuf::from(r".\Helium"))
        );
        assert_eq!(
            settings.targets[&TargetId::Helium].browser_directory,
            Some(std::path::PathBuf::from(r".\Helium\App"))
        );
        assert_eq!(
            settings.targets[&TargetId::Helium].data_directory,
            Some(std::path::PathBuf::from(r".\Helium\Profiles"))
        );
    }

    #[test]
    fn missing_config_loads_defaults_without_writing() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());

        assert_eq!(store.load().unwrap(), Settings::default());
        assert!(!path.exists());
    }

    #[test]
    fn default_settings_save_a_minimal_document() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());

        store.save(&Settings::default()).unwrap();

        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 11);
        assert!(value.get("targets").is_none());
        assert!(value.get("network").is_none());
        assert!(value.get("theme").is_none());
        assert!(value.get("language").is_none());
        assert!(value.get("windowPosition").is_none());
        assert!(fs::metadata(&path).unwrap().len() < 1_000);
        assert_eq!(store.load().unwrap(), Settings::default());
    }

    #[test]
    fn schema_v5_partial_targets_are_filled_from_defaults() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(5);
        let targets = value["targets"].as_object_mut().unwrap();
        targets.retain(|key, _| key == "helium");
        targets["helium"]["browserRoot"] = serde_json::json!(r"D:\Portable\Helium");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.schema_version, 11);
        assert_eq!(loaded.targets.len(), TargetId::ALL.len());
        assert_eq!(
            loaded.targets[&TargetId::Helium].browser_root,
            Some(std::path::PathBuf::from(r"D:\Portable\Helium"))
        );
        assert_eq!(
            loaded.targets[&TargetId::Firefox],
            TargetConfig::for_target(TargetId::Firefox)
        );
    }

    #[test]
    fn schema_v5_migrates_default_chromeplus_cache_to_profile_default() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(5);
        value["targets"]["chrome"]["browserRoot"] = serde_json::json!(r"D:\Portable\Chrome");
        value["targets"]["chrome"]["cacheDirectory"] =
            serde_json::json!(r"D:\Portable\Chrome\Profiles\Cache");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.targets[&TargetId::Chrome].cache_directory, None);
    }

    #[test]
    fn schema_v5_preserves_custom_chromeplus_cache_directory() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(5);
        value["targets"]["chrome"]["browserRoot"] = serde_json::json!(r"D:\Portable\Chrome");
        value["targets"]["chrome"]["cacheDirectory"] = serde_json::json!(r"E:\BrowserCache\Chrome");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(
            loaded.targets[&TargetId::Chrome].cache_directory,
            Some(std::path::PathBuf::from(r"E:\BrowserCache\Chrome"))
        );
    }

    #[test]
    fn schema_v5_migrates_default_cache_when_root_is_relative_but_paths_are_absolute() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(5);
        value["targets"]["edge"]["browserRoot"] = serde_json::json!(r".\Edge");
        value["targets"]["edge"]["browserDirectory"] =
            serde_json::json!(r"D:\Tools\Browser\Edge\App");
        value["targets"]["edge"]["dataDirectory"] =
            serde_json::json!(r"D:\Tools\Browser\Edge\Profiles");
        value["targets"]["edge"]["profileDirectory"] =
            serde_json::json!(r"D:\Tools\Browser\Edge\Profiles");
        value["targets"]["edge"]["cacheDirectory"] =
            serde_json::json!(r"D:\Tools\Browser\Edge\Profiles\Cache");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.targets[&TargetId::Edge].cache_directory, None);
    }

    #[test]
    fn schema_v6_moves_flat_and_zen_programs_into_app() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(6);

        value["targets"]["sky_kakapo"]["browserRoot"] = serde_json::json!(r"D:\Portable\SkyKakapo");
        value["targets"]["sky_kakapo"]["browserDirectory"] =
            serde_json::json!(r"D:\Portable\SkyKakapo");
        value["targets"]["sky_kakapo"]["dataDirectory"] =
            serde_json::json!(r"D:\Portable\SkyKakapo\Profiles");
        value["targets"]["sky_kakapo"]["profileDirectory"] =
            serde_json::json!(r"D:\Portable\SkyKakapo\Profiles");
        value["targets"]["sky_kakapo"]["cacheDirectory"] =
            serde_json::json!(r"D:\Portable\SkyKakapo\Profiles\Cache");

        value["targets"]["zen"]["browserRoot"] = serde_json::json!(r"D:\Portable\Zen");
        value["targets"]["zen"]["browserDirectory"] = serde_json::json!(r"D:\Portable\Zen\core");
        value["targets"]["zen"]["dataDirectory"] = serde_json::json!(r"D:\Portable\Zen\Profiles");
        value["targets"]["zen"]["profileDirectory"] =
            serde_json::json!(r"D:\Portable\Zen\Profiles");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();
        let sky = &loaded.targets[&TargetId::SkyKakapo];
        let zen = &loaded.targets[&TargetId::Zen];

        assert_eq!(
            sky.browser_directory,
            Some(std::path::PathBuf::from(r"D:\Portable\SkyKakapo\App"))
        );
        assert_eq!(sky.cache_directory, None);
        assert_eq!(
            zen.browser_directory,
            Some(std::path::PathBuf::from(r"D:\Portable\Zen\App\core"))
        );
        assert_eq!(zen.cache_directory, None);
    }

    #[test]
    fn schema_v7_repairs_thorium_paths_written_to_firefox_targets() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(7);
        value["selectedBrowser"] = serde_json::json!("iceweasel");
        value["targets"]
            .as_object_mut()
            .unwrap()
            .retain(|target, _| matches!(target.as_str(), "iceweasel" | "sky_kakapo"));
        for (target, installed_version) in [("iceweasel", "159.0"), ("sky_kakapo", "158.0.1")] {
            value["targets"][target]["browserRoot"] =
                serde_json::json!(r"D:\Tools\Browser\Thorium");
            value["targets"][target]["browserDirectory"] =
                serde_json::json!(r"D:\Tools\Browser\Thorium\App");
            value["targets"][target]["dataDirectory"] =
                serde_json::json!(r"D:\Tools\Browser\Thorium\Profiles");
            value["targets"][target]["profileDirectory"] =
                serde_json::json!(r"D:\Tools\Browser\Thorium\Profiles");
            value["targets"][target]["installedVersion"] = serde_json::json!(installed_version);
        }
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.schema_version, 11);
        for (target, installed_version) in [
            (TargetId::Iceweasel, "159.0"),
            (TargetId::SkyKakapo, "158.0.1"),
        ] {
            let config = &loaded.targets[&target];
            assert_eq!(
                config.browser_root,
                Some(std::path::PathBuf::from(format!(
                    r".\{}",
                    target.display_name()
                )))
            );
            assert_eq!(config.installed_version.as_deref(), Some(installed_version));
        }
        assert!(store.compact_if_needed(&loaded).unwrap());
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 11);
    }

    #[test]
    fn schema_v8_moves_edge_version_directory_to_stable_application_root() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(8);
        value["selectedBrowser"] = serde_json::json!("edge");
        value["targets"]
            .as_object_mut()
            .unwrap()
            .retain(|target, _| target == "edge");
        value["targets"]["edge"]["browserRoot"] = serde_json::json!(r"D:\Tools\Browser\Edge");
        value["targets"]["edge"]["browserDirectory"] = serde_json::json!(
            r"D:\Tools\Browser\Edge\App\Microsoft\Edge\Application\156.0.4314.42"
        );
        value["targets"]["edge"]["installedVersion"] = serde_json::json!("156.0.4314.42");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.schema_version, 11);
        let edge = &loaded.targets[&TargetId::Edge];
        assert_eq!(
            edge.browser_directory,
            Some(std::path::PathBuf::from(
                r"D:\Tools\Browser\Edge\App\Microsoft\Edge\Application"
            ))
        );
        assert_eq!(edge.installed_version.as_deref(), Some("156.0.4314.42"));
        assert!(store.compact_if_needed(&loaded).unwrap());
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 11);
    }

    #[test]
    fn schema_v10_preserves_floorp_and_librewolf_profiles_for_direct_runtime() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(10);
        value["selectedBrowser"] = serde_json::json!("floorp");
        value["targets"]
            .as_object_mut()
            .unwrap()
            .retain(|target, _| matches!(target.as_str(), "floorp" | "libre_wolf"));

        value["targets"]["floorp"]["browserRoot"] = serde_json::json!(r"D:\Portable\Floorp");
        value["targets"]["floorp"]["browserDirectory"] =
            serde_json::json!(r"D:\Portable\Floorp\app");
        value["targets"]["floorp"]["dataDirectory"] = serde_json::json!(r"D:\Portable\Floorp\data");
        value["targets"]["floorp"]["profileDirectory"] =
            serde_json::json!(r"D:\Portable\Floorp\data");

        value["targets"]["libre_wolf"]["browserRoot"] = serde_json::json!(r"D:\Portable\LibreWolf");
        value["targets"]["libre_wolf"]["browserDirectory"] =
            serde_json::json!(r"D:\Portable\LibreWolf\LibreWolf");
        value["targets"]["libre_wolf"]["dataDirectory"] =
            serde_json::json!(r"D:\Portable\LibreWolf\Profiles");
        value["targets"]["libre_wolf"]["profileDirectory"] =
            serde_json::json!(r"D:\Portable\LibreWolf\Profiles");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.schema_version, 11);
        assert_eq!(
            loaded.targets[&TargetId::Floorp].profile_directory,
            Some(std::path::PathBuf::from(
                r"D:\Portable\Floorp\data\profile\default"
            ))
        );
        assert_eq!(
            loaded.targets[&TargetId::LibreWolf].profile_directory,
            Some(std::path::PathBuf::from(
                r"D:\Portable\LibreWolf\Profiles\Default"
            ))
        );
    }

    #[test]
    fn schema_v10_keeps_custom_floorp_profile_directory() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(10);
        value["targets"]
            .as_object_mut()
            .unwrap()
            .retain(|target, _| target == "floorp");
        value["targets"]["floorp"]["browserRoot"] = serde_json::json!(r"D:\Portable\Floorp");
        value["targets"]["floorp"]["dataDirectory"] = serde_json::json!(r"D:\Portable\Floorp\data");
        value["targets"]["floorp"]["profileDirectory"] =
            serde_json::json!(r"E:\BrowserProfiles\Floorp");
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.schema_version, 11);
        assert_eq!(
            loaded.targets[&TargetId::Floorp].profile_directory,
            Some(std::path::PathBuf::from(r"E:\BrowserProfiles\Floorp"))
        );
    }

    #[test]
    fn saving_a_legacy_full_configuration_compacts_default_targets() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(4);
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();
        assert!(store.compact_if_needed(&loaded).unwrap());

        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 11);
        assert!(saved.get("targets").is_none());
        assert_eq!(store.load().unwrap(), loaded);
        assert!(!store.compact_if_needed(&loaded).unwrap());
    }

    #[test]
    fn settings_round_trip_with_non_ascii_paths_and_no_temp_file() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut settings = Settings {
            theme: ThemeMode::Dark,
            language: Language::English,
            window_position: Some(WindowPosition { x: 320, y: 180 }),
            ..Settings::default()
        };
        settings
            .targets
            .get_mut(&TargetId::Firefox)
            .unwrap()
            .browser_root = Some(r"D:\浏览器\🔥".into());

        store.save(&settings).unwrap();

        assert_eq!(store.load().unwrap(), settings);
        assert!(!directory.path().join("manager.json.tmp").exists());
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"schemaVersion\": 11"));
        assert!(raw.contains("\"selectedBrowser\": \"firefox\""));
        assert!(raw.contains("\"theme\": \"dark\""));
        assert!(raw.contains("\"language\": \"english\""));
        assert!(raw.contains(r#""windowPosition": {"#));
        assert!(raw.contains(r#""browserRoot": "D:\\浏览器\\🔥""#));
    }

    #[test]
    fn disabled_auto_shortcut_round_trips() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut settings = Settings::default();
        settings
            .targets
            .get_mut(&TargetId::Firefox)
            .unwrap()
            .auto_create_shortcut = false;

        store.save(&settings).unwrap();
        let loaded = store.load().unwrap();

        assert!(!loaded.targets[&TargetId::Firefox].auto_create_shortcut);
        assert!(
            fs::read_to_string(path)
                .unwrap()
                .contains("\"autoCreateShortcut\": false")
        );
    }

    #[test]
    fn older_chromeplus_settings_receive_defaults_for_new_public_options() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        let chrome_plus = value["targets"]["chrome"]["chromePlus"]
            .as_object_mut()
            .unwrap();
        chrome_plus.retain(|key, _| {
            matches!(
                key.as_str(),
                "doubleClickClose" | "rightClickClose" | "keepLastTab" | "wheelTab" | "commandLine"
            )
        });
        chrome_plus.insert("doubleClickClose".to_owned(), serde_json::json!(false));
        chrome_plus.insert(
            "commandLine".to_owned(),
            serde_json::json!("--keep-existing"),
        );
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();
        let config = &loaded.targets[&TargetId::Chrome].chrome_plus;

        assert!(!config.double_click_close);
        assert_eq!(config.command_line, "--keep-existing");
        assert!(config.show_password);
        assert!(config.keep_last_tab);
        assert!(config.wheel_tab_when_press_rbutton);
        assert_eq!(config.hover_tab_delay, 400);
        assert_eq!(config.keymapping, None);
    }

    #[test]
    fn saving_replaces_an_existing_config() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path);
        let mut settings = Settings::default();
        store.save(&settings).unwrap();

        settings.selected_browser = TargetId::Zen;
        settings
            .targets
            .get_mut(&TargetId::Zen)
            .unwrap()
            .browser_root = Some(r"D:\updated".into());
        store.save(&settings).unwrap();

        assert_eq!(store.load().unwrap(), settings);
        assert!(!directory.path().join("manager.json.tmp").exists());
    }

    #[test]
    fn malformed_config_is_not_modified() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let malformed = br#"{"schemaVersion": 1, "targets": }"#;
        fs::write(&path, malformed).unwrap();

        let error = store.load().unwrap_err();

        assert!(matches!(error, ConfigError::InvalidJson { .. }));
        assert_eq!(fs::read(&path).unwrap(), malformed);
    }

    #[test]
    fn unsupported_schema_is_rejected_before_targets_are_decoded() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let future = br#"{"schemaVersion": 999}"#;
        fs::write(&path, future).unwrap();

        let error = store.load().unwrap_err();

        assert!(matches!(
            error,
            ConfigError::UnsupportedVersion {
                found: 999,
                supported: 11,
                ..
            }
        ));
        assert_eq!(fs::read(&path).unwrap(), future);
    }

    #[test]
    fn version_one_config_migrates_to_system_theme_without_writing() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(1);
        value.as_object_mut().unwrap().remove("theme");
        let raw = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&path, &raw).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.schema_version, 11);
        assert_eq!(loaded.theme, ThemeMode::System);
        assert_eq!(fs::read(&path).unwrap(), raw);
    }

    #[test]
    fn version_two_config_migrates_target_specific_build_defaults() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(2);
        let targets = value["targets"].as_object_mut().unwrap();
        for target in targets.values_mut() {
            target.as_object_mut().unwrap().remove("channel");
            target.as_object_mut().unwrap().remove("cpuBuild");
            target.as_object_mut().unwrap().remove("chromePlus");
        }
        let raw = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&path, &raw).unwrap();

        let loaded = store.load().unwrap();

        assert_eq!(loaded.schema_version, 11);
        assert_eq!(
            loaded.targets[&TargetId::Iceweasel].channel,
            crate::domain::release::ReleaseChannel::Esr
        );
        assert_eq!(
            loaded.targets[&TargetId::Thorium].cpu_build,
            crate::domain::release::CpuBuild::Sse3
        );
        assert_eq!(fs::read(&path).unwrap(), raw);
    }

    #[test]
    fn version_three_preserves_choices_and_adds_default_paths() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(3);
        value["targets"]["helium"]["browserRoot"] = serde_json::json!(r".\Helium");
        value["targets"]["helium"]["browserDirectory"] = serde_json::json!(r".\Helium\App");
        value["targets"]["helium"]["dataDirectory"] = serde_json::Value::Null;
        value["targets"]["helium"]["profileDirectory"] = serde_json::Value::Null;
        value["targets"]["helium"]["cacheDirectory"] = serde_json::Value::Null;
        value["targets"]["thorium"]["cpuBuild"] = serde_json::json!("avx512");
        value["targets"]["turbo"]["browserRoot"] = serde_json::json!(r"D:\Portable\Turbo");
        for field in [
            "browserDirectory",
            "dataDirectory",
            "profileDirectory",
            "cacheDirectory",
        ] {
            value["targets"]["turbo"][field] = serde_json::Value::Null;
        }
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let loaded = store.load().unwrap();
        let helium = &loaded.targets[&TargetId::Helium];

        assert_eq!(loaded.schema_version, 11);
        assert_eq!(
            helium.browser_root,
            Some(std::path::PathBuf::from(r".\Helium"))
        );
        assert_eq!(
            helium.data_directory,
            Some(std::path::PathBuf::from(r".\Helium\Profiles"))
        );
        assert_eq!(
            loaded.targets[&TargetId::Thorium].cpu_build,
            crate::domain::release::CpuBuild::Avx512
        );
        let turbo = &loaded.targets[&TargetId::Turbo];
        assert_eq!(
            turbo.browser_directory,
            Some(std::path::PathBuf::from(r"D:\Portable\Turbo"))
        );
        assert_eq!(
            turbo.data_directory,
            Some(std::path::PathBuf::from(r"D:\Portable\Turbo\User Data"))
        );
    }

    #[test]
    fn schema_v1_config_missing_a_target_is_rejected_without_writing() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["schemaVersion"] = serde_json::json!(1);
        value["targets"].as_object_mut().unwrap().remove("zen");
        let raw = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&path, &raw).unwrap();

        let error = store.load().unwrap_err();

        assert!(matches!(
            error,
            ConfigError::MissingTarget {
                target: TargetId::Zen,
                ..
            }
        ));
        assert_eq!(fs::read(&path).unwrap(), raw);
    }

    #[test]
    fn failed_save_keeps_previous_config_and_cleans_no_prior_data() {
        let directory = tempdir().unwrap();
        let path = temp_path(&directory);
        let store = ConfigStore::new(path.clone());
        let settings = Settings::default();
        store.save(&settings).unwrap();
        let previous = fs::read(&path).unwrap();
        fs::create_dir(directory.path().join("manager.json.tmp")).unwrap();

        let error = store.save(&settings).unwrap_err();

        assert!(matches!(error, ConfigError::Write { .. }));
        assert_eq!(fs::read(&path).unwrap(), previous);
    }
}
