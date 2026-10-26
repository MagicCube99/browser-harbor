//! Background operation messages shared by the UI and service pipeline.

use crate::domain::target::TargetId;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};

use crate::{
    domain::{
        release::{CpuBuild, ReleaseChannel, ReleaseDescriptor},
        settings::{NetworkSettings, TargetConfig},
        version::compare_versions,
    },
    providers::ProviderResolver,
    runtime::{self},
    services::{
        download::Downloader,
        http::HttpClient,
        install::{InstallReceipt, InstallRequest, InstallService, RuntimeArchive},
        release_cache::ReleaseCache,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationPhase {
    Preparing,
    Resolving,
    Notice(String),
    Downloading {
        resource: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Verifying,
    Extracting,
    Installing,
    Checked {
        check: UpstreamCheck,
    },
    SameVersion {
        current_version: String,
        upstream: UpstreamCheck,
    },
    Installed {
        receipt: InstallReceipt,
    },
    ChromePlusChecked {
        current_version: Option<String>,
        latest_version: String,
    },
    Cancelled,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamCheck {
    pub version: String,
    pub channel: ReleaseChannel,
    pub cpu_build: CpuBuild,
    pub cached: bool,
    pub checked_at: u64,
}

impl UpstreamCheck {
    pub fn fresh(version: String, channel: ReleaseChannel, cpu_build: CpuBuild) -> Self {
        Self {
            version,
            channel,
            cpu_build,
            cached: false,
            checked_at: unix_now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationEvent {
    pub target: TargetId,
    pub phase: OperationPhase,
}

#[derive(Debug)]
pub struct OperationController {
    pub receiver: Receiver<OperationEvent>,
    cancellation: Arc<AtomicBool>,
}

impl OperationController {
    pub fn cancel(&self) {
        self.cancellation.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
pub struct StartOperationRequest {
    pub target: TargetId,
    pub mode: OperationMode,
    pub target_config: TargetConfig,
    pub network: NetworkSettings,
    pub cache_dir: PathBuf,
    pub current_version: Option<String>,
    pub replace_same_version: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationMode {
    Check,
    ChromePlusCheck,
    Install,
}

pub fn start_operation(request: StartOperationRequest) -> OperationController {
    let (sender, receiver) = mpsc::channel();
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancellation = Arc::clone(&cancellation);
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = sender.send(OperationEvent::new(
                    request.target,
                    OperationPhase::Failed {
                        message: format!("无法启动后台任务：{error}"),
                    },
                ));
                return;
            }
        };
        let target = request.target;
        let result = runtime.block_on(run_operation(request, sender.clone(), worker_cancellation));
        match result {
            Ok(OperationResult::Checked(check)) => {
                let _ = sender.send(OperationEvent::new(
                    target,
                    OperationPhase::Checked { check },
                ));
            }
            Ok(OperationResult::SameVersion {
                current_version,
                upstream,
            }) => {
                let _ = sender.send(OperationEvent::new(
                    target,
                    OperationPhase::SameVersion {
                        current_version,
                        upstream,
                    },
                ));
            }
            Ok(OperationResult::Installed(receipt)) => {
                let _ = sender.send(OperationEvent::new(
                    target,
                    OperationPhase::Installed { receipt },
                ));
            }
            Ok(OperationResult::ChromePlusChecked {
                current_version,
                latest_version,
            }) => {
                let _ = sender.send(OperationEvent::new(
                    target,
                    OperationPhase::ChromePlusChecked {
                        current_version,
                        latest_version,
                    },
                ));
            }
            Err(error) => {
                let phase = if error.to_string().contains("cancelled") {
                    OperationPhase::Cancelled
                } else {
                    OperationPhase::Failed {
                        message: error.to_string(),
                    }
                };
                let _ = sender.send(OperationEvent::new(target, phase));
            }
        }
    });
    OperationController {
        receiver,
        cancellation,
    }
}

async fn run_operation(
    request: StartOperationRequest,
    sender: mpsc::Sender<OperationEvent>,
    cancellation: Arc<AtomicBool>,
) -> Result<OperationResult, Box<dyn std::error::Error + Send + Sync>> {
    let target = request.target;
    let emit = |phase| {
        let _ = sender.send(OperationEvent::new(target, phase));
    };
    emit(OperationPhase::Resolving);
    let http = HttpClient::new(&request.network)?;
    if request.mode == OperationMode::ChromePlusCheck {
        let release = runtime::chromeplus::resolve_latest(&http).await?;
        return Ok(OperationResult::ChromePlusChecked {
            current_version: request.target_config.chrome_plus_version,
            latest_version: release.version,
        });
    }
    let provider = ProviderResolver::new(http.clone());
    let cache_path = request.cache_dir.join("releases.json");
    let mut cache = ReleaseCache::load(cache_path).ok();
    let mut used_cache = false;
    let mut cache_checked_at = None;
    let descriptor = match provider
        .resolve(
            target,
            request.target_config.channel,
            request.target_config.cpu_build,
        )
        .await
    {
        Ok(descriptor) => descriptor,
        Err(error) => {
            if let Some((cached, checked_at)) = cache.as_ref().and_then(|cache| {
                cache.get_any_with_checked_at(
                    target,
                    request.target_config.channel,
                    request.target_config.cpu_build,
                )
            }) {
                used_cache = true;
                cache_checked_at = Some(checked_at);
                emit(OperationPhase::Notice(
                    "实时元数据不可用，使用上次成功的发布记录".to_owned(),
                ));
                cached.clone()
            } else {
                return Err(Box::new(error));
            }
        }
    };
    if descriptor
        .artifact
        .urls
        .iter()
        .all(|url| !contains_transient_query(url))
        && let Some(cache) = &mut cache
    {
        cache.store(descriptor.clone())?;
    }
    if cancellation.load(Ordering::Relaxed) {
        return Err(Box::new(crate::services::ServiceError::Cancelled));
    }
    let check = UpstreamCheck {
        version: descriptor.version.clone(),
        channel: descriptor.channel,
        cpu_build: descriptor.cpu_build,
        cached: used_cache,
        checked_at: cache_checked_at.unwrap_or_else(unix_now),
    };
    if request.mode == OperationMode::Check {
        return Ok(OperationResult::Checked(check));
    }
    if let Some(current_version) = request.current_version.as_deref()
        && requires_replacement_confirmation(
            request.replace_same_version,
            current_version,
            &descriptor.version,
        )
    {
        return Ok(OperationResult::SameVersion {
            current_version: current_version.to_owned(),
            upstream: check,
        });
    }

    let (direct_client, fallback_client) = HttpClient::download_clients(&request.network)?;
    let downloader = Downloader::with_fallback_clients(
        direct_client,
        fallback_client,
        request.cache_dir.clone(),
        cancellation.clone(),
    );
    emit(OperationPhase::Preparing);
    let browser_artifact = if let Some(local) = request.target_config.local_package.as_ref() {
        if !local.is_file() {
            return Err(format!("本地安装包不存在：{}", local.display()).into());
        }
        crate::services::download::verify_artifact(local, &descriptor.artifact)?;
        local.clone()
    } else {
        download_artifact(
            &downloader,
            &descriptor,
            &request.network,
            target.display_name(),
            &emit,
        )
        .await?
    };
    if let Some(signature_url) = &descriptor.artifact.signature_url {
        let signature = http.get_bytes(signature_url).await?;
        let content = std::fs::read(&browser_artifact)?;
        if !crate::providers::verify_artifact_signature(target, &content, &signature)? {
            return Err("安装包签名校验失败".into());
        }
    }

    let (chrome_plus_runtime, libportable_runtime) =
        prepare_runtime_archives(&http, &downloader, &request, &emit).await?;

    if cancellation.load(Ordering::Relaxed) {
        return Err(Box::new(crate::services::ServiceError::Cancelled));
    }
    emit(OperationPhase::Installing);
    let receipt = InstallService::install(InstallRequest {
        release: descriptor,
        browser_artifact,
        browser_root: request
            .target_config
            .browser_root
            .clone()
            .ok_or("请先设置浏览器根目录")?,
        browser_directory: request.target_config.browser_directory.clone(),
        data_directory: request.target_config.data_directory.clone(),
        profile_directory: request.target_config.profile_directory.clone(),
        cache_directory: request.target_config.cache_directory.clone(),
        chrome_plus_config: request.target_config.chrome_plus.clone(),
        chrome_plus_runtime,
        libportable_runtime,
    })
    .map_err(Box::<dyn std::error::Error + Send + Sync>::from)?;
    Ok(OperationResult::Installed(receipt))
}

fn requires_replacement_confirmation(
    replace_same_version: bool,
    current: &str,
    upstream: &str,
) -> bool {
    !replace_same_version
        && compare_versions(current, upstream) == crate::domain::version::VersionRelation::Same
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

enum OperationResult {
    Checked(UpstreamCheck),
    SameVersion {
        current_version: String,
        upstream: UpstreamCheck,
    },
    Installed(InstallReceipt),
    ChromePlusChecked {
        current_version: Option<String>,
        latest_version: String,
    },
}

async fn download_artifact(
    downloader: &Downloader,
    descriptor: &ReleaseDescriptor,
    network: &NetworkSettings,
    resource: &str,
    emit: &(impl Fn(OperationPhase) + Sync),
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let mut last_emit = 0u64;
    let path = downloader
        .download(&descriptor.artifact, network, |progress| {
            if progress.downloaded.saturating_sub(last_emit) >= 512 * 1024 {
                last_emit = progress.downloaded;
                emit(OperationPhase::Downloading {
                    resource: resource.to_owned(),
                    downloaded: progress.downloaded,
                    total: progress.total,
                });
            }
        })
        .await?;
    emit(OperationPhase::Verifying);
    Ok(path)
}

async fn prepare_runtime_archives(
    http: &HttpClient,
    downloader: &Downloader,
    request: &StartOperationRequest,
    emit: &(impl Fn(OperationPhase) + Sync),
) -> Result<
    (Option<RuntimeArchive>, Option<RuntimeArchive>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    use crate::domain::runtime::RuntimeKind;

    match request.target.spec().runtime {
        RuntimeKind::ChromePlusFull | RuntimeKind::ChromePlusDataOnly => {
            let release = runtime::chromeplus::resolve_latest(http).await?;
            let descriptor = ReleaseDescriptor {
                target: request.target,
                version: release.version.clone(),
                channel: ReleaseChannel::Stable,
                cpu_build: request.target_config.cpu_build,
                provider: crate::domain::release::ProviderKind::GitHubRelease,
                artifact: release.artifact.clone(),
                placement: request.target.spec().placement,
                inner_archive: None,
                source: "Bush2021/chrome_plus".to_owned(),
                published_at: None,
            };
            let path =
                download_artifact(downloader, &descriptor, &request.network, "Chrome++", emit)
                    .await?;
            let setdll_archive = if request.target == TargetId::Edge {
                let descriptor = ReleaseDescriptor {
                    target: request.target,
                    version: release.version.clone(),
                    channel: ReleaseChannel::Stable,
                    cpu_build: request.target_config.cpu_build,
                    provider: crate::domain::release::ProviderKind::GitHubRelease,
                    artifact: release.setdll_artifact.clone(),
                    placement: request.target.spec().placement,
                    inner_archive: None,
                    source: "Bush2021/chrome_plus setdll".to_owned(),
                    published_at: None,
                };
                Some(
                    download_artifact(
                        downloader,
                        &descriptor,
                        &request.network,
                        "Chrome++ setdll",
                        emit,
                    )
                    .await?,
                )
            } else {
                None
            };
            Ok((
                Some(RuntimeArchive {
                    archive_path: path,
                    version: release.version,
                    setdll_archive,
                }),
                None,
            ))
        }
        RuntimeKind::Libportable => {
            let release = runtime::libportable::resolve_latest(http).await?;
            let descriptor = ReleaseDescriptor {
                target: request.target,
                version: release.version.clone(),
                channel: ReleaseChannel::Stable,
                cpu_build: request.target_config.cpu_build,
                provider: crate::domain::release::ProviderKind::GitHubRelease,
                artifact: release.artifact.clone(),
                placement: request.target.spec().placement,
                inner_archive: None,
                source: "adonais/libportable".to_owned(),
                published_at: None,
            };
            let path = download_artifact(
                downloader,
                &descriptor,
                &request.network,
                "Libportable",
                emit,
            )
            .await?;
            Ok((
                None,
                Some(RuntimeArchive {
                    archive_path: path,
                    version: release.version,
                    setdll_archive: None,
                }),
            ))
        }
        _ => Ok((None, None)),
    }
}

fn contains_transient_query(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("p1=")
        || lower.contains("auth_key=")
        || lower.contains("token=")
        || lower.contains("signature=")
        || lower.contains("sig=")
}

impl OperationEvent {
    pub fn new(target: TargetId, phase: OperationPhase) -> Self {
        Self { target, phase }
    }
}

impl OperationPhase {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Checked { .. }
                | Self::SameVersion { .. }
                | Self::Installed { .. }
                | Self::ChromePlusChecked { .. }
                | Self::Cancelled
                | Self::Failed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationPhase, requires_replacement_confirmation};

    #[test]
    fn only_final_operation_phases_are_terminal() {
        assert!(!OperationPhase::Resolving.is_terminal());
        assert!(
            !OperationPhase::Downloading {
                resource: "browser".to_owned(),
                downloaded: 1,
                total: Some(2)
            }
            .is_terminal()
        );
        assert!(OperationPhase::Cancelled.is_terminal());
        assert!(
            OperationPhase::ChromePlusChecked {
                current_version: Some("1.18.2".to_owned()),
                latest_version: "1.18.2".to_owned(),
            }
            .is_terminal()
        );
        assert!(
            OperationPhase::SameVersion {
                current_version: "1.0.0".to_owned(),
                upstream: super::UpstreamCheck {
                    version: "1.0.0".to_owned(),
                    channel: crate::domain::release::ReleaseChannel::Stable,
                    cpu_build: crate::domain::release::CpuBuild::X64,
                    cached: false,
                    checked_at: 0,
                },
            }
            .is_terminal()
        );
        assert!(
            OperationPhase::Failed {
                message: "failed".to_owned()
            }
            .is_terminal()
        );
    }

    #[test]
    fn same_version_preflight_requires_confirmation_once() {
        assert!(requires_replacement_confirmation(
            false, "156.0.1", "156.0.1"
        ));
        assert!(!requires_replacement_confirmation(
            true, "156.0.1", "156.0.1"
        ));
        assert!(!requires_replacement_confirmation(
            false, "155.0.1", "156.0.1"
        ));
    }
}
