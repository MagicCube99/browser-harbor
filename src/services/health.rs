//! Read-only browser configuration diagnostics and relocation.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

use crate::{
    domain::{runtime::RuntimeKind, settings::TargetConfig, target::TargetId},
    infra::ini::{ini_section_entries, read_ini},
    infra::windows::{
        process::{ProcessError, is_process_running},
        version_info::executable_product_name,
    },
    runtime::{RuntimeError, chromeplus, is_pe_x64, libportable, native, portable_relative_path},
};

use super::version_probe::{case_insensitive_child, locate_executable, probe_local_version};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthState {
    NotInstalled,
    Healthy,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthAspect {
    BrowserRoot,
    Program,
    Version,
    Runtime,
    Data,
    Profile,
    Cache,
    PortableConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthItem {
    pub aspect: HealthAspect,
    pub state: HealthState,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    pub state: HealthState,
    pub items: Vec<HealthItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramIdentity {
    NoExecutable,
    Unknown {
        product_name: String,
    },
    Match {
        product_name: String,
    },
    Mismatch {
        actual: TargetId,
        product_name: String,
    },
}

const PRODUCT_EXECUTABLES: &[&str] = &[
    "chrome.exe",
    "brave.exe",
    "msedge.exe",
    "catsxp.exe",
    "firefox.exe",
    "floorp.exe",
    "librewolf.exe",
    "zen.exe",
    "iceweasel.exe",
    "skykakapo.exe",
    "thorium.exe",
    "turbo.exe",
    "helium.exe",
    "vivaldi.exe",
];

#[derive(Debug, Error)]
pub enum RelocationError {
    #[error("browser root does not exist: {0}")]
    RootNotFound(PathBuf),
    #[error("selected browser executable could not be found below {0}")]
    ExecutableNotFound(PathBuf),
    #[error("selected browser executable is not x64: {0}")]
    InvalidExecutable(PathBuf),
    #[error(
        "selected browser executable belongs to {actual:?}, expected {expected:?}: {product_name}"
    )]
    IdentityMismatch {
        expected: TargetId,
        actual: TargetId,
        product_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromePlusPathRepair {
    pub ini_path: PathBuf,
    pub backup_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortablePathRepair {
    pub ini_path: PathBuf,
    pub backup_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum RepairError {
    #[error("the selected target has no supported portable configuration")]
    UnsupportedTarget,
    #[error(
        "the selected target is missing a Browser Root, Program Directory, or Profile Directory"
    )]
    MissingPath,
    #[error("browser executable could not be found below {0}")]
    ExecutableNotFound(PathBuf),
    #[error("close the browser before repairing portable paths: {0}")]
    BrowserRunning(PathBuf),
    #[error("path cannot be made relative to the browser program: {path}")]
    CannotRelativize { path: PathBuf },
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

pub fn inspect(target: TargetId, config: &TargetConfig) -> HealthReport {
    let Some(root) = config.browser_root.as_deref() else {
        return HealthReport {
            state: HealthState::NotInstalled,
            items: vec![item(
                HealthAspect::BrowserRoot,
                HealthState::NotInstalled,
                "Browser Root is not configured",
            )],
        };
    };
    if !root.is_dir() {
        return HealthReport {
            state: HealthState::NotInstalled,
            items: vec![item(
                HealthAspect::BrowserRoot,
                HealthState::NotInstalled,
                root.display().to_string(),
            )],
        };
    }

    let mut items = vec![item(
        HealthAspect::BrowserRoot,
        HealthState::Healthy,
        root.display().to_string(),
    )];
    let Some(program) = config.browser_directory.as_deref() else {
        items.push(item(
            HealthAspect::Program,
            HealthState::Error,
            "program directory is not configured".to_owned(),
        ));
        return report(items);
    };
    if !program.is_dir() {
        items.push(item(
            HealthAspect::Program,
            HealthState::Error,
            program.display().to_string(),
        ));
        return report(items);
    }

    let executable = match locate_executable(target, program) {
        Ok(executable) => {
            items.push(item(
                HealthAspect::Program,
                HealthState::Healthy,
                executable.display().to_string(),
            ));
            executable
        }
        Err(error) => {
            items.push(item(
                HealthAspect::Program,
                HealthState::Error,
                error.to_string(),
            ));
            return report(items);
        }
    };
    let program = executable.parent().unwrap_or(program);
    if let ProgramIdentity::Mismatch {
        actual,
        product_name,
    } = inspect_program_identity(target, program)
    {
        items.push(item(
            HealthAspect::Program,
            HealthState::Error,
            format!(
                "程序身份为 {}（{product_name}），当前目标是 {}",
                actual.display_name(),
                target.display_name()
            ),
        ));
    }
    match is_pe_x64(&executable) {
        Ok(true) => {}
        Ok(false) => items.push(item(
            HealthAspect::Program,
            HealthState::Error,
            format!("{} is not an x64 executable", executable.display()),
        )),
        Err(error) => items.push(item(
            HealthAspect::Program,
            HealthState::Error,
            format!("{}: {error}", executable.display()),
        )),
    }

    match probe_local_version(target, program) {
        Ok(version) => items.push(item(
            HealthAspect::Version,
            HealthState::Healthy,
            format!("{} ({:?})", version.version, version.source),
        )),
        Err(error) => items.push(item(
            HealthAspect::Version,
            HealthState::Warning,
            error.to_string(),
        )),
    }

    inspect_runtime(target, program, config, &mut items);
    inspect_directory(
        HealthAspect::Data,
        config.data_directory.as_deref(),
        &mut items,
    );
    if target.spec().layout.data != target.spec().layout.profile {
        inspect_directory(
            HealthAspect::Profile,
            config.profile_directory.as_deref(),
            &mut items,
        );
    }
    inspect_directory(
        HealthAspect::Cache,
        config.cache_directory.as_deref(),
        &mut items,
    );
    report(items)
}

pub fn inspect_program_identity(target: TargetId, program: &Path) -> ProgramIdentity {
    if let Ok(executable) = locate_executable(target, program)
        && let Ok(product_name) = executable_product_name(&executable)
        && identify_product(&product_name).is_some_and(|actual| actual == target)
    {
        return ProgramIdentity::Match { product_name };
    }
    let mut matched = None;
    let mut mismatch = None;
    let mut unknown = None;
    for candidate in PRODUCT_EXECUTABLES {
        let Some(executable) = case_insensitive_child(program, candidate) else {
            continue;
        };
        let Ok(product_name) = executable_product_name(&executable) else {
            continue;
        };
        match identify_product(&product_name) {
            Some(actual) if actual == target => {
                matched = Some(product_name);
                break;
            }
            Some(actual) if mismatch.is_none() => mismatch = Some((actual, product_name)),
            Some(_) => {}
            None if unknown.is_none() => unknown = Some(product_name),
            None => {}
        }
    }
    if let Some(product_name) = matched {
        ProgramIdentity::Match { product_name }
    } else if let Some((actual, product_name)) = mismatch {
        ProgramIdentity::Mismatch {
            actual,
            product_name,
        }
    } else if let Some(product_name) = unknown {
        ProgramIdentity::Unknown { product_name }
    } else {
        ProgramIdentity::NoExecutable
    }
}

fn identify_product(product_name: &str) -> Option<TargetId> {
    let name = product_name.to_ascii_lowercase();
    if name.contains("google chrome") {
        Some(TargetId::Chrome)
    } else if name.contains("microsoft edge") || name.contains("microsoft® edge") {
        Some(TargetId::Edge)
    } else if name.contains("helium") {
        Some(TargetId::Helium)
    } else if name.contains("thorium") {
        Some(TargetId::Thorium)
    } else if name.contains("catsxp") {
        Some(TargetId::Catsxp)
    } else if name.contains("brave") {
        Some(TargetId::Brave)
    } else if name.contains("librewolf") {
        Some(TargetId::LibreWolf)
    } else if name.contains("iceweasel") {
        Some(TargetId::Iceweasel)
    } else if name.contains("skykakapo") {
        Some(TargetId::SkyKakapo)
    } else if name.contains("floorp") {
        Some(TargetId::Floorp)
    } else if name.contains("zen browser") {
        Some(TargetId::Zen)
    } else if name.contains("vivaldi") {
        Some(TargetId::Vivaldi)
    } else if name.contains("turbo") {
        Some(TargetId::Turbo)
    } else if name.contains("firefox") {
        Some(TargetId::Firefox)
    } else if name.contains("chromium") {
        Some(TargetId::UngoogledChromium)
    } else {
        None
    }
}

pub fn relocate(
    target: TargetId,
    config: &TargetConfig,
    new_root: &Path,
) -> Result<TargetConfig, RelocationError> {
    if !new_root.is_dir() {
        return Err(RelocationError::RootNotFound(new_root.to_path_buf()));
    }
    let old_root = config.browser_root.as_deref();
    let mut relocated = config.clone();
    relocated.browser_root = Some(new_root.to_path_buf());
    relocated.browser_directory =
        remap_path(old_root, new_root, config.browser_directory.as_deref());
    relocated.data_directory = remap_path(old_root, new_root, config.data_directory.as_deref());
    relocated.profile_directory =
        remap_path(old_root, new_root, config.profile_directory.as_deref());
    relocated.cache_directory = remap_path(old_root, new_root, config.cache_directory.as_deref());

    let program = relocated
        .browser_directory
        .as_deref()
        .ok_or_else(|| RelocationError::ExecutableNotFound(new_root.to_path_buf()))?;
    let executable = locate_executable(target, program)
        .map_err(|_| RelocationError::ExecutableNotFound(program.to_path_buf()))?;
    if !is_pe_x64(&executable).unwrap_or(false) {
        return Err(RelocationError::InvalidExecutable(executable));
    }
    validate_relocation_identity(target, &inspect_program_identity(target, program))?;
    Ok(relocated)
}

fn validate_relocation_identity(
    target: TargetId,
    identity: &ProgramIdentity,
) -> Result<(), RelocationError> {
    match identity {
        ProgramIdentity::Mismatch {
            actual,
            product_name,
        } => Err(RelocationError::IdentityMismatch {
            expected: target,
            actual: *actual,
            product_name: product_name.clone(),
        }),
        _ => Ok(()),
    }
}

pub fn repair_chromeplus_paths(
    target: TargetId,
    config: &TargetConfig,
) -> Result<ChromePlusPathRepair, RepairError> {
    if !target.spec().runtime.uses_chrome_plus() {
        return Err(RepairError::UnsupportedTarget);
    }
    let (Some(root), Some(configured_program), Some(profile)) = (
        config.browser_root.as_deref(),
        config.browser_directory.as_deref(),
        config
            .profile_directory
            .as_deref()
            .or(config.data_directory.as_deref()),
    ) else {
        return Err(RepairError::MissingPath);
    };
    let executable = locate_executable(target, configured_program)
        .map_err(|_| RepairError::ExecutableNotFound(configured_program.to_path_buf()))?;
    let program = executable
        .parent()
        .ok_or_else(|| RepairError::ExecutableNotFound(configured_program.to_path_buf()))?;
    if is_process_running(&executable)? {
        return Err(RepairError::BrowserRunning(executable));
    }
    let data_dir = chromeplus_runtime_path(program, profile)?;
    let cache_dir = config
        .cache_directory
        .as_deref()
        .map(|path| chromeplus_runtime_path(program, path))
        .transpose()?
        .unwrap_or_else(|| "none".to_owned());
    let ini_path = program.join("chrome++.ini");
    let backup_path = chromeplus::backup_ini(&ini_path, &root.join("backups"), "path-repair")?;
    chromeplus::update_portable_paths(&ini_path, &data_dir, &cache_dir)?;
    Ok(ChromePlusPathRepair {
        ini_path,
        backup_path,
    })
}

pub fn repair_portable_paths(
    target: TargetId,
    config: &TargetConfig,
) -> Result<PortablePathRepair, RepairError> {
    if !matches!(
        target,
        TargetId::Firefox
            | TargetId::Floorp
            | TargetId::LibreWolf
            | TargetId::Zen
            | TargetId::Iceweasel
            | TargetId::SkyKakapo
    ) {
        return Err(RepairError::UnsupportedTarget);
    }
    let (Some(root), Some(configured_program), Some(data), Some(profile)) = (
        config.browser_root.as_deref(),
        config.browser_directory.as_deref(),
        config.data_directory.as_deref(),
        config.profile_directory.as_deref(),
    ) else {
        return Err(RepairError::MissingPath);
    };
    let executable = locate_executable(target, configured_program)
        .map_err(|_| RepairError::ExecutableNotFound(configured_program.to_path_buf()))?;
    if is_process_running(&executable)? {
        return Err(RepairError::BrowserRunning(executable));
    }
    let program = executable
        .parent()
        .ok_or_else(|| RepairError::ExecutableNotFound(configured_program.to_path_buf()))?;
    let ini_name = if target == TargetId::SkyKakapo {
        "tmemutil.ini"
    } else {
        "portable.ini"
    };
    let ini_path = program.join(ini_name);
    let backup_path = chromeplus::backup_ini(&ini_path, &root.join("backups"), "path-repair")?;
    match target {
        TargetId::Firefox | TargetId::Floorp | TargetId::LibreWolf | TargetId::Zen => {
            libportable::write_portable_ini(
                program,
                program,
                profile,
                config.cache_directory.as_deref(),
            )?;
        }
        TargetId::Iceweasel | TargetId::SkyKakapo => {
            native::configure_app_launcher(
                program,
                program,
                target,
                data,
                profile,
                config.cache_directory.as_deref(),
            )?;
        }
        _ => return Err(RepairError::UnsupportedTarget),
    }
    Ok(PortablePathRepair {
        ini_path,
        backup_path,
    })
}

fn chromeplus_runtime_path(program: &Path, target: &Path) -> Result<String, RepairError> {
    portable_relative_path(program, target)
        .map(|relative| format!(r"%app%\{relative}"))
        .ok_or_else(|| RepairError::CannotRelativize {
            path: target.to_path_buf(),
        })
}

fn inspect_runtime(
    target: TargetId,
    program: &Path,
    config: &TargetConfig,
    items: &mut Vec<HealthItem>,
) {
    let required = match target.spec().runtime {
        RuntimeKind::ChromePlusFull | RuntimeKind::ChromePlusDataOnly => {
            &["version.dll", "chrome++.ini"][..]
        }
        RuntimeKind::Libportable => &["portable64.dll", "portable.ini", "upcheck.exe"][..],
        RuntimeKind::IceweaselNative => &["portable64.dll", "portable.ini"][..],
        RuntimeKind::SkyKakapoNative => &["tmemutil.dll", "tmemutil.ini"][..],
        RuntimeKind::TurboLauncher => &[".portable"][..],
        RuntimeKind::VivaldiStandalone => &["stp.viv"][..],
        _ => &[][..],
    };
    let missing = required
        .iter()
        .filter(|name| !program.join(name).is_file())
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        items.push(item(
            HealthAspect::Runtime,
            HealthState::Healthy,
            match target.spec().runtime {
                RuntimeKind::ChromePlusFull | RuntimeKind::ChromePlusDataOnly => {
                    "Chrome++ runtime files found"
                }
                RuntimeKind::Libportable => "Libportable runtime files found",
                RuntimeKind::IceweaselNative => "Iceweasel portable runtime files found",
                RuntimeKind::SkyKakapoNative => "SkyKakapo portable runtime files found",
                RuntimeKind::TurboLauncher => "Turbo portable marker found",
                RuntimeKind::VivaldiStandalone => "Vivaldi standalone portable marker found",
                _ => "bundled runtime does not require additional files",
            },
        ));
    } else {
        items.push(item(
            HealthAspect::Runtime,
            HealthState::Error,
            format!("missing: {}", missing.join(", ")),
        ));
    }
    if target == TargetId::Edge {
        let executable = program.join("msedge.exe");
        match crate::runtime::pe_imports_dll(&executable, "version.dll") {
            Ok(true) => items.push(item(
                HealthAspect::Runtime,
                HealthState::Healthy,
                "msedge.exe imports version.dll".to_owned(),
            )),
            Ok(false) => items.push(item(
                HealthAspect::Runtime,
                HealthState::Error,
                "msedge.exe does not import version.dll".to_owned(),
            )),
            Err(error) => items.push(item(
                HealthAspect::Runtime,
                HealthState::Error,
                error.to_string(),
            )),
        }
    }
    match target.spec().runtime {
        RuntimeKind::ChromePlusFull | RuntimeKind::ChromePlusDataOnly => {
            inspect_chrome_plus_config(program, config, items);
        }
        RuntimeKind::Libportable | RuntimeKind::IceweaselNative => {
            inspect_libportable_config(program, config, items);
        }
        RuntimeKind::SkyKakapoNative => {
            inspect_skykakapo_config(program, config, items);
        }
        _ => {}
    }
}

fn inspect_chrome_plus_config(program: &Path, config: &TargetConfig, items: &mut Vec<HealthItem>) {
    let path = program.join("chrome++.ini");
    let (text, _) = match read_ini(&path) {
        Ok(ini) => ini,
        Err(error) => {
            items.push(item(
                HealthAspect::PortableConfig,
                HealthState::Error,
                error.to_string(),
            ));
            return;
        }
    };
    let entries = ini_section_entries(&text, "general");
    let Some(data_value) = ini_value(&entries, "data_dir") else {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Error,
            "chrome++.ini has no data_dir".to_owned(),
        ));
        return;
    };
    if data_value.eq_ignore_ascii_case("none") || data_value.trim().is_empty() {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Error,
            "chrome++.ini data_dir disables portable data".to_owned(),
        ));
    } else {
        inspect_configured_path(
            "chrome++.ini data_dir",
            &data_value,
            program,
            config
                .data_directory
                .as_deref()
                .or(config.profile_directory.as_deref()),
            items,
        );
    }

    let Some(cache_value) = ini_value(&entries, "cache_dir") else {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Warning,
            "chrome++.ini has no cache_dir".to_owned(),
        ));
        return;
    };
    if cache_value.eq_ignore_ascii_case("none") {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Healthy,
            "chrome++.ini cache_dir=none".to_owned(),
        ));
    } else if cache_value.trim().is_empty() {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Warning,
            "chrome++.ini cache_dir is empty".to_owned(),
        ));
    } else {
        inspect_configured_path(
            "chrome++.ini cache_dir",
            &cache_value,
            program,
            config.cache_directory.as_deref(),
            items,
        );
    }
}

