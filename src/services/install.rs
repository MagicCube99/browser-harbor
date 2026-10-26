//! Atomic browser installation, preservation and rollback.

use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use thiserror::Error;
use walkdir::WalkDir;

use super::{
    ServiceError,
    archive::{ArchiveError, ArchiveService},
    download::verify_artifact,
};
use crate::{
    domain::{
        release::{ProgramPlacement, ReleaseDescriptor},
        runtime::ChromePlusConfig,
        target::{ChromePlusMode, TargetId},
    },
    infra::windows::process::{ProcessError, is_process_running},
    runtime::{RuntimeError, chromeplus, libportable, native},
};

#[derive(Debug, Clone)]
pub struct RuntimeArchive {
    pub archive_path: PathBuf,
    pub version: String,
    pub artifact: crate::domain::release::Artifact,
    pub setdll_archive: Option<PathBuf>,
    pub setdll_artifact: Option<crate::domain::release::Artifact>,
}

#[derive(Debug, Clone)]
pub struct InstallRequest {
    pub release: ReleaseDescriptor,
    pub browser_artifact: PathBuf,
    pub browser_root: PathBuf,
    pub browser_directory: Option<PathBuf>,
    pub data_directory: Option<PathBuf>,
    pub profile_directory: Option<PathBuf>,
    pub cache_directory: Option<PathBuf>,
    pub chrome_plus_config: ChromePlusConfig,
    pub chrome_plus_runtime: Option<RuntimeArchive>,
    pub libportable_runtime: Option<RuntimeArchive>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReceipt {
    pub version: String,
    pub browser_directory: PathBuf,
    pub data_directory: PathBuf,
    pub profile_directory: PathBuf,
    pub cache_directory: Option<PathBuf>,
    pub chrome_plus_version: Option<String>,
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("Browser Root must be an absolute path")]
    InvalidBrowserRoot,
    #[error("program directory must remain within Browser Root")]
    ProgramOutsideRoot,
    #[error("data, profile and cache directories must not be inside the program directory")]
    DataInsideProgram,
    #[error("browser executable is already running: {0}")]
    BrowserRunning(PathBuf),
    #[error("browser executable could not be located")]
    ExecutableNotFound,
    #[error("declared inner archive was not found: {0}")]
    InnerArchiveMissing(String),
    #[error("browser executable is not a 64-bit Windows PE file")]
    InvalidExecutable,
    #[error("existing program directory does not contain the expected executable")]
    ExistingTargetMismatch,
    #[error("required {0} runtime is missing")]
    RuntimeMissing(&'static str),
    #[error("installation rollback failed")]
    Rollback(#[source] std::io::Error),
    #[error("artifact verification failed")]
    Artifact(#[source] ServiceError),
    #[error("installable artifact has no integrity value")]
    ArtifactIntegrityMissing,
    #[error("archive extraction failed")]
    Archive(#[from] ArchiveError),
    #[error("runtime preparation failed")]
    Runtime(#[from] RuntimeError),
    #[error("process protection check failed")]
    Process(#[from] ProcessError),
    #[error("file operation failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub struct InstallService;

fn resolve_cache_directory(
    target: TargetId,
    browser_root: &Path,
    requested: Option<PathBuf>,
) -> Result<Option<PathBuf>, InstallError> {
    if !target.supports_custom_cache() {
        return Ok(None);
    }
    let Some(path) = requested else {
        return if target.cache_follows_profile() {
            Ok(None)
        } else {
            Ok(Some(browser_root.join(target.spec().layout.cache)))
        };
    };
    absolute_path(browser_root, path).map(Some)
}

impl InstallService {
    pub fn install(request: InstallRequest) -> Result<InstallReceipt, InstallError> {
        if request.release.artifact.integrity.is_none() {
            return Err(InstallError::ArtifactIntegrityMissing);
        }
        verify_artifact(&request.browser_artifact, &request.release.artifact)
            .map_err(InstallError::Artifact)?;
        if !request.browser_root.is_absolute() {
            return Err(InstallError::InvalidBrowserRoot);
        }
        fs::create_dir_all(&request.browser_root)
            .map_err(|source| io_error(&request.browser_root, source))?;
        let browser_root = normalize_path(&request.browser_root);
        let layout = request.release.target.spec().layout;
        let data_directory = absolute_path(
            &browser_root,
            request
                .data_directory
                .clone()
                .unwrap_or_else(|| browser_root.join(layout.data)),
        )?;
        let profile_directory = absolute_path(
            &browser_root,
            request
                .profile_directory
                .clone()
                .unwrap_or_else(|| browser_root.join(layout.profile)),
        )?;
        let cache_directory = resolve_cache_directory(
            request.release.target,
            &browser_root,
            request.cache_directory.clone(),
        )?;
        for directory in [
            Some(data_directory.as_path()),
            Some(profile_directory.as_path()),
            cache_directory.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            fs::create_dir_all(directory).map_err(|source| io_error(directory, source))?;
        }

        let work = WorkDirectory::create(&browser_root, request.release.target)?;
        let extracted = work.path.join("extracted");
        ArchiveService::extract(
            &request.browser_artifact,
            request.release.artifact.format,
            &extracted,
        )?;
        let payload = if let Some(inner_archive) = &request.release.inner_archive {
            let inner_path = find_named_file(&extracted, inner_archive)
                .ok_or_else(|| InstallError::InnerArchiveMissing(inner_archive.clone()))?;
            let inner_output = work.path.join("inner");
            ArchiveService::extract(
                &inner_path,
                crate::domain::release::ArchiveFormat::SevenZip,
                &inner_output,
            )?;
            inner_output
        } else {
            extracted
        };
        let located = find_executable(&payload, request.release.target)?;
        if matches!(request.release.placement, ProgramPlacement::NativePackage) {
            return install_native_package(
                &request,
                &work,
                &payload,
                &located,
                &browser_root,
                &data_directory,
                &profile_directory,
                cache_directory.as_deref(),
            );
        }
        let executable_name = located
            .file_name()
            .ok_or(InstallError::ExecutableNotFound)?
            .to_os_string();
        let source_program = located.parent().ok_or(InstallError::ExecutableNotFound)?;
        let staging_program = work.path.join("program");
        copy_tree(source_program, &staging_program)?;
        copy_package_extras(
            &payload,
            source_program,
            &staging_program,
            request.release.target,
        )?;

        let (commit_root, final_program) = placement_paths(
            &browser_root,
            request.browser_directory.as_deref(),
            &request.release,
        )?;
        ensure_not_inside(&data_directory, &commit_root)?;
        ensure_not_inside(&profile_directory, &commit_root)?;
        if let Some(cache_directory) = &cache_directory {
            ensure_not_inside(cache_directory, &commit_root)?;
        }
        let staged_program = if matches!(request.release.placement, ProgramPlacement::EdgeVersioned)
        {
            let staged = work.path.join("edge-tree").join(&request.release.version);
            copy_tree(&staging_program, &staged)?;
            staged
        } else {
            staging_program
        };

        let previous_program = existing_program(&commit_root, request.release.target);
        if let Some(previous_program) = &previous_program
            && let Some(executable) = find_executable(previous_program, request.release.target).ok()
            && is_process_running(&executable)?
        {
            return Err(InstallError::BrowserRunning(executable));
        }

        if !matches!(request.release.placement, ProgramPlacement::EdgeVersioned) {
            native::preserve_files(
                previous_program.as_deref(),
                &staged_program,
                request.release.target.spec().preserved_program_files,
            )?;
        }
        native::configure_app_launcher(
            &staged_program,
            &final_program,
            request.release.target,
            &data_directory,
            &profile_directory,
            cache_directory.as_deref(),
        )?;

        let chrome_plus_version = prepare_runtime(
            &request,
            &work.path,
            &staged_program,
            &final_program,
            previous_program.as_deref(),
            &profile_directory,
            cache_directory.as_deref(),
        )?;
        if !staged_program.join(&executable_name).is_file()
            || !is_pe_x64(&staged_program.join(&executable_name))?
        {
            return Err(InstallError::InvalidExecutable);
        }

        let backup = work.path.join("backup");
        let had_previous = commit_root.exists();
        if had_previous {
            fs::rename(&commit_root, &backup).map_err(|source| io_error(&commit_root, source))?;
        }
        if let Some(parent) = commit_root.parent()
            && let Err(source) = fs::create_dir_all(parent)
        {
            if had_previous {
                let _ = fs::rename(&backup, &commit_root);
            }
            return Err(io_error(parent, source));
        }
        if let Err(source) = fs::rename(work.staged_root(request.release.placement), &commit_root) {
            if had_previous {
                fs::rename(&backup, &commit_root).map_err(InstallError::Rollback)?;
            }
            return Err(io_error(&commit_root, source));
        }

        let executable = final_program.join(&executable_name);
        if let Err(error) = verify_installed(&executable, &final_program, request.release.target) {
            let _ = fs::remove_dir_all(&commit_root);
            if had_previous {
                fs::rename(&backup, &commit_root).map_err(InstallError::Rollback)?;
            }
            return Err(error);
        }

        Ok(InstallReceipt {
            version: request.release.version,
            browser_directory: stored_browser_directory(
                request.release.placement,
                &commit_root,
                &final_program,
            ),
            data_directory,
            profile_directory,
            cache_directory,
            chrome_plus_version,
        })
    }
}

#[allow(clippy::too_many_arguments)] // The transaction passes each already-validated directory explicitly.
fn install_native_package(
    request: &InstallRequest,
    work: &WorkDirectory,
    payload: &Path,
    executable: &Path,
    browser_root: &Path,
    data_directory: &Path,
    profile_directory: &Path,
    cache_directory: Option<&Path>,
) -> Result<InstallReceipt, InstallError> {
    let layout = request.release.target.spec().layout;
    let package_source = strip_single_wrapper(payload);
    let relative_executable = executable
        .strip_prefix(&package_source)
        .map_err(|_| InstallError::ExecutableNotFound)?;
    let staging_package = work.path.join("native-package");
    copy_tree(&package_source, &staging_package)?;
    let staged_executable = staging_package.join(relative_executable);
    if !staged_executable.is_file() || !is_pe_x64(&staged_executable)? {
        return Err(InstallError::InvalidExecutable);
    }

    let previous_program = existing_program(browser_root, request.release.target);
    if let Some(previous) = previous_program.as_deref()
        && let Some(previous_executable) = find_executable(previous, request.release.target).ok()
        && is_process_running(&previous_executable)?
    {
        return Err(InstallError::BrowserRunning(previous_executable));
    }
    let destination = if let Some(package_prefix) = layout.package_prefix {
        let configured_program = absolute_path(
            browser_root,
            request
                .browser_directory
                .clone()
                .unwrap_or_else(|| browser_root.join(layout.program.unwrap_or(package_prefix))),
        )?;
        let program_inside_package = layout
            .program
            .and_then(|program| Path::new(program).strip_prefix(package_prefix).ok())
            .unwrap_or_else(|| Path::new(""));
        let mut package_root = configured_program;
        for _ in program_inside_package.components() {
            if !package_root.pop() {
                return Err(InstallError::ProgramOutsideRoot);
            }
        }
        package_root
    } else {
        browser_root.to_path_buf()
    };
    if !is_within(browser_root, &destination) {
        return Err(InstallError::ProgramOutsideRoot);
    }
    if request.release.target == TargetId::Turbo {
        preserve_turbo_legacy_data(browser_root, &browser_root.join("App"))?;
    }
    let legacy_turbo_app = if request.release.target == TargetId::Turbo {
        let legacy = browser_root.join("App");
        if legacy.is_dir() {
            let backup = work.path.join("turbo-legacy-app");
            fs::rename(&legacy, &backup).map_err(|error| io_error(&legacy, error))?;
            Some((legacy, backup))
        } else {
            None
        }
    } else {
        None
    };
    if layout.package_prefix.is_some() {
        for directory in [
            Some(data_directory),
            Some(profile_directory),
            cache_directory,
        ]
        .into_iter()
        .flatten()
        {
            if is_within(&destination, directory)
                && staging_package
                    .join(directory.strip_prefix(&destination).unwrap_or(directory))
                    .exists()
            {
                return Err(InstallError::DataInsideProgram);
            }
        }
    }

    let staged_program = staged_executable
        .parent()
        .ok_or(InstallError::ExecutableNotFound)?;
    native::preserve_files(
        previous_program.as_deref(),
        staged_program,
        request.release.target.spec().preserved_program_files,
    )?;
    let backup = work.path.join("native-backup");
    native::configure_native_package(
        &staging_package,
        &destination,
        request.release.target,
        data_directory,
        cache_directory,
    )?;
    let preserved_root_files = if layout.package_prefix.is_some() {
        request.release.target.spec().preserved_program_files
    } else {
        &[]
    };
    let final_program = destination.join(
        relative_executable
            .parent()
            .ok_or(InstallError::ExecutableNotFound)?,
    );
    native::configure_app_launcher(
        staged_program,
        &final_program,
        request.release.target,
        data_directory,
        profile_directory,
        cache_directory,
    )?;
    let chrome_plus_version = prepare_runtime(
        request,
        &work.path,
        staged_program,
        &final_program,
        previous_program.as_deref(),
        profile_directory,
        cache_directory,
    )?;
    verify_installed(&staged_executable, staged_program, request.release.target)?;
    if let Err(error) = commit_native_package(
        &staging_package,
        &destination,
        &backup,
        request.release.target,
        preserved_root_files,
        data_directory,
        cache_directory,
    ) {
        if let Some((legacy, backup)) = legacy_turbo_app {
            let _ = fs::rename(backup, legacy);
        }
        return Err(error);
    }
    let installed_executable = destination.join(relative_executable);
    if !installed_executable.is_file() || !is_pe_x64(&installed_executable)? {
        return Err(InstallError::InvalidExecutable);
    }
    verify_installed(
        &installed_executable,
        &final_program,
        request.release.target,
    )?;
    Ok(InstallReceipt {
        version: request.release.version.clone(),
        browser_directory: final_program,
        data_directory: data_directory.to_path_buf(),
        profile_directory: profile_directory.to_path_buf(),
        cache_directory: cache_directory.map(Path::to_path_buf),
        chrome_plus_version,
    })
}

fn strip_single_wrapper(root: &Path) -> PathBuf {
    let entries = fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    if let [entry] = entries.as_slice()
        && entry.file_type().is_ok_and(|kind| kind.is_dir())
    {
        entry.path()
    } else {
        root.to_path_buf()
    }
}

fn commit_native_package(
    staging: &Path,
    destination: &Path,
    backup: &Path,
    target: TargetId,
    preserved_root_files: &[&str],
    data_directory: &Path,
    cache_directory: Option<&Path>,
) -> Result<(), InstallError> {
    fs::create_dir_all(destination).map_err(|source| io_error(destination, source))?;
    fs::create_dir_all(backup).map_err(|source| io_error(backup, source))?;
    let staged_entries = fs::read_dir(staging)
        .map_err(|source| io_error(staging, source))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    let staged_names = staged_entries
        .iter()
        .filter_map(|path| path.file_name().map(|name| name.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    let mut replace_names = staged_entries
        .iter()
        .filter_map(|path| path.file_name().map(|name| name.to_os_string()))
        .collect::<Vec<_>>();

    for entry in fs::read_dir(destination)
        .map_err(|source| io_error(destination, source))?
        .filter_map(Result::ok)
    {
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && !is_reserved_root_entry(&entry.file_name())
            && !staged_names.contains(&entry.file_name().to_ascii_lowercase())
            && find_executable(&entry.path(), target).is_ok()
        {
            replace_names.push(entry.file_name());
        }
    }
    replace_names.sort_by_key(|name| name.to_ascii_lowercase());

    let mut installed = Vec::new();
    let mut backups = Vec::new();
    for name in &replace_names {
        if directory_name_matches(destination, data_directory, name)
            || cache_directory.is_some_and(|cache| directory_name_matches(destination, cache, name))
        {
            let source = staging.join(name);
            if source.exists() {
                copy_missing_tree(&source, &destination.join(name))?;
            }
        }
    }
    for name in replace_names {
        let data_or_cache = directory_name_matches(destination, data_directory, &name)
            || cache_directory
                .is_some_and(|cache| directory_name_matches(destination, cache, &name));
        if is_reserved_root_entry(&name) || data_or_cache {
            continue;
        }
        if preserved_root_files
            .iter()
            .any(|preserved| name.to_string_lossy().eq_ignore_ascii_case(preserved))
            && destination.join(&name).exists()
        {
            continue;
        }
        let staged = staging.join(&name);
        if !staged.exists() {
            continue;
        }
        let target = destination.join(&name);
        if target.exists() {
            let saved = backup.join(&name);
            if let Err(source) = fs::rename(&target, &saved) {
                rollback_native(&installed, &backups, destination);
                return Err(io_error(&target, source));
            }
            backups.push((name.clone(), saved));
        }
        if let Err(source) = fs::rename(&staged, &target) {
            rollback_native(&installed, &backups, destination);
            return Err(io_error(&target, source));
        }
        installed.push(target);
    }
    Ok(())
}

fn rollback_native(
    installed: &[PathBuf],
    backups: &[(std::ffi::OsString, PathBuf)],
    destination: &Path,
) {
    for path in installed.iter().rev() {
        let _ = fs::remove_dir_all(path).or_else(|_| fs::remove_file(path));
    }
    for (name, saved) in backups.iter().rev() {
        let _ = fs::rename(saved, destination.join(name));
    }
}

fn is_reserved_root_entry(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    matches!(
        name.to_ascii_lowercase().as_str(),
        "data" | "cache" | "profile" | "profiles" | "backups"
    ) || name.starts_with(".pbm-")
}

fn directory_name_matches(root: &Path, candidate: &Path, name: &std::ffi::OsStr) -> bool {
    candidate
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|component| component.as_os_str().eq_ignore_ascii_case(name))
}

fn copy_missing_tree(source: &Path, destination: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(destination).map_err(|error| io_error(destination, error))?;
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|error| InstallError::Io {
            path: PathBuf::from(error.path().unwrap_or(source)),
            source: std::io::Error::other(error.to_string()),
        })?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| InstallError::ExecutableNotFound)?;
        if relative.as_os_str().is_empty() || entry.file_type().is_symlink() {
            continue;
        }
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|error| io_error(&target, error))?;
        } else if entry.file_type().is_file() && !target.exists() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
            }
            fs::copy(entry.path(), &target).map_err(|error| io_error(&target, error))?;
        }
    }
    Ok(())
}

fn prepare_runtime(
    request: &InstallRequest,
    work: &Path,
    staging_program: &Path,
    final_program: &Path,
    previous_program: Option<&Path>,
    profile_directory: &Path,
    cache_directory: Option<&Path>,
) -> Result<Option<String>, InstallError> {
    let backup_dir = request.browser_root.join("backups");
    match request.release.target.spec().runtime {
        crate::domain::runtime::RuntimeKind::ChromePlusFull
        | crate::domain::runtime::RuntimeKind::ChromePlusDataOnly => {
            let runtime = request
                .chrome_plus_runtime
                .as_ref()
                .ok_or(InstallError::RuntimeMissing("Chrome++"))?;
            verify_runtime_artifact(&runtime.archive_path, &runtime.artifact)?;
            let runtime_root = work.join("chromeplus");
            ArchiveService::extract(
                &runtime.archive_path,
                crate::domain::release::ArchiveFormat::SevenZip,
                &runtime_root,
            )?;
            let setdll_runtime_root = if request.release.target == TargetId::Edge {
                let archive = runtime
                    .setdll_archive
                    .as_ref()
                    .ok_or(InstallError::RuntimeMissing("Chrome++ setdll"))?;
                let artifact = runtime
                    .setdll_artifact
                    .as_ref()
                    .ok_or(InstallError::ArtifactIntegrityMissing)?;
                verify_runtime_artifact(archive, artifact)?;
                let root = work.join("chromeplus-setdll");
                ArchiveService::extract(
                    archive,
                    crate::domain::release::ArchiveFormat::SevenZip,
                    &root,
                )?;
                Some(root)
            } else {
                None
            };
            chromeplus::prepare(chromeplus::ChromePlusPrepareRequest {
                runtime_root: &runtime_root,
                staging_program,
                final_program,
                existing_program: previous_program,
                backup_dir: &backup_dir,
                data_dir: profile_directory,
                cache_dir: cache_directory,
                config: &request.chrome_plus_config,
                mode: match request.release.target.spec().runtime {
                    crate::domain::runtime::RuntimeKind::ChromePlusFull => ChromePlusMode::Full,
                    _ => ChromePlusMode::DataOnly,
                },
                version: &runtime.version,
                product: request.release.target,
                setdll_runtime_root: setdll_runtime_root.as_deref(),
            })?;
            Ok(Some(runtime.version.clone()))
        }
        crate::domain::runtime::RuntimeKind::Libportable => {
            let runtime = request
                .libportable_runtime
                .as_ref()
                .ok_or(InstallError::RuntimeMissing("Libportable"))?;
            verify_runtime_artifact(&runtime.archive_path, &runtime.artifact)?;
            let runtime_root = work.join("libportable");
            ArchiveService::extract(
                &runtime.archive_path,
                crate::domain::release::ArchiveFormat::SevenZip,
                &runtime_root,
            )?;
            libportable::prepare(libportable::LibportablePrepareRequest {
                runtime_root: &runtime_root,
                staging_program,
                final_program,
                product: request.release.target,
                product_version: &request.release.version,
                data_dir: profile_directory,
                cache_dir: cache_directory,
                backup_dir: &backup_dir,
            })?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn verify_runtime_artifact(
    path: &Path,
    artifact: &crate::domain::release::Artifact,
) -> Result<(), InstallError> {
    if artifact.integrity.is_none() {
        return Err(InstallError::ArtifactIntegrityMissing);
    }
    verify_artifact(path, artifact).map_err(InstallError::Artifact)
}

fn placement_paths(
    browser_root: &Path,
    configured_program: Option<&Path>,
    release: &ReleaseDescriptor,
) -> Result<(PathBuf, PathBuf), InstallError> {
    match release.placement {
        ProgramPlacement::App => {
            let default_program = release.target.spec().layout.program.unwrap_or("App");
            let target = configured_program.map_or_else(
                || Ok(browser_root.join(default_program)),
                |path| absolute_path(browser_root, path.to_path_buf()),
            )?;
            if !is_within(browser_root, &target) {
                return Err(InstallError::ProgramOutsideRoot);
            }
            Ok((target.clone(), target))
        }
        ProgramPlacement::EdgeVersioned => {
            let mut application = configured_program.map_or_else(
                || {
                    Ok(browser_root.join(
                        release
                            .target
                            .spec()
                            .layout
                            .program
                            .unwrap_or(r"App\Microsoft\Edge\Application"),
                    ))
                },
                |path| absolute_path(browser_root, path.to_path_buf()),
            )?;
            if application.join("msedge.exe").is_file()
                || is_edge_version_directory(&application, &release.version)
            {
                application = application
                    .parent()
                    .ok_or(InstallError::ProgramOutsideRoot)?
                    .to_path_buf();
            }
            if !is_within(browser_root, &application) {
                return Err(InstallError::ProgramOutsideRoot);
            }
            Ok((application.clone(), application.join(&release.version)))
        }
        ProgramPlacement::NativePackage => Err(InstallError::ExecutableNotFound),
    }
}

fn stored_browser_directory(
    placement: ProgramPlacement,
    commit_root: &Path,
    final_program: &Path,
) -> PathBuf {
    if placement == ProgramPlacement::EdgeVersioned {
        commit_root.to_path_buf()
    } else {
        final_program.to_path_buf()
    }
}

fn existing_program(root: &Path, target: TargetId) -> Option<PathBuf> {
    super::version_probe::locate_executable(target, root)
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
}

fn is_edge_version_directory(path: &Path, new_version: &str) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    (name == new_version
        || name
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.'))
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|parent| parent.eq_ignore_ascii_case("Application"))
}

fn verify_installed(
    executable: &Path,
    program: &Path,
    target: TargetId,
) -> Result<(), InstallError> {
    if !executable.is_file() || !is_pe_x64(executable)? {
        return Err(InstallError::InvalidExecutable);
    }
    match target.spec().runtime {
        crate::domain::runtime::RuntimeKind::ChromePlusFull
        | crate::domain::runtime::RuntimeKind::ChromePlusDataOnly => {
            if !program.join("version.dll").is_file() || !program.join("chrome++.ini").is_file() {
                return Err(InstallError::RuntimeMissing("Chrome++"));
            }
            if target == TargetId::Edge {
                let executable = program.join("msedge.exe");
                if !crate::runtime::pe_imports_dll(&executable, "version.dll")
                    .map_err(|source| io_error(&executable, source))?
                {
                    return Err(InstallError::RuntimeMissing("Edge version.dll import"));
                }
            }
        }
        crate::domain::runtime::RuntimeKind::Libportable
            if !program.join("portable64.dll").is_file()
                || !program.join("portable.ini").is_file()
                || !program.join("upcheck.exe").is_file() =>
        {
            return Err(InstallError::RuntimeMissing("Libportable"));
        }
        crate::domain::runtime::RuntimeKind::IceweaselNative
            if !program.join("portable64.dll").is_file()
                || !program.join("portable.ini").is_file() =>
        {
            return Err(InstallError::RuntimeMissing("Iceweasel portable runtime"));
        }
        crate::domain::runtime::RuntimeKind::SkyKakapoNative
            if !program.join("tmemutil.dll").is_file()
                || !program.join("tmemutil.ini").is_file() =>
        {
            return Err(InstallError::RuntimeMissing("SkyKakapo portable runtime"));
        }
        crate::domain::runtime::RuntimeKind::TurboLauncher
            if !program.join(".portable").is_file() =>
        {
            return Err(InstallError::RuntimeMissing("Turbo portable marker"));
        }
        _ => {}
    }
    Ok(())
}

fn find_executable(root: &Path, target: TargetId) -> Result<PathBuf, InstallError> {
    if !root.is_dir() {
        return Err(InstallError::ExecutableNotFound);
    }
    let candidates = target.spec().executable_candidates;
    let mut matches = Vec::new();
    for entry in WalkDir::new(root)
        .max_depth(7)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".pbm-work-")
        })
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if let Some(priority) = candidates
            .iter()
            .position(|candidate| name.eq_ignore_ascii_case(candidate))
        {
            matches.push((entry.depth(), priority, entry.into_path()));
        }
    }
    matches.sort();
    matches
        .into_iter()
        .map(|(_, _, path)| path)
        .find(|path| is_pe_x64(path).unwrap_or(false))
        .ok_or(InstallError::ExecutableNotFound)
}

