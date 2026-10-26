use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{RuntimeError, download_artifact, io_error, pe_imports_dll};
use crate::{
    domain::{
        runtime::{ChromePlusConfig, ChromePlusInstallation, ChromePlusRelease},
        target::{ChromePlusMode, TargetId},
    },
    infra::ini::{
        IniEncoding, IniUpdate, ini_section_entries, read_ini, update_ini, update_ini_map,
        write_ini,
    },
    infra::windows::version_info::executable_product_version,
    services::http::{HttpClient, github_api_candidates},
};

const CHROME_PLUS_LATEST: &str =
    "https://api.github.com/repos/Bush2021/chrome_plus/releases/latest";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BROWSER_DEFAULT_CACHE_DIR: &str = "none";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

pub async fn resolve_latest(http: &HttpClient) -> Result<ChromePlusRelease, RuntimeError> {
    let release = http
        .get_json_candidates::<GitHubRelease>(&github_api_candidates(CHROME_PLUS_LATEST))
        .await?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.starts_with("Chrome++_v") && asset.name.ends_with(".7z"))
        .ok_or(RuntimeError::MissingComponent)?;
    let setdll = release
        .assets
        .iter()
        .find(|asset| asset.name == "setdll.7z")
        .ok_or(RuntimeError::MissingComponent)?;
    Ok(ChromePlusRelease {
        version: release.tag_name,
        artifact: download_artifact(
            asset.name.clone(),
            asset.browser_download_url.clone(),
            Some(asset.size),
            asset.digest.clone(),
        ),
        setdll_artifact: download_artifact(
            setdll.name.clone(),
            setdll.browser_download_url.clone(),
            Some(setdll.size),
            setdll.digest.clone(),
        ),
    })
}

pub fn installed_version(program_dir: &Path) -> Result<String, RuntimeError> {
    executable_product_version(&program_dir.join("version.dll")).map_err(Into::into)
}

pub struct ChromePlusPrepareRequest<'a> {
    pub runtime_root: &'a Path,
    pub staging_program: &'a Path,
    pub final_program: &'a Path,
    pub existing_program: Option<&'a Path>,
    pub backup_dir: &'a Path,
    pub data_dir: &'a Path,
    pub cache_dir: Option<&'a Path>,
    pub config: &'a ChromePlusConfig,
    pub mode: ChromePlusMode,
    pub version: &'a str,
    pub product: TargetId,
    pub setdll_runtime_root: Option<&'a Path>,
}

pub fn prepare(
    request: ChromePlusPrepareRequest<'_>,
) -> Result<ChromePlusInstallation, RuntimeError> {
    let runtime_app = request.runtime_root.join("x64").join("App");
    let source_dll = runtime_app.join("version.dll");
    let source_ini = runtime_app.join("chrome++.ini");
    if !source_dll.is_file() || !source_ini.is_file() {
        return Err(RuntimeError::MissingComponent);
    }

    let new_hash = sha256_file(&source_dll)?;
    let existing_dll = request
        .existing_program
        .map(|path| path.join("version.dll"));
    let existing_ini = request
        .existing_program
        .map(|path| path.join("chrome++.ini"));
    let same_version = existing_dll
        .as_deref()
        .is_some_and(|path| sha256_file(path).is_ok_and(|hash| hash == new_hash));

    let target_dll = request.staging_program.join("version.dll");
    let target_ini = request.staging_program.join("chrome++.ini");
    fs::copy(&source_dll, &target_dll).map_err(|source| io_error(&target_dll, source))?;
    let cache_dir = request
        .cache_dir
        .unwrap_or_else(|| Path::new(BROWSER_DEFAULT_CACHE_DIR));
    let portable_data_dir = chromeplus_portable_path(request.final_program, request.data_dir);
    let portable_cache_dir = request
        .cache_dir
        .map(|path| chromeplus_portable_path(request.final_program, path));
    let portable_cache = portable_cache_dir.as_deref().unwrap_or(cache_dir);

    match request.mode {
        ChromePlusMode::DataOnly => {
            write_ini(
                &target_ini,
                &data_only_ini(&portable_data_dir, portable_cache),
                IniEncoding::Utf16Le,
            )?;
        }
        ChromePlusMode::Full
            if same_version && existing_ini.as_deref().is_some_and(|path| path.is_file()) =>
        {
            let existing_ini = existing_ini
                .as_deref()
                .ok_or(RuntimeError::MissingComponent)?;
            fs::copy(existing_ini, &target_ini).map_err(|source| io_error(&target_ini, source))?;
        }
        ChromePlusMode::Full => {
            if let Some(existing_ini) = existing_ini.as_deref().filter(|path| path.is_file()) {
                fs::create_dir_all(request.backup_dir)
                    .map_err(|source| io_error(request.backup_dir, source))?;
                let backup = request.backup_dir.join(format!(
                    "chrome++.ini.{}.bak",
                    safe_version(request.version)
                ));
                fs::copy(existing_ini, &backup).map_err(|source| io_error(&backup, source))?;
            }
            let (template, encoding) = read_ini(&source_ini)?;
            let updated = apply_config(
                &template,
                request.config,
                Some(&portable_data_dir),
                Some(portable_cache),
            )?;
            write_ini(&target_ini, &updated, encoding)?;
        }
        ChromePlusMode::Disabled => return Err(RuntimeError::MissingComponent),
    }

    if !target_dll.is_file() || !target_ini.is_file() {
        return Err(RuntimeError::MissingComponent);
    }
    if request.product == TargetId::Edge {
        inject_edge_import(request.staging_program, request.setdll_runtime_root)?;
    }
    Ok(ChromePlusInstallation {
        version: request.version.to_owned(),
        dll_sha256: new_hash,
        ini_path: target_ini,
    })
}

