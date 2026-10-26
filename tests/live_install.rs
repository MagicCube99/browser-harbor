use std::sync::{Arc, atomic::AtomicBool};

use browser_harbor::{
    domain::{runtime::ChromePlusConfig, settings::NetworkSettings, target::TargetId},
    infra::ini::read_ini,
    providers::ProviderResolver,
    runtime,
    services::{
        download::Downloader,
        http::HttpClient,
        install::{InstallRequest, InstallService, RuntimeArchive},
        version_probe::locate_executable,
    },
};
use tempfile::tempdir;

#[tokio::test]
#[ignore = "downloads Zen and Libportable, then performs a real injection in a temporary directory"]
async fn installs_zen_with_required_libportable_runtime() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let root = tempdir().unwrap();
    let downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-zen-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let resolver = ProviderResolver::new(http.clone());
    let release = resolver
        .resolve(
            TargetId::Zen,
            TargetId::Zen.spec().default_channel,
            TargetId::Zen.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let browser_artifact = downloader
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap();
    let libportable = runtime::libportable::resolve_latest(&http).await.unwrap();
    let runtime_artifact = downloader
        .download(&libportable.artifact, &network, |_| {})
        .await
        .unwrap();

    let receipt = InstallService::install(InstallRequest {
        release,
        browser_artifact,
        browser_root: root.path().join("Zen"),
        browser_directory: None,
        data_directory: None,
        profile_directory: None,
        cache_directory: None,
        chrome_plus_config: ChromePlusConfig::default(),
        chrome_plus_runtime: None,
        libportable_runtime: Some(RuntimeArchive {
            archive_path: runtime_artifact,
            version: libportable.version,
            setdll_archive: None,
        }),
    })
    .unwrap();

    assert!(receipt.browser_directory.join("zen.exe").is_file());
    assert_eq!(receipt.browser_directory, root.path().join(r"Zen/App/core"));
    assert_eq!(receipt.data_directory, root.path().join("Zen/Profiles"));
    assert!(receipt.browser_directory.join("portable64.dll").is_file());
    assert!(receipt.browser_directory.join("upcheck.exe").is_file());
    assert!(receipt.browser_directory.join("portable.ini").is_file());
    let (portable, _) = read_ini(&receipt.browser_directory.join("portable.ini")).unwrap();
    assert!(portable.contains(r"PortableDataPath=..\..\Profiles"));
    assert!(receipt.data_directory.is_dir());
}

#[tokio::test]
#[ignore = "downloads Chrome and Chrome++, then performs a real portable install in a temporary directory"]
async fn installs_chrome_with_full_chrome_plus_runtime() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let root = tempdir().unwrap();
    let browser_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-chrome-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let runtime_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-chromeplus-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let release = ProviderResolver::new(http.clone())
        .resolve(
            TargetId::Chrome,
            TargetId::Chrome.spec().default_channel,
            TargetId::Chrome.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let browser_artifact = browser_downloader
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap();
    let chrome_plus = runtime::chromeplus::resolve_latest(&http).await.unwrap();
    let runtime_artifact = runtime_downloader
        .download(&chrome_plus.artifact, &network, |_| {})
        .await
        .unwrap();

    let receipt = InstallService::install(InstallRequest {
        release,
        browser_artifact,
        browser_root: root.path().join("Chrome"),
        browser_directory: None,
        data_directory: None,
        profile_directory: None,
        cache_directory: None,
        chrome_plus_config: ChromePlusConfig::default(),
        chrome_plus_runtime: Some(RuntimeArchive {
            archive_path: runtime_artifact,
            version: chrome_plus.version,
            setdll_archive: None,
        }),
        libportable_runtime: None,
    })
    .unwrap();

    assert!(receipt.browser_directory.join("chrome.exe").is_file());
    assert_eq!(receipt.browser_directory, root.path().join("Chrome/App"));
    assert_eq!(receipt.data_directory, root.path().join("Chrome/Profiles"));
    assert!(receipt.browser_directory.join("version.dll").is_file());
    assert!(receipt.browser_directory.join("chrome++.ini").is_file());
    let (ini, _) = read_ini(&receipt.browser_directory.join("chrome++.ini")).unwrap();
    assert!(ini.contains(r"data_dir=%app%\..\Profiles"));
    assert_eq!(receipt.chrome_plus_version.as_deref(), Some("1.18.2"));
}