fn find_named_file(root: &Path, file_name: &str) -> Option<PathBuf> {
    WalkDir::new(root)
        .max_depth(5)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(file_name)
        })
        .map(walkdir::DirEntry::into_path)
}

fn copy_package_extras(
    extracted: &Path,
    source_program: &Path,
    staging_program: &Path,
    target: TargetId,
) -> Result<(), InstallError> {
    let wanted = |name: &str| match target {
        TargetId::Catsxp => {
            name.eq_ignore_ascii_case("StartupParm.txt")
                || (name.to_ascii_lowercase().starts_with("catsxp")
                    && name.to_ascii_lowercase().ends_with(".exe"))
        }
        TargetId::Turbo => {
            name.eq_ignore_ascii_case(".portable")
                || (name.to_ascii_lowercase().starts_with("turbo")
                    && name.to_ascii_lowercase().ends_with(".exe"))
        }
        TargetId::Floorp => {
            name.eq_ignore_ascii_case("floorp-portable.yml")
                || (name.to_ascii_lowercase().contains("floorp-portable")
                    && name.to_ascii_lowercase().ends_with(".exe"))
        }
        TargetId::LibreWolf => {
            name.to_ascii_lowercase().contains("portable")
                && name.to_ascii_lowercase().ends_with(".exe")
        }
        _ => false,
    };
    for entry in WalkDir::new(extracted)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file()
            || entry.path().starts_with(source_program)
            || !wanted(&entry.file_name().to_string_lossy())
        {
            continue;
        }
        let destination = staging_program.join(entry.file_name());
        if !destination.exists() {
            fs::copy(entry.path(), &destination)
                .map_err(|source| io_error(&destination, source))?;
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(destination).map_err(|source| io_error(destination, source))?;
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|error| InstallError::Io {
            path: PathBuf::from(error.path().unwrap_or(source)),
            source: std::io::Error::other(error.to_string()),
        })?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| InstallError::Io {
                path: entry.path().to_path_buf(),
                source: std::io::Error::other("path escaped source tree"),
            })?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if entry.file_type().is_symlink() {
            return Err(InstallError::Io {
                path: entry.path().to_path_buf(),
                source: std::io::Error::other("symbolic links are not allowed"),
            });
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|source| io_error(&target, source))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
            }
            fs::copy(entry.path(), &target).map_err(|source| io_error(&target, source))?;
        }
    }
    Ok(())
}

