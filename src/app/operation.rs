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

const SIGNED_METADATA_URL: &str = "https://raw.githubusercontent.com/MagicCube99/browser-harbor/metadata-live/assets/trusted-metadata.json";
const SIGNED_METADATA_SIGNATURE_URL: &str = "https://raw.githubusercontent.com/MagicCube99/browser-harbor/metadata-live/assets/trusted-metadata.sig";
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
    SignedCache,
    Embedded,
}

impl MetadataSource {
    pub fn is_signed(self) -> bool {
        self != Self::Official
    }
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
            && (self.release.target != TargetId::Edge
                || (self.check.source == MetadataSource::Official
                    && self.release.artifact.format
                        == crate::domain::release::ArchiveFormat::SevenZip
                    && self.release.inner_archive.as_deref() == Some("MSEDGE.7Z")))
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
    pub allow_local_package: bool,
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
    if matches!(
        request.mode,
        OperationMode::Install | OperationMode::ChromePlusInstall
    ) {
        InstallService::prepare_retry(target, &request.target_config)?;
    }
    emit(OperationPhase::Resolving);
    let http = HttpClient::new(&request.network)?.with_operation(Arc::clone(&cancellation));
    if request.mode == OperationMode::ChromePlusCheck {
        let latest_version =
            resolve_chrome_plus_release(&http, &request.network, &request.cache_dir, true)
                .await?
                .version;
        return Ok(OperationResult::ChromePlusChecked {
            current_version: request.target_config.chrome_plus_version,
            latest_version,
        });
    }
    if request.mode == OperationMode::ChromePlusInstall {
        let release =
            resolve_chrome_plus_release(&http, &request.network, &request.cache_dir, true).await?;
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
        let runtime = download_chrome_plus_runtime(&downloader, &request, release, &emit).await?;
        let receipt = ChromePlusUpdateService::update(ChromePlusUpdateRequest {
            target,
            config: request.target_config,
            runtime,
        })
        .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> {
            match error {
                crate::services::install::InstallError::BrowserRunning(path) => format!(
                    "浏览器仍在运行，请手工关闭所有实例后再更新 Chrome++：{}",
                    path.display()
                )
                .into(),
                crate::services::install::InstallError::Io { path, source }
                    if source.kind() == std::io::ErrorKind::PermissionDenied
                        || source.raw_os_error() == Some(5) =>
                {
                    format!(
                        "文件操作受系统权限保护（{}）。请将浏览器根目录移至普通用户目录（如 D 盘或 C:\\Tools），或右键以管理员身份运行。",
                        path.display()
                    )
                    .into()
                }
                error => error.into(),
            }
        })?;
        return Ok(OperationResult::ChromePlusInstalled {
            version: receipt.version,
        });
    }
    let provider = ProviderResolver::new(http.clone());
    let mut metadata_source = MetadataSource::Official;
    let mut metadata_checked_at = None;
    let mut descriptor = if let Some(cached) = request.cached_release.clone().filter(|cached| {
        request.mode != OperationMode::Check
            && cached.is_fresh()
            && cached.release.target == target
            && cached.release.channel == request.target_config.channel
            && cached.release.cpu_build == request.target_config.cpu_build
            && cached.release.artifact.integrity.is_some()
    }) {
        metadata_source = cached.check.source;
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
            Ok(official) => {
                let (metadata, source) = load_trusted_metadata_for_release(
                    &http,
                    &request.network,
                    &request.cache_dir,
                    (
                        target,
                        request.target_config.channel,
                        request.target_config.cpu_build,
                    ),
                    true,
                    Some(&official.version),
                )
                .await
                .map_err(|error| {
                    format!(
                        "已发现 {} 新版 {}，等待校验信息：{error}",
                        target.display_name(),
                        official.version
                    )
                })?;
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
                metadata_source = source;
                metadata_checked_at = Some(metadata.generated_at);
                descriptor
            }
            Err(error) => {
                let (metadata, source) = load_trusted_metadata_for_release(
                    &http,
                    &request.network,
                    &request.cache_dir,
                    (
                        target,
                        request.target_config.channel,
                        request.target_config.cpu_build,
                    ),
                    request.mode == OperationMode::Check,
                    None,
                )
                .await
                .map_err(|signed_error| {
                    format!("官方元数据不可用：{error}；签名发布清单也不可用：{signed_error}")
                })?;
                emit(OperationPhase::Notice(
                    "官方元数据不可用，使用已验证签名快照".to_owned(),
                ));
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
                metadata_source = source;
                metadata_checked_at = Some(metadata.generated_at);
                descriptor
            }
        }
    };
    if descriptor.artifact.size.is_none()
        && let Ok(embedded) = TrustedMetadata::load_embedded()
    {
        embedded.complete_release_size(&mut descriptor);
    }
    if cancellation.load(Ordering::Relaxed) {
        return Err(Box::new(crate::services::ServiceError::Cancelled));
    }
    let check = UpstreamCheck {
        version: descriptor.version.clone(),
        channel: descriptor.channel,
        cpu_build: descriptor.cpu_build,
        source: metadata_source,
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

    if target.spec().runtime == crate::domain::runtime::RuntimeKind::Libportable
        && !runtime::libportable::is_compatible(target, &descriptor.version)
    {
        return Err(format!(
            "Libportable 暂不支持 {} 版本 {}，请等待适配或选择兼容版本。",
            target.display_name(),
            descriptor.version
        )
        .into());
    }
    let (direct_client, fallback_client) = HttpClient::download_clients(&request.network)?;
    let downloader = Downloader::with_fallback_clients(
        direct_client,
        fallback_client,
        request.cache_dir.clone(),
        cancellation.clone(),
    );
    emit(OperationPhase::Preparing);
    let browser_artifact = if let Some(local) = request
        .target_config
        .local_package
        .as_ref()
        .filter(|_| request.allow_local_package)
    {
        if !local.is_file() {
            return Err(format!("本地安装包不存在：{}", local.display()).into());
        }
        verify_local_package(local, &descriptor)?;
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
    // Downloads can take hours; the next metadata phase gets its own resolution budget.
    let http = http.renew_resolution_budget();
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
    .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> {
        match error {
            crate::services::install::InstallError::Io { path, source }
                if source.kind() == std::io::ErrorKind::PermissionDenied
                    || source.raw_os_error() == Some(5) =>
            {
                format!(
                    "文件操作受系统权限保护（{}）。请将浏览器根目录移至普通用户目录（如 D 盘或 C:\\Tools），或右键以管理员身份运行。",
                    path.display()
                )
                .into()
            }
            crate::services::install::InstallError::Runtime(
                crate::runtime::RuntimeError::UnsupportedLibportableVersion { product, version },
            ) => format!(
                "Libportable 暂不支持 {product} 版本 {version}，请等待适配或选择兼容版本。"
            )
            .into(),
            crate::services::install::InstallError::Runtime(error) => {
                format!("便携运行时准备失败：{error}").into()
            }
            error => Box::new(error),
        }
    })?;
    Ok(OperationResult::Installed(receipt))
}