#[tokio::test]
#[ignore = "downloads Edge and Chrome++, builds a portable Edge tree in a temporary directory"]
async fn installs_edge_from_official_setup_with_chrome_plus() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let root = tempdir().unwrap();
    let browser_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-edge-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let runtime_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-chromeplus-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let release = ProviderResolver::new(http.clone())
        .resolve(
            TargetId::Edge,
            TargetId::Edge.spec().default_channel,
            TargetId::Edge.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let browser_artifact = browser_downloader
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap();
    let chrome_plus = runtime::chromeplus::resolve_latest(&http).await.unwrap();
    let runtime_artifact = runtime_downloader
        .download(&chrome_plus.artifact, &network, |_| {})
        .await
        .unwrap();
    let setdll_artifact = runtime_downloader
        .download(&chrome_plus.setdll_artifact, &network, |_| {})
        .await
        .unwrap();

    let receipt = InstallService::install(InstallRequest {
        release,
        browser_artifact,
        browser_root: root.path().join("Edge"),
        browser_directory: None,
        data_directory: None,
        profile_directory: None,
        cache_directory: None,
        chrome_plus_config: ChromePlusConfig::default(),
        chrome_plus_runtime: Some(RuntimeArchive {
            archive_path: runtime_artifact,
            version: chrome_plus.version,
            setdll_archive: Some(setdll_artifact),
        }),
        libportable_runtime: None,
    })
    .unwrap();

    assert_eq!(
        receipt.browser_directory,
        root.path().join(r"Edge\App\Microsoft\Edge\Application")
    );
    let executable = locate_executable(TargetId::Edge, &receipt.browser_directory).unwrap();
    assert!(executable.is_file());
    let program = executable.parent().unwrap();
    assert_eq!(receipt.data_directory, root.path().join("Edge/Profiles"));
    assert!(program.join("version.dll").is_file());
    let (ini, _) = read_ini(&program.join("chrome++.ini")).unwrap();
    assert!(ini.contains(r"data_dir=%app%\..\..\..\..\..\Profiles"));
}

#[tokio::test]
#[ignore = "downloads and installs all native-package browser targets in temporary directories"]
async fn installs_each_native_package_target() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let resolver = ProviderResolver::new(http.clone());
    let downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-native-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let root = tempdir().unwrap();

    for target in [
        TargetId::Catsxp,
        TargetId::Floorp,
        TargetId::LibreWolf,
        TargetId::SkyKakapo,
        TargetId::Turbo,
    ] {
        let release = resolver
            .resolve(
                target,
                target.spec().default_channel,
                target.spec().default_cpu_build,
            )
            .await
            .unwrap();
        let artifact = downloader
            .download(&release.artifact, &network, |_| {})
            .await
            .unwrap();
        let browser_root = root.path().join(target.display_name());
        let receipt = InstallService::install(InstallRequest {
            release,
            browser_artifact: artifact,
            browser_root: browser_root.clone(),
            browser_directory: None,
            data_directory: None,
            profile_directory: None,
            cache_directory: None,
            chrome_plus_config: ChromePlusConfig::default(),
            chrome_plus_runtime: None,
            libportable_runtime: None,
        })
        .unwrap_or_else(|error| panic!("{} install failed: {error}", target.display_name()));

        if !find_file(
            &receipt.browser_directory,
            &[
                "chrome.exe",
                "catsxp.exe",
                "floorp.exe",
                "librewolf.exe",
                "skykakapo.exe",
                "turbo.exe",
            ],
        ) {
            let top_level = std::fs::read_dir(&browser_root)
                .unwrap()
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .collect::<Vec<_>>();
            panic!(
                "{} executable missing at {}; top-level={top_level:?}",
                target.display_name(),
                receipt.browser_directory.display()
            );
        }
        match target {
            TargetId::Catsxp => {
                assert!(receipt.browser_directory.join("StartupParm.txt").is_file());
                assert_eq!(receipt.data_directory, browser_root.join("Profiles"));
                assert_eq!(receipt.browser_directory, browser_root.join("App"));
            }
            TargetId::Floorp => {
                assert!(browser_root.join("floorp-portable.exe").is_file());
                assert!(browser_root.join("portapp.json").is_file());
                let config =
                    std::fs::read_to_string(browser_root.join("floorp-portable.yml")).unwrap();
                assert!(config.contains("check_for_updates: false"));
            }
            TargetId::LibreWolf => {
                assert!(browser_root.join("Profiles").is_dir());
                assert!(browser_root.join("LibreWolf-Portable.exe").is_file());
            }
            TargetId::SkyKakapo => {
                assert_eq!(receipt.browser_directory, browser_root.join("App"));
                assert!(receipt.browser_directory.join("tmemutil.dll").is_file());
                let ini = std::fs::read_to_string(receipt.browser_directory.join("tmemutil.ini"))
                    .unwrap();
                assert!(ini.contains("Portable=1"));
                assert!(ini.contains(r"PortableDataPath=..\Profiles"));
                assert!(!ini.contains("TmpDataPath"));
                assert_eq!(receipt.profile_directory, browser_root.join("Profiles"));
                assert!(receipt.profile_directory.join("default").is_dir());
                let profiles = std::fs::read_to_string(
                    browser_root.join("Profiles/AppData/SkyKakapo/profiles.ini"),
                )
                .unwrap();
                assert!(profiles.contains("IsRelative=1"));
                assert!(profiles.contains("Path=../../default"));
            }
            TargetId::Turbo => {
                assert_eq!(receipt.browser_directory, browser_root);
                assert!(receipt.browser_directory.join("turbo.exe").is_file());
                assert!(browser_root.join(".portable").is_file());
                assert_eq!(receipt.data_directory, browser_root.join("User Data"));
            }
            _ => {}
        }
    }
}

