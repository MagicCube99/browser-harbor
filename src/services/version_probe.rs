//! Local browser version detection without launching the browser.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{
    domain::{
        target::TargetId,
        version::{VersionSource, normalize_version},
    },
    infra::ini::{IniFileError, read_ini},
    infra::windows::version_info::{
        VersionInfoError, executable_file_version, executable_product_version,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionProbe {
    pub version: String,
    pub source: VersionSource,
    pub executable: PathBuf,
}

#[derive(Debug, Error)]
pub enum VersionProbeError {
    #[error("browser executable could not be located below {0}")]
    ExecutableNotFound(PathBuf),
    #[error("application.ini is missing beside {0}")]
    ApplicationIniMissing(PathBuf),
    #[error("application.ini has no non-empty [App] Version beside {0}")]
    ApplicationVersionMissing(PathBuf),
    #[error("application.ini cannot be read at {path}: {source}")]
    ApplicationIniRead {
        path: PathBuf,
        #[source]
        source: IniFileError,
    },
    #[error("Windows version resource is unavailable for {path}: {source}")]
    ExecutableVersion {
        path: PathBuf,
        #[source]
        source: VersionInfoError,
    },
    #[error("Brave product version is malformed for {path}: {version}")]
    BraveVersionFormat { path: PathBuf, version: String },
}

pub fn probe_local_version(
    target: TargetId,
    program_directory: &Path,
) -> Result<VersionProbe, VersionProbeError> {
    let executable = locate_executable(target, program_directory)?;
    let source = target.spec().version_source;
    let version = match source {
        VersionSource::ApplicationIni => application_ini_version(target, &executable)?,
        VersionSource::ExecutableResource => {
            executable_product_version(&executable).map_err(|source| {
                VersionProbeError::ExecutableVersion {
                    path: executable.clone(),
                    source,
                }
            })?
        }
        VersionSource::ExecutableFileVersion => {
            executable_file_version(&executable).map_err(|source| {
                VersionProbeError::ExecutableVersion {
                    path: executable.clone(),
                    source,
                }
            })?
        }
        VersionSource::BraveProductVersion => brave_product_version(&executable)?,
        VersionSource::InstallReceipt => {
            return Err(VersionProbeError::ApplicationVersionMissing(executable));
        }
    };
    Ok(VersionProbe {
        version,
        source,
        executable,
    })
}

fn brave_product_version(executable: &Path) -> Result<String, VersionProbeError> {
    let raw = executable_product_version(executable).map_err(|source| {
        VersionProbeError::ExecutableVersion {
            path: executable.to_path_buf(),
            source,
        }
    })?;
    normalize_brave_product_version(&raw).ok_or_else(|| VersionProbeError::BraveVersionFormat {
        path: executable.to_path_buf(),
        version: raw,
    })
}

fn normalize_brave_product_version(raw: &str) -> Option<String> {
    let mut parts = raw.split('.');
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();
    if first.len() >= 3 {
        return raw
            .split_once('.')
            .map(|(_, rest)| rest.to_owned())
            .filter(|rest| !rest.is_empty());
    }
    (!first.is_empty() && !second.is_empty()).then(|| raw.to_owned())
}

pub fn locate_executable(
    target: TargetId,
    program_directory: &Path,
) -> Result<PathBuf, VersionProbeError> {
    for candidate in target.spec().executable_candidates {
        if let Some(path) = case_insensitive_child(program_directory, candidate) {
            return Ok(path);
        }
    }
    if target == TargetId::Edge
        && let Ok(entries) = std::fs::read_dir(program_directory)
    {
        let mut versions = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                (path.is_dir() && is_version_directory_name(&name))
                    .then(|| (version_directory_key(&name), path))
            })
            .collect::<Vec<_>>();
        versions.sort_by(|left, right| right.0.cmp(&left.0));
        for (_, directory) in versions {
            for candidate in target.spec().executable_candidates {
                if let Some(path) = case_insensitive_child(&directory, candidate) {
                    return Ok(path);
                }
            }
        }
    }
    Err(VersionProbeError::ExecutableNotFound(
        program_directory.to_path_buf(),
    ))
}

pub(crate) fn case_insensitive_child(directory: &Path, name: &str) -> Option<PathBuf> {
    let direct = directory.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            (entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
                && entry.path().is_file())
            .then(|| entry.path())
        })
}

fn is_version_directory_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
        && name.chars().any(|character| character.is_ascii_digit())
}

fn version_directory_key(name: &str) -> Vec<u64> {
    name.split('.')
        .map(|part| part.parse().unwrap_or_default())
        .collect()
}

