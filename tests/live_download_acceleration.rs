//! Small, signed runtime download only; never installs or executes its contents.
use browser_harbor::{
    domain::settings::NetworkSettings,
    services::{
        ServiceError,
        download::{Downloader, verify_artifact},
        http::HttpClient,
        trusted_metadata::TrustedMetadata,
    },
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[tokio::test]
#[ignore = "downloads a small signed official runtime via current accelerators; requires live network"]
async fn signed_runtime_supports_cancel_retry_and_verified_cache_reuse() {
    let metadata = TrustedMetadata::load_embedded().unwrap();
    let artifact = metadata.chrome_plus.unwrap().artifact;
    assert!(artifact.size.is_some_and(|size| size < 2 * 1024 * 1024));
    let dir = tempfile::tempdir().unwrap();
    let network = NetworkSettings::default();
    let (direct, fallback) = HttpClient::download_clients(&network).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let first = Downloader::with_fallback_clients(
        direct.clone(),
        fallback.clone(),
        dir.path().into(),
        cancel.clone(),
    );
    let started = Instant::now();
    let interrupted = tokio::time::timeout(
        Duration::from_secs(120),
        first.download(&artifact, &network, |p| {
            if p.downloaded > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        }),
    )
    .await
    .unwrap();
    assert!(
        matches!(interrupted, Err(ServiceError::Cancelled)),
        "expected an intentional cancellation, got {interrupted:?}"
    );
    let initial_elapsed = started.elapsed();
    let second = Downloader::with_fallback_clients(
        direct,
        fallback,
        dir.path().into(),
        Arc::new(AtomicBool::new(false)),
    );
    let mut first_progress = None;
    let started = Instant::now();
    let path = tokio::time::timeout(
        Duration::from_secs(120),
        second.download(&artifact, &network, |p| {
            first_progress.get_or_insert(p.downloaded);
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let retry_elapsed = started.elapsed();
    verify_artifact(&path, &artifact).unwrap();
    let mut unreachable = artifact.clone();
    unreachable.urls = vec!["https://invalid.invalid/cache-hit-must-not-connect".into()];
    let started = Instant::now();
    let cached = tokio::time::timeout(
        Duration::from_secs(2),
        second.download(&unreachable, &network, |_| {}),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(cached, path);
    eprintln!(
        "signed runtime: bytes={:?}, cancel={initial_elapsed:?}, retry={retry_elapsed:?}, first_retry_progress={first_progress:?}, verified_cache={:?}",
        artifact.size,
        started.elapsed()
    );
}