fn find_file(root: &std::path::Path, names: &[&str]) -> bool {
    walkdir::WalkDir::new(root)
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
#[ignore = "downloads Brave and performs a real Chrome++ Full install"]
async fn installs_brave_with_full_chromeplus() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let root = tempdir().unwrap();
    let browser_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-brave-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let runtime_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-chromeplus-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let release = ProviderResolver::new(http.clone())
        .resolve(
            TargetId::Brave,
            TargetId::Brave.spec().default_channel,
            TargetId::Brave.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let browser_artifact = browser_downloader
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap();
    let chrome_plus = runtime::chromeplus::resolve_latest(&http).await.unwrap();
    let runtime_artifact = runtime_downloader
        .download(&chrome_plus.artifact, &network, |_| {})
        .await
        .unwrap();

    let receipt = InstallService::install(InstallRequest {
        release,
        browser_artifact,
        browser_root: root.path().join("Brave"),
        browser_directory: None,
        data_directory: None,
        profile_directory: None,
        cache_directory: None,
        chrome_plus_config: ChromePlusConfig::default(),
        chrome_plus_runtime: Some(RuntimeArchive {
            archive_path: runtime_artifact,
            version: chrome_plus.version,
            setdll_archive: None,
        }),
        libportable_runtime: None,
    })
    .unwrap();

    assert!(receipt.browser_directory.join("brave.exe").is_file());
    assert_eq!(receipt.browser_directory, root.path().join("Brave/App"));
    assert_eq!(receipt.data_directory, root.path().join("Brave/Profiles"));
    let (ini, _) = read_ini(&receipt.browser_directory.join("chrome++.ini")).unwrap();
    assert!(ini.contains(r"data_dir=%app%\..\Profiles"));
    assert!(ini.contains("[tabs]"));
    assert!(ini.contains("show_password=1"));
}

#[tokio::test]
#[ignore = "downloads Iceweasel and performs a native portable install"]
async fn installs_iceweasel_native_layout() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let root = tempdir().unwrap();
    let downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-iceweasel-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let release = ProviderResolver::new(http)
        .resolve(
            TargetId::Iceweasel,
            TargetId::Iceweasel.spec().default_channel,
            TargetId::Iceweasel.spec().default_cpu_build,
        )
        .await
        .unwrap();
    let browser_artifact = downloader
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap();
    let browser_root = root.path().join("Iceweasel");

    let receipt = InstallService::install(InstallRequest {
        release,
        browser_artifact,
        browser_root: browser_root.clone(),
        browser_directory: None,
        data_directory: None,
        profile_directory: None,
        cache_directory: None,
        chrome_plus_config: ChromePlusConfig::default(),
        chrome_plus_runtime: None,
        libportable_runtime: None,
    })
    .unwrap();

    assert!(find_file(
        &receipt.browser_directory,
        &["iceweasel.exe", "firefox.exe"]
    ));
    assert!(receipt.data_directory.is_dir());
    assert_eq!(receipt.browser_directory, browser_root.join("App"));
    assert_eq!(receipt.data_directory, browser_root.join("Profiles"));
    let (portable, _) = read_ini(&receipt.browser_directory.join("portable.ini")).unwrap();
    assert!(portable.contains(r"PortableDataPath=..\Profiles"));
    assert!(portable.lines().any(|line| line.trim() == "TmpDataPath="));
}