fn is_pe_x64(path: &Path) -> Result<bool, InstallError> {
    crate::runtime::is_pe_x64(path).map_err(|source| io_error(path, source))
}

fn absolute_path(root: &Path, path: PathBuf) -> Result<PathBuf, InstallError> {
    if path.is_absolute() {
        Ok(normalize_path(&path))
    } else if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        Err(InstallError::InvalidBrowserRoot)
    } else {
        Ok(normalize_path(&root.join(path)))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                output.pop();
            }
            _ => output.push(component.as_os_str()),
        }
    }
    output
}

fn is_within(root: &Path, path: &Path) -> bool {
    let root = normalize_path(root)
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase();
    let path = normalize_path(path).to_string_lossy().to_ascii_lowercase();
    path == root || path.starts_with(&format!("{root}\\")) || path.starts_with(&format!("{root}/"))
}

fn preserve_turbo_legacy_data(
    browser_root: &Path,
    program_directory: &Path,
) -> Result<(), InstallError> {
    for (source_name, target_name) in [
        ("User Data", "User Data"),
        ("User Data.old", "User Data.old"),
    ] {
        let source = program_directory.join(source_name);
        if source.is_dir() {
            copy_missing_tree(&source, &browser_root.join(target_name))?;
        }
    }
    Ok(())
}

