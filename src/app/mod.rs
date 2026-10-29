//! Application state and transactional editing.

pub mod operation;

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

use crate::{
    config::{ConfigError, ConfigStore},
    domain::{
        settings::{Settings, TargetConfig},
        target::TargetId,
    },
    infra::storage,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathResolutionError {
    #[error("root-relative path is not supported; use an absolute path or start with .\\: {0}")]
    RootRelative(PathBuf),
    #[error("path must not contain '..': {0}")]
    ParentTraversal(PathBuf),
}

#[derive(Debug)]
pub struct AppState {
    saved: Settings,
    draft: Settings,
    store: ConfigStore,
    user_config_path: PathBuf,
    portable_config_path: PathBuf,
    tool_root: PathBuf,
    last_error: Option<String>,
}

impl AppState {
    pub fn new(saved: Settings, store: ConfigStore) -> Self {
        let tool_root = store
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Self::with_tool_root(saved, store, tool_root)
    }

    pub fn with_tool_root(saved: Settings, store: ConfigStore, tool_root: PathBuf) -> Self {
        let portable_config_path = storage::config_path(&tool_root);
        let user_config_path = if store.path() == portable_config_path {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| tool_root.clone())
                .join("browser-harbor")
                .join(storage::CONFIG_FILE_NAME)
        } else {
            store.path().to_path_buf()
        };
        Self::with_config_paths(
            saved,
            store,
            tool_root,
            user_config_path,
            portable_config_path,
        )
    }

    pub fn with_config_paths(
        saved: Settings,
        store: ConfigStore,
        tool_root: PathBuf,
        user_config_path: PathBuf,
        portable_config_path: PathBuf,
    ) -> Self {
        Self {
            draft: saved.clone(),
            saved,
            store,
            user_config_path,
            portable_config_path,
            tool_root,
            last_error: None,
        }
    }

    pub fn selected_target(&self) -> TargetId {
        self.draft.selected_browser
    }

    pub fn saved(&self) -> &Settings {
        &self.saved
    }

    pub fn draft(&self) -> &Settings {
        &self.draft
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn cache_dir(&self) -> std::path::PathBuf {
        std::env::temp_dir().join("browser-harbor").join("cache")
    }

    pub fn tool_root(&self) -> &Path {
        &self.tool_root
    }

    pub fn config_path(&self) -> &Path {
        self.store.path()
    }

    pub fn is_portable_config(&self) -> bool {
        self.store.path() == self.portable_config_path
    }

    pub fn switch_config_location(&mut self, portable: bool) -> Result<(), ConfigError> {
        let target_path = if portable {
            self.portable_config_path.clone()
        } else {
            self.user_config_path.clone()
        };
        if self.store.path() == target_path {
            return Ok(());
        }

        let mut next = self.draft.clone();
        normalize_settings_paths(&mut next, &self.tool_root)?;
        let target = ConfigStore::new(target_path);
        target.save(&next)?;

        if !portable {
            fs::remove_file(&self.portable_config_path).map_err(|source| ConfigError::Write {
                path: self.portable_config_path.clone(),
                source,
            })?;
        }

        self.store = target;
        self.saved = next.clone();
        self.draft = next;
        self.last_error = None;
        Ok(())
    }

    pub fn resolve_operation_paths(
        &self,
        mut config: TargetConfig,
    ) -> Result<TargetConfig, PathResolutionError> {
        config.browser_root = config
            .browser_root
            .map(|path| resolve_tool_path(self.tool_root(), path))
            .transpose()?;
        config.browser_directory = config
            .browser_directory
            .map(|path| resolve_tool_path(self.tool_root(), path))
            .transpose()?;
        config.data_directory = config
            .data_directory
            .map(|path| resolve_tool_path(self.tool_root(), path))
            .transpose()?;
        config.profile_directory = config
            .profile_directory
            .map(|path| resolve_tool_path(self.tool_root(), path))
            .transpose()?;
        config.cache_directory = config
            .cache_directory
            .map(|path| resolve_tool_path(self.tool_root(), path))
            .transpose()?;
        config.local_package = config
            .local_package
            .map(|path| resolve_tool_path(self.tool_root(), path))
            .transpose()?;
        Ok(config)
    }

    pub fn select(&mut self, target: TargetId) {
        self.draft.selected_browser = target;
    }

    pub fn update_draft(&mut self, update: impl FnOnce(&mut Settings)) {
        update(&mut self.draft);
    }

    pub fn save(&mut self) -> Result<(), ConfigError> {
        let mut next = self.draft.clone();
        normalize_settings_paths(&mut next, &self.tool_root)?;
        match self.store.save(&next) {
            Ok(()) => {
                self.saved = next.clone();
                self.draft = next;
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn save_if_dirty(&mut self) -> Result<bool, ConfigError> {
        if self.draft == self.saved {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    pub fn discard(&mut self) {
        self.draft = self.saved.clone();
        self.last_error = None;
    }

    pub fn record_install_success(
        &mut self,
        target: TargetId,
        receipt: &crate::services::install::InstallReceipt,
    ) -> Result<(), ConfigError> {
        let mut next = self.draft.clone();
        let config = next
            .targets
            .get_mut(&target)
            .ok_or_else(|| ConfigError::MissingTarget {
                path: self.store.path().to_path_buf(),
                target,
            })?;
        config.installed_version = Some(receipt.version.clone());
        config.browser_directory = Some(receipt.browser_directory.clone());
        config.data_directory = Some(receipt.data_directory.clone());
        config.profile_directory = Some(receipt.profile_directory.clone());
        config.cache_directory = receipt.cache_directory.clone();
        if let Some(chrome_plus_version) = &receipt.chrome_plus_version {
            config.chrome_plus_version = Some(chrome_plus_version.to_owned());
        }
        normalize_settings_paths(&mut next, &self.tool_root)?;
        match self.store.save(&next) {
            Ok(()) => {
                self.saved = next.clone();
                self.draft = next;
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

fn normalize_settings_paths(settings: &mut Settings, tool_root: &Path) -> Result<(), ConfigError> {
    for (target, config) in &mut settings.targets {
        if *config == TargetConfig::for_target(*target) {
            continue;
        }
        config.browser_root = absolute_path(tool_root, config.browser_root.take())?;
        config.browser_directory = absolute_path(tool_root, config.browser_directory.take())?;
        config.data_directory = absolute_path(tool_root, config.data_directory.take())?;
        config.profile_directory = absolute_path(tool_root, config.profile_directory.take())?;
        config.cache_directory = absolute_path(tool_root, config.cache_directory.take())?;
        config.local_package = absolute_path(tool_root, config.local_package.take())?;
    }
    Ok(())
}

fn absolute_path(tool_root: &Path, path: Option<PathBuf>) -> Result<Option<PathBuf>, ConfigError> {
    path.map(|path| {
        if path.is_absolute() {
            return Ok(path);
        }
        if path.has_root()
            || path
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(ConfigError::InvalidPath { path });
        }
        let mut relative = PathBuf::new();
        for component in path.components() {
            if component != Component::CurDir {
                relative.push(component.as_os_str());
            }
        }
        Ok(tool_root.join(relative))
    })
    .transpose()
}

fn resolve_tool_path(root: &Path, path: PathBuf) -> Result<PathBuf, PathResolutionError> {
    if path.is_absolute() {
        return Ok(path);
    }
    if path.has_root() {
        return Err(PathResolutionError::RootRelative(path));
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(PathResolutionError::ParentTraversal(path));
    }
    Ok(root.join(path))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::tempdir;

    use super::{AppState, PathResolutionError};
    use crate::{
        config::ConfigStore,
        domain::{
            settings::{Settings, TargetConfig},
            target::TargetId,
        },
    };

    fn state_in(directory: &tempfile::TempDir) -> AppState {
        AppState::new(
            Settings::default(),
            ConfigStore::new(directory.path().join("BrowserHarbor.json")),
        )
    }

    #[test]
    fn selecting_a_target_changes_only_the_draft_selection() {
        let directory = tempdir().unwrap();
        let mut settings = Settings::default();
        settings
            .targets
            .get_mut(&TargetId::Chrome)
            .unwrap()
            .browser_root = Some(PathBuf::from(r"D:\Browsers\Chrome"));
        let mut state = AppState::new(
            settings,
            ConfigStore::new(directory.path().join("BrowserHarbor.json")),
        );

        state.select(TargetId::Chrome);

        assert_eq!(state.selected_target(), TargetId::Chrome);
        assert_eq!(state.saved().selected_browser, TargetId::Firefox);
        assert_eq!(
            state.draft().targets[&TargetId::Chrome].browser_root,
            Some(PathBuf::from(r"D:\Browsers\Chrome"))
        );
    }

    #[test]
    fn save_commits_the_draft_to_saved_state_and_disk() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("BrowserHarbor.json"));
        let mut state = AppState::new(Settings::default(), store);
        state.select(TargetId::Zen);
        state.update_draft(|settings| {
            settings
                .targets
                .get_mut(&TargetId::Zen)
                .unwrap()
                .browser_root = Some(PathBuf::from(r"D:\浏览器\Zen"));
        });

        state.save().unwrap();

        assert_eq!(state.saved().selected_browser, TargetId::Zen);
        assert_eq!(
            state.saved().targets[&TargetId::Zen].browser_root,
            Some(PathBuf::from(r"D:\浏览器\Zen"))
        );
        assert!(state.last_error().is_none());
    }

    #[test]
    fn discard_restores_saved_settings() {
        let directory = tempdir().unwrap();
        let mut state = state_in(&directory);
        state.select(TargetId::Thorium);
        state.update_draft(|settings| {
            settings
                .targets
                .get_mut(&TargetId::Firefox)
                .unwrap()
                .browser_root = Some(PathBuf::from(r"D:\discarded"));
        });

        state.discard();

        assert_eq!(state.selected_target(), TargetId::Firefox);
        assert_eq!(state.draft(), state.saved());
    }

    #[test]
    fn failed_save_preserves_draft_and_previous_saved_state() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("BrowserHarbor.json");
        let store = ConfigStore::new(path.clone());
        store.save(&Settings::default()).unwrap();
        fs::create_dir(directory.path().join("BrowserHarbor.json.tmp")).unwrap();
        let mut state = AppState::new(store.load().unwrap(), ConfigStore::new(path));
        state.update_draft(|settings| {
            settings
                .targets
                .get_mut(&TargetId::Firefox)
                .unwrap()
                .browser_root = Some(PathBuf::from(r"D:\draft"));
        });

        assert!(state.save().is_err());

        assert_eq!(state.saved(), &Settings::default());
        assert_eq!(
            state.draft().targets[&TargetId::Firefox].browser_root,
            Some(PathBuf::from(r"D:\draft"))
        );
        assert!(state.last_error().is_some());
    }

    #[test]
    fn successful_install_receipt_is_persisted_atomically() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::new(directory.path().join("BrowserHarbor.json"));
        let mut state = AppState::new(Settings::default(), store);
        let receipt = crate::services::install::InstallReceipt {
            version: "1.2.3".to_owned(),
            browser_directory: PathBuf::from(r"D:\Browsers\Chrome\APP"),
            data_directory: PathBuf::from(r"D:\Browsers\Chrome\Data"),
            profile_directory: PathBuf::from(r"D:\Browsers\Chrome\Data"),
            cache_directory: Some(PathBuf::from(r"D:\Browsers\Chrome\Cache")),
            chrome_plus_version: Some("1.18.2".to_owned()),
        };

        state
            .record_install_success(TargetId::Chrome, &receipt)
            .unwrap();

        let saved = &state.saved().targets[&TargetId::Chrome];
        assert_eq!(saved.installed_version.as_deref(), Some("1.2.3"));
        assert_eq!(saved.chrome_plus_version.as_deref(), Some("1.18.2"));
        assert_eq!(
            saved.browser_directory.as_deref(),
            Some(std::path::Path::new(r"D:\Browsers\Chrome\APP"))
        );
        assert_eq!(state.draft(), state.saved());
    }

    #[test]
    fn relative_manager_paths_resolve_against_the_executable_directory() {
        let data_directory = tempdir().unwrap();
        let tool_directory = tempdir().unwrap();
        let state = AppState::with_tool_root(
            Settings::default(),
            ConfigStore::new(data_directory.path().join("BrowserHarbor.json")),
            tool_directory.path().to_path_buf(),
        );
        let mut config = TargetConfig::for_target(TargetId::Helium);
        config.browser_root = Some(PathBuf::from(r".\Helium"));
        config.browser_directory = Some(PathBuf::from(r".\Helium\App"));
        config.local_package = Some(PathBuf::from(r".\Download\helium.zip"));

        let resolved = state.resolve_operation_paths(config).unwrap();

        assert_eq!(
            resolved.browser_root,
            Some(tool_directory.path().join("Helium"))
        );
        assert_eq!(
            resolved.browser_directory,
            Some(tool_directory.path().join(r"Helium\App"))
        );
        assert_eq!(
            resolved.local_package,
            Some(tool_directory.path().join(r"Download\helium.zip"))
        );
        assert_eq!(
            resolved.data_directory,
            Some(tool_directory.path().join(r"Helium\Profiles"))
        );
    }

    #[test]
    fn saving_paths_inside_tool_root_rewrites_them_as_absolute() {
        let data_directory = tempdir().unwrap();
        let tool_directory = tempdir().unwrap();
        let browser_root = tool_directory.path().join("Thorium_AVX512");
        let mut state = AppState::with_tool_root(
            Settings::default(),
            ConfigStore::new(data_directory.path().join("BrowserHarbor.json")),
            tool_directory.path().to_path_buf(),
        );
        let mut config = TargetConfig::for_target(TargetId::Thorium);
        config.browser_root = Some(browser_root.clone());
        config.browser_directory = Some(browser_root.join("BIN"));
        config.data_directory = Some(browser_root.join("USER_DATA"));
        config.profile_directory = Some(browser_root.join("USER_DATA"));
        config.cache_directory = None;
        config.local_package = Some(PathBuf::from(r"D:\Downloads\thorium.7z"));
        state.update_draft(|settings| {
            settings.targets.insert(TargetId::Thorium, config);
        });

        state.save().unwrap();

        let saved = &state.saved().targets[&TargetId::Thorium];
        assert_eq!(saved.browser_root, Some(browser_root.clone()));
        assert_eq!(saved.browser_directory, Some(browser_root.join("BIN")));
        assert_eq!(saved.data_directory, Some(browser_root.join("USER_DATA")));
        assert_eq!(
            saved.local_package,
            Some(PathBuf::from(r"D:\Downloads\thorium.7z"))
        );
    }

    #[test]
    fn save_if_dirty_only_writes_after_a_change() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("BrowserHarbor.json");
        let mut state = AppState::new(Settings::default(), ConfigStore::new(path.clone()));

        assert!(!state.save_if_dirty().unwrap());
        assert!(!path.exists());

        state.update_draft(|settings| settings.theme = crate::domain::settings::ThemeMode::Dark);

        assert!(state.save_if_dirty().unwrap());
        assert!(path.exists());
        assert!(!state.save_if_dirty().unwrap());
    }

    #[test]
    fn switches_between_user_and_portable_configuration_files() {
        let directory = tempdir().unwrap();
        let tool_root = directory.path().join("Tool");
        fs::create_dir_all(&tool_root).unwrap();
        let user_path = directory
            .path()
            .join("Local/browser-harbor/BrowserHarbor.json");
        let portable_path = tool_root.join("BrowserHarbor.json");
        let mut state = AppState::with_config_paths(
            Settings::default(),
            ConfigStore::new(user_path.clone()),
            tool_root,
            user_path.clone(),
            portable_path.clone(),
        );

        state.switch_config_location(true).unwrap();

        assert!(state.is_portable_config());
        assert_eq!(state.config_path(), portable_path);
        assert!(portable_path.is_file());
        assert_eq!(
            ConfigStore::new(portable_path.clone()).load().unwrap(),
            state.saved().clone()
        );

        state.switch_config_location(false).unwrap();

        assert!(!state.is_portable_config());
        assert_eq!(state.config_path(), user_path);
        assert!(user_path.is_file());
        assert!(!portable_path.exists());
    }

    #[test]
    fn root_relative_and_parent_paths_are_rejected() {
        let directory = tempdir().unwrap();
        let state = AppState::new(
            Settings::default(),
            ConfigStore::new(directory.path().join("BrowserHarbor.json")),
        );
        for (path, expected) in [
            (
                PathBuf::from(r"\Helium"),
                PathResolutionError::RootRelative(PathBuf::from(r"\Helium")),
            ),
            (
                PathBuf::from(r"..\Helium"),
                PathResolutionError::ParentTraversal(PathBuf::from(r"..\Helium")),
            ),
        ] {
            let mut config = TargetConfig::for_target(TargetId::Helium);
            config.browser_root = Some(path);
            assert_eq!(state.resolve_operation_paths(config).unwrap_err(), expected);
        }
    }

    #[test]
    fn disposable_cache_lives_under_the_system_temp_directory() {
        let directory = tempdir().unwrap();
        let state = AppState::new(
            Settings::default(),
            ConfigStore::new(directory.path().join("BrowserHarbor.json")),
        );

        assert_eq!(
            state.cache_dir(),
            std::env::temp_dir().join("browser-harbor").join("cache")
        );
        assert!(!state.cache_dir().starts_with(directory.path()));
    }
}