fn inject_edge_import(program_dir: &Path, setdll_root: Option<&Path>) -> Result<(), RuntimeError> {
    let setdll_root = setdll_root.ok_or(RuntimeError::MissingComponent)?;
    let helper = setdll_root.join("setdll-x64.exe");
    let executable = program_dir.join("msedge.exe");
    if !helper.is_file() || !executable.is_file() {
        return Err(RuntimeError::MissingComponent);
    }
    let status = Command::new(&helper)
        .arg("/d:version.dll")
        .arg(&executable)
        .current_dir(program_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|source| io_error(&helper, source))?;
    if !status.success() {
        return Err(RuntimeError::HelperFailed(status.code().unwrap_or(-1)));
    }
    if !pe_imports_dll(&executable, "version.dll")
        .map_err(|source| io_error(&executable, source))?
    {
        return Err(RuntimeError::MissingComponent);
    }
    Ok(())
}

pub fn update_existing_ini(ini_path: &Path, config: &ChromePlusConfig) -> Result<(), RuntimeError> {
    let (original, encoding) = read_ini(ini_path)?;
    let updated = apply_config(&original, config, None, None)?;
    write_ini_atomic(ini_path, &updated, encoding)
}

pub fn update_portable_paths(
    ini_path: &Path,
    data_dir: &str,
    cache_dir: &str,
) -> Result<(), RuntimeError> {
    let (original, encoding) = read_ini(ini_path)?;
    let updated = update_ini(
        &original,
        &[
            IniUpdate {
                section: "general",
                key: "data_dir",
                value: data_dir,
            },
            IniUpdate {
                section: "general",
                key: "cache_dir",
                value: cache_dir,
            },
        ],
    );
    write_ini_atomic(ini_path, &updated, encoding)
}

pub fn backup_ini(
    ini_path: &Path,
    backup_dir: &Path,
    label: &str,
) -> Result<PathBuf, RuntimeError> {
    if !ini_path.is_file() {
        return Err(RuntimeError::MissingComponent);
    }
    fs::create_dir_all(backup_dir).map_err(|source| io_error(backup_dir, source))?;
    let file_name = ini_path.file_name().unwrap_or_default().to_string_lossy();
    let mut backup = backup_dir.join(format!("{file_name}.{label}.bak"));
    let mut suffix = 2;
    while backup.exists() {
        backup = backup_dir.join(format!("{file_name}.{label}.{suffix}.bak"));
        suffix += 1;
    }
    fs::copy(ini_path, &backup).map_err(|source| io_error(&backup, source))?;
    Ok(backup)
}

pub fn restore_default_config(ini_path: &Path, backup_dir: &Path) -> Result<PathBuf, RuntimeError> {
    let backup = backup_ini(ini_path, backup_dir, "reset")?;
    let (original, encoding) = read_ini(ini_path)?;
    let updated = apply_config(&original, &ChromePlusConfig::default(), None, None)?;
    write_ini_atomic(ini_path, &updated, encoding)?;
    Ok(backup)
}