fn verify_local_package(
    path: &std::path::Path,
    descriptor: &ReleaseDescriptor,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let expected_package = format!(
        "{} {} {} 的 {} {}",
        descriptor.target.display_name(),
        descriptor.channel.display_name(),
        descriptor.cpu_build.display_name(),
        descriptor.version,
        descriptor.artifact.file_name
    );
    match crate::services::download::verify_artifact(path, &descriptor.artifact) {
        Ok(()) => Ok(()),
        Err(crate::services::ServiceError::SizeMismatch {
            expected: expected_bytes,
            actual: actual_bytes,
        }) => Err(format!(
            "本地安装包与当前目标不匹配：应为 {expected_package}；文件大小应为 {expected_bytes} 字节，实际为 {actual_bytes} 字节。"
        )
        .into()),
        Err(crate::services::ServiceError::HashMismatch { .. }) => Err(format!(
            "本地安装包摘要与当前目标不匹配：请确认版本、渠道和构建，并选择 {expected_package}。"
        )
        .into()),
        Err(crate::services::ServiceError::Io { .. }) => Err(format!(
            "无法读取本地安装包：{}",
            path.display()
        )
        .into()),
        Err(error) => Err(format!("本地安装包校验失败：{error}").into()),
    }
}

fn requires_replacement_confirmation(
    replace_same_version: bool,
    current: &str,
    upstream: &str,
) -> bool {
    !replace_same_version
        && matches!(
            compare_versions(current, upstream),
            crate::domain::version::VersionRelation::Same
                | crate::domain::version::VersionRelation::UpstreamOlder
        )
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
    force_refresh: bool,
) -> Result<TrustedMetadata, Box<dyn std::error::Error + Send + Sync>> {
    select_verified_metadata(
        TrustedMetadata::load_cached(cache_dir).ok().flatten(),
        cached_metadata_is_recent(cache_dir),
        force_refresh,
        |_| true,
        fetch_trusted_metadata(http, network, cache_dir, |_| true),
    )
    .await
    .map(|(metadata, _)| metadata)
}