fn inspect_libportable_config(program: &Path, config: &TargetConfig, items: &mut Vec<HealthItem>) {
    let path = program.join("portable.ini");
    let (text, _) = match read_ini(&path) {
        Ok(ini) => ini,
        Err(error) => {
            items.push(item(
                HealthAspect::PortableConfig,
                HealthState::Error,
                error.to_string(),
            ));
            return;
        }
    };
    let general = ini_section_entries(&text, "general");
    match ini_value(&general, "PortableDataPath") {
        Some(data_value) => inspect_configured_path(
            "portable.ini PortableDataPath",
            &data_value,
            program,
            config.data_directory.as_deref(),
            items,
        ),
        None => items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Error,
            "portable.ini has no PortableDataPath".to_owned(),
        )),
    }
    let env = ini_section_entries(&text, "env");
    if let Some(cache_value) = ini_value(&env, "TmpDataPath") {
        if cache_value.trim().is_empty() {
            items.push(item(
                HealthAspect::PortableConfig,
                HealthState::Healthy,
                "portable.ini TmpDataPath follows PortableDataPath".to_owned(),
            ));
        } else {
            inspect_configured_path(
                "portable.ini TmpDataPath",
                &cache_value,
                program,
                config.cache_directory.as_deref(),
                items,
            );
        }
    }
}

