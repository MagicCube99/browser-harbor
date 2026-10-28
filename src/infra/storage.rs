//! Per-user application storage and legacy-file migration.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStorage {
    pub config_path: PathBuf,
    pub user_config_path: PathBuf,
    pub portable_config_path: PathBuf,
    pub legacy_dirs: Vec<PathBuf>,
}

pub fn resolve(executable: &Path, local_app_data: Option<&std::ffi::OsStr>) -> AppStorage {
    let executable_dir = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let local_app_base = local_app_data
        .map(PathBuf::from)
        .unwrap_or_else(|| executable_dir.clone());
    let data_dir = local_app_base.join("browser-harbor");
    let user_config_path = data_dir.join("manager.json");
    let portable_config_path = executable_dir.join("manager.json");
    let config_path = if portable_config_path.is_file() {
        portable_config_path.clone()
    } else {
        user_config_path.clone()
    };
    let mut legacy_dirs = vec![local_app_base.join("portable-browser-manager")];
    legacy_dirs.dedup();
    AppStorage {
        config_path,
        user_config_path,
        portable_config_path,
        legacy_dirs,
    }
}

pub fn migrate_legacy(storage: &AppStorage) -> Result<(), StorageError> {
    let Some(parent) = storage.config_path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent).map_err(StorageError::Create)?;
    for directory in &storage.legacy_dirs {
        migrate_config(&directory.join("manager.json"), &storage.config_path)?;
        remove_legacy_log(&directory.join("manager.log"))?;
    }
    Ok(())
}

fn migrate_config(legacy: &Path, destination: &Path) -> Result<(), StorageError> {
    if !legacy.is_file() {
        return Ok(());
    }
    let legacy_bytes = fs::read(legacy).map_err(|source| StorageError::Migrate {
        path: legacy.to_path_buf(),
        source,
    })?;
    if destination.is_file() {
        let destination_bytes = fs::read(destination).map_err(|source| StorageError::Migrate {
            path: destination.to_path_buf(),
            source,
        })?;
        if destination_bytes != legacy_bytes {
            backup_conflicting_config(legacy, destination)?;
            return Ok(());
        }
        return remove_legacy_file(legacy);
    }

    let temp = destination.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)
        .map_err(|source| StorageError::Migrate {
            path: temp.clone(),
            source,
        })?;
    file.write_all(&legacy_bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| StorageError::Migrate {
            path: temp.clone(),
            source,
        })?;
    drop(file);
    fs::rename(&temp, destination).map_err(|source| {
        let _ = fs::remove_file(&temp);
        StorageError::Migrate {
            path: destination.to_path_buf(),
            source,
        }
    })?;
    let copied = fs::read(destination).map_err(|source| StorageError::Migrate {
        path: destination.to_path_buf(),
        source,
    })?;
    if copied != legacy_bytes {
        return Err(StorageError::Migrate {
            path: destination.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "copied configuration differs from legacy file",
            ),
        });
    }
    remove_legacy_file(legacy)
}

fn backup_conflicting_config(legacy: &Path, destination: &Path) -> Result<(), StorageError> {
    let parent = destination.parent().ok_or_else(|| StorageError::Migrate {
        path: destination.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination has no parent",
        ),
    })?;
    let mut index = 1;
    let backup = loop {
        let candidate = parent.join(format!("manager.legacy-{index}.json"));
        if !candidate.exists() {
            break candidate;
        }
        index += 1;
    };
    let bytes = fs::read(legacy).map_err(|source| StorageError::Migrate {
        path: legacy.to_path_buf(),
        source,
    })?;
    fs::write(&backup, &bytes).map_err(|source| StorageError::Migrate {
        path: backup.clone(),
        source,
    })?;
    if fs::read(&backup).map_err(|source| StorageError::Migrate {
        path: backup.clone(),
        source,
    })? != bytes
    {
        return Err(StorageError::Migrate {
            path: backup,
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "backup differs from legacy configuration",
            ),
        });
    }
    remove_legacy_file(legacy)
}

fn remove_legacy_log(path: &Path) -> Result<(), StorageError> {
    if path.is_file() {
        remove_legacy_file(path)?;
    }
    Ok(())
}