fn ensure_not_inside(data: &Path, program: &Path) -> Result<(), InstallError> {
    if is_within(program, data) {
        Err(InstallError::DataInsideProgram)
    } else {
        Ok(())
    }
}

fn io_error(path: &Path, source: std::io::Error) -> InstallError {
    InstallError::Io {
        path: path.to_path_buf(),
        source,
    }
}

struct WorkDirectory {
    path: PathBuf,
}

impl WorkDirectory {
    fn create(root: &Path, target: TargetId) -> Result<Self, InstallError> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = root.join(format!(".pbm-work-{}-{nonce}", target.sort_key()));
        fs::create_dir(&path).map_err(|source| io_error(&path, source))?;
        Ok(Self { path })
    }

    fn staged_root(&self, placement: ProgramPlacement) -> PathBuf {
        match placement {
            ProgramPlacement::App => self.path.join("program"),
            ProgramPlacement::EdgeVersioned => self.path.join("edge-tree"),
            ProgramPlacement::NativePackage => self.path.join("native-package"),
        }
    }
}

impl Drop for WorkDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
    };

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{
        InstallRequest, InstallService, commit_native_package, placement_paths,
        resolve_cache_directory, stored_browser_directory,
    };
    use crate::domain::{
        release::{
            ArchiveFormat, Artifact, CpuBuild, ProgramPlacement, ProviderKind, ReleaseChannel,
            ReleaseDescriptor,
        },
        runtime::ChromePlusConfig,
        target::TargetId,
    };

    fn fake_x64_exe() -> Vec<u8> {
        let mut bytes = vec![0u8; 0x200];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[0x80 + 24..0x80 + 26].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes
    }

    fn browser_zip(path: &std::path::Path, version: &str) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("iceweasel/iceweasel.exe", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&fake_x64_exe()).unwrap();
        writer
            .start_file("iceweasel/version.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(version.as_bytes()).unwrap();
        writer
            .start_file("iceweasel/portable64.dll", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"portable runtime fixture").unwrap();
        writer.finish().unwrap();
    }

    fn release(version: &str, artifact: &std::path::Path) -> ReleaseDescriptor {
        let digest = hex::encode(Sha256::digest(fs::read(artifact).unwrap()));
        ReleaseDescriptor {
            target: TargetId::Iceweasel,
            version: version.to_owned(),
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            provider: ProviderKind::GitHubRelease,
            artifact: Artifact {
                file_name: "iceweasel.zip".to_owned(),
                format: ArchiveFormat::Zip,
                size: Some(fs::metadata(artifact).unwrap().len()),
                integrity: Some(crate::domain::release::Integrity::Sha256(digest)),
                urls: vec!["https://example.com/iceweasel.zip".to_owned()],
                signature_url: None,
            },
            placement: ProgramPlacement::App,
            inner_archive: None,
            source: "fixture".to_owned(),
            published_at: None,
        }
    }

    fn request(
        release: ReleaseDescriptor,
        artifact: &std::path::Path,
        root: &std::path::Path,
    ) -> InstallRequest {
        InstallRequest {
            release,
            browser_artifact: artifact.to_path_buf(),
            browser_root: root.to_path_buf(),
            browser_directory: None,
            data_directory: None,
            profile_directory: None,
            cache_directory: None,
            chrome_plus_config: ChromePlusConfig::default(),
            chrome_plus_runtime: None,
            libportable_runtime: None,
        }
    }

    #[test]
    fn chromeplus_default_cache_follows_profile_while_native_targets_keep_the_layout() {
        let root = std::path::Path::new(r"D:\Browsers\Chrome");

        assert_eq!(
            resolve_cache_directory(TargetId::Chrome, root, None).unwrap(),
            None
        );
        assert_eq!(
            resolve_cache_directory(TargetId::Turbo, root, None).unwrap(),
            Some(std::path::PathBuf::from(r"D:\Browsers\Chrome\User Data"))
        );
        assert_eq!(
            resolve_cache_directory(
                TargetId::Chrome,
                root,
                Some(std::path::PathBuf::from(r"E:\BrowserCache\Chrome"))
            )
            .unwrap(),
            Some(std::path::PathBuf::from(r"E:\BrowserCache\Chrome"))
        );
    }

    #[test]
    fn update_replaces_program_and_preserves_external_data() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("Iceweasel");
        let first = directory.path().join("first.zip");
        let second = directory.path().join("second.zip");
        browser_zip(&first, "1.0");
        browser_zip(&second, "2.0");
        InstallService::install(request(release("1.0", &first), &first, &root)).unwrap();
        fs::write(root.join("Profiles/user-profile"), b"keep me").unwrap();

        let receipt =
            InstallService::install(request(release("2.0", &second), &second, &root)).unwrap();

        assert_eq!(receipt.version, "2.0");
        assert_eq!(receipt.data_directory, root.join("Profiles"));
        assert_eq!(fs::read(root.join("APP/version.txt")).unwrap(), b"2.0");
        assert_eq!(
            fs::read(root.join("Profiles/user-profile")).unwrap(),
            b"keep me"
        );
    }

    #[test]
    fn installation_rejects_artifact_without_integrity() {
        let directory = tempdir().unwrap();
        let first = directory.path().join("first.zip");
        browser_zip(&first, "1.0");
        let mut descriptor = release("1.0", &first);
        descriptor.artifact.integrity = None;

        let error = InstallService::install(request(
            descriptor,
            &first,
            &directory.path().join("Browser"),
        ))
        .unwrap_err();

        assert!(matches!(
            error,
            super::InstallError::ArtifactIntegrityMissing
        ));
    }

    #[test]
    fn native_package_layout_and_user_root_files_are_preserved() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("Floorp");
        let archive = directory.path().join("floorp.zip");
        let file = File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in [
            ("app/floorp.exe", fake_x64_exe()),
            ("app/version.txt", b"1.0".to_vec()),
            ("floorp-portable.exe", fake_x64_exe()),
            ("portapp.json", br#"{"version":"1.0"}"#.to_vec()),
            (
                "floorp-portable.yml",
                b"app:\n  check_for_updates: true\n".to_vec(),
            ),
        ] {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        fs::create_dir_all(root.join("Data")).unwrap();
        fs::write(root.join("Data/user-profile"), b"keep").unwrap();
        fs::write(
            root.join("floorp-portable.yml"),
            b"app:\n  check_for_updates: false\n",
        )
        .unwrap();
        let mut descriptor = release("1.0", &archive);
        descriptor.target = TargetId::Floorp;
        descriptor.artifact.file_name = "floorp.zip".to_owned();
        descriptor.placement = ProgramPlacement::NativePackage;

        let receipt = InstallService::install(request(descriptor, &archive, &root)).unwrap();

        assert_eq!(receipt.browser_directory, root.join("app"));
        assert!(root.join("app/floorp.exe").is_file());
        assert!(root.join("floorp-portable.exe").is_file());
        assert_eq!(
            fs::read(root.join("floorp-portable.yml")).unwrap(),
            b"app:\n  check_for_updates: false\n"
        );
        assert_eq!(fs::read(root.join("Data/user-profile")).unwrap(), b"keep");
    }

    #[test]
    fn thorium_root_layout_replaces_bin_and_preserves_user_data() {
        let directory = tempdir().unwrap();
        let staging = directory.path().join("staging");
        let destination = directory.path().join("destination");
        let backup = directory.path().join("backup");
        fs::create_dir_all(destination.join("BIN")).unwrap();
        fs::create_dir_all(destination.join("USER_DATA/Default")).unwrap();
        fs::write(destination.join("BIN/old.txt"), b"old").unwrap();
        fs::write(destination.join("USER_DATA/Default/Cookies"), b"keep login").unwrap();
        fs::create_dir_all(staging.join("BIN")).unwrap();
        fs::create_dir_all(staging.join("USER_DATA/Default")).unwrap();
        fs::write(staging.join("BIN/thorium.exe"), b"new").unwrap();
        fs::write(staging.join("USER_DATA/Default/First Run"), b"new").unwrap();
        let user_data = destination.join("USER_DATA");

        commit_native_package(
            &staging,
            &destination,
            &backup,
            TargetId::Thorium,
            &[],
            &user_data,
            Some(&user_data),
        )
        .unwrap();

        assert!(destination.join("BIN/thorium.exe").is_file());
        assert!(!destination.join("BIN/old.txt").exists());
        assert_eq!(
            fs::read(destination.join("USER_DATA/Default/Cookies")).unwrap(),
            b"keep login"
        );
        assert!(destination.join("USER_DATA/Default/First Run").is_file());
    }

    #[test]
    fn turbo_root_layout_preserves_official_tree_and_migrates_legacy_data() {
        let directory = tempdir().unwrap();
        let archive = directory.path().join("turbo.zip");
        let file = File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in [
            ("turbo.exe", fake_x64_exe()),
            ("154/chrome.dll", b"release tree".to_vec()),
            (".portable", Vec::new()),
        ] {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        let root = directory.path().join("Turbo");
        fs::create_dir_all(root.join("App/User Data/Default")).unwrap();
        fs::write(root.join("App/turbo.exe"), fake_x64_exe()).unwrap();
        fs::write(root.join("App/User Data/Default/Cookies"), b"legacy login").unwrap();
        fs::create_dir_all(root.join("App/User Data.old/Default")).unwrap();
        fs::write(
            root.join("App/User Data.old/Default/Cookies"),
            b"older login",
        )
        .unwrap();
        let mut descriptor = release("154", &archive);
        descriptor.target = TargetId::Turbo;
        descriptor.placement = ProgramPlacement::NativePackage;

        let receipt = InstallService::install(request(descriptor, &archive, &root)).unwrap();

        assert_eq!(receipt.browser_directory, root);
        assert_eq!(receipt.data_directory, root.join("User Data"));
        assert!(root.join("turbo.exe").is_file());
        assert!(root.join("154/chrome.dll").is_file());
        assert!(root.join(".portable").is_file());
        let top_level = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>();
        assert!(!root.join("App").exists(), "top-level={top_level:?}");
        assert_eq!(
            fs::read(root.join("User Data/Default/Cookies")).unwrap(),
            b"legacy login"
        );
        assert_eq!(
            fs::read(root.join("User Data.old/Default/Cookies")).unwrap(),
            b"older login"
        );

        fs::write(root.join("User Data/Cookies"), b"keep login").unwrap();
        let mut descriptor = release("155", &archive);
        descriptor.target = TargetId::Turbo;
        descriptor.placement = ProgramPlacement::NativePackage;
        InstallService::install(request(descriptor, &archive, &root)).unwrap();
        assert_eq!(
            fs::read(root.join("User Data/Cookies")).unwrap(),
            b"keep login"
        );
    }

    #[test]
    fn turbo_package_requires_its_portable_marker_before_commit() {
        let directory = tempdir().unwrap();
        let archive = directory.path().join("turbo-without-marker.zip");
        let file = File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("turbo.exe", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&fake_x64_exe()).unwrap();
        writer.finish().unwrap();
        let mut descriptor = release("154", &archive);
        descriptor.target = TargetId::Turbo;
        descriptor.placement = ProgramPlacement::NativePackage;

        let error = InstallService::install(request(
            descriptor,
            &archive,
            &directory.path().join("Turbo"),
        ))
        .unwrap_err();

        assert!(matches!(
            error,
            super::InstallError::RuntimeMissing("Turbo portable marker")
        ));
        assert!(!directory.path().join("Turbo/turbo.exe").exists());
    }

    #[test]
    fn catsxp_program_tree_is_wrapped_in_app_with_external_profiles() {
        let directory = tempdir().unwrap();
        let archive = directory.path().join("catsxp.zip");
        let file = File::create(&archive).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, bytes) in [
            ("catsxp.exe", fake_x64_exe()),
            ("156/catsxp.dll", b"release tree".to_vec()),
            ("StartupParm.txt", b"--disable-catsxp-update\r\n".to_vec()),
        ] {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        let root = directory.path().join("Catsxp");
        let mut descriptor = release("156", &archive);
        descriptor.target = TargetId::Catsxp;
        descriptor.placement = ProgramPlacement::NativePackage;

        let receipt = InstallService::install(request(descriptor, &archive, &root)).unwrap();

        assert_eq!(receipt.browser_directory, root.join("App"));
        assert_eq!(receipt.data_directory, root.join("Profiles"));
        assert!(root.join("App/catsxp.exe").is_file());
        assert!(root.join("App/156/catsxp.dll").is_file());
        assert!(!root.join("catsxp.exe").exists());
        let startup = fs::read_to_string(root.join("App/StartupParm.txt")).unwrap();
        assert!(startup.contains("--user-data-dir="), "{startup}");
        assert!(startup.contains(r"\Profiles"), "{startup}");
    }

    #[test]
    fn edge_version_tree_is_placed_below_app_and_reuses_the_parent_on_update() {
        let directory = tempdir().unwrap();
        let archive = directory.path().join("fixture.zip");
        fs::write(&archive, b"fixture").unwrap();
        let root = std::path::Path::new(r"D:\Browsers\Edge");
        let mut descriptor = release("1.0", &archive);
        descriptor.target = TargetId::Edge;
        descriptor.placement = ProgramPlacement::EdgeVersioned;
        descriptor.version = "156.0.1".to_owned();

        let (base, version) = placement_paths(root, None, &descriptor).unwrap();
        assert_eq!(base, root.join(r"App\Microsoft\Edge\Application"));
        assert_eq!(
            version,
            root.join(r"App\Microsoft\Edge\Application\156.0.1")
        );
        assert_eq!(
            stored_browser_directory(ProgramPlacement::EdgeVersioned, &base, &version),
            base
        );

        let previous_version = root.join(r"App\Microsoft\Edge\Application\155.0.1");
        let (base, version) = placement_paths(root, Some(&previous_version), &descriptor).unwrap();
        assert_eq!(base, root.join(r"App\Microsoft\Edge\Application"));
        assert_eq!(
            version,
            root.join(r"App\Microsoft\Edge\Application\156.0.1")
        );
    }
}
