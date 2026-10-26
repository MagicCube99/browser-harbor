use std::{
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

use super::{RuntimeError, io_error};
use crate::{
    domain::target::TargetId,
    infra::ini::{IniEncoding, IniUpdate, read_ini, update_ini, write_ini},
};

pub fn preserve_files(
    existing_program: Option<&Path>,
    staging_program: &Path,
    relative_paths: &[&str],
) -> Result<(), RuntimeError> {
    let Some(existing_program) = existing_program else {
        return Ok(());
    };
    for relative in relative_paths {
        let source = existing_program.join(relative);
        if !source.exists() {
            continue;
        }
        let destination = staging_program.join(relative);
        copy_path(&source, &destination)?;
    }
    Ok(())
}

pub fn configure_app_launcher(
    program_dir: &Path,
    portable_program_dir: &Path,
    target: TargetId,
    data_dir: &Path,
    profile_dir: &Path,
    cache_dir: Option<&Path>,
) -> Result<(), RuntimeError> {
    match target {
        TargetId::Iceweasel => {
            configure_iceweasel(program_dir, portable_program_dir, data_dir, cache_dir)?
        }
        TargetId::SkyKakapo => {
            configure_skykakapo(program_dir, portable_program_dir, data_dir, profile_dir)?
        }
        _ => {}
    }
    Ok(())
}

pub fn configure_native_package(
    package_staging: &Path,
    existing_root: &Path,
    target: TargetId,
    data_dir: &Path,
    cache_dir: Option<&Path>,
) -> Result<(), RuntimeError> {
    match target {
        TargetId::Catsxp => configure_catsxp(
            package_staging,
            existing_root,
            data_dir,
            cache_dir.ok_or(RuntimeError::MissingComponent)?,
        )?,
        TargetId::Floorp => configure_floorp(package_staging, existing_root)?,
        _ => {}
    }
    Ok(())
}

fn configure_skykakapo(
    program_dir: &Path,
    portable_program_dir: &Path,
    data_dir: &Path,
    profile_dir: &Path,
) -> Result<(), RuntimeError> {
    let path = program_dir.join("tmemutil.ini");
    let (template, encoding) = if path.is_file() {
        read_ini(&path)?
    } else {
        (
            "[General]\r\nPortable=0\r\nPortableDataPath=PortableData\r\nCreateCrashDump=0\r\nCrashDumpType=0\r\nProcessAffinityMask=0\r\n\r\n[Env]\r\n; MOZ_LEGACY_PROFILES=1\r\n"
                .to_owned(),
            IniEncoding::Utf8,
        )
    };
    let data_path = skykakapo_portable_data_path(portable_program_dir, data_dir);
    let updated = update_ini(
        &template,
        &[
            IniUpdate {
                section: "general",
                key: "Portable",
                value: "1",
            },
            IniUpdate {
                section: "general",
                key: "PortableDataPath",
                value: &data_path,
            },
            IniUpdate {
                section: "env",
                key: "MOZ_LEGACY_PROFILES",
                value: "1",
            },
        ],
    );
    write_ini(&path, &updated, encoding)?;
    configure_skykakapo_profile(data_dir, profile_dir)?;
    Ok(())
}

fn skykakapo_portable_data_path(program_dir: &Path, data_dir: &Path) -> String {
    super::portable_relative_path(program_dir, data_dir)
        .unwrap_or_else(|| data_dir.display().to_string())
}

fn configure_skykakapo_profile(data_dir: &Path, profile_dir: &Path) -> Result<(), RuntimeError> {
    let profiles_path = data_dir
        .join("AppData")
        .join("SkyKakapo")
        .join("profiles.ini");
    if profiles_path.exists() {
        return Ok(());
    }
    let profile_path = profile_dir.join("default");
    fs::create_dir_all(&profile_path).map_err(|source| io_error(&profile_path, source))?;
    let (relative, path) = if profile_dir == data_dir {
        (true, "../../default".to_owned())
    } else {
        (false, profile_path.display().to_string())
    };
    let profiles = format!(
        "[General]\r\nStartWithLastProfile=1\r\nVersion=2\r\n\r\n[Profile0]\r\nName=default\r\nIsRelative={}\r\nPath={}\r\nDefault=1\r\n",
        u8::from(relative),
        path
    );
    if let Some(parent) = profiles_path.parent() {
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    }
    fs::write(&profiles_path, profiles).map_err(|source| io_error(&profiles_path, source))
}

fn configure_iceweasel(
    program_dir: &Path,
    portable_program_dir: &Path,
    data_dir: &Path,
    cache_dir: Option<&Path>,
) -> Result<(), RuntimeError> {
    let path = program_dir.join("portable.ini");
    let example = program_dir.join("portable(example).ini");
    let (template, encoding) = if path.is_file() {
        read_ini(&path)?
    } else if example.is_file() {
        read_ini(&example)?
    } else {
        (
            "[General]\r\nPortable=1\r\nUpdate=0\r\n\r\n[Env]\r\nTmpDataPath=\r\n".to_owned(),
            IniEncoding::Utf8,
        )
    };
    let data_path = super::portable_relative_path(portable_program_dir, data_dir)
        .unwrap_or_else(|| data_dir.display().to_string());
    let cache_path = cache_dir.map(|path| {
        super::portable_relative_path(portable_program_dir, path)
            .unwrap_or_else(|| path.display().to_string())
    });
    let updated = update_ini(
        &template,
        &[
            IniUpdate {
                section: "general",
                key: "Portable",
                value: "1",
            },
            IniUpdate {
                section: "general",
                key: "PortableDataPath",
                value: &data_path,
            },
            IniUpdate {
                section: "general",
                key: "Update",
                value: "0",
            },
            IniUpdate {
                section: "env",
                key: "TmpDataPath",
                value: cache_path.as_deref().unwrap_or_default(),
            },
        ],
    );
    write_ini(&path, &updated, encoding)?;
    Ok(())
}

fn configure_catsxp(
    package_staging: &Path,
    existing_root: &Path,
    data_dir: &Path,
    cache_dir: &Path,
) -> Result<(), RuntimeError> {
    let target = package_staging.join("StartupParm.txt");
    let existing = existing_root.join("StartupParm.txt");
    if existing.is_file() {
        fs::copy(&existing, &target).map_err(|source| io_error(&target, source))?;
    }
    let template = if target.is_file() {
        fs::read_to_string(&target).map_err(|source| io_error(&target, source))?
    } else {
        "--disable-catsxp-update\r\n--disable-encryption\r\n".to_owned()
    };
    let updated = update_flat_arguments(
        &template,
        &[
            ("--user-data-dir", &quote_argument(data_dir)),
            ("--disk-cache-dir", &quote_argument(cache_dir)),
        ],
    );
    fs::write(&target, updated).map_err(|source| io_error(&target, source))
}

fn configure_floorp(package_staging: &Path, existing_root: &Path) -> Result<(), RuntimeError> {
    let target = package_staging.join("floorp-portable.yml");
    let existing = existing_root.join("floorp-portable.yml");
    if existing.is_file() {
        fs::copy(&existing, &target).map_err(|source| io_error(&target, source))?;
    }
    let template = if target.is_file() {
        fs::read_to_string(&target).map_err(|source| io_error(&target, source))?
    } else {
        "app:\n  check_for_updates: true\n".to_owned()
    };
    fs::write(&target, disable_floorp_updates(&template))
        .map_err(|source| io_error(&target, source))
}

fn disable_floorp_updates(input: &str) -> String {
    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let mut in_app = false;
    let mut child_indent = None;
    let mut app_line = None;
    let mut replaced = false;
    let mut lines = Vec::new();

    for line in input.lines() {
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            if indent == 0 {
                in_app = trimmed == "app:";
                child_indent = None;
                if in_app {
                    app_line = Some(lines.len());
                }
            } else if in_app {
                child_indent.get_or_insert(indent);
            }
        }
        if in_app
            && child_indent == Some(indent)
            && trimmed.strip_prefix("check_for_updates:").is_some()
        {
            lines.push(format!("{}check_for_updates: false", &line[..indent]));
            replaced = true;
        } else {
            lines.push(line.to_owned());
        }
    }

    match app_line {
        None => lines.extend(["app:".to_owned(), "  check_for_updates: false".to_owned()]),
        Some(index) if !replaced => {
            lines.insert(index + 1, "  check_for_updates: false".to_owned());
        }
        Some(_) => {}
    }
    let mut output = lines.join(newline);
    output.push_str(newline);
    output
}