fn remove_legacy_file(path: &Path) -> Result<(), StorageError> {
    fs::remove_file(path).map_err(|source| StorageError::Migrate {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("failed to create application data directory")]
    Create(#[source] std::io::Error),
    #[error("failed to migrate {path}")]
    Migrate {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{migrate_legacy, resolve};

    #[test]
    fn resolved_config_lives_under_local_app_data() {
        let executable = std::path::Path::new(r"D:\Tools\browser-harbor.exe");
        let storage = resolve(
            executable,
            Some(std::ffi::OsStr::new(r"C:\Users\me\AppData\Local")),
        );

        assert_eq!(
            storage.config_path,
            std::path::PathBuf::from(r"C:\Users\me\AppData\Local\browser-harbor\manager.json")
        );
        assert_eq!(
            storage.user_config_path,
            std::path::PathBuf::from(r"C:\Users\me\AppData\Local\browser-harbor\manager.json")
        );
        assert_eq!(
            storage.portable_config_path,
            std::path::PathBuf::from(r"D:\Tools\manager.json")
        );
        assert_eq!(
            storage.legacy_dirs,
            vec![std::path::PathBuf::from(
                r"C:\Users\me\AppData\Local\portable-browser-manager"
            )]
        );
    }

    #[test]
    fn portable_manager_config_takes_precedence() {
        let executable_dir = tempdir().unwrap();
        let local_app_data = tempdir().unwrap();
        let executable = executable_dir.path().join("browser-harbor.exe");
        fs::write(&executable, b"fixture").unwrap();
        fs::write(
            executable_dir.path().join("manager.json"),
            b"{\"schemaVersion\":5}",
        )
        .unwrap();
        let storage = resolve(&executable, Some(local_app_data.path().as_os_str()));

        assert_eq!(
            storage.config_path,
            executable_dir.path().join("manager.json")
        );
        assert_eq!(
            storage.user_config_path,
            local_app_data.path().join("browser-harbor/manager.json")
        );
    }

    #[test]
    fn migrates_the_previous_local_app_data_directory() {
        let executable_dir = tempdir().unwrap();
        let local_app_data = tempdir().unwrap();
        let executable = executable_dir.path().join("browser-harbor.exe");
        fs::write(&executable, b"fixture").unwrap();
        let old_data_dir = local_app_data.path().join("portable-browser-manager");
        fs::create_dir_all(&old_data_dir).unwrap();
        fs::write(old_data_dir.join("manager.json"), b"{\"schemaVersion\":5}").unwrap();
        fs::write(old_data_dir.join("manager.log"), b"legacy log\n").unwrap();
        let storage = resolve(&executable, Some(local_app_data.path().as_os_str()));

        migrate_legacy(&storage).unwrap();

        assert_eq!(
            fs::read(&storage.config_path).unwrap(),
            b"{\"schemaVersion\":5}"
        );
        assert!(!old_data_dir.join("manager.json").exists());
        assert!(!old_data_dir.join("manager.log").exists());
    }

    #[test]
    fn conflicting_legacy_config_is_backed_up_instead_of_overwritten() {
        let executable_dir = tempdir().unwrap();
        let local_app_data = tempdir().unwrap();
        let executable = executable_dir.path().join("browser-harbor.exe");
        fs::write(&executable, b"fixture").unwrap();
        let old_data_dir = local_app_data.path().join("portable-browser-manager");
        fs::create_dir_all(&old_data_dir).unwrap();
        fs::write(old_data_dir.join("manager.json"), b"legacy").unwrap();
        let storage = resolve(&executable, Some(local_app_data.path().as_os_str()));
        fs::create_dir_all(storage.config_path.parent().unwrap()).unwrap();
        fs::write(&storage.config_path, b"current").unwrap();

        migrate_legacy(&storage).unwrap();

        assert_eq!(fs::read(&storage.config_path).unwrap(), b"current");
        assert_eq!(
            fs::read(
                storage
                    .config_path
                    .parent()
                    .unwrap()
                    .join("manager.legacy-1.json")
            )
            .unwrap(),
            b"legacy"
        );
        assert!(!old_data_dir.join("manager.json").exists());
    }
}
