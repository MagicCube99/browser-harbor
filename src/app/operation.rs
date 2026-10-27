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
        runtime::ChromePlusRelease,
        settings::{NetworkSettings, TargetConfig},
        version::compare_versions,
    },
    providers::ProviderResolver,
    runtime::{self},
    services::{
        download::Downloader,
        http::{HttpClient, signed_manifest_candidates},
        install::{
            ChromePlusUpdateRequest, ChromePlusUpdateService, InstallReceipt, InstallRequest,
            InstallService, RuntimeArchive,
        },
        trusted_metadata::{METADATA_FILE, TrustedMetadata, verify_with_public_key},
    },
};

const SIGNED_METADATA_URL: &str = "https://raw.githubusercontent.com/MagicCube99/browser-harbor/main/assets/trusted-metadata.json";
const SIGNED_METADATA_SIGNATURE_URL: &str =
    "https://raw.githubusercontent.com/MagicCube99/browser-harbor/main/assets/trusted-metadata.sig";
const TRUSTED_METADATA_CACHE_MAX_AGE_SECONDS: u64 = 24 * 60 * 60;
pub const UPSTREAM_REUSE_SECONDS: u64 = 10 * 60;

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
        release: ReleaseDescriptor,
    },
    SameVersion {
        current_version: String,
        upstream: UpstreamCheck,
        release: ReleaseDescriptor,
    },
    Installed {
        receipt: InstallReceipt,
    },
    ChromePlusChecked {
        current_version: Option<String>,
        latest_version: String,
    },
    ChromePlusInstalled {
        version: String,
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
    pub source: MetadataSource,
    pub checked_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataSource {
    Official,
    SignedManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedReleaseMetadata {
    pub check: UpstreamCheck,
    pub release: ReleaseDescriptor,
    pub observed_at: u64,
}

impl CachedReleaseMetadata {
    pub fn is_fresh(&self) -> bool {
        unix_now().saturating_sub(self.observed_at) <= UPSTREAM_REUSE_SECONDS
    }
}

impl UpstreamCheck {
    pub fn fresh(version: String, channel: ReleaseChannel, cpu_build: CpuBuild) -> Self {
        Self {
            version,
            channel,
            cpu_build,
            source: MetadataSource::Official,
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
    pub chrome_plus_expected_version: Option<String>,
    pub replace_same_version: bool,
    pub cached_release: Option<CachedReleaseMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationMode {
    Check,
    ChromePlusCheck,
    ChromePlusInstall,
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
            Ok(OperationResult::Checked { check, release }) => {
                let _ = sender.send(OperationEvent::new(
                    target,
                    OperationPhase::Checked { check, release },
                ));
            }
            Ok(OperationResult::SameVersion {
                current_version,
                upstream,
                release,
            }) => {
                let _ = sender.send(OperationEvent::new(
                    target,
                    OperationPhase::SameVersion {
                        current_version,
                        upstream,
                        release,
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
            Ok(OperationResult::ChromePlusInstalled { version }) => {
                let _ = sender.send(OperationEvent::new(
                    target,
                    OperationPhase::ChromePlusInstalled { version },
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
        let latest_version =
            resolve_chrome_plus_release(&http, &request.network, &request.cache_dir)
                .await?
                .version;
        return Ok(OperationResult::ChromePlusChecked {
            current_version: request.target_config.chrome_plus_version,
            latest_version,
        });
    }
    if request.mode == OperationMode::ChromePlusInstall {
        let release = resolve_chrome_plus_release(&http, &request.network, &request.cache_dir).await?;
        require_expected_chrome_plus_version(
            request.chrome_plus_expected_version.as_deref(),
            &release.version,
        )?;
        let (direct_client, fallback_client) = HttpClient::download_clients(&request.network)?;
        let downloader = Downloader::with_fallback_clients(
            direct_client,
            fallback_client,
            request.cache_dir.clone(),
            cancellation.clone(),
        );
        let runtime =
            download_chrome_plus_runtime(&downloader, &request, release, &emit).await?;
        let receipt = ChromePlusUpdateService::update(ChromePlusUpdateRequest {
            target,
            config: request.target_config,
            runtime,
        })
        .map_err(Box::<dyn std::error::Error + Send + Sync>::from)?;
        return Ok(OperationResult::ChromePlusInstalled {
            version: receipt.version,
        });
    }
    let provider = ProviderResolver::new(http.clone());
    let mut used_signed_metadata = false;
    let mut metadata_checked_at = None;
    let descriptor = if let Some(cached) = request.cached_release.clone().filter(|cached| {
        cached.is_fresh()
            && cached.release.target == target
            && cached.release.channel == request.target_config.channel
            && cached.release.cpu_build == request.target_config.cpu_build
            && cached.release.artifact.integrity.is_some()
    }) {
        used_signed_metadata = cached.check.source == MetadataSource::SignedManifest;
        metadata_checked_at = Some(cached.check.checked_at);
        cached.release
    } else {
        match provider
            .resolve(
                target,
                request.target_config.channel,
                request.target_config.cpu_build,
            )
            .await
        {
            Ok(descriptor) if descriptor.artifact.integrity.is_some() => descriptor,
            Ok(_) => {
                let metadata = load_trusted_metadata(&http, &request.network, &request.cache_dir)
                    .await
                    .map_err(|error| {
                        format!("官方元数据缺少可信摘要，签名发布清单也不可用：{error}")
                    })?;
                if metadata.is_expired() {
                    emit(OperationPhase::Notice(
                        "签名发布清单已过期，但使用它完成本次安装".to_owned(),
                    ));
                }
                let descriptor = metadata
                    .release(
                        target,
                        request.target_config.channel,
                        request.target_config.cpu_build,
                    )
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "签名发布清单没有 {}/{:?}/{:?} 的完整记录",
                            target.display_name(),
                            request.target_config.channel,
                            request.target_config.cpu_build
                        )
                    })?;
                used_signed_metadata = true;
                metadata_checked_at = Some(metadata.generated_at);
                descriptor
            }
            Err(error) => {
                let metadata = load_trusted_metadata(&http, &request.network, &request.cache_dir)
                    .await
                    .map_err(|signed_error| {
                        format!("官方元数据不可用：{error}；签名发布清单也不可用：{signed_error}")
                    })?;
                if metadata.is_expired() {
                    emit(OperationPhase::Notice(
                        "官方元数据不可用；签名发布清单已过期，但使用它完成本次安装".to_owned(),
                    ));
                } else {
                    emit(OperationPhase::Notice(
                        "官方元数据不可用，使用签名发布清单".to_owned(),
                    ));
                }
                let descriptor = metadata
                    .release(
                        target,
                        request.target_config.channel,
                        request.target_config.cpu_build,
                    )
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "签名发布清单没有 {}/{:?}/{:?} 的完整记录",
                            target.display_name(),
                            request.target_config.channel,
                            request.target_config.cpu_build
                        )
                    })?;
                used_signed_metadata = true;
                metadata_checked_at = Some(metadata.generated_at);
                descriptor
            }
        }
    };
    if cancellation.load(Ordering::Relaxed) {
        return Err(Box::new(crate::services::ServiceError::Cancelled));
    }
    let check = UpstreamCheck {
        version: descriptor.version.clone(),
        channel: descriptor.channel,
        cpu_build: descriptor.cpu_build,
        source: if used_signed_metadata {
            MetadataSource::SignedManifest
        } else {
            MetadataSource::Official
        },
        checked_at: metadata_checked_at.unwrap_or_else(unix_now),
    };
    if request.mode == OperationMode::Check {
        return Ok(OperationResult::Checked {
            check,
            release: descriptor,
        });
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
            release: descriptor,
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

async fn load_trusted_metadata(
    http: &HttpClient,
    network: &NetworkSettings,
    cache_dir: &std::path::Path,
) -> Result<TrustedMetadata, Box<dyn std::error::Error + Send + Sync>> {
    let cached = TrustedMetadata::load_cached(cache_dir).ok().flatten();
    if let Some(metadata) = cached
        .as_ref()
        .filter(|_| cached_metadata_is_recent(cache_dir))
    {
        return Ok(metadata.clone());
    }
    match fetch_trusted_metadata(http, network, cache_dir).await {
        Ok(metadata) => Ok(metadata),
        Err(error) => {
            if let Some(cached) = cached {
                return Ok(cached);
            }
            match TrustedMetadata::load_embedded() {
                Ok(metadata) => Ok(metadata),
                Err(embedded_error) => Err(format!(
                    "live signed metadata failed: {error}; embedded metadata failed: {embedded_error}"
                )
                .into()),
            }
        }
    }
}

fn cached_metadata_is_recent(cache_dir: &std::path::Path) -> bool {
    let Ok(modified) =
        std::fs::metadata(cache_dir.join(METADATA_FILE)).and_then(|metadata| metadata.modified())
    else {
        return false;
    };
    std::time::SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age.as_secs() < TRUSTED_METADATA_CACHE_MAX_AGE_SECONDS)
}

async fn fetch_trusted_metadata(
    http: &HttpClient,
    network: &NetworkSettings,
    cache_dir: &std::path::Path,
) -> Result<TrustedMetadata, Box<dyn std::error::Error + Send + Sync>> {
    let metadata_urls = signed_manifest_candidates(SIGNED_METADATA_URL, network)?;
    let signature_urls = signed_manifest_candidates(SIGNED_METADATA_SIGNATURE_URL, network)?;
    let mut last_error = None;
    for (metadata_url, signature_url) in metadata_urls.iter().zip(&signature_urls) {
        let result = async {
            let metadata = http.get_bytes(metadata_url).await?;
            let signature = http.get_bytes(signature_url).await?;
            let signature = std::str::from_utf8(&signature)?;
            let parsed = verify_with_public_key(
                &metadata,
                signature,
                include_str!("../../assets/metadata-public-key.hex"),
            )?;
            parsed.store(&metadata, signature, cache_dir)?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(parsed)
        }
        .await;
        match result {
            Ok(metadata) => return Ok(metadata),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "no signed metadata candidate succeeded".into()))
}

enum OperationResult {
    Checked {
        check: UpstreamCheck,
        release: ReleaseDescriptor,
    },
    SameVersion {
        current_version: String,
        upstream: UpstreamCheck,
        release: ReleaseDescriptor,
    },
    Installed(InstallReceipt),
    ChromePlusChecked {
        current_version: Option<String>,
        latest_version: String,
    },
    ChromePlusInstalled {
        version: String,
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

fn require_expected_chrome_plus_version(
    expected: Option<&str>,
    actual: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let expected = expected.ok_or("Chrome++ 更新缺少已检查版本")?;
    if expected != actual {
        return Err(format!("Chrome++ 上游版本已从 {expected} 变化为 {actual}，请重新检查").into());
    }
    Ok(())
}

async fn resolve_chrome_plus_release(
    http: &HttpClient,
    network: &NetworkSettings,
    cache_dir: &std::path::Path,
) -> Result<ChromePlusRelease, Box<dyn std::error::Error + Send + Sync>> {
    match runtime::chromeplus::resolve_latest(http).await {
        Ok(release)
            if release.artifact.integrity.is_some()
                && release.setdll_artifact.integrity.is_some() =>
        {
            Ok(release)
        }
        Ok(_) | Err(_) => load_trusted_metadata(http, network, cache_dir)
            .await?
            .chrome_plus
            .ok_or_else(|| "签名发布清单没有 Chrome++ 完整记录".into()),
    }
}

async fn download_chrome_plus_runtime(
    downloader: &Downloader,
    request: &StartOperationRequest,
    release: ChromePlusRelease,
    emit: &(impl Fn(OperationPhase) + Sync),
) -> Result<RuntimeArchive, Box<dyn std::error::Error + Send + Sync>> {
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
    let path = download_artifact(downloader, &descriptor, &request.network, "Chrome++", emit).await?;
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
    Ok(RuntimeArchive {
        archive_path: path,
        version: release.version,
        artifact: release.artifact,
        setdll_archive,
        setdll_artifact: release.setdll_artifact.into(),
    })
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
            let release =
                resolve_chrome_plus_release(http, &request.network, &request.cache_dir).await?;
            Ok((
                Some(download_chrome_plus_runtime(downloader, request, release, emit).await?),
                None,
            ))
        }
        RuntimeKind::Libportable => {
            let release = match runtime::libportable::resolve_latest(http).await {
                Ok(release) if release.artifact.integrity.is_some() => release,
                Ok(_) | Err(_) => load_trusted_metadata(http, &request.network, &request.cache_dir)
                    .await?
                    .libportable
                    .ok_or("签名发布清单没有 Libportable 记录")?,
            };
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
                    artifact: release.artifact,
                    setdll_archive: None,
                    setdll_artifact: None,
                }),
            ))
        }
        _ => Ok((None, None)),
    }
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
                | Self::ChromePlusInstalled { .. }
                | Self::Cancelled
                | Self::Failed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CachedReleaseMetadata, MetadataSource, OperationPhase, UPSTREAM_REUSE_SECONDS,
        require_expected_chrome_plus_version, requires_replacement_confirmation, unix_now,
    };
    use crate::domain::release::{
        ArchiveFormat, Artifact, CpuBuild, Integrity, ProgramPlacement, ProviderKind,
        ReleaseChannel, ReleaseDescriptor,
    };

    fn descriptor() -> ReleaseDescriptor {
        ReleaseDescriptor {
            target: crate::domain::target::TargetId::Brave,
            version: "1.0.0".to_owned(),
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            provider: ProviderKind::GitHubRelease,
            artifact: Artifact {
                file_name: "brave.zip".to_owned(),
                format: ArchiveFormat::Zip,
                size: Some(1),
                integrity: Some(Integrity::Sha256("0".repeat(64))),
                urls: vec!["https://example.com/brave.zip".to_owned()],
                signature_url: None,
            },
            placement: ProgramPlacement::NativePackage,
            inner_archive: None,
            source: "fixture".to_owned(),
            published_at: None,
        }
    }

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
            OperationPhase::ChromePlusInstalled {
                version: "1.18.3".to_owned(),
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
                    source: MetadataSource::Official,
                    checked_at: 0,
                },
                release: descriptor(),
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

    #[test]
    fn chromeplus_update_rejects_a_changed_checked_version() {
        assert!(
            require_expected_chrome_plus_version(Some("1.18.3"), "1.18.3").is_ok()
        );
        assert!(
            require_expected_chrome_plus_version(Some("1.18.3"), "1.18.4").is_err()
        );
        assert!(require_expected_chrome_plus_version(None, "1.18.3").is_err());
    }

    #[test]
    fn cached_release_metadata_is_reused_only_for_ten_minutes() {
        let cached = CachedReleaseMetadata {
            check: super::UpstreamCheck::fresh(
                "1.0.0".to_owned(),
                ReleaseChannel::Stable,
                CpuBuild::X64,
            ),
            release: descriptor(),
            observed_at: unix_now(),
        };
        assert!(cached.is_fresh());

        let stale = CachedReleaseMetadata {
            observed_at: unix_now().saturating_sub(UPSTREAM_REUSE_SECONDS + 1),
            ..cached
        };
        assert!(!stale.is_fresh());
    }
}