fn update_flat_arguments(input: &str, updates: &[(&str, &str)]) -> String {
    let newline = if input.contains("\r\n") { "\r\n" } else { "\n" };
    let mut seen = vec![false; updates.len()];
    let mut lines = Vec::new();
    for line in input.lines() {
        let mut replaced = false;
        for (index, (key, value)) in updates.iter().enumerate() {
            if line
                .trim_start()
                .strip_prefix(key)
                .is_some_and(|rest| rest.starts_with('='))
            {
                lines.push(format!("{key}={value}"));
                seen[index] = true;
                replaced = true;
                break;
            }
        }
        if !replaced {
            lines.push(line.to_owned());
        }
    }
    for (index, (key, value)) in updates.iter().enumerate() {
        if !seen[index] {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut result = lines.join(newline);
    result.push_str(newline);
    result
}

fn quote_argument(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn copy_path(source: &Path, destination: &Path) -> Result<(), RuntimeError> {
    if source.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        }
        fs::copy(source, destination).map_err(|error| io_error(destination, error))?;
        return Ok(());
    }
    for entry in WalkDir::new(source) {
        let entry = entry.map_err(|error| RuntimeError::Io {
            path: PathBuf::from(error.path().unwrap_or(source)),
            source: error
                .io_error()
                .map(|error| std::io::Error::new(error.kind(), error.to_string()))
                .unwrap_or_else(|| std::io::Error::other(error.to_string())),
        })?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| RuntimeError::MissingComponent)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|error| io_error(&target, error))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
            }
            fs::copy(entry.path(), &target).map_err(|error| io_error(&target, error))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{
        configure_app_launcher, configure_native_package, disable_floorp_updates,
        skykakapo_portable_data_path,
    };
    use crate::domain::target::TargetId;

    #[test]
    fn floorp_update_guard_preserves_other_settings() {
        let directory = tempdir().unwrap();
        let staging = directory.path().join("staging");
        let existing = directory.path().join("existing");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&existing).unwrap();
        fs::write(
            existing.join("floorp-portable.yml"),
            "app:\r\n  check_for_updates: true\r\n  custom: keep\r\nother: value\r\n",
        )
        .unwrap();

        configure_native_package(
            &staging,
            &existing,
            TargetId::Floorp,
            &directory.path().join("Data"),
            Some(&directory.path().join("Cache")),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(staging.join("floorp-portable.yml")).unwrap(),
            "app:\r\n  check_for_updates: false\r\n  custom: keep\r\nother: value\r\n"
        );
    }

    #[test]
    fn floorp_update_guard_is_created_when_missing() {
        assert_eq!(
            disable_floorp_updates("other: value\n"),
            "other: value\napp:\n  check_for_updates: false\n"
        );
        assert_eq!(
            disable_floorp_updates("app:\n  custom: keep\n"),
            "app:\n  check_for_updates: false\n  custom: keep\n"
        );
    }

    #[test]
    fn iceweasel_portable_ini_uses_configurable_data_and_cache() {
        let directory = tempdir().unwrap();
        let program = directory.path().join("App");
        fs::create_dir_all(&program).unwrap();
        fs::write(
            program.join("portable(example).ini"),
            "[General]\r\nPortable=1\r\nUpdate=1\r\nCustom=keep\r\n[Env]\r\nTmpDataPath=\r\n",
        )
        .unwrap();

        configure_app_launcher(
            &program,
            Path::new(r"D:\Browsers\Iceweasel\App"),
            TargetId::Iceweasel,
            Path::new(r"D:\Browsers\Iceweasel\Profiles"),
            Path::new(r"D:\Browsers\Iceweasel\Profiles"),
            Some(Path::new(r"D:\Browsers\Iceweasel\Cache")),
        )
        .unwrap();

        let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\Profiles"));
        assert!(ini.contains("Update=0"));
        assert!(ini.contains("Custom=keep"));
        assert!(ini.contains(r"TmpDataPath=..\Cache"));
    }

    #[test]
    fn iceweasel_blank_cache_follows_portable_data_path() {
        let directory = tempdir().unwrap();
        let program = directory.path().join("App");
        fs::create_dir_all(&program).unwrap();

        configure_app_launcher(
            &program,
            Path::new(r"D:\Browsers\Iceweasel\App"),
            TargetId::Iceweasel,
            Path::new(r"D:\Browsers\Iceweasel\Profiles"),
            Path::new(r"D:\Browsers\Iceweasel\Profiles"),
            None,
        )
        .unwrap();

        let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\Profiles"));
        assert!(
            ini.lines().any(|line| line.trim() == "TmpDataPath="),
            "{ini}"
        );
        assert!(!ini.contains(r"\Cache"));
    }

    #[test]
    fn iceweasel_uses_relative_paths_across_browser_branches_on_same_volume() {
        let directory = tempdir().unwrap();
        let program = directory.path().join("App");
        fs::create_dir_all(&program).unwrap();

        configure_app_launcher(
            &program,
            Path::new(r"D:\Browsers\Iceweasel\App"),
            TargetId::Iceweasel,
            Path::new(r"D:\Shared\Iceweasel Data"),
            Path::new(r"D:\Shared\Iceweasel Data"),
            Some(Path::new(r"D:\Shared\Iceweasel Cache")),
        )
        .unwrap();

        let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\..\..\Shared\Iceweasel Data"));
        assert!(ini.contains(r"TmpDataPath=..\..\..\Shared\Iceweasel Cache"));
    }

    #[test]
    fn skykakapo_does_not_write_unsupported_tmp_data_path() {
        let directory = tempdir().unwrap();
        let program = directory.path().join("App");
        fs::create_dir_all(&program).unwrap();

        configure_app_launcher(
            &program,
            &program,
            TargetId::SkyKakapo,
            &directory.path().join("Profiles"),
            &directory.path().join("Profiles"),
            None,
        )
        .unwrap();

        let ini = fs::read_to_string(program.join("tmemutil.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\Profiles"));
        assert!(!ini.contains("TmpDataPath"));
        assert!(directory.path().join("Profiles/default").is_dir());
        let profiles = fs::read_to_string(
            directory
                .path()
                .join("Profiles/AppData/SkyKakapo/profiles.ini"),
        )
        .unwrap();
        assert!(profiles.contains("Path=../../default"));
        assert!(profiles.contains("Default=1"));
    }

    #[test]
    fn skykakapo_uses_relative_path_across_browser_branches_on_same_volume() {
        assert_eq!(
            skykakapo_portable_data_path(
                Path::new(r"D:\Browsers\SkyKakapo\App"),
                Path::new(r"D:\Shared\SkyKakapo Data"),
            ),
            r"..\..\..\Shared\SkyKakapo Data"
        );
    }

    #[test]
    fn catsxp_startup_parameters_use_external_data_and_cache() {
        let directory = tempdir().unwrap();
        let staging = directory.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(
            staging.join("StartupParm.txt"),
            "--disable-catsxp-update\r\n",
        )
        .unwrap();

        configure_native_package(
            &staging,
            &directory.path().join("existing"),
            TargetId::Catsxp,
            &directory.path().join("Profiles"),
            Some(&directory.path().join("Profiles").join("Cache")),
        )
        .unwrap();

        let startup = fs::read_to_string(staging.join("StartupParm.txt")).unwrap();
        assert!(startup.contains("--user-data-dir="), "{startup}");
        assert!(startup.contains(r"\Profiles"), "{startup}");
        assert!(startup.contains("--disk-cache-dir="), "{startup}");
        assert!(startup.contains(r"Profiles\Cache"), "{startup}");
    }
}