fn inspect_skykakapo_config(program: &Path, config: &TargetConfig, items: &mut Vec<HealthItem>) {
    let path = program.join("tmemutil.ini");
    let (text, _) = match read_ini(&path) {
        Ok(ini) => ini,
        Err(error) => {
            items.push(item(
                HealthAspect::PortableConfig,
                HealthState::Error,
                error.to_string(),
            ));
            return;
        }
    };
    let general = ini_section_entries(&text, "general");
    let portable = ini_value(&general, "Portable").unwrap_or_default();
    if portable.trim() != "1" {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Error,
            "tmemutil.ini Portable is not enabled".to_owned(),
        ));
    }
    match ini_value(&general, "PortableDataPath") {
        Some(data_value) => inspect_configured_path(
            "tmemutil.ini PortableDataPath",
            &data_value,
            program,
            config.data_directory.as_deref(),
            items,
        ),
        None => items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Error,
            "tmemutil.ini has no PortableDataPath".to_owned(),
        )),
    }
}

fn inspect_configured_path(
    label: &str,
    value: &str,
    program: &Path,
    expected: Option<&Path>,
    items: &mut Vec<HealthItem>,
) {
    let Some(resolved) = resolve_runtime_path(value, program) else {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Error,
            format!("{label} is empty"),
        ));
        return;
    };
    if let Some(expected) = expected
        && !same_path(&resolved, expected)
    {
        items.push(item(
            HealthAspect::PortableConfig,
            HealthState::Error,
            format!("{label}={value}; expected {}", expected.display()),
        ));
        return;
    }
    let state = if is_relocatable_value(value) {
        HealthState::Healthy
    } else {
        HealthState::Warning
    };
    items.push(item(
        HealthAspect::PortableConfig,
        state,
        format!("{label}={value}"),
    ));
}