fn write_ini_atomic(
    ini_path: &Path,
    text: &str,
    encoding: IniEncoding,
) -> Result<(), RuntimeError> {
    let temp = ini_path.with_extension("ini.pbm.tmp");
    write_ini(&temp, text, encoding)?;
    if let Err(source) = fs::rename(&temp, ini_path) {
        let _ = fs::remove_file(&temp);
        return Err(io_error(ini_path, source));
    }
    Ok(())
}

pub fn keymapping_text(ini_path: &Path) -> Result<String, RuntimeError> {
    let (text, _) = read_ini(ini_path)?;
    Ok(ini_section_entries(&text, "keymapping")
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn read_config_from_ini(
    ini_path: &Path,
    base: &ChromePlusConfig,
) -> Result<ChromePlusConfig, RuntimeError> {
    let (text, _) = read_ini(ini_path)?;
    Ok(merge_config_from_ini(&text, base))
}

fn merge_config_from_ini(input: &str, base: &ChromePlusConfig) -> ChromePlusConfig {
    let general = ini_section_entries(input, "general");
    let tabs = ini_section_entries(input, "tabs");
    let keymapping = ini_section_entries(input, "keymapping")
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut config = base.clone();
    apply_string(&general, "command_line", &mut config.command_line);
    apply_string(&general, "launch_on_startup", &mut config.launch_on_startup);
    apply_string(&general, "launch_on_exit", &mut config.launch_on_exit);
    apply_string(&general, "boss_key", &mut config.boss_key);
    apply_string(&general, "translate_key", &mut config.translate_key);
    apply_bool(&general, "show_password", &mut config.show_password);
    apply_bool(&general, "win32k", &mut config.win32k);
    apply_bool(&general, "ignore_policies", &mut config.ignore_policies);
    apply_bool(
        &general,
        "suppress_false_upgrade_notification",
        &mut config.suppress_false_upgrade_notification,
    );
    apply_bool(&tabs, "double_click_close", &mut config.double_click_close);
    apply_bool(&tabs, "right_click_close", &mut config.right_click_close);
    apply_bool(&tabs, "keep_last_tab", &mut config.keep_last_tab);
    apply_bool(&tabs, "wheel_tab", &mut config.wheel_tab);
    apply_bool(
        &tabs,
        "wheel_tab_when_press_rbutton",
        &mut config.wheel_tab_when_press_rbutton,
    );
    apply_bool(&tabs, "open_url_new_tab", &mut config.open_url_new_tab);
    apply_bool(
        &tabs,
        "open_bookmark_new_tab",
        &mut config.open_bookmark_new_tab,
    );
    apply_bool(&tabs, "new_tab_disable", &mut config.new_tab_disable);
    apply_string(
        &tabs,
        "new_tab_disable_name",
        &mut config.new_tab_disable_name,
    );
    apply_bool(&tabs, "hover_tab", &mut config.hover_tab);
    if let Some(value) = entry(&tabs, "hover_tab_delay").and_then(|value| value.parse().ok()) {
        config.hover_tab_delay = value;
    }
    config.keymapping = Some(keymapping);
    config
}

fn entry<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}

fn apply_string(entries: &[(String, String)], key: &str, target: &mut String) {
    if let Some(value) = entry(entries, key) {
        *target = value.to_owned();
    }
}

fn apply_bool(entries: &[(String, String)], key: &str, target: &mut bool) {
    if let Some(value) = entry(entries, key).and_then(|value| match value.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }) {
        *target = value;
    }
}

