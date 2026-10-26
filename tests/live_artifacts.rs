use std::{path::Path, sync::Arc};

use browser_harbor::{
    domain::{settings::NetworkSettings, target::TargetId},
    providers::ProviderResolver,
    runtime,
    services::{archive::ArchiveService, download::Downloader, http::HttpClient},
};
use tempfile::tempdir;
use walkdir::WalkDir;

#[tokio::test]
#[ignore = "downloads official runtime and browser artifacts"]
async fn downloads_and_extracts_runtime_and_iceweasel_payloads() {
    let http = HttpClient::new(&NetworkSettings::default()).unwrap();
    let downloader = Downloader::new(
        HttpClient::download_client(&NetworkSettings::default()).unwrap(),
        tempdir().unwrap().keep(),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let directory = tempdir().unwrap();
    let network = NetworkSettings::default();

    for (label, artifact) in [
        (
            "chromeplus",
            runtime::chromeplus::resolve_latest(&http)
                .await
                .unwrap()
                .artifact,
        ),
        (
            "libportable",
            runtime::libportable::resolve_latest(&http)
                .await
                .unwrap()
                .artifact,
        ),
    ] {
        let path = downloader
            .download(&artifact, &network, |_| {})
            .await
            .unwrap();
        let output = directory.path().join(label);
        ArchiveService::extract(&path, artifact.format, &output).unwrap();
        assert!(
            WalkDir::new(output)
                .into_iter()
                .filter_map(Result::ok)
                .any(|entry| { entry.file_type().is_file() })
        );
    }

    let release = ProviderResolver::new(http.clone())
        .resolve(
            TargetId::Iceweasel,
            TargetId::Iceweasel.spec().default_channel,
            TargetId::Iceweasel.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let artifact = downloader
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap();
    let output = directory.path().join("iceweasel");
    ArchiveService::extract(&artifact, release.artifact.format, &output).unwrap();
    assert!(contains_executable(
        &output,
        &["iceweasel.exe", "firefox.exe"]
    ));
}

fn contains_executable(root: &Path, names: &[&str]) -> bool {
    assert!(root.is_dir(), "{} was not extracted", root.display());
    WalkDir::new(root)
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
        .any(|entry| {
            entry.file_type().is_file()
                && names.iter().any(|name| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(name)
                })
        })
}

#[tokio::test]
#[ignore = "downloads and extracts the official Microsoft Edge MSI"]
async fn downloads_and_extracts_edge_enterprise_msi() {
    let http = HttpClient::new(&NetworkSettings::default()).unwrap();
    let directory = tempdir().unwrap();
    let downloader = Downloader::new(
        HttpClient::download_client(&NetworkSettings::default()).unwrap(),
        std::env::temp_dir().join("pbm-live-edge-cache"),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let release = ProviderResolver::new(http)
        .resolve(
            TargetId::Edge,
            TargetId::Edge.spec().default_channel,
            TargetId::Edge.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let artifact = downloader
        .download(&release.artifact, &NetworkSettings::default(), |_| {})
        .await
        .unwrap();
    let output = directory.path().join("edge");
    ArchiveService::extract(&artifact, release.artifact.format, &output).unwrap();
    let inner = WalkDir::new(&output)
        .max_depth(5)
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("MSEDGE.7Z")
        })
        .expect("MSEDGE.7Z")
        .into_path();
    let inner_output = directory.path().join("edge-inner");
    ArchiveService::extract(
        &inner,
        browser_harbor::domain::release::ArchiveFormat::SevenZip,
        &inner_output,
    )
    .unwrap();

    assert!(contains_executable(&inner_output, &["msedge.exe"]));
}

#[tokio::test]
#[ignore = "downloads and extracts the official Google Chrome offline installer"]
async fn downloads_and_extracts_chrome_installer() {
    let http = HttpClient::new(&NetworkSettings::default()).unwrap();
    let directory = tempdir().unwrap();
    let downloader = Downloader::new(
        HttpClient::download_client(&NetworkSettings::default()).unwrap(),
        std::env::temp_dir().join("pbm-live-chrome-cache"),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let release = ProviderResolver::new(http)
        .resolve(
            TargetId::Chrome,
            TargetId::Chrome.spec().default_channel,
            TargetId::Chrome.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let artifact = downloader
        .download(&release.artifact, &NetworkSettings::default(), |_| {})
        .await
        .unwrap();
    let output = directory.path().join("chrome");
    ArchiveService::extract(&artifact, release.artifact.format, &output).unwrap();

    assert!(contains_executable(&output, &["chrome.exe"]));
}

#[tokio::test]
#[ignore = "downloads LibreWolf and verifies its detached OpenPGP signature"]
async fn verifies_librewolf_openpgp_signature() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-native-cache"),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let release = ProviderResolver::new(http.clone())
        .resolve(
            TargetId::LibreWolf,
            TargetId::LibreWolf.spec().default_channel,
            TargetId::LibreWolf.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let artifact = downloader
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap();
    let signature_url = release.artifact.signature_url.as_ref().unwrap();
    let signature = http.get_bytes(signature_url).await.unwrap();
    let content = std::fs::read(artifact).unwrap();

    assert!(
        browser_harbor::providers::verify_artifact_signature(
            TargetId::LibreWolf,
            &content,
            &signature
        )
        .unwrap()
    );
}