fn inspect_directory(aspect: HealthAspect, path: Option<&Path>, items: &mut Vec<HealthItem>) {
    let Some(path) = path else {
        return;
    };
    items.push(item(
        aspect,
        if path.is_dir() {
            HealthState::Healthy
        } else {
            HealthState::Warning
        },
        path.display().to_string(),
    ));
}

fn ini_value(entries: &[(String, String)], key: &str) -> Option<String> {
    entries
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.trim().trim_matches('"').to_owned())
}

fn resolve_runtime_path(value: &str, program: &Path) -> Option<PathBuf> {
    let value = value.trim().trim_matches('"');
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        return None;
    }
    let program_text = program.display().to_string();
    let expanded = value
        .replace("%app%", &program_text)
        .replace("%APP%", &program_text);
    let path = PathBuf::from(expand_windows_environment(&expanded));
    Some(if path.is_absolute() {
        normalize_path(&path)
    } else {
        normalize_path(&program.join(path))
    })
}

fn expand_windows_environment(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('%') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &after_start[..end];
        if let Ok(value) = std::env::var(name) {
            output.push_str(&value);
        } else {
            output.push('%');
            output.push_str(name);
            output.push('%');
        }
        rest = &after_start[end + 1..];
    }
    output.push_str(rest);
    output
}