async fn load_trusted_metadata_for_release(
    http: &HttpClient,
    network: &NetworkSettings,
    cache_dir: &std::path::Path,
    (target, channel, cpu_build): (TargetId, ReleaseChannel, CpuBuild),
    force_refresh: bool,
    expected_version: Option<&str>,
) -> Result<(TrustedMetadata, MetadataSource), Box<dyn std::error::Error + Send + Sync>> {
    if target == TargetId::Edge {
        return Err(
            "Edge 下载信息需从 Microsoft 实时获取，不能使用旧 MSI 签名记录；请检查网络后重试"
                .into(),
        );
    }
    let accepts = |metadata: &TrustedMetadata| {
        metadata
            .release(target, channel, cpu_build)
            .is_some_and(|release| {
                expected_version.is_none_or(|version| {
                    signed_version_matches_discovery(version, &release.version)
                })
            })
    };
    select_verified_metadata(
        TrustedMetadata::load_cached(cache_dir).ok().flatten(),
        cached_metadata_is_recent(cache_dir),
        force_refresh,
        &accepts,
        fetch_trusted_metadata(http, network, cache_dir, &accepts),
    )
    .await
}

fn signed_version_matches_discovery(discovered: &str, signed: &str) -> bool {
    matches!(
        compare_versions(discovered, signed),
        crate::domain::version::VersionRelation::Same
            | crate::domain::version::VersionRelation::UpstreamNewer
    )
}