fn application_ini_version(
    target: TargetId,
    executable: &Path,
) -> Result<String, VersionProbeError> {
    let parent = executable
        .parent()
        .ok_or_else(|| VersionProbeError::ApplicationIniMissing(executable.to_path_buf()))?;
    let ini_path = parent.join("application.ini");
    if !ini_path.is_file() {
        return Err(VersionProbeError::ApplicationIniMissing(
            executable.to_path_buf(),
        ));
    }
    let (text, _) =
        read_ini(&ini_path).map_err(|source| VersionProbeError::ApplicationIniRead {
            path: ini_path.clone(),
            source,
        })?;
    let mut in_app = false;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_app = line[1..line.len() - 1].trim().eq_ignore_ascii_case("App");
            continue;
        }
        if in_app
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("Version")
        {
            let version = normalize_version(value);
            if !version.is_empty() {
                return Ok(if target == TargetId::Floorp {
                    version
                        .split_once('@')
                        .map_or(version.clone(), |(product, _)| product.to_owned())
                } else {
                    version
                });
            }
        }
    }
    Err(VersionProbeError::ApplicationVersionMissing(
        executable.to_path_buf(),
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use tempfile::tempdir;

    use super::{VersionProbeError, normalize_brave_product_version, probe_local_version};
    use crate::domain::{target::TargetId, version::VersionSource};
    use crate::infra::windows::version_info::executable_file_version;

    fn touch(path: &Path) {
        fs::write(path, b"fixture").unwrap();
    }

    #[test]
    fn gecko_reads_the_product_version_from_application_ini() {
        let directory = tempdir().unwrap();
        touch(&directory.path().join("zen.exe"));
        fs::write(
            directory.path().join("application.ini"),
            "[App]\r\nName=Zen\r\nVersion=1.23.2b\r\nBuildID=20261009073723\r\n",
        )
        .unwrap();

        let probe = probe_local_version(TargetId::Zen, directory.path()).unwrap();

        assert_eq!(probe.version, "1.23.2b");
        assert_eq!(probe.source, VersionSource::ApplicationIni);
        assert_eq!(probe.executable, directory.path().join("zen.exe"));
    }

    #[test]
    fn gecko_rejects_a_missing_application_version() {
        let directory = tempdir().unwrap();
        touch(&directory.path().join("firefox.exe"));
        fs::write(
            directory.path().join("application.ini"),
            "[App]\r\nName=Firefox\r\n",
        )
        .unwrap();

        let error = probe_local_version(TargetId::Firefox, directory.path()).unwrap_err();

        assert!(matches!(
            error,
            VersionProbeError::ApplicationVersionMissing(_)
        ));
    }

    #[test]
    fn floorp_local_version_excludes_the_embedded_gecko_suffix() {
        let directory = tempdir().unwrap();
        touch(&directory.path().join("floorp.exe"));
        fs::write(
            directory.path().join("application.ini"),
            "[App]\r\nName=Floorp\r\nVersion=12.20.2@158.0.1\r\n[Gecko]\r\nMinVersion=158.0.1\r\n",
        )
        .unwrap();

        let probe = probe_local_version(TargetId::Floorp, directory.path()).unwrap();

        assert_eq!(probe.version, "12.20.2");
        assert_eq!(probe.source, VersionSource::ApplicationIni);
    }

    #[test]
    fn chromium_reads_the_copied_executable_version_resource() {
        let directory = tempdir().unwrap();
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let source = Path::new(&windows).join(r"System32\notepad.exe");
        let executable = directory.path().join("chrome.exe");
        fs::copy(source, &executable).unwrap();

        let probe = probe_local_version(TargetId::Chrome, directory.path()).unwrap();

        assert_eq!(probe.source, VersionSource::ExecutableResource);
        assert_eq!(probe.executable, executable);
        assert!(probe.version.contains('.'));
    }

    #[test]
    fn launcher_wrappers_are_not_accepted_as_browser_executables() {
        let directory = tempdir().unwrap();
        touch(&directory.path().join("RunChrome_x64.exe"));

        let error = probe_local_version(TargetId::Chrome, directory.path()).unwrap_err();

        assert!(matches!(error, VersionProbeError::ExecutableNotFound(_)));
    }

    #[test]
    fn edge_application_parent_uses_the_newest_version_directory() {
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let source = Path::new(&windows).join(r"System32\notepad.exe");
        let directory = tempdir().unwrap();
        for version in ["155.0.1", "156.0.2"] {
            let version_directory = directory.path().join(version);
            fs::create_dir_all(&version_directory).unwrap();
            fs::copy(&source, version_directory.join("msedge.exe")).unwrap();
        }

        let probe = probe_local_version(TargetId::Edge, directory.path()).unwrap();

        assert_eq!(
            probe.executable,
            directory.path().join("156.0.2").join("msedge.exe")
        );
    }

    #[test]
    #[ignore = "requires BROWSER_HARBOR_HELIUM_ROOT"]
    fn helium_compares_the_installed_file_version() {
        let root = std::env::var_os("BROWSER_HARBOR_HELIUM_ROOT")
            .map(PathBuf::from)
            .expect("BROWSER_HARBOR_HELIUM_ROOT");
        let expected = executable_file_version(&root.join("chrome.exe")).unwrap();

        let probe = probe_local_version(TargetId::Helium, &root).unwrap();

        assert_eq!(probe.version, expected);
        assert_eq!(probe.source, VersionSource::ExecutableFileVersion);
    }

    #[test]
    fn brave_removes_the_chromium_major_prefix_from_its_product_version() {
        assert_eq!(
            normalize_brave_product_version("156.1.98.60").as_deref(),
            Some("1.98.60")
        );
        assert_eq!(
            normalize_brave_product_version("1.98.60").as_deref(),
            Some("1.98.60")
        );
        assert_eq!(normalize_brave_product_version(""), None);
    }
}