fn apply_config(
    input: &str,
    config: &ChromePlusConfig,
    data_dir: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Result<String, RuntimeError> {
    let mut owned = Vec::new();
    if let Some(data_dir) = data_dir {
        owned.push(("general", "data_dir", ini_path_value(data_dir)));
    }
    if let Some(cache_dir) = cache_dir {
        owned.push(("general", "cache_dir", ini_path_value(cache_dir)));
    }
    owned.extend([
        ("general", "command_line", config.command_line.clone()),
        (
            "general",
            "launch_on_startup",
            config.launch_on_startup.clone(),
        ),
        ("general", "launch_on_exit", config.launch_on_exit.clone()),
        ("general", "boss_key", config.boss_key.clone()),
        ("general", "translate_key", config.translate_key.clone()),
        (
            "general",
            "show_password",
            bool_value(config.show_password).to_owned(),
        ),
        ("general", "win32k", bool_value(config.win32k).to_owned()),
        (
            "general",
            "ignore_policies",
            bool_value(config.ignore_policies).to_owned(),
        ),
        (
            "general",
            "suppress_false_upgrade_notification",
            bool_value(config.suppress_false_upgrade_notification).to_owned(),
        ),
        (
            "tabs",
            "double_click_close",
            bool_value(config.double_click_close).to_owned(),
        ),
        (
            "tabs",
            "right_click_close",
            bool_value(config.right_click_close).to_owned(),
        ),
        (
            "tabs",
            "keep_last_tab",
            bool_value(config.keep_last_tab).to_owned(),
        ),
        ("tabs", "wheel_tab", bool_value(config.wheel_tab).to_owned()),
        (
            "tabs",
            "wheel_tab_when_press_rbutton",
            bool_value(config.wheel_tab_when_press_rbutton).to_owned(),
        ),
        (
            "tabs",
            "open_url_new_tab",
            bool_value(config.open_url_new_tab).to_owned(),
        ),
        (
            "tabs",
            "open_bookmark_new_tab",
            bool_value(config.open_bookmark_new_tab).to_owned(),
        ),
        (
            "tabs",
            "new_tab_disable",
            bool_value(config.new_tab_disable).to_owned(),
        ),
        (
            "tabs",
            "new_tab_disable_name",
            config.new_tab_disable_name.clone(),
        ),
        ("tabs", "hover_tab", bool_value(config.hover_tab).to_owned()),
        (
            "tabs",
            "hover_tab_delay",
            config.hover_tab_delay.to_string(),
        ),
    ]);
    let updates = owned
        .iter()
        .map(|(section, key, value)| IniUpdate {
            section,
            key,
            value,
        })
        .collect::<Vec<_>>();
    let mut output = update_ini(input, &updates);
    if let Some(keymapping) = config.keymapping.as_deref() {
        output = update_ini_map(&output, "keymapping", &parse_keymapping(keymapping)?);
    }
    Ok(output)
}

fn parse_keymapping(input: &str) -> Result<Vec<(String, String)>, RuntimeError> {
    let mut entries = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(RuntimeError::InvalidChromePlusKeyMapping {
                line: index + 1,
                value: line.to_owned(),
            });
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty()
            || value.is_empty()
            || entries
                .iter()
                .any(|(existing, _): &(String, String)| existing.eq_ignore_ascii_case(key))
        {
            return Err(RuntimeError::InvalidChromePlusKeyMapping {
                line: index + 1,
                value: line.to_owned(),
            });
        }
        entries.push((key.to_owned(), value.to_owned()));
    }
    Ok(entries)
}

fn data_only_ini(data_dir: &Path, cache_dir: &Path) -> String {
    format!(
        "[general]\r\ndata_dir={}\r\ncache_dir={}\r\n",
        ini_path_value(data_dir),
        ini_path_value(cache_dir)
    )
}

