use std::sync::{Arc, atomic::AtomicBool};

use browser_harbor::{
    domain::{
        release::ReleaseChannel, runtime::ChromePlusConfig, settings::NetworkSettings,
        target::TargetId,
    },
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
#[ignore = "downloads the signed Catsxp Beta ZIP and verifies portable installation without launching it"]
async fn installs_catsxp_beta_and_recognizes_the_real_build_after_reload() {
    use browser_harbor::{
        app::AppState,
        config::ConfigStore,
        domain::{
            release::CpuBuild,
            settings::{Settings, TargetConfig},
        },
        services::{catsxp, trusted_metadata::TrustedMetadata, version_probe::probe_local_version},
    };
    let metadata = TrustedMetadata::load_embedded().unwrap();
    let release = metadata
        .release(TargetId::Catsxp, ReleaseChannel::Beta, CpuBuild::X64)
        .expect("signed Catsxp Beta entry is required")
        .clone();
    let directory = tempdir().unwrap();
    let network = NetworkSettings::default();
    let artifact = if let Some(path) = std::env::var_os("BROWSER_HARBOR_CATSXP_PACKAGE") {
        path.into()
    } else {
        let (direct, fallback) = HttpClient::download_clients(&network).unwrap();
        Downloader::with_fallback_clients(
            direct,
            fallback,
            directory.path().join("cache"),
            Arc::new(AtomicBool::new(false)),
        )
        .download(&release.artifact, &network, |_| {})
        .await
        .unwrap()
    };
    let root = directory.path().join("Catsxp Beta");
    let request = InstallRequest {
        release,
        browser_artifact: artifact,
        browser_root: root.clone(),
        browser_directory: None,
        data_directory: None,
        profile_directory: None,
        cache_directory: None,
        chrome_plus_config: ChromePlusConfig::default(),
        chrome_plus_runtime: None,
        libportable_runtime: None,
    };
    let receipt = InstallService::install(request.clone()).unwrap();
    assert_eq!(receipt.browser_directory, root.join("App"));
    assert_eq!(receipt.data_directory, root.join("Profiles"));
    let build = receipt
        .catsxp_build
        .as_ref()
        .expect("a real Catsxp package must establish its file identity");
    let raw = probe_local_version(TargetId::Catsxp, &receipt.browser_directory)
        .unwrap()
        .version;
    assert_eq!(
        catsxp::base_version(&build.version, build.channel).as_deref(),
        Some(raw.as_str())
    );
    assert_ne!(build.version, raw);
    let mut settings = Settings::default();
    let mut config = TargetConfig::for_target(TargetId::Catsxp);
    config.browser_root = Some(root.clone());
    config.derive_paths_from_root(TargetId::Catsxp);
    settings.targets.insert(TargetId::Catsxp, config);
    let config_file = directory.path().join("BrowserHarbor.json");
    let mut state = AppState::new(settings, ConfigStore::new(config_file.clone()));
    state
        .record_install_success(TargetId::Catsxp, &receipt)
        .unwrap();
    let loaded = ConfigStore::new(config_file).load().unwrap();
    let persisted = loaded.targets[&TargetId::Catsxp]
        .catsxp_build
        .as_ref()
        .unwrap();
    let observation = catsxp::observe(
        &receipt.browser_directory,
        Some(persisted),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(observation.build.channel, ReleaseChannel::Beta);
    assert_eq!(
        browser_harbor::domain::version::compare_versions(
            &observation.build.version,
            &request.release.version
        ),
        browser_harbor::domain::version::VersionRelation::Same
    );
    std::fs::write(receipt.data_directory.join("keep-user-data.txt"), b"keep").unwrap();
    let ini_path = receipt.browser_directory.join("StartupParm.txt");
    let mut ini = std::fs::read_to_string(&ini_path).unwrap();
    ini.push_str("\r\n--custom-user-flag\r\n");
    std::fs::write(&ini_path, ini).unwrap();
    let second = InstallService::install(request).unwrap();
    assert_eq!(
        std::fs::read(second.data_directory.join("keep-user-data.txt")).unwrap(),
        b"keep"
    );
    assert!(
        std::fs::read_to_string(&ini_path)
            .unwrap()
            .contains("--custom-user-flag")
    );
    let moved = directory.path().join("Moved Catsxp");
    std::fs::rename(&root, &moved).unwrap();
    assert!(
        catsxp::observe(
            &moved.join("App"),
            second.catsxp_build.as_ref(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_some()
    );
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(moved.join("App").join(raw).join("chrome.dll"))
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert!(
        catsxp::observe(
            &moved.join("App"),
            second.catsxp_build.as_ref(),
            &AtomicBool::new(false)
        )
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
#[ignore = "downloads newly enabled Beta/ESR channels and their portable runtimes"]
async fn installs_new_release_channels() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let resolver = ProviderResolver::new(http.clone());
    let root = tempdir().unwrap();
    let browser_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-channel-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let runtime_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-channel-runtime-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let chrome_plus = runtime::chromeplus::resolve_latest(&http).await.unwrap();
    let chrome_plus_archive = runtime_downloader
        .download(&chrome_plus.artifact, &network, |_| {})
        .await
        .unwrap();
    let setdll_archive = runtime_downloader
        .download(&chrome_plus.setdll_artifact, &network, |_| {})
        .await
        .unwrap();
    let libportable = runtime::libportable::resolve_latest(&http).await.unwrap();
    let libportable_archive = runtime_downloader
        .download(&libportable.artifact, &network, |_| {})
        .await
        .unwrap();

    for target in [TargetId::Chrome, TargetId::Edge, TargetId::Brave] {
        let release = resolver
            .resolve(
                target,
                ReleaseChannel::Beta,
                target.spec().default_cpu_build,
            )
            .await
            .unwrap();
        assert_eq!(release.channel, ReleaseChannel::Beta);
        let artifact = browser_downloader
            .download(&release.artifact, &network, |_| {})
            .await
            .unwrap();
        let receipt = InstallService::install(InstallRequest {
            release,
            browser_artifact: artifact,
            browser_root: root.path().join(target.display_name()),
            browser_directory: None,
            data_directory: None,
            profile_directory: None,
            cache_directory: None,
            chrome_plus_config: ChromePlusConfig::default(),
            chrome_plus_runtime: Some(RuntimeArchive {
                archive_path: chrome_plus_archive.clone(),
                version: chrome_plus.version.clone(),
                artifact: chrome_plus.artifact.clone(),
                setdll_archive: (target == TargetId::Edge).then(|| setdll_archive.clone()),
                setdll_artifact: (target == TargetId::Edge)
                    .then(|| chrome_plus.setdll_artifact.clone()),
            }),
            libportable_runtime: None,
        })
        .unwrap_or_else(|error| {
            panic!(
                "{} Beta install failed: {error}\n{error:?}",
                target.display_name()
            )
        });

        locate_executable(target, &receipt.browser_directory).unwrap_or_else(|error| {
            panic!("{} Beta executable missing: {error}", target.display_name())
        });
        assert!(receipt.data_directory.is_dir());
        assert_eq!(
            receipt.chrome_plus_version.as_deref(),
            Some(chrome_plus.version.as_str())
        );
    }

    for channel in [ReleaseChannel::Esr, ReleaseChannel::Beta] {
        let release = resolver
            .resolve(
                TargetId::Firefox,
                channel,
                TargetId::Firefox.spec().default_cpu_build,
            )
            .await
            .unwrap();
        assert_eq!(release.channel, channel);
        let artifact = browser_downloader
            .download(&release.artifact, &network, |_| {})
            .await
            .unwrap();
        let browser_root = root
            .path()
            .join(format!("Firefox-{}", channel.display_name()));
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
            libportable_runtime: Some(RuntimeArchive {
                archive_path: libportable_archive.clone(),
                version: libportable.version.clone(),
                artifact: libportable.artifact.clone(),
                setdll_archive: None,
                setdll_artifact: None,
            }),
        })
        .unwrap_or_else(|error| {
            panic!("Firefox {} install failed: {error}", channel.display_name())
        });

        assert_eq!(receipt.browser_directory, browser_root.join("App"));
        assert!(receipt.browser_directory.join("portable64.dll").is_file());
        assert!(receipt.browser_directory.join("portable.ini").is_file());
    }
}

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
            artifact: libportable.artifact,
            setdll_archive: None,
            setdll_artifact: None,
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
            artifact: chrome_plus.artifact,
            setdll_archive: None,
            setdll_artifact: None,
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
            artifact: chrome_plus.artifact.clone(),
            setdll_archive: Some(setdll_artifact),
            setdll_artifact: Some(chrome_plus.setdll_artifact),
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
    let runtime_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-libportable-cache"),
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
        let libportable_runtime = if target.spec().runtime.uses_libportable() {
            let runtime = runtime::libportable::resolve_latest(&http).await.unwrap();
            let path = runtime_downloader
                .download(&runtime.artifact, &network, |_| {})
                .await
                .unwrap();
            Some(RuntimeArchive {
                archive_path: path,
                version: runtime.version,
                artifact: runtime.artifact,
                setdll_archive: None,
                setdll_artifact: None,
            })
        } else {
            None
        };
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
            libportable_runtime,
        })
        .unwrap_or_else(|error| {
            panic!(
                "{} install failed: {error}\n{error:?}",
                target.display_name()
            )
        });

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
                assert!(!browser_root.join("floorp-portable.exe").exists());
                assert!(browser_root.join("portapp.json").is_file());
                assert!(receipt.browser_directory.join("portable.ini").is_file());
                assert!(receipt.browser_directory.join("portable64.dll").is_file());
                assert_eq!(
                    receipt.profile_directory,
                    browser_root.join(r"data\profile\default")
                );
                let portable =
                    std::fs::read_to_string(receipt.browser_directory.join("portable.ini"))
                        .unwrap();
                assert!(portable.contains(r"PortableDataPath=..\data\profile\default"));
                let config =
                    std::fs::read_to_string(browser_root.join("floorp-portable.yml")).unwrap();
                assert!(config.contains("check_for_updates: false"));
            }
            TargetId::LibreWolf => {
                for name in [
                    "LibreWolf-Portable.exe",
                    "LibreWolf-WinUpdater.exe",
                    "LibreWolf-WinUpdater.ini",
                    "ScheduledTask-Create.ps1",
                    "ScheduledTask-Remove.ps1",
                ] {
                    assert!(!browser_root.join(name).exists(), "{name} remained");
                }
                assert!(receipt.browser_directory.join("portable.ini").is_file());
                assert!(receipt.browser_directory.join("portable64.dll").is_file());
                assert_eq!(
                    receipt.profile_directory,
                    browser_root.join(r"Profiles\Default")
                );
                let portable =
                    std::fs::read_to_string(receipt.browser_directory.join("portable.ini"))
                        .unwrap();
                assert!(portable.contains(r"PortableDataPath=..\Profiles\Default"));
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
            artifact: chrome_plus.artifact,
            setdll_archive: None,
            setdll_artifact: None,
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
#[ignore = "downloads Floorp, LibreWolf and Libportable into temporary directories"]
async fn installs_floorp_and_librewolf_with_direct_libportable_runtime() {
    let network = NetworkSettings::default();
    let http = HttpClient::new(&network).unwrap();
    let resolver = ProviderResolver::new(http.clone());
    let browser_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-firefox-direct-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let runtime_downloader = Downloader::new(
        HttpClient::download_client(&network).unwrap(),
        std::env::temp_dir().join("pbm-live-libportable-cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let root = tempdir().unwrap();
    let runtime = runtime::libportable::resolve_latest(&http).await.unwrap();
    let runtime_artifact = runtime_downloader
        .download(&runtime.artifact, &network, |_| {})
        .await
        .unwrap();

    for (target, expected_profile, expected_path) in [
        (
            TargetId::Floorp,
            r"data\profile\default",
            r"PortableDataPath=..\data\profile\default",
        ),
        (
            TargetId::LibreWolf,
            r"Profiles\Default",
            r"PortableDataPath=..\Profiles\Default",
        ),
    ] {
        let release = resolver
            .resolve(
                target,
                target.spec().default_channel,
                target.spec().default_cpu_build,
            )
            .await
            .unwrap();
        let artifact = browser_downloader
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
            libportable_runtime: Some(RuntimeArchive {
                archive_path: runtime_artifact.clone(),
                version: runtime.version.clone(),
                artifact: runtime.artifact.clone(),
                setdll_archive: None,
                setdll_artifact: None,
            }),
        })
        .unwrap_or_else(|error| {
            panic!(
                "{} install failed: {error}\n{error:?}",
                target.display_name()
            )
        });

        assert!(receipt.browser_directory.join("portable.ini").is_file());
        assert!(receipt.browser_directory.join("portable64.dll").is_file());
        assert!(receipt.browser_directory.join("upcheck.exe").is_file());
        if target == TargetId::Floorp {
            assert!(!browser_root.join("floorp-portable.exe").exists());
        } else {
            assert!(!browser_root.join("LibreWolf-Portable.exe").exists());
            assert!(!browser_root.join("LibreWolf-WinUpdater.exe").exists());
        }
        assert_eq!(
            receipt.profile_directory,
            browser_root.join(expected_profile)
        );
        let portable =
            std::fs::read_to_string(receipt.browser_directory.join("portable.ini")).unwrap();
        assert!(portable.contains(expected_path));
    }
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
                        artifact: release.artifact,
                        setdll_archive: None,
                        setdll_artifact: None,
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
                        artifact: release.artifact,
                        setdll_archive: None,
                        setdll_artifact: None,
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

#[tokio::test]
#[ignore = "downloads and installs the Vivaldi package from the verified embedded manifest"]
async fn installs_vivaldi_from_signed_manifest() {
    let directory = tempdir().unwrap();
    let browser_root = directory.path().join("Vivaldi");
    let network = NetworkSettings::default();
    let metadata =
        browser_harbor::services::trusted_metadata::TrustedMetadata::load_embedded().unwrap();
    let descriptor = metadata
        .release(
            TargetId::Vivaldi,
            ReleaseChannel::Stable,
            browser_harbor::domain::release::CpuBuild::X64,
        )
        .unwrap()
        .clone();
    let (direct, fallback) = HttpClient::download_clients(&network).unwrap();
    let downloader = Downloader::with_fallback_clients(
        direct,
        fallback,
        directory.path().join("cache"),
        Arc::new(AtomicBool::new(false)),
    );
    let local_file = downloader
        .download(&descriptor.artifact, &network, |_| {})
        .await
        .unwrap();

    let receipt = InstallService::install(InstallRequest {
        release: descriptor,
        browser_artifact: local_file,
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

    assert_eq!(receipt.browser_directory, browser_root.join("App"));
    assert_eq!(receipt.data_directory, browser_root.join("User Data"));
    assert!(receipt.browser_directory.join("vivaldi.exe").is_file());
    assert!(receipt.browser_directory.join("stp.viv").is_file());
    assert!(
        !receipt
            .browser_directory
            .join("update_notifier.exe")
            .exists()
    );
}