#[tokio::test]
#[ignore = "downloads and installs Firefox, Helium, Thorium and Ungoogled Chromium"]
async fn installs_remaining_generic_targets() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let resolver = ProviderResolver::new(http.clone());
    let root = tempdir().unwrap();

    for target in [
        TargetId::Firefox,
        TargetId::Helium,
        TargetId::Thorium,
        TargetId::UngoogledChromium,
    ] {
        let browser_downloader = Downloader::new(
            HttpClient::download_client(&network).unwrap(),
            std::env::temp_dir().join(format!("pbm-live-{}-cache", target.sort_key())),
            Arc::new(AtomicBool::new(false)),
        );
        let release = resolver
            .resolve(
                target,
                target.spec().default_channel,
                target.spec().default_cpu_build,
            )
            .await
            .unwrap();
        let browser_artifact = browser_downloader
            .download(&release.artifact, &network, |_| {})
            .await
            .unwrap();

        let runtime_downloader = Downloader::new(
            HttpClient::download_client(&network).unwrap(),
            std::env::temp_dir().join("pbm-live-chromeplus-cache"),
            Arc::new(AtomicBool::new(false)),
        );
        let (chrome_plus_runtime, libportable_runtime) = match target.spec().runtime {
            browser_harbor::domain::runtime::RuntimeKind::Libportable => {
                let release = runtime::libportable::resolve_latest(&http).await.unwrap();
                let path = runtime_downloader
                    .download(&release.artifact, &network, |_| {})
                    .await
                    .unwrap();
                (
                    None,
                    Some(RuntimeArchive {
                        archive_path: path,
                        version: release.version,
                        setdll_archive: None,
                    }),
                )
            }
            _ => {
                let release = runtime::chromeplus::resolve_latest(&http).await.unwrap();
                let path = runtime_downloader
                    .download(&release.artifact, &network, |_| {})
                    .await
                    .unwrap();
                (
                    Some(RuntimeArchive {
                        archive_path: path,
                        version: release.version,
                        setdll_archive: None,
                    }),
                    None,
                )
            }
        };

        let browser_root = root.path().join(target.display_name());
        let receipt = InstallService::install(InstallRequest {
            release,
            browser_artifact,
            browser_root: browser_root.clone(),
            browser_directory: None,
            data_directory: None,
            profile_directory: None,
            cache_directory: None,
            chrome_plus_config: ChromePlusConfig::default(),
            chrome_plus_runtime,
            libportable_runtime,
        })
        .unwrap_or_else(|error| panic!("{} install failed: {error}", target.display_name()));

        match target {
            TargetId::Firefox => {
                assert_eq!(receipt.browser_directory, browser_root.join("App"));
                assert_eq!(receipt.data_directory, browser_root.join("Profiles"));
                assert!(receipt.browser_directory.join("firefox.exe").is_file());
                assert!(receipt.browser_directory.join("portable64.dll").is_file());
                assert!(receipt.browser_directory.join("portable.ini").is_file());
                let (portable, _) =
                    read_ini(&receipt.browser_directory.join("portable.ini")).unwrap();
                assert!(portable.contains(r"PortableDataPath=..\Profiles"));
                assert!(portable.lines().any(|line| line.trim() == "TmpDataPath="));
            }
            TargetId::Helium => {
                assert_eq!(receipt.browser_directory, browser_root.join("App"));
                assert_eq!(receipt.data_directory, browser_root.join("Profiles"));
                assert!(find_file(
                    &receipt.browser_directory,
                    &["helium.exe", "chrome.exe"]
                ));
                let (ini, _) = read_ini(&receipt.browser_directory.join("chrome++.ini")).unwrap();
                assert!(ini.contains(r"data_dir=%app%\..\Profiles"));
                assert!(ini.contains("[tabs]"));
                assert!(ini.contains("show_password=1"));
            }
            TargetId::Thorium => {
                assert_eq!(receipt.browser_directory, browser_root.join("BIN"));
                assert!(receipt.browser_directory.join("thorium.exe").is_file());
                assert_eq!(receipt.data_directory, browser_root.join("USER_DATA"));
                assert!(receipt.data_directory.is_dir());
                assert!(browser_root.join("THORIUM.BAT").is_file());
                let (ini, _) = read_ini(&receipt.browser_directory.join("chrome++.ini")).unwrap();
                assert!(ini.contains(r"data_dir=%app%\..\USER_DATA"));
                assert!(ini.contains("[tabs]"));
            }
            TargetId::UngoogledChromium => {
                assert_eq!(receipt.browser_directory, browser_root.join("App"));
                assert_eq!(receipt.data_directory, browser_root.join("Profiles"));
                assert!(receipt.browser_directory.join("chrome.exe").is_file());
                let (ini, _) = read_ini(&receipt.browser_directory.join("chrome++.ini")).unwrap();
                assert!(ini.contains(r"data_dir=%app%\..\Profiles"));
                assert!(ini.contains("[tabs]"));
                assert!(ini.contains("show_password=1"));
            }
            _ => {}
        }
    }
}