async fn select_verified_metadata(
    cached: Option<TrustedMetadata>,
    recent: bool,
    force_refresh: bool,
    accepts: impl Fn(&TrustedMetadata) -> bool,
    live: impl std::future::Future<
        Output = Result<TrustedMetadata, Box<dyn std::error::Error + Send + Sync>>,
    >,
) -> Result<(TrustedMetadata, MetadataSource), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(metadata) = cached
        .as_ref()
        .filter(|metadata| !force_refresh && recent && accepts(metadata))
    {
        return Ok((metadata.clone(), MetadataSource::SignedCache));
    }
    let error = match live.await {
        Ok(metadata) if accepts(&metadata) => {
            return Ok((metadata, MetadataSource::SignedManifest));
        }
        Ok(_) => "live signed metadata has no compatible release entry".to_owned(),
        Err(error)
            if error
                .downcast_ref::<crate::services::ServiceError>()
                .is_some_and(|error| matches!(error, crate::services::ServiceError::Cancelled)) =>
        {
            return Err(error);
        }
        Err(error) => error.to_string(),
    };
    if let Some(cached) = cached.filter(&accepts) {
        return Ok((cached, MetadataSource::SignedCache));
    }
    match TrustedMetadata::load_embedded() {
        Ok(metadata) if accepts(&metadata) => Ok((metadata, MetadataSource::Embedded)),
        Ok(_) => Err(format!("live signed metadata failed: {error}; cached and embedded metadata have no compatible release entry").into()),
        Err(embedded_error) => Err(format!(
            "live signed metadata failed: {error}; embedded metadata failed: {embedded_error}"
        ).into()),
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
    accepts: impl Fn(&TrustedMetadata) -> bool,
) -> Result<TrustedMetadata, Box<dyn std::error::Error + Send + Sync>> {
    let metadata_urls = signed_manifest_candidates(SIGNED_METADATA_URL, network)?;
    let signature_urls = signed_manifest_candidates(SIGNED_METADATA_SIGNATURE_URL, network)?;
    let mut last_error = None;
    for (metadata_url, signature_url) in metadata_urls.iter().zip(&signature_urls) {
        let result = fetch_trusted_metadata_candidate(
            http,
            metadata_url,
            signature_url,
            cache_dir,
            &accepts,
        )
        .await;
        match result {
            Ok(metadata) => return Ok(metadata),
            Err(error)
                if error
                    .downcast_ref::<crate::services::ServiceError>()
                    .is_some_and(|error| {
                        matches!(error, crate::services::ServiceError::Cancelled)
                    }) =>
            {
                return Err(error);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "no signed metadata candidate succeeded".into()))
}

async fn fetch_trusted_metadata_candidate(
    http: &HttpClient,
    metadata_url: &str,
    signature_url: &str,
    cache_dir: &std::path::Path,
    accepts: &impl Fn(&TrustedMetadata) -> bool,
) -> Result<TrustedMetadata, Box<dyn std::error::Error + Send + Sync>> {
    let (metadata, signature) =
        tokio::try_join!(http.get_bytes(metadata_url), http.get_bytes(signature_url),)?;
    let signature = std::str::from_utf8(&signature)?;
    let parsed = verify_with_public_key(
        &metadata,
        signature,
        include_str!("../../assets/metadata-public-key.hex"),
    )?;
    if !accepts(&parsed) {
        return Err("signed metadata mirror has no compatible release entry".into());
    }
    parsed.store(&metadata, signature, cache_dir)?;
    Ok(parsed)
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
    force_refresh: bool,
) -> Result<ChromePlusRelease, Box<dyn std::error::Error + Send + Sync>> {
    match runtime::chromeplus::resolve_latest(http).await {
        Ok(release)
            if release.artifact.integrity.is_some()
                && release.setdll_artifact.integrity.is_some() =>
        {
            Ok(release)
        }
        Ok(_) | Err(_) => load_trusted_metadata(http, network, cache_dir, force_refresh)
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
    let path =
        download_artifact(downloader, &descriptor, &request.network, "Chrome++", emit).await?;
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
                resolve_chrome_plus_release(http, &request.network, &request.cache_dir, false)
                    .await?;
            if request.chrome_plus_expected_version.is_some() {
                require_expected_chrome_plus_version(
                    request.chrome_plus_expected_version.as_deref(),
                    &release.version,
                )?;
            }
            Ok((
                Some(download_chrome_plus_runtime(downloader, request, release, emit).await?),
                None,
            ))
        }
        RuntimeKind::Libportable => {
            let release = match runtime::libportable::resolve_latest(http).await {
                Ok(release) if release.artifact.integrity.is_some() => release,
                Ok(_) | Err(_) => {
                    load_trusted_metadata(http, &request.network, &request.cache_dir, false)
                        .await?
                        .libportable
                        .ok_or("签名发布清单没有 Libportable 记录")?
                }
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
    #[test]
    fn discovery_gate_accepts_newer_verified_metadata_but_never_older() {
        assert!(super::signed_version_matches_discovery(
            "8.2.4133.94",
            "8.2.4133.96"
        ));
        assert!(super::signed_version_matches_discovery(
            "8.2.4133.96",
            "8.2.4133.96"
        ));
        assert!(!super::signed_version_matches_discovery(
            "8.2.4133.96",
            "8.2.4133.94"
        ));
        assert!(!super::signed_version_matches_discovery("1.0a", "1.0b"));
    }
    #[tokio::test]
    async fn cancelled_chromeplus_check_does_not_report_a_cached_success() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        metadata
            .store(
                include_bytes!("../../assets/trusted-metadata.json"),
                include_str!("../../assets/trusted-metadata.sig"),
                directory.path(),
            )
            .unwrap();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let network = crate::domain::settings::NetworkSettings::default();
        let http = crate::services::http::HttpClient::new(&network)
            .unwrap()
            .with_operation(cancelled);
        let result =
            super::resolve_chrome_plus_release(&http, &network, directory.path(), true).await;
        assert!(
            result.is_err(),
            "cancelled checks must not finish successfully from cache"
        );
        assert!(result.unwrap_err().to_string().contains("cancelled"));
    }
    use std::fs;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    use super::{
        CachedReleaseMetadata, MetadataSource, OperationPhase, UPSTREAM_REUSE_SECONDS,
        require_expected_chrome_plus_version, requires_replacement_confirmation, unix_now,
        verify_local_package,
    };
    use crate::domain::release::{
        ArchiveFormat, Artifact, CpuBuild, Integrity, ProgramPlacement, ProviderKind,
        ReleaseChannel, ReleaseDescriptor,
    };

    fn serve_signed_pair(signature: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let started = Instant::now();
            let mut pending = Vec::new();
            while pending.len() < 2 && started.elapsed() < Duration::from_secs(3) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut request = [0; 2048];
                        let size = socket.read(&mut request).unwrap();
                        let is_signature = String::from_utf8_lossy(&request[..size])
                            .starts_with("GET /signature ");
                        pending.push((socket, is_signature));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("local metadata server: {error}"),
                }
            }
            // Neither response is sent until both requests arrive.
            for (mut socket, is_signature) in pending {
                let body = if is_signature {
                    signature.as_slice()
                } else {
                    include_bytes!("../../assets/trusted-metadata.json").as_slice()
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if socket.write_all(header.as_bytes()).is_ok() {
                    let _ = socket.write_all(body);
                }
            }
        });
        (format!("http://{address}"), server)
    }

    #[tokio::test]
    async fn signed_metadata_requests_are_concurrent_and_verified_before_caching() {
        let (url, server) =
            serve_signed_pair(include_bytes!("../../assets/trusted-metadata.sig").to_vec());
        let directory = tempfile::tempdir().unwrap();
        let http = crate::services::http::HttpClient::new(
            &crate::domain::settings::NetworkSettings::default(),
        )
        .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            super::fetch_trusted_metadata_candidate(
                &http,
                &format!("{url}/metadata"),
                &format!("{url}/signature"),
                directory.path(),
                &|_| true,
            ),
        )
        .await;
        server.join().unwrap();
        let metadata = result
            .expect("both requests must be in flight together")
            .unwrap();
        assert_eq!(
            crate::services::trusted_metadata::TrustedMetadata::load_cached(directory.path())
                .unwrap(),
            Some(metadata)
        );
    }

    #[tokio::test]
    async fn concurrently_fetched_invalid_signature_is_never_cached() {
        let (url, server) = serve_signed_pair(b"0".repeat(128));
        let directory = tempfile::tempdir().unwrap();
        let http = crate::services::http::HttpClient::new(
            &crate::domain::settings::NetworkSettings::default(),
        )
        .unwrap();
        let result = super::fetch_trusted_metadata_candidate(
            &http,
            &format!("{url}/metadata"),
            &format!("{url}/signature"),
            directory.path(),
            &|_| true,
        )
        .await;
        server.join().unwrap();
        assert!(result.is_err());
        assert!(!directory.path().join(super::METADATA_FILE).exists());
        assert!(
            !directory
                .path()
                .join(crate::services::trusted_metadata::SIGNATURE_FILE)
                .exists()
        );
    }

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

    #[tokio::test]
    async fn recent_manifest_missing_vivaldi_refreshes_instead_of_returning_incomplete_cache() {
        let mut cached =
            crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        cached.releases.clear();
        let mut live = cached.clone();
        let mut release = descriptor();
        release.target = crate::domain::target::TargetId::Vivaldi;
        live.releases.push(release);
        let selected = super::select_verified_metadata(
            Some(cached),
            true,
            false,
            |m| {
                m.release(
                    crate::domain::target::TargetId::Vivaldi,
                    ReleaseChannel::Stable,
                    CpuBuild::X64,
                )
                .is_some()
            },
            async { Ok(live) },
        )
        .await
        .unwrap();
        assert!(
            selected
                .0
                .release(
                    crate::domain::target::TargetId::Vivaldi,
                    ReleaseChannel::Stable,
                    CpuBuild::X64
                )
                .is_some()
        );
    }

    #[tokio::test]
    async fn incomplete_cache_does_not_hide_complete_embedded_metadata_when_offline() {
        let embedded = crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        let mut cached = embedded.clone();
        cached.releases.clear();
        let selected = super::select_verified_metadata(
            Some(cached),
            true,
            false,
            |m| {
                m.release(
                    crate::domain::target::TargetId::Vivaldi,
                    ReleaseChannel::Stable,
                    CpuBuild::X64,
                )
                .is_some()
            },
            async { Err("offline".into()) },
        )
        .await
        .unwrap();
        assert!(
            selected
                .0
                .release(
                    crate::domain::target::TargetId::Vivaldi,
                    ReleaseChannel::Stable,
                    CpuBuild::X64
                )
                .is_some()
        );
    }

    #[tokio::test]
    async fn edge_rejects_historical_signed_msi_even_when_fetched_from_live_manifest() {
        let mut cached =
            crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        let mut edge = descriptor();
        edge.target = crate::domain::target::TargetId::Edge;
        edge.artifact.format = ArchiveFormat::Msi;
        cached.releases = vec![edge];
        let selected = super::select_verified_metadata(
            Some(cached.clone()),
            true,
            false,
            |m| {
                m.release(
                    crate::domain::target::TargetId::Edge,
                    ReleaseChannel::Stable,
                    CpuBuild::X64,
                )
                .is_some_and(|r| r.artifact.format == ArchiveFormat::SevenZip)
            },
            async { Ok(cached) },
        )
        .await;
        assert!(selected.is_err());
    }

    #[tokio::test]
    async fn edge_official_failure_never_falls_back_to_cached_msi() {
        let directory = tempfile::tempdir().unwrap();
        let http = crate::services::http::HttpClient::new(
            &crate::domain::settings::NetworkSettings::default(),
        )
        .unwrap();
        let result = super::load_trusted_metadata_for_release(
            &http,
            &crate::domain::settings::NetworkSettings::default(),
            directory.path(),
            (
                crate::domain::target::TargetId::Edge,
                ReleaseChannel::Stable,
                CpuBuild::X64,
            ),
            true,
            None,
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("Microsoft"));
    }

    #[test]
    fn local_package_mismatch_identifies_the_expected_target() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wrong-package.zip");
        fs::write(&path, b"two bytes").unwrap();

        let error = verify_local_package(&path, &descriptor())
            .unwrap_err()
            .to_string();

        assert!(error.contains("Brave Stable x64"));
        assert!(error.contains("1.0.0"));
        assert!(error.contains("brave.zip"));
        assert!(error.contains("文件大小"));
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
    fn older_upstream_requires_explicit_replacement_confirmation() {
        assert!(requires_replacement_confirmation(false, "160.0", "156.0"));
        assert!(!requires_replacement_confirmation(true, "160.0", "156.0"));
    }

    #[tokio::test]
    async fn manual_refresh_does_not_reuse_a_complete_recent_manifest() {
        let cached = crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        let mut live = cached.clone();
        live.generated_at = cached.generated_at + 1;
        let selected =
            super::select_verified_metadata(Some(cached.clone()), true, true, |_| true, async {
                Ok(live)
            })
            .await
            .unwrap();
        assert_eq!(selected.0.generated_at, cached.generated_at + 1);
        assert_eq!(selected.1, MetadataSource::SignedManifest);
    }

    #[tokio::test]
    async fn failed_refresh_keeps_the_cache_source_and_original_generation_time() {
        let mut cached =
            crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        cached.generated_at = 123;
        let (metadata, source) =
            super::select_verified_metadata(Some(cached), true, true, |_| true, async {
                Err("offline".into())
            })
            .await
            .unwrap();
        assert_eq!(metadata.generated_at, 123);
        assert_eq!(source, MetadataSource::SignedCache);
    }

    #[tokio::test]
    async fn a_new_official_version_cannot_select_an_older_signed_snapshot() {
        let mut cached =
            crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        let mut old = descriptor();
        old.version = "1.0".to_owned();
        cached.releases = vec![old.clone()];
        let mut live = cached.clone();
        old.version = "2.0".to_owned();
        live.releases = vec![old];
        let accepts = |metadata: &crate::services::trusted_metadata::TrustedMetadata| {
            metadata
                .release(
                    crate::domain::target::TargetId::Brave,
                    ReleaseChannel::Stable,
                    CpuBuild::X64,
                )
                .is_some_and(|release| release.version == "2.0")
        };
        let (selected, source) =
            super::select_verified_metadata(Some(cached.clone()), true, true, accepts, async {
                Ok(live)
            })
            .await
            .unwrap();
        assert_eq!(selected.releases[0].version, "2.0");
        assert_eq!(source, MetadataSource::SignedManifest);
        let result =
            super::select_verified_metadata(Some(cached.clone()), true, true, accepts, async {
                Ok(cached)
            })
            .await;
        assert!(
            result.is_err(),
            "an older signed release must not hide the newly discovered official release"
        );
    }

    #[tokio::test]
    async fn unsupported_libportable_is_rejected_before_using_a_browser_package() {
        let directory = tempfile::tempdir().unwrap();
        let target = crate::domain::target::TargetId::Firefox;
        let mut release = descriptor();
        release.target = target;
        release.version = "999.0".to_owned();
        let mut config = crate::domain::settings::TargetConfig::for_target(target);
        config.browser_root = Some(directory.path().join("browser-root"));
        config.derive_paths_from_root(target);
        config.local_package = Some(directory.path().join("missing-package.zip"));
        let (sender, _) = std::sync::mpsc::channel();
        let result = super::run_operation(
            super::StartOperationRequest {
                target,
                mode: super::OperationMode::Install,
                target_config: config,
                network: crate::domain::settings::NetworkSettings::default(),
                cache_dir: directory.path().to_path_buf(),
                current_version: None,
                chrome_plus_expected_version: None,
                replace_same_version: false,
                allow_local_package: true,
                cached_release: Some(CachedReleaseMetadata {
                    check: super::UpstreamCheck::fresh(
                        "999.0".to_owned(),
                        ReleaseChannel::Stable,
                        CpuBuild::X64,
                    ),
                    release,
                    observed_at: unix_now(),
                }),
            },
            sender,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .await;
        let error = result
            .err()
            .expect("unsupported browser must be rejected")
            .to_string();
        assert!(error.contains("Libportable"), "{error}");
        assert!(error.contains("999.0"), "{error}");
    }

    #[tokio::test]
    async fn install_preflight_rejects_unsafe_paths_before_creating_http_clients() {
        let directory = tempfile::tempdir().unwrap();
        let target = crate::domain::target::TargetId::Firefox;
        let mut config = crate::domain::settings::TargetConfig::for_target(target);
        let root = directory.path().join("browser-root");
        config.browser_root = Some(root.clone());
        config.browser_directory = Some(root.join("App"));
        config.data_directory = Some(root.join("App/private-profile"));
        let (sender, _) = std::sync::mpsc::channel();
        let result = super::run_operation(
            super::StartOperationRequest {
                target,
                mode: super::OperationMode::Install,
                target_config: config,
                network: crate::domain::settings::NetworkSettings {
                    proxy: Some("://".to_owned()),
                    ..Default::default()
                },
                cache_dir: directory.path().join("cache"),
                current_version: None,
                chrome_plus_expected_version: None,
                replace_same_version: false,
                allow_local_package: false,
                cached_release: None,
            },
            sender,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .await;
        let error = result
            .err()
            .expect("unsafe paths must be rejected")
            .to_string();
        assert!(error.contains("inside the program directory"), "{error}");
        assert!(!root.exists());
        assert!(!directory.path().join("cache").exists());
    }

    #[tokio::test]
    async fn disabled_local_packages_ignore_a_persisted_selection() {
        use sha2::{Digest, Sha256};
        let directory = tempfile::tempdir().unwrap();
        let bytes = b"verified browser artifact";
        let digest = hex::encode(Sha256::digest(bytes));
        let cache = directory.path().join(format!("{digest}.artifact"));
        fs::write(&cache, bytes).unwrap();
        let mut release = descriptor();
        release.artifact.size = Some(bytes.len() as u64);
        release.artifact.integrity = Some(Integrity::Sha256(digest));
        // Stop at signature URL validation after selecting and verifying the browser artifact.
        release.artifact.signature_url = Some("://".to_owned());
        let target = release.target;
        let mut config = crate::domain::settings::TargetConfig::for_target(target);
        config.browser_root = Some(directory.path().join("browser-root"));
        config.derive_paths_from_root(target);
        config.local_package = Some(directory.path().join("missing-local-package.zip"));
        let (sender, _) = std::sync::mpsc::channel();
        let result = super::run_operation(
            super::StartOperationRequest {
                target,
                mode: super::OperationMode::Install,
                target_config: config,
                network: Default::default(),
                cache_dir: directory.path().to_path_buf(),
                current_version: None,
                chrome_plus_expected_version: None,
                replace_same_version: false,
                allow_local_package: false,
                cached_release: Some(CachedReleaseMetadata {
                    check: super::UpstreamCheck::fresh(
                        "1.0.0".to_owned(),
                        ReleaseChannel::Stable,
                        CpuBuild::X64,
                    ),
                    release,
                    observed_at: unix_now(),
                }),
            },
            sender,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .await;
        assert_eq!(
            result
                .err()
                .expect("signature URL must be rejected")
                .to_string(),
            "URL is invalid"
        );
        assert_eq!(fs::read(cache).unwrap(), bytes);
    }

    #[test]
    fn chromeplus_update_rejects_a_changed_checked_version() {
        assert!(require_expected_chrome_plus_version(Some("1.18.3"), "1.18.3").is_ok());
        assert!(require_expected_chrome_plus_version(Some("1.18.3"), "1.18.4").is_err());
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

    #[test]
    fn edge_install_cannot_reuse_an_msi_or_signed_fallback_check() {
        let mut cached = CachedReleaseMetadata {
            check: super::UpstreamCheck::fresh(
                "156.0.4314.50".to_owned(),
                ReleaseChannel::Stable,
                CpuBuild::X64,
            ),
            release: descriptor(),
            observed_at: unix_now(),
        };
        cached.release.target = crate::domain::target::TargetId::Edge;
        cached.release.artifact.format = ArchiveFormat::Msi;
        assert!(!cached.is_fresh());
        cached.release.artifact.format = ArchiveFormat::SevenZip;
        cached.release.inner_archive = Some("MSEDGE.7Z".to_owned());
        cached.check.source = MetadataSource::SignedManifest;
        assert!(!cached.is_fresh());
        cached.check.source = MetadataSource::Official;
        assert!(cached.is_fresh());
    }

    #[tokio::test]
    async fn recent_complete_metadata_cache_avoids_a_redundant_fetch() {
        let cached = crate::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
        let selected =
            super::select_verified_metadata(Some(cached.clone()), true, false, |_| true, async {
                panic!("a complete recent cache must be reused")
            })
            .await
            .unwrap();
        assert_eq!(selected, (cached, MetadataSource::SignedCache));
    }

    #[tokio::test]
    #[ignore = "contacts official Vivaldi and signed metadata endpoints"]
    async fn checks_vivaldi_with_an_empty_metadata_cache() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, _) = std::sync::mpsc::channel();
        let result = super::run_operation(
            super::StartOperationRequest {
                target: crate::domain::target::TargetId::Vivaldi,
                mode: super::OperationMode::Check,
                target_config: crate::domain::settings::TargetConfig::for_target(
                    crate::domain::target::TargetId::Vivaldi,
                ),
                network: crate::domain::settings::NetworkSettings::default(),
                cache_dir: directory.path().to_path_buf(),
                current_version: None,
                chrome_plus_expected_version: None,
                replace_same_version: false,
                allow_local_package: false,
                cached_release: None,
            },
            sender,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
        .await
        .unwrap();
        let super::OperationResult::Checked { check, release } = result else {
            panic!("expected a completed version check")
        };
        assert_eq!(check.source, MetadataSource::SignedManifest);
        assert_eq!(release.target, crate::domain::target::TargetId::Vivaldi);
        assert!(release.artifact.integrity.is_some());
        assert!(release.artifact.size.is_some_and(|size| size > 0));
        assert!(
            crate::services::trusted_metadata::TrustedMetadata::load_cached(directory.path())
                .unwrap()
                .is_some_and(|m| m
                    .release(
                        crate::domain::target::TargetId::Vivaldi,
                        ReleaseChannel::Stable,
                        CpuBuild::X64
                    )
                    .is_some())
        );
    }
}