fn sha256_file(path: &Path) -> Result<String, RuntimeError> {
    let bytes = fs::read(path).map_err(|source| io_error(path, source))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn ini_path_value(path: &Path) -> String {
    path.display().to_string()
}

fn chromeplus_portable_path(program_dir: &Path, target: &Path) -> PathBuf {
    super::portable_relative_path(program_dir, target).map_or_else(
        || target.to_path_buf(),
        |relative| PathBuf::from(format!(r"%app%\{relative}")),
    )
}

fn bool_value(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn safe_version(version: &str) -> String {
    version
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ChromePlusPrepareRequest, apply_config, backup_ini, data_only_ini, installed_version,
        merge_config_from_ini, prepare, resolve_latest, restore_default_config,
    };
    use crate::{
        domain::{
            runtime::ChromePlusConfig,
            settings::NetworkSettings,
            target::{ChromePlusMode, TargetId},
        },
        infra::ini::{IniEncoding, read_ini, write_ini},
        services::http::HttpClient,
    };
    use std::{fs, path::Path};
    use tempfile::tempdir;

    #[test]
    fn data_only_ini_contains_only_paths() {
        let ini = data_only_ini(
            Path::new(r"D:\Portable\Data"),
            Path::new(r"D:\Portable\Cache"),
        );
        assert!(ini.contains("data_dir=D:\\Portable\\Data"));
        assert!(ini.contains("cache_dir=D:\\Portable\\Cache"));
        assert!(!ini.contains("[tabs]"));
        assert!(!ini.contains("boss_key"));
    }

    #[test]
    fn full_config_writes_all_chromeplus_1812_options_and_replaces_mapping() {
        let config = ChromePlusConfig {
            command_line: "--disable-features=OutdatedBuildDetector".to_owned(),
            launch_on_startup: "startup.bat".to_owned(),
            launch_on_exit: "clean.bat;taskkill /f /im chrome.exe;".to_owned(),
            boss_key: "Ctrl+Alt+B".to_owned(),
            translate_key: "Ctrl+Alt+T".to_owned(),
            keymapping: Some("F3=Ctrl+PageDown".to_owned()),
            ..ChromePlusConfig::default()
        };
        let output = apply_config(
            "[general]\r\ndata_dir=old\r\n[keymapping]\r\n; note\r\nF2=Ctrl+PageUp\r\n",
            &config,
            Some(Path::new(r"D:\Portable\Data")),
            Some(Path::new(r"D:\Portable\Cache")),
        )
        .unwrap();

        for key in [
            "data_dir=",
            "cache_dir=",
            "command_line=",
            "launch_on_startup=",
            "launch_on_exit=",
            "boss_key=",
            "translate_key=",
            "show_password=",
            "win32k=",
            "ignore_policies=",
            "suppress_false_upgrade_notification=",
            "double_click_close=",
            "right_click_close=",
            "keep_last_tab=",
            "wheel_tab=",
            "wheel_tab_when_press_rbutton=",
            "open_url_new_tab=",
            "open_bookmark_new_tab=",
            "new_tab_disable=",
            "new_tab_disable_name=",
            "hover_tab=",
            "hover_tab_delay=",
        ] {
            assert!(output.contains(key), "missing {key}");
        }
        assert!(output.contains(r"cache_dir=D:\Portable\Cache"));
        assert!(output.contains("F3=Ctrl+PageDown"));
        assert!(!output.contains("F2=Ctrl+PageUp"));
        assert!(output.contains("; note"));
    }

    #[test]
    fn invalid_keymapping_is_rejected() {
        let config = ChromePlusConfig {
            keymapping: Some("F2 Ctrl+PageUp".to_owned()),
            ..ChromePlusConfig::default()
        };
        assert!(apply_config("[keymapping]\r\n", &config, None, None).is_err());
    }

    #[test]
    fn ini_path_values_with_spaces_are_not_quoted() {
        let output = apply_config(
            "[general]\r\n",
            &ChromePlusConfig::default(),
            Some(Path::new(r"D:\My Browsers\Chrome\Profiles")),
            None,
        )
        .unwrap();

        assert!(output.contains(r"data_dir=D:\My Browsers\Chrome\Profiles"));
        assert!(!output.contains(r#"data_dir="D:\My Browsers\Chrome\Profiles""#));
    }

    #[test]
    fn existing_ini_values_are_loaded_before_editing() {
        let config = merge_config_from_ini(
            "[general]\r\nshow_password=0\r\nboss_key=Ctrl+Alt+B\r\n[tabs]\r\ndouble_click_close=0\r\nhover_tab_delay=750\r\n[keymapping]\r\nF2=Ctrl+PageUp\r\n",
            &ChromePlusConfig::default(),
        );

        assert!(!config.show_password);
        assert_eq!(config.boss_key, "Ctrl+Alt+B");
        assert!(!config.double_click_close);
        assert_eq!(config.hover_tab_delay, 750);
        assert_eq!(config.keymapping.as_deref(), Some("F2=Ctrl+PageUp"));
    }

    #[test]
    fn same_chromeplus_version_preserves_the_user_ini_verbatim() {
        let directory = tempdir().unwrap();
        let runtime = directory.path().join("runtime/x64/App");
        let existing = directory.path().join("existing");
        let staging = directory.path().join("staging");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&existing).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(runtime.join("version.dll"), b"same-version").unwrap();
        fs::write(existing.join("version.dll"), b"same-version").unwrap();
        write_ini(
            &runtime.join("chrome++.ini"),
            "[general]\r\ndata_dir=default\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();
        let custom = "; user comment\r\n[general]\r\nunknown=keep\r\n";
        write_ini(&existing.join("chrome++.ini"), custom, IniEncoding::Utf16Le).unwrap();

        prepare(ChromePlusPrepareRequest {
            runtime_root: &directory.path().join("runtime"),
            staging_program: &staging,
            final_program: Path::new(r"D:\Portable\App"),
            existing_program: Some(&existing),
            backup_dir: &directory.path().join("backups"),
            data_dir: Path::new(r"D:\Portable\Data"),
            cache_dir: Some(Path::new(r"D:\Portable\Cache")),
            config: &ChromePlusConfig::default(),
            mode: ChromePlusMode::Full,
            version: "1.18.2",
            product: TargetId::Chrome,
            setdll_runtime_root: None,
        })
        .unwrap();

        let (text, _) = read_ini(&staging.join("chrome++.ini")).unwrap();
        assert_eq!(text, custom);
    }

    #[test]
    fn changed_chromeplus_version_backs_up_old_ini_and_writes_new_defaults() {
        let directory = tempdir().unwrap();
        let runtime = directory.path().join("runtime/x64/App");
        let existing = directory.path().join("existing");
        let staging = directory.path().join("staging");
        let backups = directory.path().join("backups");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&existing).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(runtime.join("version.dll"), b"new-version").unwrap();
        fs::write(existing.join("version.dll"), b"old-version").unwrap();
        write_ini(
            &runtime.join("chrome++.ini"),
            "[general]\r\ndata_dir=default\r\n[unknown]\r\nkeep=1\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();
        write_ini(
            &existing.join("chrome++.ini"),
            "[general]\r\ndata_dir=user\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();

        prepare(ChromePlusPrepareRequest {
            runtime_root: &directory.path().join("runtime"),
            staging_program: &staging,
            final_program: Path::new(r"D:\Portable\App"),
            existing_program: Some(&existing),
            backup_dir: &backups,
            data_dir: Path::new(r"D:\Portable\Data"),
            cache_dir: Some(Path::new(r"D:\Portable\Cache")),
            config: &ChromePlusConfig::default(),
            mode: ChromePlusMode::Full,
            version: "1.18.3",
            product: TargetId::Chrome,
            setdll_runtime_root: None,
        })
        .unwrap();

        assert!(backups.join("chrome++.ini.1.18.3.bak").is_file());
        let (text, encoding) = read_ini(&staging.join("chrome++.ini")).unwrap();
        assert_eq!(encoding, IniEncoding::Utf16Le);
        assert!(text.contains("[unknown]\r\nkeep=1\r\n"));
        assert!(text.contains(r"data_dir=%app%\..\Data"));
    }

    #[test]
    fn full_config_without_cache_override_writes_browser_default_none() {
        let directory = tempdir().unwrap();
        let runtime = directory.path().join("runtime/x64/App");
        let staging = directory.path().join("staging");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(runtime.join("version.dll"), b"new-version").unwrap();
        write_ini(
            &runtime.join("chrome++.ini"),
            "[general]\r\ndata_dir=default\r\ncache_dir=default\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();

        prepare(ChromePlusPrepareRequest {
            runtime_root: &directory.path().join("runtime"),
            staging_program: &staging,
            final_program: Path::new(r"D:\Portable\App"),
            existing_program: None,
            backup_dir: &directory.path().join("backups"),
            data_dir: Path::new(r"D:\Portable\Profiles"),
            cache_dir: None,
            config: &ChromePlusConfig::default(),
            mode: ChromePlusMode::Full,
            version: "1.18.2",
            product: TargetId::Chrome,
            setdll_runtime_root: None,
        })
        .unwrap();

        let (text, _) = read_ini(&staging.join("chrome++.ini")).unwrap();
        assert!(text.contains("cache_dir=none"));
        assert!(!text.contains("cache_dir=default"));
    }

    #[test]
    fn full_config_writes_relocatable_data_path() {
        let directory = tempdir().unwrap();
        let runtime = directory.path().join("runtime/x64/App");
        let staging = directory.path().join("staging");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(runtime.join("version.dll"), b"new-version").unwrap();
        write_ini(
            &runtime.join("chrome++.ini"),
            "[general]\r\ndata_dir=default\r\ncache_dir=default\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();

        prepare(ChromePlusPrepareRequest {
            runtime_root: &directory.path().join("runtime"),
            staging_program: &staging,
            final_program: Path::new(r"D:\Browsers\Chrome\App"),
            existing_program: None,
            backup_dir: &directory.path().join("backups"),
            data_dir: Path::new(r"D:\Browsers\Chrome\Profiles"),
            cache_dir: None,
            config: &ChromePlusConfig::default(),
            mode: ChromePlusMode::Full,
            version: "1.18.2",
            product: TargetId::Chrome,
            setdll_runtime_root: None,
        })
        .unwrap();

        let (text, _) = read_ini(&staging.join("chrome++.ini")).unwrap();
        assert!(text.contains(r"data_dir=%app%\..\Profiles"));
        assert!(text.contains("cache_dir=none"));
    }

    #[test]
    fn full_config_uses_relative_path_across_browser_branches_on_same_volume() {
        let directory = tempdir().unwrap();
        let runtime = directory.path().join("runtime/x64/App");
        let staging = directory.path().join("staging");
        fs::create_dir_all(&runtime).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(runtime.join("version.dll"), b"new-version").unwrap();
        write_ini(
            &runtime.join("chrome++.ini"),
            "[general]\r\ndata_dir=default\r\ncache_dir=default\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();

        prepare(ChromePlusPrepareRequest {
            runtime_root: &directory.path().join("runtime"),
            staging_program: &staging,
            final_program: Path::new(r"D:\Browsers\Helium\App"),
            existing_program: None,
            backup_dir: &directory.path().join("backups"),
            data_dir: Path::new(r"D:\Shared\Helium Profiles"),
            cache_dir: None,
            config: &ChromePlusConfig::default(),
            mode: ChromePlusMode::Full,
            version: "1.18.2",
            product: TargetId::Helium,
            setdll_runtime_root: None,
        })
        .unwrap();

        let (text, _) = read_ini(&staging.join("chrome++.ini")).unwrap();
        assert!(text.contains(r"data_dir=%app%\..\..\..\Shared\Helium Profiles"));
    }

    #[test]
    fn backup_and_restore_default_config_keep_a_recoverable_copy() {
        let directory = tempdir().unwrap();
        let ini = directory.path().join("chrome++.ini");
        let backups = directory.path().join("backups");
        let original = "; keep\r\n[general]\r\ndata_dir=D:\\Portable\\Data\r\nshow_password=0\r\n[unknown]\r\nkeep=1\r\n";
        write_ini(&ini, original, IniEncoding::Utf16Le).unwrap();

        let manual = backup_ini(&ini, &backups, "manual").unwrap();
        let (manual_text, _) = read_ini(&manual).unwrap();
        assert_eq!(manual_text, original);

        let before_reset = restore_default_config(&ini, &backups).unwrap();
        let (backup_text, _) = read_ini(&before_reset).unwrap();
        assert_eq!(backup_text, original);
        let (restored, encoding) = read_ini(&ini).unwrap();
        assert_eq!(encoding, IniEncoding::Utf16Le);
        assert!(restored.contains("show_password=1"));
        assert!(restored.contains("data_dir=D:\\Portable\\Data"));
        assert!(restored.contains("; keep"));
        assert!(restored.contains("[unknown]\r\nkeep=1"));
    }

    #[test]
    fn backup_names_are_readable_and_collision_numbered() {
        let directory = tempdir().unwrap();
        let ini = directory.path().join("chrome++.ini");
        let backups = directory.path().join("backups");
        write_ini(
            &ini,
            "[general]\r\ndata_dir=default\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();

        let first = backup_ini(&ini, &backups, "path-repair").unwrap();
        let second = backup_ini(&ini, &backups, "path-repair").unwrap();

        assert_eq!(first, backups.join("chrome++.ini.path-repair.bak"));
        assert_eq!(second, backups.join("chrome++.ini.path-repair.2.bak"));
    }

    #[test]
    fn installed_version_reads_version_dll_resource() {
        let directory = tempdir().unwrap();
        let version_dll = directory.path().join("version.dll");
        fs::copy(std::env::current_exe().unwrap(), &version_dll).unwrap();

        let version = installed_version(directory.path()).unwrap();

        assert!(!version.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires access to GitHub Release metadata"]
    async fn resolves_the_latest_chrome_plus_release() {
        let http = HttpClient::new(&NetworkSettings::default()).unwrap();
        let release = resolve_latest(&http).await.unwrap();

        assert!(!release.version.is_empty());
        assert!(
            release
                .artifact
                .urls
                .iter()
                .all(|url| url.contains("github"))
        );
    }
}