fn is_relocatable_value(value: &str) -> bool {
    let value = value.trim().trim_matches('"');
    value.to_ascii_lowercase().contains("%app%") || !Path::new(value).is_absolute()
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left)
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(
            normalize_path(right)
                .to_string_lossy()
                .trim_end_matches(['\\', '/']),
        )
}

fn remap_path(old_root: Option<&Path>, new_root: &Path, path: Option<&Path>) -> Option<PathBuf> {
    path.map(|path| {
        old_root
            .and_then(|old_root| strip_prefix_windows(old_root, path))
            .map_or_else(|| path.to_path_buf(), |relative| new_root.join(relative))
    })
}

fn strip_prefix_windows(prefix: &Path, path: &Path) -> Option<PathBuf> {
    let prefix = prefix.components().collect::<Vec<_>>();
    let path = path.components().collect::<Vec<_>>();
    if prefix.len() > path.len()
        || !prefix.iter().zip(&path).all(|(left, right)| {
            left.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
        })
    {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in &path[prefix.len()..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn item(aspect: HealthAspect, state: HealthState, detail: impl Into<String>) -> HealthItem {
    HealthItem {
        aspect,
        state,
        detail: detail.into(),
    }
}

fn report(items: Vec<HealthItem>) -> HealthReport {
    let state = items
        .iter()
        .map(|item| item.state)
        .max()
        .unwrap_or(HealthState::Healthy);
    HealthReport { state, items }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::tempdir;

    use super::{
        HealthState, ProgramIdentity, identify_product, inspect, relocate, repair_chromeplus_paths,
        repair_portable_paths, validate_relocation_identity,
    };
    use crate::domain::{settings::TargetConfig, target::TargetId};

    fn chrome_root(directory: &tempfile::TempDir) -> std::path::PathBuf {
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
        root
    }

    #[test]
    fn chromium_forks_are_distinguished_by_product_name() {
        assert_eq!(identify_product("Google Chrome"), Some(TargetId::Chrome));
        assert_eq!(identify_product("Helium"), Some(TargetId::Helium));
        assert_eq!(identify_product("Thorium"), Some(TargetId::Thorium));
        assert_eq!(identify_product("Microsoft® Edge"), Some(TargetId::Edge));
        assert_eq!(identify_product("Custom Browser"), None);
    }

    #[test]
    fn iceweasel_portable_path_repair_creates_backup_and_relativizes_paths() {
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
        fs::write(
            program.join("portable.ini"),
            format!(
                "[General]\r\nPortable=1\r\nPortableDataPath={}\r\n[Env]\r\nTmpDataPath=\r\n",
                root.join("Profiles").display()
            ),
        )
        .unwrap();
        let mut config = TargetConfig::for_target(TargetId::Iceweasel);
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Iceweasel);

        let repair = repair_portable_paths(TargetId::Iceweasel, &config).unwrap();

        assert!(repair.backup_path.is_file());
        let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\Profiles"));
        assert!(ini.lines().any(|line| line.trim() == "TmpDataPath="));
    }

    #[test]
    fn portable_path_repair_supports_floorp_and_librewolf() {
        for (target, root_name, program_name, profile_path) in [
            (TargetId::Floorp, "Floorp", "app", r"data\profile\default"),
            (
                TargetId::LibreWolf,
                "LibreWolf",
                "LibreWolf",
                r"Profiles\Default",
            ),
        ] {
            let directory = tempdir().unwrap();
            let root = directory.path().join(root_name);
            let program = root.join(program_name);
            let profile = root.join(profile_path);
            fs::create_dir_all(&program).unwrap();
            fs::create_dir_all(&profile).unwrap();
            fs::copy(
                std::env::current_exe().unwrap(),
                program.join(target.spec().executable_candidates[0]),
            )
            .unwrap();
            fs::write(
                program.join("portable.ini"),
                format!(
                    "[General]\r\nPortable=1\r\nPortableDataPath={}\r\n[Env]\r\nTmpDataPath=\r\n",
                    profile.display()
                ),
            )
            .unwrap();
            let mut config = TargetConfig::for_target(target);
            config.browser_root = Some(root);
            config.derive_paths_from_root(target);

            let repair = repair_portable_paths(target, &config)
                .unwrap_or_else(|error| panic!("{} repair failed: {error}", target.display_name()));

            assert!(repair.backup_path.is_file());
            let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
            let expected = if target == TargetId::Floorp {
                r"PortableDataPath=..\data\profile\default"
            } else {
                r"PortableDataPath=..\Profiles\Default"
            };
            assert!(ini.contains(expected), "{}: {ini}", target.display_name());
        }
    }

    #[test]
    fn healthy_chrome_report_includes_relocatable_runtime_config() {
        let directory = tempdir().unwrap();
        let root = chrome_root(&directory);
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Chrome);

        let report = inspect(TargetId::Chrome, &config);

        assert_eq!(report.state, HealthState::Healthy);
        assert!(
            report
                .items
                .iter()
                .any(|item| item.detail.contains("%app%"))
        );
    }

    #[test]
    fn stale_chromeplus_data_path_is_an_error() {
        let directory = tempdir().unwrap();
        let root = chrome_root(&directory);
        fs::write(
            root.join("App/chrome++.ini"),
            "[general]\r\ndata_dir=C:\\Old\\Profiles\r\ncache_dir=none\r\n",
        )
        .unwrap();
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(root);
        config.derive_paths_from_root(TargetId::Chrome);

        let report = inspect(TargetId::Chrome, &config);

        assert_eq!(report.state, HealthState::Error);
    }

    #[test]
    fn health_resolves_windows_environment_variables_in_runtime_paths() {
        let directory = tempdir().unwrap();
        let root = chrome_root(&directory);
        let profiles = root.join("Profiles");
        let temp_prefix = std::env::temp_dir();
        let relative = profiles.strip_prefix(&temp_prefix).unwrap();
        fs::write(
            root.join("App/chrome++.ini"),
            format!(
                "[general]\r\ndata_dir=%TEMP%\\{}\r\ncache_dir=none\r\n",
                relative.display()
            ),
        )
        .unwrap();
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(root);
        config.derive_paths_from_root(TargetId::Chrome);

        let report = inspect(TargetId::Chrome, &config);

        assert_eq!(report.state, HealthState::Healthy);
    }

    #[test]
    fn chromeplus_path_repair_backs_up_and_writes_relative_paths() {
        let directory = tempdir().unwrap();
        let root = chrome_root(&directory);
        fs::write(
            root.join("App/chrome++.ini"),
            format!(
                "; keep\r\n[general]\r\ndata_dir={}\r\ncache_dir=none\r\nshow_password=0\r\n[custom]\r\nkeep=1\r\n",
                root.join("Profiles").display(),
            ),
        )
        .unwrap();
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Chrome);

        let repair = repair_chromeplus_paths(TargetId::Chrome, &config).unwrap();

        assert!(repair.backup_path.is_file());
        let text = fs::read_to_string(root.join("App/chrome++.ini")).unwrap();
        assert!(text.contains(r"data_dir=%app%\..\Profiles"));
        assert!(text.contains("cache_dir=none"));
        assert!(text.contains("; keep"));
        assert!(text.contains("show_password=0"));
        assert!(text.contains("[custom]\r\nkeep=1"));
    }

    #[test]
    fn chromeplus_path_repair_refuses_cross_volume_paths() {
        let directory = tempdir().unwrap();
        let root = chrome_root(&directory);
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Chrome);
        config.profile_directory = Some(r"D:\Shared\Profiles".into());

        let error = repair_chromeplus_paths(TargetId::Chrome, &config).unwrap_err();

        assert!(matches!(error, super::RepairError::CannotRelativize { .. }));
        assert!(!root.join("backups").exists());
    }

    #[test]
    fn missing_libportable_data_path_is_an_error() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("Firefox");
        let program = root.join("App");
        fs::create_dir_all(&program).unwrap();
        fs::create_dir_all(root.join("Profiles")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            program.join("firefox.exe"),
        )
        .unwrap();
        fs::write(
            program.join("application.ini"),
            "[App]\r\nName=Firefox\r\nVersion=158.0.1\r\n",
        )
        .unwrap();
        for file in ["portable64.dll", "upcheck.exe"] {
            fs::write(program.join(file), b"fixture").unwrap();
        }
        fs::write(
            program.join("portable.ini"),
            "[General]\r\nPortable=1\r\n[Env]\r\nTmpDataPath=\r\n",
        )
        .unwrap();
        let mut config = TargetConfig::for_target(TargetId::Firefox);
        config.browser_root = Some(root);
        config.derive_paths_from_root(TargetId::Firefox);

        let report = inspect(TargetId::Firefox, &config);

        assert_eq!(report.state, HealthState::Error);
    }

    #[test]
    fn relocation_remaps_paths_inside_old_root_and_keeps_external_paths() {
        let directory = tempdir().unwrap();
        let old_root = directory.path().join("Old");
        let new_root = directory.path().join("New");
        fs::create_dir_all(new_root.join("App")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            new_root.join("App/chrome.exe"),
        )
        .unwrap();
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(old_root.clone());
        config.browser_directory = Some(old_root.join("App"));
        config.data_directory = Some(old_root.join("Profiles"));
        config.profile_directory = Some(r"D:\External\Profiles".into());
        config.cache_directory = None;

        let relocated = relocate(TargetId::Chrome, &config, &new_root).unwrap();

        assert_eq!(relocated.browser_root, Some(new_root.clone()));
        assert_eq!(relocated.browser_directory, Some(new_root.join("App")));
        assert_eq!(relocated.data_directory, Some(new_root.join("Profiles")));
        assert_eq!(
            relocated.profile_directory,
            Some(r"D:\External\Profiles".into())
        );
    }

    #[test]
    fn relocation_rejects_a_directory_without_the_selected_browser() {
        let directory = tempdir().unwrap();
        let old_root = directory.path().join("Old");
        let new_root = directory.path().join("New");
        fs::create_dir_all(&new_root).unwrap();
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(old_root);
        config.derive_paths_from_root(TargetId::Chrome);

        assert!(relocate(TargetId::Chrome, &config, &new_root).is_err());
    }

    #[test]
    fn relocation_rejects_a_known_browser_identity_mismatch() {
        let identity = ProgramIdentity::Mismatch {
            actual: TargetId::Helium,
            product_name: "Helium".to_owned(),
        };

        assert!(validate_relocation_identity(TargetId::Chrome, &identity).is_err());
    }

    #[test]
    fn relocation_remaps_windows_paths_with_different_case() {
        let directory = tempdir().unwrap();
        let old_root = directory.path().join("Old");
        let new_root = directory.path().join("New");
        fs::create_dir_all(new_root.join("App")).unwrap();
        fs::copy(
            std::env::current_exe().unwrap(),
            new_root.join("App/chrome.exe"),
        )
        .unwrap();
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(old_root.clone());
        config.browser_directory = Some(old_root.join("App"));
        config.data_directory =
            Some(PathBuf::from(old_root.to_string_lossy().to_uppercase()).join("Profiles"));
        config.profile_directory = config.data_directory.clone();
        config.cache_directory = None;

        let relocated = relocate(TargetId::Chrome, &config, &new_root).unwrap();

        assert_eq!(relocated.data_directory, Some(new_root.join("Profiles")));
    }

    #[test]
    fn inspect_chromeplus_uses_data_directory_when_profile_differs() {
        let directory = tempdir().unwrap();
        let root = chrome_root(&directory);
        let mut config = TargetConfig::for_target(TargetId::Chrome);
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Chrome);
        config.profile_directory = Some(root.join("LegacyProfile"));

        let report = inspect(TargetId::Chrome, &config);

        assert_eq!(report.state, HealthState::Healthy);
    }
}
