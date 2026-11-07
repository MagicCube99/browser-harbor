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
    install_transaction::{self, FileTransaction},
};
use crate::{
    domain::{
        release::{ProgramPlacement, ReleaseDescriptor},
        runtime::{ChromePlusConfig, RuntimeKind},
        settings::TargetConfig,
        target::{ChromePlusMode, TargetId},
    },
    infra::windows::process::{
        ProcessError, RunningProcess, is_process_running, running_processes_in_directory,
    },
    runtime::{RuntimeError, chromeplus, libportable, native},
    services::version_probe::locate_executable,
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
pub struct ChromePlusUpdateRequest {
    pub target: TargetId,
    pub config: TargetConfig,
    pub runtime: RuntimeArchive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromePlusUpdateReceipt {
    pub version: String,
    pub program_directory: PathBuf,
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
    pub catsxp_build: Option<crate::domain::release::CatsxpBuildIdentity>,
    pub browser_directory: PathBuf,
    pub data_directory: PathBuf,
    pub profile_directory: PathBuf,
    pub cache_directory: Option<PathBuf>,
    pub chrome_plus_version: Option<String>,
    pub previous_chrome_plus_version: Option<String>,
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("Browser Root must be an absolute path")]
    InvalidBrowserRoot,
    #[error("Browser Root, Program Directory, or Profile Directory is missing")]
    MissingPath,
    #[error("program directory must remain within Browser Root")]
    ProgramOutsideRoot,
    #[error("data, profile and cache directories must not be inside the program directory")]
    DataInsideProgram,
    #[error("browser executable is already running: {0}")]
    BrowserRunning(PathBuf),
    #[error("browser processes are still running in {path}")]
    BrowserRunningInDirectory {
        path: PathBuf,
        processes: Vec<RunningProcess>,
    },
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
    #[error("installed Chrome++ content does not match the verified runtime")]
    RuntimeContentMismatch,
    #[error("installation rollback failed")]
    Rollback(#[source] std::io::Error),
    #[error("installation failed: {cause}; recovery failed: {source}; originals kept at {backup}")]
    RecoveryRequired {
        cause: String,
        backup: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("artifact verification failed")]
    Artifact(#[source] ServiceError),
    #[error("installable artifact has no integrity value")]
    ArtifactIntegrityMissing,
    #[error("archive extraction failed: {0}")]
    Archive(#[from] ArchiveError),
    #[error("runtime preparation failed: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("process protection check failed")]
    Process(#[from] ProcessError),
    #[error("file operation failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub struct InstallService;

pub struct ChromePlusUpdateService;

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
    pub fn prepare_retry(target: TargetId, config: &TargetConfig) -> Result<usize, InstallError> {
        let root = config
            .browser_root
            .as_deref()
            .ok_or(InstallError::MissingPath)?;
        if !root.is_absolute() {
            return Err(InstallError::InvalidBrowserRoot);
        }
        let program = config.browser_directory.clone().unwrap_or_else(|| {
            target
                .spec()
                .layout
                .program
                .map_or_else(|| root.to_path_buf(), |path| root.join(path))
        });
        if !is_within(root, &program) {
            return Err(InstallError::ProgramOutsideRoot);
        }
        if root.is_dir() {
            install_transaction::reject_links(root, &program)
                .map_err(|source| io_error(&program, source))?;
            ensure_target_closed(&program)?;
        }
        let protected = [
            config.data_directory.as_deref(),
            config.profile_directory.as_deref(),
            config.cache_directory.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let mut program_unit = program.clone();
        if let Some(prefix) = target.spec().layout.package_prefix {
            let suffix = Path::new(target.spec().layout.program.unwrap_or(prefix))
                .strip_prefix(prefix)
                .unwrap_or_else(|_| Path::new(""));
            for _ in suffix.components() {
                program_unit.pop();
            }
        }
        if target != TargetId::Turbo {
            for path in &protected {
                ensure_not_inside(path, &program_unit)?;
            }
        }
        if root.is_dir() {
            recover_installation(root, target, &protected)
        } else {
            Ok(0)
        }
    }

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
        install_transaction::reject_links(&browser_root, &browser_root)
            .map_err(|source| io_error(&browser_root, source))?;
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

        recover_installation(
            &browser_root,
            request.release.target,
            &[
                Some(data_directory.as_path()),
                Some(profile_directory.as_path()),
                cache_directory.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        )?;

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
        let previous_chrome_plus_version = previous_program
            .as_deref()
            .and_then(|program| chromeplus::installed_version(program).ok());
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
        let prepared_hash = prepared_runtime_hash(&staged_program, request.release.target)?;
        if !staged_program.join(&executable_name).is_file()
            || !is_pe_x64(&staged_program.join(&executable_name))?
        {
            return Err(InstallError::InvalidExecutable);
        }

        ensure_target_closed(&commit_root)?;
        let parent = commit_root
            .parent()
            .ok_or(InstallError::ProgramOutsideRoot)?;
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
        let backup = work.path.join("backup");
        install_transaction::reject_links(&browser_root, &commit_root)
            .map_err(|source| io_error(&commit_root, source))?;
        let mut transaction =
            FileTransaction::new(&backup, parent).map_err(|source| io_error(&backup, source))?;
        let name = commit_root
            .file_name()
            .ok_or(InstallError::ProgramOutsideRoot)?;
        if let Err(source) =
            transaction.replace(name, Some(&work.staged_root(request.release.placement)))
        {
            return Err(rollback_error(
                &mut transaction,
                io_error(&commit_root, source),
                &backup,
            ));
        }
        let executable = final_program.join(&executable_name);
        if let Err(error) = verify_installed(&executable, &final_program, request.release.target)
            .and_then(|()| {
                verify_runtime_result(
                    &final_program,
                    prepared_hash.as_deref(),
                    chrome_plus_version.as_deref(),
                )
            })
        {
            return Err(rollback_error(&mut transaction, error, &backup));
        }
        work.finish()?;

        Ok(InstallReceipt {
            version: request.release.version,
            catsxp_build: None,
            browser_directory: stored_browser_directory(
                request.release.placement,
                &commit_root,
                &final_program,
            ),
            data_directory,
            profile_directory,
            cache_directory,
            chrome_plus_version,
            previous_chrome_plus_version,
        })
    }
}

impl ChromePlusUpdateService {
    pub fn update(
        request: ChromePlusUpdateRequest,
    ) -> Result<ChromePlusUpdateReceipt, InstallError> {
        let target = request.target;
        if !matches!(
            target.spec().runtime,
            RuntimeKind::ChromePlusFull | RuntimeKind::ChromePlusDataOnly
        ) {
            return Err(InstallError::RuntimeMissing("Chrome++"));
        }
        let browser_root = request
            .config
            .browser_root
            .as_deref()
            .ok_or(InstallError::MissingPath)?;
        if !browser_root.is_absolute() {
            return Err(InstallError::InvalidBrowserRoot);
        }
        let configured_program = request
            .config
            .browser_directory
            .as_deref()
            .ok_or(InstallError::MissingPath)?;
        let profile_directory = request
            .config
            .profile_directory
            .as_deref()
            .ok_or(InstallError::MissingPath)?;
        recover_installation(browser_root, target, &[profile_directory])?;
        let executable = locate_executable(target, configured_program)
            .map_err(|_| InstallError::ExecutableNotFound)?;
        let program_directory = executable
            .parent()
            .ok_or(InstallError::ExecutableNotFound)?;
        let running = running_processes_in_directory(configured_program)?;
        if !running.is_empty() {
            return Err(InstallError::BrowserRunningInDirectory {
                path: configured_program.to_path_buf(),
                processes: running,
            });
        }

        verify_runtime_artifact(&request.runtime.archive_path, &request.runtime.artifact)?;
        let work = WorkDirectory::create(browser_root, target)?;
        let runtime_root = work.path.join("chromeplus");
        ArchiveService::extract(
            &request.runtime.archive_path,
            crate::domain::release::ArchiveFormat::SevenZip,
            &runtime_root,
        )?;
        let setdll_runtime_root = if target == TargetId::Edge {
            let archive = request
                .runtime
                .setdll_archive
                .as_ref()
                .ok_or(InstallError::RuntimeMissing("Chrome++ setdll"))?;
            let artifact = request
                .runtime
                .setdll_artifact
                .as_ref()
                .ok_or(InstallError::ArtifactIntegrityMissing)?;
            verify_runtime_artifact(archive, artifact)?;
            let root = work.path.join("chromeplus-setdll");
            ArchiveService::extract(
                archive,
                crate::domain::release::ArchiveFormat::SevenZip,
                &root,
            )?;
            Some(root)
        } else {
            None
        };

        let staging_program = work.path.join("staging");
        fs::create_dir_all(&staging_program)
            .map_err(|source| io_error(&staging_program, source))?;
        if target == TargetId::Edge {
            let staged_executable = staging_program.join("msedge.exe");
            fs::copy(&executable, &staged_executable)
                .map_err(|source| io_error(&staged_executable, source))?;
        }
        chromeplus::prepare(chromeplus::ChromePlusPrepareRequest {
            runtime_root: &runtime_root,
            staging_program: &staging_program,
            final_program: program_directory,
            existing_program: Some(program_directory),
            backup_dir: &browser_root.join("backups"),
            data_dir: profile_directory,
            cache_dir: request.config.cache_directory.as_deref(),
            config: &request.config.chrome_plus,
            mode: target.chrome_plus_mode(),
            version: &request.runtime.version,
            product: target,
            setdll_runtime_root: setdll_runtime_root.as_deref(),
        })?;
        let prepared_hash = prepared_runtime_hash(&staging_program, target)?;

        let running = running_processes_in_directory(configured_program)?;
        if !running.is_empty() {
            return Err(InstallError::BrowserRunningInDirectory {
                path: configured_program.to_path_buf(),
                processes: running,
            });
        }
        commit_chrome_plus_files(
            &staging_program,
            program_directory,
            &work.path.join("replacement-backup"),
            target,
        )?;
        if let Err(error) = verify_runtime_result(
            program_directory,
            prepared_hash.as_deref(),
            Some(&request.runtime.version),
        ) {
            return Err(recover_error(
                &work,
                browser_root,
                &[profile_directory],
                error,
            ));
        }
        work.finish()?;
        Ok(ChromePlusUpdateReceipt {
            version: request.runtime.version,
            program_directory: program_directory.to_path_buf(),
        })
    }
}

fn commit_chrome_plus_files(
    staging: &Path,
    destination: &Path,
    backup: &Path,
    target: TargetId,
) -> Result<(), InstallError> {
    fs::create_dir_all(backup).map_err(|source| io_error(backup, source))?;
    let mut names = vec![
        std::ffi::OsString::from("version.dll"),
        std::ffi::OsString::from("chrome++.ini"),
    ];
    if target == TargetId::Edge {
        names.insert(0, "msedge.exe".into());
    }

    let mut transaction =
        FileTransaction::new(backup, destination).map_err(|source| io_error(backup, source))?;
    for name in names {
        let staged = staging.join(&name);
        if !staged.is_file() {
            return Err(rollback_error(
                &mut transaction,
                InstallError::RuntimeMissing("Chrome++"),
                backup,
            ));
        }
        let target_path = destination.join(&name);
        if let Err(source) = transaction.replace(&name, Some(&staged)) {
            return Err(rollback_error(
                &mut transaction,
                io_error(&target_path, source),
                backup,
            ));
        }
    }
    Ok(())
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

    let configured_previous = request.browser_directory.clone().unwrap_or_else(|| {
        request.release.target.spec().layout.program.map_or_else(
            || browser_root.to_path_buf(),
            |path| browser_root.join(path),
        )
    });
    let previous_program = existing_program(&configured_previous, request.release.target)
        .or_else(|| existing_program(browser_root, request.release.target));
    ensure_target_closed(&configured_previous)?;
    let previous_chrome_plus_version = previous_program
        .as_deref()
        .and_then(|program| chromeplus::installed_version(program).ok());
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
    install_transaction::reject_links(browser_root, &destination)
        .map_err(|source| io_error(&destination, source))?;
    if request.release.target == TargetId::Turbo {
        preserve_turbo_legacy_data(browser_root, &browser_root.join("App"))?;
    }
    // Legacy program trees remain until a later, explicit cleanup; never move user data during preparation.
    if layout.package_prefix.is_some() {
        for directory in [
            Some(data_directory),
            Some(profile_directory),
            cache_directory,
        ]
        .into_iter()
        .flatten()
        {
            ensure_not_inside(directory, &destination)?;
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
    // Prepared runtime files already implement version-aware INI preservation.
    let preserved_root_files = &[];
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
    let prepared_hash = prepared_runtime_hash(staged_program, request.release.target)?;
    verify_installed(&staged_executable, staged_program, request.release.target)?;
    ensure_target_closed(&configured_previous)?;
    commit_native_package(
        &staging_package,
        &destination,
        &backup,
        request.release.target,
        preserved_root_files,
        data_directory,
        cache_directory,
    )?;
    let installed_executable = destination.join(relative_executable);
    let mut catsxp_build = None;
    if let Err(error) = verify_installed(
        &installed_executable,
        &final_program,
        request.release.target,
    )
    .and_then(|()| {
        verify_runtime_result(
            &final_program,
            prepared_hash.as_deref(),
            chrome_plus_version.as_deref(),
        )
    })
    .and_then(|()| {
        if request.release.target == TargetId::Catsxp {
            catsxp_build = super::catsxp::capture(
                &final_program,
                &request.release.version,
                request.release.channel,
            )
            .map_err(|source| io_error(&final_program, source))?;
        }
        Ok(())
    }) {
        return Err(recover_error(
            work,
            browser_root,
            &[data_directory, profile_directory],
            error,
        ));
    }
    work.finish()?;
    Ok(InstallReceipt {
        version: request.release.version.clone(),
        catsxp_build,
        browser_directory: final_program,
        data_directory: data_directory.to_path_buf(),
        profile_directory: profile_directory.to_path_buf(),
        cache_directory: cache_directory.map(Path::to_path_buf),
        chrome_plus_version,
        previous_chrome_plus_version,
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
    let obsolete = obsolete_native_files(target);
    for entry in fs::read_dir(staging).map_err(|source| io_error(staging, source))? {
        let entry = entry.map_err(|source| io_error(staging, source))?;
        if entry.file_type().is_ok_and(|kind| kind.is_file())
            && obsolete.iter().any(|name| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
            })
        {
            let path = entry.path();
            fs::remove_file(&path).map_err(|source| io_error(&path, source))?;
        }
    }
    let mut transaction =
        FileTransaction::new(backup, destination).map_err(|source| io_error(backup, source))?;
    if target == TargetId::Turbo
        && destination.join("App").is_dir()
        && find_executable(&destination.join("App"), target).is_ok()
        && let Err(source) = transaction.replace(std::ffi::OsStr::new("App"), None)
    {
        return Err(rollback_error(
            &mut transaction,
            io_error(&destination.join("App"), source),
            backup,
        ));
    }
    for entry in fs::read_dir(destination).map_err(|source| io_error(destination, source))? {
        let entry = entry.map_err(|source| io_error(destination, source))?;
        if entry.file_type().is_ok_and(|kind| kind.is_file())
            && obsolete.iter().any(|name| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
            })
        {
            let name = entry.file_name();
            let saved = backup.join(&name);
            if let Err(source) = transaction.replace(&name, None) {
                return Err(rollback_error(
                    &mut transaction,
                    io_error(&saved, source),
                    backup,
                ));
            }
        }
    }
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

    for name in &replace_names {
        let program_unit = destination.join(name);
        let overlaps_data = directory_name_matches(destination, data_directory, name)
            || cache_directory
                .is_some_and(|cache| directory_name_matches(destination, cache, name));
        if overlaps_data && find_executable(&staging.join(name), target).is_ok() {
            return Err(rollback_error(
                &mut transaction,
                InstallError::DataInsideProgram,
                backup,
            ));
        }
        install_transaction::reject_links(destination, &program_unit)
            .map_err(|source| io_error(&program_unit, source))?;
    }

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
        ensure_files_not_locked(&target)?;
        if let Err(source) = transaction.replace(&name, Some(&staged)) {
            return Err(rollback_error(
                &mut transaction,
                io_error(&target, source),
                backup,
            ));
        }
    }
    Ok(())
}

fn obsolete_native_files(target: TargetId) -> &'static [&'static str] {
    match target {
        TargetId::Floorp => &["floorp-portable.exe"],
        TargetId::LibreWolf => &[
            "LibreWolf-Portable.exe",
            "LibreWolf-WinUpdater.exe",
            "LibreWolf-WinUpdater.ini",
            "ScheduledTask-Create.ps1",
            "ScheduledTask-Remove.ps1",
        ],
        TargetId::Vivaldi => &["update_notifier.exe"],
        _ => &[],
    }
}

fn rollback_error(
    transaction: &mut FileTransaction,
    cause: InstallError,
    backup: &Path,
) -> InstallError {
    match transaction.rollback() {
        Ok(()) => cause,
        Err(source) => InstallError::RecoveryRequired {
            cause: cause.to_string(),
            backup: backup.to_path_buf(),
            source,
        },
    }
}

fn recover_error(
    work: &WorkDirectory,
    root: &Path,
    protected: &[&Path],
    cause: InstallError,
) -> InstallError {
    match install_transaction::recover_work(&work.path, root, protected) {
        Ok(()) => cause,
        Err(source) => InstallError::RecoveryRequired {
            cause: cause.to_string(),
            backup: work.path.clone(),
            source,
        },
    }
}

fn ensure_target_closed(program: &Path) -> Result<(), InstallError> {
    let processes = running_processes_in_directory(program)?;
    if !processes.is_empty() {
        return Err(InstallError::BrowserRunningInDirectory {
            path: program.to_path_buf(),
            processes,
        });
    }
    ensure_files_not_locked(program)?;
    Ok(())
}

fn ensure_files_not_locked(path: &Path) -> Result<(), InstallError> {
    if path.is_file() {
        if is_file_locked(path) {
            return Err(InstallError::BrowserRunning(path.to_path_buf()));
        }
    } else if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|source| io_error(path, source))? {
            let entry = entry.map_err(|source| io_error(path, source))?;
            let entry_path = entry.path();
            if entry_path.is_file() {
                let is_binary = entry_path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("dll")
                    });
                if is_binary && is_file_locked(&entry_path) {
                    return Err(InstallError::BrowserRunning(entry_path));
                }
            }
        }
    }
    Ok(())
}

fn is_file_locked(path: &Path) -> bool {
    match fs::OpenOptions::new().write(true).open(path) {
        Ok(_) => false,
        Err(error) => is_sharing_violation(&error),
    }
}

fn is_sharing_violation(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(32) || error.raw_os_error() == Some(33)
}

fn prepared_runtime_hash(program: &Path, target: TargetId) -> Result<Option<String>, InstallError> {
    if target.spec().runtime.uses_chrome_plus() {
        Ok(Some(chromeplus::sha256_file(&program.join("version.dll"))?))
    } else {
        Ok(None)
    }
}

fn verify_runtime_result(
    program: &Path,
    expected_hash: Option<&str>,
    expected_version: Option<&str>,
) -> Result<(), InstallError> {
    if let Some(expected_hash) = expected_hash {
        if chromeplus::sha256_file(&program.join("version.dll"))? != expected_hash {
            return Err(InstallError::RuntimeContentMismatch);
        }
        // Hash identity is mandatory; version resources are checked when the upstream DLL provides them.
        if let (Some(expected), Ok(actual)) =
            (expected_version, chromeplus::installed_version(program))
            && crate::domain::version::compare_versions(expected, &actual)
                != crate::domain::version::VersionRelation::Same
        {
            return Err(InstallError::RuntimeContentMismatch);
        }
    }
    Ok(())
}

fn recover_installation(
    root: &Path,
    target: TargetId,
    protected: &[&Path],
) -> Result<usize, InstallError> {
    let prefix = format!(".pbm-work-{}-", target.sort_key());
    let mut recovered = 0;
    for entry in fs::read_dir(root).map_err(|source| io_error(root, source))? {
        let entry = entry.map_err(|source| io_error(root, source))?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix)
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let work = entry.path();
        if !install_transaction::has_journal(&work) {
            continue;
        }
        ensure_target_closed(root)?;
        install_transaction::recover_work(&work, root, protected).map_err(|source| {
            InstallError::RecoveryRequired {
                cause: "previous installation was interrupted".to_owned(),
                backup: work.clone(),
                source,
            }
        })?;
        fs::remove_dir_all(&work).map_err(|source| io_error(&work, source))?;
        recovered += 1;
    }
    Ok(recovered)
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
        crate::domain::runtime::RuntimeKind::Libportable => {
            if !program.join("portable64.dll").is_file()
                || !program.join("portable.ini").is_file()
                || !program.join("upcheck.exe").is_file()
            {
                return Err(InstallError::RuntimeMissing("Libportable"));
            }
            if !crate::runtime::pe_imports_dll(&program.join("mozglue.dll"), "portable64.dll")
                .map_err(|source| io_error(&program.join("mozglue.dll"), source))?
            {
                return Err(InstallError::RuntimeMissing("Libportable mozglue import"));
            }
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
        TargetId::Floorp => name.eq_ignore_ascii_case("floorp-portable.yml"),
        TargetId::LibreWolf => false,
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
    fn finish(&self) -> Result<(), InstallError> {
        install_transaction::finish_work(&self.path).map_err(|source| io_error(&self.path, source))
    }
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
        if !install_transaction::has_journal(&self.path) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn failed_rollback_keeps_the_unrecovered_original() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Browser");
        std::fs::create_dir_all(&root).unwrap();
        let work =
            super::WorkDirectory::create(&root, crate::domain::target::TargetId::Chrome).unwrap();
        let backup = work.path.join("replacement-backup");
        std::fs::create_dir_all(&backup).unwrap();
        let saved = backup.join("version.dll");
        let target = root.join("version.dll");
        std::fs::write(&target, b"original").unwrap();
        let staged = work.path.join("new-version.dll");
        std::fs::write(&staged, b"new locked file").unwrap();
        let mut transaction = super::FileTransaction::new(&backup, &root).unwrap();
        transaction
            .replace(std::ffi::OsStr::new("version.dll"), Some(&staged))
            .unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        assert!(transaction.rollback().is_err());
        drop(work);
        assert!(
            saved.exists(),
            "failed recovery must never discard the only original"
        );
        drop(lock);
        assert_eq!(
            super::recover_installation(&root, crate::domain::target::TargetId::Chrome, &[])
                .unwrap(),
            1
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
    }

    #[test]
    fn ensure_target_closed_rejects_file_locked_by_another_process() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let file_path = directory.path().join("thorium.exe");
        std::fs::write(&file_path, fake_x64_exe()).unwrap();
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&file_path)
            .unwrap();

        let err = super::ensure_target_closed(directory.path()).unwrap_err();
        assert!(matches!(err, super::InstallError::BrowserRunning(path) if path == file_path));
    }

    #[test]
    fn interrupted_install_is_restored_before_retry_without_resuming_download() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Browser");
        std::fs::create_dir_all(root.join("App")).unwrap();
        std::fs::write(root.join("App/old-marker"), b"old").unwrap();
        let work =
            super::WorkDirectory::create(&root, crate::domain::target::TargetId::Firefox).unwrap();
        let staging = work.path.join("program");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("new-marker"), b"new").unwrap();
        let mut transaction =
            super::FileTransaction::new(&work.path.join("backup"), &root).unwrap();
        transaction
            .replace(std::ffi::OsStr::new("App"), Some(&staging))
            .unwrap();
        drop(work);
        assert!(!root.join("App/old-marker").exists());
        assert_eq!(
            super::recover_installation(
                &root,
                crate::domain::target::TargetId::Firefox,
                &[&root.join("Profiles")]
            )
            .unwrap(),
            1
        );
        assert_eq!(std::fs::read(root.join("App/old-marker")).unwrap(), b"old");
        assert!(!root.join("App/new-marker").exists());
        assert_eq!(
            super::recover_installation(&root, crate::domain::target::TargetId::Firefox, &[])
                .unwrap(),
            0
        );
    }

    #[test]
    fn recovery_rejects_a_journal_pointing_outside_browser_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Browser");
        let external = directory.path().join("External");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(external.join("keep"), b"user").unwrap();
        let work =
            super::WorkDirectory::create(&root, crate::domain::target::TargetId::Firefox).unwrap();
        let backup = work.path.join("backup");
        let _transaction = super::FileTransaction::new(&backup, &external).unwrap();
        drop(work);
        assert!(
            super::recover_installation(&root, crate::domain::target::TargetId::Firefox, &[])
                .is_err()
        );
        assert_eq!(std::fs::read(external.join("keep")).unwrap(), b"user");
        assert!(backup.join("transaction.json").exists());
    }

    #[test]
    fn recovery_protects_profiles_with_case_and_trailing_dot_aliases() {
        for alias in ["profiles", "Profiles.", "Profiles "] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("Browser");
            let profile = root.join("Profiles");
            fs::create_dir_all(&profile).unwrap();
            fs::write(profile.join("keep"), b"user data").unwrap();
            let work = super::WorkDirectory::create(&root, TargetId::Firefox).unwrap();
            fs::create_dir_all(work.path.join("backup")).unwrap();
            fs::write(
                work.path.join("backup/transaction.json"),
                serde_json::to_vec(&serde_json::json!({
                    "destination": root, "entries": [{"name":alias,"had_previous":false}]
                }))
                .unwrap(),
            )
            .unwrap();
            assert!(
                super::recover_installation(&root, TargetId::Firefox, &[&profile]).is_err(),
                "alias must be rejected: {alias}"
            );
            assert_eq!(fs::read(profile.join("keep")).unwrap(), b"user data");
        }
    }

    #[test]
    fn install_preflight_rejects_a_program_junction_without_touching_its_target() {
        use std::os::windows::process::CommandExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Helium");
        let external = directory.path().join("OtherBrowser");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("chrome.exe"), b"other browser").unwrap();
        let status = Command::new(std::env::var_os("COMSPEC").unwrap())
            .args(["/D", "/C", "mklink", "/J"])
            .arg(root.join("App"))
            .arg(&external)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status()
            .unwrap();
        assert!(status.success(), "could not create test junction");
        let mut config = TargetConfig::for_target(TargetId::Helium);
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Helium);
        let result = super::InstallService::prepare_retry(TargetId::Helium, &config);
        assert!(
            result.is_err(),
            "preflight must reject the program junction"
        );
        assert_eq!(
            fs::read(external.join("chrome.exe")).unwrap(),
            b"other browser"
        );
        fs::remove_dir(root.join("App")).unwrap();
    }

    #[test]
    fn install_preflight_rejects_a_running_turbo_before_extraction() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Turbo");
        fs::create_dir_all(&root).unwrap();
        let command = std::env::var_os("COMSPEC").unwrap();
        fs::copy(&command, root.join("turbo.exe")).unwrap();
        let mut child = Command::new(root.join("turbo.exe"))
            .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if running_process_count_in_directory(&root).unwrap() > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        let mut config = TargetConfig::for_target(TargetId::Turbo);
        config.browser_root = Some(root.clone());
        config.derive_paths_from_root(TargetId::Turbo);
        let error = InstallService::prepare_retry(TargetId::Turbo, &config).unwrap_err();

        let _ = child.kill();
        let _ = child.wait();
        let processes = match error {
            InstallError::BrowserRunningInDirectory { path, processes } => {
                assert_eq!(path, root);
                processes
            }
            other => panic!("unexpected error: {other:?}"),
        };
        assert!(
            processes
                .iter()
                .any(|process| process.name.eq_ignore_ascii_case("turbo.exe")),
            "the error should name the detected processes: {processes:?}"
        );
    }

    #[test]
    fn native_update_rejects_data_inside_program_instead_of_mixing_versions() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Zen");
        std::fs::create_dir_all(root.join("App/core/private-profile")).unwrap();
        let data = root.join("App/core/private-profile");
        std::fs::write(data.join("keep"), b"data").unwrap();
        std::fs::write(root.join("App/core/zen.exe"), b"old").unwrap();
        let staging = directory.path().join("staging");
        std::fs::create_dir_all(staging.join("core")).unwrap();
        std::fs::write(staging.join("core/zen.exe"), fake_x64_exe()).unwrap();
        let result = super::commit_native_package(
            &staging,
            &root.join("App"),
            &directory.path().join("backup"),
            crate::domain::target::TargetId::Zen,
            &[],
            &data,
            None,
        );
        assert!(matches!(
            result,
            Err(super::InstallError::DataInsideProgram)
        ));
        assert_eq!(std::fs::read(data.join("keep")).unwrap(), b"data");
        assert_eq!(
            std::fs::read(root.join("App/core/zen.exe")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn same_runtime_update_preserves_custom_ini_in_all_wrapped_chromium_targets() {
        for target in [
            TargetId::Brave,
            TargetId::Helium,
            TargetId::UngoogledChromium,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join(target.display_name());
            let program = root.join("App");
            fs::create_dir_all(&program).unwrap();
            fs::write(
                program.join(target.spec().executable_candidates[0]),
                fake_x64_exe(),
            )
            .unwrap();
            fs::write(program.join("version.dll"), b"new-chromeplus-dll").unwrap();
            let original = "[general]\r\ncustom_key=keep\r\ncommand_line=--custom\r\n";
            fs::write(program.join("chrome++.ini"), original).unwrap();
            let archive = directory.path().join("browser.zip");
            let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            writer
                .start_file(
                    format!("browser/{}", target.spec().executable_candidates[0]),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(&fake_x64_exe()).unwrap();
            writer.finish().unwrap();
            let mut descriptor = release("2.0", &archive);
            descriptor.target = target;
            descriptor.placement = ProgramPlacement::NativePackage;
            let mut install = request(descriptor, &archive, &root);
            install.chrome_plus_runtime = Some(chrome_plus_runtime(
                &directory.path().join("runtime.7z"),
                "1.18.3",
            ));
            super::InstallService::install(install).unwrap();
            assert_eq!(
                fs::read_to_string(program.join("chrome++.ini")).unwrap(),
                original,
                "{target:?}"
            );
            assert!(!fs::read_dir(&root).unwrap().flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".pbm-work-")
            }));
        }
    }

    #[test]
    fn wrapped_native_update_commits_new_chromeplus_files() {
        use std::io::Write;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Helium");
        std::fs::create_dir_all(root.join("App")).unwrap();
        std::fs::write(root.join("App/chrome.exe"), fake_x64_exe()).unwrap();
        std::fs::write(root.join("App/version.dll"), b"old-chromeplus-dll").unwrap();
        std::fs::write(
            root.join("App/chrome++.ini"),
            b"[general]\ncustom_key=must-backup\n",
        )
        .unwrap();
        let archive = directory.path().join("helium.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .start_file(
                "helium/chrome.exe",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(&fake_x64_exe()).unwrap();
        writer.finish().unwrap();
        let mut descriptor = release("2.0", &archive);
        descriptor.target = crate::domain::target::TargetId::Helium;
        descriptor.placement = crate::domain::release::ProgramPlacement::NativePackage;
        let mut install = request(descriptor, &archive, &root);
        install.chrome_plus_runtime = Some(chrome_plus_runtime(
            &directory.path().join("runtime.7z"),
            "1.18.3",
        ));
        super::InstallService::install(install).unwrap();
        assert_eq!(
            std::fs::read(root.join("App/version.dll")).unwrap(),
            b"new-chromeplus-dll"
        );
        assert!(
            std::fs::read_dir(root.join("backups"))
                .unwrap()
                .flatten()
                .any(|entry| std::fs::read(entry.path())
                    .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains("must-backup")))
        );
    }
    use std::{
        fs::{self, File},
        io::Write,
        path::Path,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use sevenz_rust2::ArchiveWriter;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{
        ChromePlusUpdateRequest, ChromePlusUpdateService, InstallError, InstallRequest,
        InstallService, RuntimeArchive, commit_chrome_plus_files, commit_native_package,
        placement_paths, resolve_cache_directory, stored_browser_directory,
    };
    use crate::domain::{
        release::{
            ArchiveFormat, Artifact, CpuBuild, Integrity, ProgramPlacement, ProviderKind,
            ReleaseChannel, ReleaseDescriptor,
        },
        runtime::ChromePlusConfig,
        settings::TargetConfig,
        target::TargetId,
    };
    use crate::infra::ini::{IniEncoding, write_ini};
    use crate::infra::windows::process::running_process_count_in_directory;

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

    fn chrome_plus_runtime(path: &Path, version: &str) -> RuntimeArchive {
        let source = path.parent().unwrap().join("chromeplus-runtime-source");
        let runtime_app = source.join("x64/App");
        fs::create_dir_all(&runtime_app).unwrap();
        fs::write(runtime_app.join("version.dll"), b"new-chromeplus-dll").unwrap();
        write_ini(
            &runtime_app.join("chrome++.ini"),
            "[general]\r\ncommand_line=\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();
        let mut writer = ArchiveWriter::create(path).unwrap();
        writer.push_source_path(&source, |_| true).unwrap();
        writer.finish().unwrap();
        let bytes = fs::read(path).unwrap();
        RuntimeArchive {
            archive_path: path.to_path_buf(),
            version: version.to_owned(),
            artifact: Artifact {
                file_name: "Chrome++_fixture.7z".to_owned(),
                format: ArchiveFormat::SevenZip,
                size: Some(bytes.len() as u64),
                integrity: Some(Integrity::Sha256(hex::encode(Sha256::digest(&bytes)))),
                urls: vec!["https://example.com/Chrome++_fixture.7z".to_owned()],
                signature_url: None,
            },
            setdll_archive: None,
            setdll_artifact: None,
        }
    }

    fn chrome_plus_update_request(
        directory: &Path,
        target: TargetId,
        runtime: RuntimeArchive,
    ) -> ChromePlusUpdateRequest {
        let root = directory.join("browser");
        let program = root.join("App");
        fs::create_dir_all(&program).unwrap();
        let executable_name = if target == TargetId::Edge {
            "msedge.exe"
        } else {
            "chrome.exe"
        };
        fs::write(program.join(executable_name), fake_x64_exe()).unwrap();
        fs::write(program.join("browser-marker.bin"), b"keep-browser").unwrap();
        fs::write(program.join("version.dll"), b"old-chromeplus-dll").unwrap();
        write_ini(
            &program.join("chrome++.ini"),
            "[general]\r\ncommand_line=--old\r\n",
            IniEncoding::Utf16Le,
        )
        .unwrap();
        let profiles = root.join("Profiles");
        fs::create_dir_all(&profiles).unwrap();
        let mut config = TargetConfig::for_target(target);
        config.browser_root = Some(root);
        config.browser_directory = Some(program);
        config.data_directory = Some(profiles.clone());
        config.profile_directory = Some(profiles);
        ChromePlusUpdateRequest {
            target,
            config,
            runtime,
        }
    }

    #[test]
    fn standalone_chromeplus_update_replaces_only_chromeplus_files() {
        let directory = tempdir().unwrap();
        let runtime = chrome_plus_runtime(&directory.path().join("runtime.7z"), "1.18.3");
        let request = chrome_plus_update_request(directory.path(), TargetId::Chrome, runtime);
        let program = request.config.browser_directory.clone().unwrap();

        let receipt = ChromePlusUpdateService::update(request).unwrap();

        assert_eq!(receipt.version, "1.18.3");
        assert_eq!(
            fs::read(program.join("version.dll")).unwrap(),
            b"new-chromeplus-dll"
        );
        assert_eq!(
            fs::read(program.join("browser-marker.bin")).unwrap(),
            b"keep-browser"
        );
    }

    #[test]
    fn chromeplus_file_commit_rolls_back_after_a_later_replacement_failure() {
        let directory = tempdir().unwrap();
        let staging = directory.path().join("staging");
        let destination = directory.path().join("destination");
        let backup = directory.path().join("backup");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::create_dir_all(&backup).unwrap();
        fs::write(staging.join("version.dll"), b"new-dll").unwrap();
        fs::write(staging.join("chrome++.ini"), b"new-ini").unwrap();
        fs::write(destination.join("version.dll"), b"old-dll").unwrap();
        fs::write(destination.join("chrome++.ini"), b"old-ini").unwrap();
        fs::create_dir_all(backup.join("chrome++.ini")).unwrap();
        fs::write(
            backup.join("chrome++.ini").join("blocker.bin"),
            b"blocked-backup-path",
        )
        .unwrap();

        assert!(
            commit_chrome_plus_files(&staging, &destination, &backup, TargetId::Chrome).is_err()
        );
        assert_eq!(
            fs::read(destination.join("version.dll")).unwrap(),
            b"old-dll"
        );
        assert_eq!(
            fs::read(destination.join("chrome++.ini")).unwrap(),
            b"old-ini"
        );
    }

    #[test]
    fn edge_chromeplus_commit_includes_the_staged_patched_executable() {
        let directory = tempdir().unwrap();
        let staging = directory.path().join("staging");
        let destination = directory.path().join("destination");
        let backup = directory.path().join("backup");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(staging.join("msedge.exe"), b"new-edge").unwrap();
        fs::write(staging.join("version.dll"), b"new-dll").unwrap();
        fs::write(staging.join("chrome++.ini"), b"new-ini").unwrap();
        fs::write(destination.join("msedge.exe"), b"old-edge").unwrap();
        fs::write(destination.join("version.dll"), b"old-dll").unwrap();
        fs::write(destination.join("chrome++.ini"), b"old-ini").unwrap();

        commit_chrome_plus_files(&staging, &destination, &backup, TargetId::Edge).unwrap();

        assert_eq!(
            fs::read(destination.join("msedge.exe")).unwrap(),
            b"new-edge"
        );
        assert_eq!(
            fs::read(destination.join("version.dll")).unwrap(),
            b"new-dll"
        );
        assert_eq!(
            fs::read(destination.join("chrome++.ini")).unwrap(),
            b"new-ini"
        );
    }

    #[test]
    fn standalone_chromeplus_update_refuses_any_running_target_instance() {
        let directory = tempdir().unwrap();
        let runtime = chrome_plus_runtime(&directory.path().join("runtime.7z"), "1.18.3");
        let request = chrome_plus_update_request(directory.path(), TargetId::Chrome, runtime);
        let program = request.config.browser_directory.clone().unwrap();
        let command = std::env::var_os("COMSPEC").unwrap();
        fs::copy(&command, program.join("chrome.exe")).unwrap();
        let mut child = Command::new(program.join("chrome.exe"))
            .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if crate::infra::windows::process::running_process_count(&program.join("chrome.exe"))
                .unwrap()
                > 0
            {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        let error = ChromePlusUpdateService::update(request).unwrap_err();

        let _ = child.kill();
        let _ = child.wait();
        assert!(matches!(
            error,
            super::InstallError::BrowserRunningInDirectory { .. }
        ));
        assert_eq!(
            fs::read(program.join("version.dll")).unwrap(),
            b"old-chromeplus-dll"
        );
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
    fn libportable_installation_requires_injected_mozglue() {
        let directory = tempdir().unwrap();
        let program = directory.path().join("app");
        fs::create_dir_all(&program).unwrap();
        fs::write(program.join("floorp.exe"), fake_x64_exe()).unwrap();
        fs::write(program.join("mozglue.dll"), fake_x64_exe()).unwrap();
        fs::write(program.join("portable64.dll"), b"portable").unwrap();
        fs::write(program.join("upcheck.exe"), b"upcheck").unwrap();
        fs::write(program.join("portable.ini"), b"[General]\nPortable=1\n").unwrap();

        let error =
            super::verify_installed(&program.join("floorp.exe"), &program, TargetId::Floorp)
                .unwrap_err();

        assert!(matches!(
            error,
            super::InstallError::RuntimeMissing("Libportable mozglue import")
        ));
    }

    #[test]
    fn native_package_commit_removes_bundled_launchers() {
        for (target, program_name, executable, obsolete) in [
            (
                TargetId::Floorp,
                "app",
                "floorp.exe",
                &["floorp-portable.exe"][..],
            ),
            (
                TargetId::LibreWolf,
                "LibreWolf",
                "librewolf.exe",
                &[
                    "LibreWolf-Portable.exe",
                    "LibreWolf-WinUpdater.exe",
                    "LibreWolf-WinUpdater.ini",
                    "ScheduledTask-Create.ps1",
                    "ScheduledTask-Remove.ps1",
                ][..],
            ),
        ] {
            let directory = tempdir().unwrap();
            let destination = directory.path().join(target.display_name());
            let staging = directory.path().join("staging");
            let backup = directory.path().join("backup");
            fs::create_dir_all(destination.join(program_name)).unwrap();
            fs::create_dir_all(staging.join(program_name)).unwrap();
            fs::write(staging.join(program_name).join(executable), fake_x64_exe()).unwrap();
            for name in obsolete {
                fs::write(destination.join(name), b"launcher").unwrap();
            }

            commit_native_package(
                &staging,
                &destination,
                &backup,
                target,
                &[],
                &destination.join("data"),
                None,
            )
            .unwrap();

            for name in obsolete {
                assert!(
                    !destination.join(name).exists(),
                    "{} retained {name}",
                    target.display_name()
                );
            }
        }
    }

    #[test]
    fn install_receipt_records_chromeplus_before_replacing_files() {
        for placement in [ProgramPlacement::App, ProgramPlacement::NativePackage] {
            let directory = tempdir().unwrap();
            let root = directory.path().join("Chrome");
            let archive = directory.path().join("chrome.zip");
            let mut writer = ZipWriter::new(File::create(&archive).unwrap());
            writer
                .start_file("chrome.exe", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&fake_x64_exe()).unwrap();
            writer.finish().unwrap();
            let mut descriptor = release("156.0.1", &archive);
            descriptor.target = TargetId::Chrome;
            descriptor.placement = placement;
            let mut install = request(descriptor, &archive, &root);
            install.chrome_plus_runtime = Some(chrome_plus_runtime(
                &directory.path().join("runtime.7z"),
                "1.18.3",
            ));

            let fresh = InstallService::install(install.clone()).unwrap();
            assert_eq!(fresh.previous_chrome_plus_version, None);
            assert_eq!(fresh.chrome_plus_version.as_deref(), Some("1.18.3"));

            let system_dll = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/version.dll");
            fs::copy(system_dll, fresh.browser_directory.join("version.dll")).unwrap();
            let before =
                crate::runtime::chromeplus::installed_version(&fresh.browser_directory).unwrap();
            let updated = InstallService::install(install).unwrap();
            assert_eq!(updated.previous_chrome_plus_version, Some(before));
            assert_eq!(updated.chrome_plus_version.as_deref(), Some("1.18.3"));
            assert_eq!(
                fs::read(updated.browser_directory.join("version.dll")).unwrap(),
                b"new-chromeplus-dll"
            );
        }
    }

    #[test]
    fn native_package_layout_and_user_root_files_are_preserved() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("Floorp");
        let staging = directory.path().join("staging");
        let backup = directory.path().join("backup");
        fs::create_dir_all(staging.join("app")).unwrap();
        fs::write(staging.join("app/floorp.exe"), fake_x64_exe()).unwrap();
        fs::write(staging.join("app/version.txt"), b"1.0").unwrap();
        fs::write(staging.join("floorp-portable.exe"), fake_x64_exe()).unwrap();
        fs::write(staging.join("portapp.json"), br#"{"version":"1.0"}"#).unwrap();
        fs::write(
            staging.join("floorp-portable.yml"),
            b"app:\n  check_for_updates: true\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("data/profile/default")).unwrap();
        fs::write(root.join("data/profile/default/prefs.js"), b"keep").unwrap();
        fs::write(
            root.join("floorp-portable.yml"),
            b"app:\n  check_for_updates: false\n",
        )
        .unwrap();
        let data = root.join("data");
        let profile = root.join(r"data\profile\default");

        crate::runtime::native::configure_native_package(
            &staging,
            &root,
            TargetId::Floorp,
            &data,
            None,
        )
        .unwrap();
        commit_native_package(
            &staging,
            &root,
            &backup,
            TargetId::Floorp,
            &["floorp-portable.yml"],
            &data,
            None,
        )
        .unwrap();

        assert!(root.join("app/floorp.exe").is_file());
        assert!(!root.join("floorp-portable.exe").exists());
        assert_eq!(
            fs::read(root.join("floorp-portable.yml")).unwrap(),
            b"app:\n  check_for_updates: false\n"
        );
        assert_eq!(fs::read(profile.join("prefs.js")).unwrap(), b"keep");
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
