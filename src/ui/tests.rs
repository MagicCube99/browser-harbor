use std::{
    fs,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::Arc,
};

use eframe::egui::accesskit::Role;
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use egui_system_font_provider::SystemFontProvider;
use tempfile::tempdir;

use super::{
    BODY_FONT_SIZE, BrowserHarborApp, DetailTab, LIGHT_PALETTE, ProgramIdentity, StatusTone,
    chrome_plus_update_button_label, chrome_plus_update_hover, configure_context,
    create_auto_shortcut, installed_channel_label, relation_status, set_language,
};
use crate::{
    app::{
        AppState,
        operation::{MetadataSource, UpstreamCheck},
    },
    config::ConfigStore,
    domain::{
        release::{CpuBuild, ReleaseChannel},
        settings::{Language, Settings, ThemeMode},
        target::TargetId,
    },
    infra::ini::{IniEncoding, read_ini, write_ini},
};

fn harness_for(app: BrowserHarborApp) -> Harness<'static, BrowserHarborApp> {
    Harness::builder()
        .with_size(eframe::egui::vec2(560.0, 420.0))
        .with_context_setup(|ctx| {
            ctx.add_font_provider(Arc::new(SystemFontProvider::new()));
            configure_context(ctx, ThemeMode::System);
        })
        .build_eframe(move |_cc| app)
}

struct TestHarness {
    harness: Harness<'static, BrowserHarborApp>,
    _directory: tempfile::TempDir,
}

impl Deref for TestHarness {
    type Target = Harness<'static, BrowserHarborApp>;

    fn deref(&self) -> &Self::Target {
        &self.harness
    }
}

impl DerefMut for TestHarness {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.harness
    }
}

fn harness() -> TestHarness {
    let directory = tempdir().unwrap();
    let state = AppState::new(
        Settings::default(),
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    TestHarness {
        harness: harness_for(BrowserHarborApp::new(state)),
        _directory: directory,
    }
}

#[test]
fn ui_harness_removes_auto_saved_config_after_drop() {
    let marker = format!(
        r"D:\browser-harbor-tempdir-lifecycle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    {
        let mut harness = harness();
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "浏览器根目录")
            .set_text_value_accesskit(&marker);
        harness.run();
    }

    let leaked = fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
        .filter_map(|entry| fs::read(entry.path().join("BrowserHarbor.json")).ok())
        .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .any(|value| {
            value
                .pointer("/targets/firefox/browserRoot")
                .and_then(serde_json::Value::as_str)
                == Some(marker.as_str())
        });

    assert!(
        !leaked,
        "UI test config survived after its TempDir owner was dropped"
    );
}

fn firefox_harness_with_versions(
    local_version: &str,
    upstream_version: &str,
) -> (tempfile::TempDir, Harness<'static, BrowserHarborApp>) {
    let directory = tempdir().unwrap();
    let browser = directory.path().join("Firefox");
    let program = browser.join("App");
    fs::create_dir_all(&program).unwrap();
    fs::write(program.join("firefox.exe"), b"fixture").unwrap();
    fs::write(
        program.join("application.ini"),
        format!("[App]\r\nName=Firefox\r\nVersion={local_version}\r\n"),
    )
    .unwrap();

    let mut settings = Settings {
        selected_browser: TargetId::Firefox,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Firefox).unwrap();
    config.browser_root = Some(browser);
    config.browser_directory = Some(program);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.state_mut().latest_versions.insert(
        (TargetId::Firefox, ReleaseChannel::Stable, CpuBuild::X64),
        UpstreamCheck {
            version: upstream_version.to_owned(),
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            source: MetadataSource::Official,
            checked_at: 0,
        },
    );
    (directory, harness)
}

#[test]
fn browser_combo_uses_two_ordered_groups_with_sorted_members() {
    let mut harness = harness();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();

    let list = harness.get_by_role(Role::ListBox);
    let chromium = list.get_by_label("Chromium 系");
    let firefox = list.get_by_label("Firefox 系");
    assert_eq!(chromium.accesskit_node().role(), Role::Label);
    assert_eq!(firefox.accesskit_node().role(), Role::Label);
    assert!(chromium.rect().min.x < firefox.rect().min.x);

    let chromium_members = [
        "Brave",
        "Catsxp",
        "Chrome",
        "Edge",
        "Helium",
        "Thorium",
        "Turbo",
        "Ungoogled Chromium",
        "Vivaldi",
    ];
    let firefox_members = [
        "Firefox",
        "Floorp",
        "Iceweasel",
        "LibreWolf",
        "SkyKakapo",
        "Zen",
    ];
    let mut previous_y = chromium.rect().min.y;
    for member in chromium_members {
        let y = list.get_by_label(member).rect().min.y;
        assert!(y > previous_y, "{member} is out of order");
        previous_y = y;
    }
    let mut previous_y = firefox.rect().min.y;
    for member in firefox_members {
        let y = list.get_by_label(member).rect().min.y;
        assert!(y > previous_y, "{member} is out of order");
        previous_y = y;
    }
}

#[test]
fn selecting_chrome_changes_the_configured_target() {
    let mut harness = harness();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("Chrome")
        .click();

    harness.run();

    assert_eq!(harness.state().state().selected_target(), TargetId::Chrome);
    assert_eq!(
        harness.state().state().saved().selected_browser,
        TargetId::Chrome
    );
    harness.get_by_label("Chrome++");
}

#[test]
fn switching_targets_keeps_each_browser_paths_isolated() {
    let directory = tempdir().unwrap();
    let tool_root = directory.path().join("Browser");
    let thorium_root = tool_root.join("Thorium_AVX512");
    let mut settings = Settings {
        selected_browser: TargetId::Thorium,
        ..Settings::default()
    };
    let thorium = settings.targets.get_mut(&TargetId::Thorium).unwrap();
    thorium.browser_root = Some(thorium_root.clone());
    thorium.browser_directory = Some(thorium_root.join("BIN"));
    thorium.data_directory = Some(thorium_root.join("USER_DATA"));
    thorium.profile_directory = Some(thorium_root.join("USER_DATA"));
    thorium.cache_directory = None;
    let state = AppState::with_tool_root(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
        tool_root.clone(),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    for (target, expected_name) in [
        (TargetId::Iceweasel, "Iceweasel"),
        (TargetId::SkyKakapo, "SkyKakapo"),
    ] {
        harness
            .get_by_role_and_label(Role::ComboBox, "浏览器")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label(expected_name)
            .click();
        harness.run();

        assert_eq!(harness.state().state().saved().selected_browser, target);
        let config = &harness.state().state().saved().targets[&target];
        assert_eq!(
            config.browser_root,
            Some(PathBuf::from(format!(r".\{expected_name}")))
        );
        assert_eq!(
            config.browser_directory,
            Some(PathBuf::from(format!(r".\{expected_name}\App")))
        );
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "浏览器根目录")
                .accesskit_node()
                .value(),
            Some(format!(r".\{expected_name}"))
        );
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "程序目录")
                .accesskit_node()
                .value(),
            Some(format!(r".\{expected_name}\App"))
        );
    }

    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Thorium].browser_root,
        Some(thorium_root)
    );
}

#[test]
fn failed_target_switch_keeps_the_previous_browser_selected() {
    let directory = tempdir().unwrap();
    let config_path = directory.path().join("BrowserHarbor.json");
    fs::create_dir(directory.path().join("BrowserHarbor.json.tmp")).unwrap();
    let state = AppState::new(Settings::default(), ConfigStore::new(config_path));
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("Chrome")
        .click();
    harness.run();

    assert_eq!(harness.state().state().selected_target(), TargetId::Firefox);
    assert_eq!(
        harness.state().state().saved().selected_browser,
        TargetId::Firefox
    );
    assert!(harness.state().state().last_error().is_some());
}

#[test]
fn mismatched_browser_identity_requires_install_confirmation() {
    let mut harness = harness();
    harness.run();
    harness.state_mut().program_identity = ProgramIdentity::Mismatch {
        actual: TargetId::Helium,
        product_name: "Helium".to_owned(),
    };
    harness.run();

    harness.get_by_label("下载并安装").click();
    harness.run();

    harness.get_by_label("浏览器身份不一致");
    harness.get_by_label("仍然继续");
    harness.get_by_label("重新选择目录");
    assert!(harness.state().identity_prompt.is_some());
}

#[test]
fn installed_channel_is_inferred_before_replacing_program_files() {
    assert_eq!(
        installed_channel_label(TargetId::Chrome, "158.0.1", "Google Chrome Beta"),
        Some("Beta")
    );
    assert_eq!(
        installed_channel_label(TargetId::Edge, "157.0.1", "Microsoft Edge Beta"),
        Some("Beta")
    );
    assert_eq!(
        installed_channel_label(TargetId::Brave, "1.100.1", "Brave Nightly"),
        Some("Nightly")
    );
    assert_eq!(
        installed_channel_label(TargetId::Brave, "1.99.1", "Brave Beta"),
        Some("Beta")
    );
    assert_eq!(
        installed_channel_label(TargetId::Firefox, "153.6.0esr", "Firefox"),
        Some("ESR")
    );
    assert_eq!(
        installed_channel_label(TargetId::Firefox, "160.0b2", "Firefox"),
        Some("Beta")
    );
    assert_eq!(
        installed_channel_label(TargetId::Firefox, "159.0", "Firefox"),
        Some("Stable")
    );
    assert_eq!(
        installed_channel_label(TargetId::Iceweasel, "115.44.0esr", "Iceweasel"),
        Some("ESR")
    );
    assert_eq!(
        installed_channel_label(TargetId::Iceweasel, "159.0", "Iceweasel"),
        Some("Release")
    );
}

#[test]
fn channel_mismatch_shows_warning_chip_and_switch_action() {
    let directory = tempdir().unwrap();
    let root = directory.path().join("Iceweasel");
    let program = root.join("App");
    fs::create_dir_all(&program).unwrap();
    fs::write(program.join("iceweasel.exe"), b"fixture").unwrap();
    fs::write(
        program.join("application.ini"),
        "[App]\r\nName=Iceweasel\r\nVersion=159.0\r\n",
    )
    .unwrap();

    let mut settings = Settings {
        selected_browser: TargetId::Iceweasel,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Iceweasel).unwrap();
    config.browser_root = Some(root.clone());
    config.browser_directory = Some(program);
    config.channel = ReleaseChannel::Esr;

    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.state_mut().latest_versions.insert(
        (TargetId::Iceweasel, ReleaseChannel::Esr, CpuBuild::X64),
        UpstreamCheck {
            version: "115.44.0esr".to_owned(),
            channel: ReleaseChannel::Esr,
            cpu_build: CpuBuild::X64,
            source: MetadataSource::Official,
            checked_at: 0,
        },
    );
    harness.run();

    harness.get_by_label("渠道不同");
    harness.get_by_label("使用 ESR 渠道安装");
    assert!(harness.query_by_label("上游版本较旧").is_none());
    assert!(harness.query_by_label("下载上游版本").is_none());

    harness.get_by_label("使用 ESR 渠道安装").click();
    harness.run();

    harness.get_by_label("渠道与现有安装不一致");
    harness.get_by_label_contains("当前安装：Iceweasel Release 159.0");
    harness.get_by_label_contains("本次选择：ESR");
}

#[test]
fn chromeplus_tab_is_disabled_for_firefox_and_enabled_for_chrome() {
    let mut harness = harness();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "浏览器");
    harness.get_by_label("全局设置");
    harness.get_by_label("自动保存已开启");
    assert!(harness.query_by_label("高级").is_none());
    assert!(harness.query_by_label("支持级别").is_none());
    assert!(
        harness
            .get_by_label("Chrome++")
            .accesskit_node()
            .is_disabled()
    );

    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("Chrome")
        .click();
    harness.run();

    assert!(
        !harness
            .get_by_label("Chrome++")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn overview_integrates_channel_and_build_selectors() {
    let mut harness = harness();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("Iceweasel")
        .click();
    harness.run();

    harness
        .get_by_role_and_label(Role::ComboBox, "渠道")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("Release")
        .click();
    harness.run();
    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Iceweasel].channel,
        ReleaseChannel::Release
    );

    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("Thorium")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "构建")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("AVX512")
        .click();
    harness.run();
    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Thorium].cpu_build,
        CpuBuild::Avx512
    );
}

#[test]
fn beta_channels_are_exposed_for_selected_targets() {
    let mut harness = harness();
    harness.run();

    for target in [
        TargetId::Brave,
        TargetId::Chrome,
        TargetId::Edge,
        TargetId::Firefox,
    ] {
        harness
            .get_by_role_and_label(Role::ComboBox, "浏览器")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label(target.display_name())
            .click();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "渠道")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label("Beta")
            .click();
        harness.run();

        assert_eq!(
            harness.state().state().saved().targets[&target].channel,
            ReleaseChannel::Beta
        );
    }

    harness
        .get_by_role_and_label(Role::ComboBox, "渠道")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("ESR")
        .click();
    harness.run();
    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Firefox].channel,
        ReleaseChannel::Esr
    );
}

#[test]
fn overview_uses_inline_metadata_and_action_rows() {
    let mut harness = harness();
    harness.run();

    let y = |label: &str| harness.get_by_label(label).rect().center().y;
    let x = |label: &str| harness.get_by_label(label).rect().min.x;
    let browser = harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .rect();
    assert!((browser.center().y - y("渠道")).abs() < 1.0);
    assert!((y("渠道") - y("构建")).abs() < 1.0);
    assert!((y("构建") - y("来源")).abs() < 1.0);
    assert!(browser.min.x < x("渠道") && x("渠道") < x("构建") && x("构建") < x("来源"));
    assert!((y("当前版本") - y("上游版本")).abs() < 1.0);
    assert!(y("当前版本") > browser.center().y);
    assert!(harness.query_by_label("内核").is_none());
    assert!(harness.query_by_label("配置").is_none());
    assert!(harness.query_by_label("比较").is_none());

    let check = harness.get_by_label("检查更新").rect();
    let install = harness.get_by_label("下载并安装").rect();
    let local = harness.get_by_label("选择本地安装包").rect();
    assert!((check.center().y - install.center().y).abs() < 1.0);
    assert!((install.center().y - local.center().y).abs() < 1.0);
    assert!(check.max.x < install.min.x && install.max.x < local.min.x);
}

#[test]
fn overview_has_no_scroll_bar() {
    let mut harness = harness();
    harness.run();

    assert!(harness.query_by_role(Role::ScrollBar).is_none());
}

#[test]
fn footer_shows_release_menu_at_the_right_edge() {
    assert_eq!(
        super::RELEASES_URL,
        "https://github.com/MagicCube99/browser-harbor/releases"
    );
    assert_eq!(
        super::GITEE_RELEASES_URL,
        "https://gitee.com/MagicCube99/browser-harbor/releases"
    );
    let mut harness = harness();
    harness.run();

    let version_label = format!("ver {}", env!("CARGO_PKG_VERSION"));
    let version = harness.get_by_role_and_label(Role::Button, &version_label);
    let network = harness.get_by_label("全局设置");
    let theme = harness.get_by_label("外观：跟随系统");

    assert!(version.rect().min.x > network.rect().max.x);
    assert!(version.rect().min.x > theme.rect().max.x);

    version.click();
    harness.run();
    harness.get_by_role_and_label(Role::Link, "GitHub 发布页");
    harness.get_by_role_and_label(Role::Link, "Gitee 发布页");
}

#[test]
fn footer_marks_an_available_app_update() {
    let mut harness = harness();
    harness.state_mut().available_app_update = Some(super::LatestAppRelease {
        version: "99.0.0".to_owned(),
    });
    harness.run();

    harness.get_by_role_and_label(
        Role::Button,
        &format!("ver {} ↑", env!("CARGO_PKG_VERSION")),
    );
}

#[test]
fn footer_truncates_long_status_before_right_controls() {
    let mut harness = harness();
    harness.run();
    let message = format!(
        "Firefox 158.0.1 已安装；快捷方式已创建到 {}；这条状态信息故意很长以验证省略显示",
        r"D:\Tools\Browser\Firefox\Firefox.lnk"
    );
    harness.state_mut().status_message = Some(message.clone());
    harness.run();

    let status = harness.get_by_label(&message);
    let network = harness.get_by_label("全局设置");
    let theme = harness.get_by_label("外观：跟随系统");
    let language = harness.get_by_role_and_label(Role::Button, "界面语言: 中文");
    let version_label = format!("ver {}", env!("CARGO_PKG_VERSION"));
    let version = harness.get_by_role_and_label(Role::Button, &version_label);

    assert!(status.rect().max.x < network.rect().min.x);
    assert!(status.rect().max.x < theme.rect().min.x);
    assert!(status.rect().max.x < language.rect().min.x);
    assert!(status.rect().max.x < version.rect().min.x);
    let gap = network.rect().min.x - status.rect().max.x;
    assert!(
        gap <= 12.0,
        "truncated footer status should use the available width; gap was {gap}"
    );
}

#[test]
fn local_package_has_an_explicit_action() {
    let mut harness = harness();
    harness.run();

    harness.get_by_label("选择本地安装包");
}

#[test]
fn selected_local_package_changes_action_names() {
    let directory = tempdir().unwrap();
    let mut settings = Settings::default();
    settings
        .targets
        .get_mut(&TargetId::Firefox)
        .unwrap()
        .local_package = Some(directory.path().join("Firefox-package.exe"));
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness.get_by_label("更换本地安装包");
    harness.get_by_label("使用本地包安装");
    assert!(harness.query_by_label("选择本地安装包").is_none());
    assert!(harness.query_by_label("下载并安装").is_none());
}

#[test]
fn install_action_is_available_with_the_default_browser_root() {
    let mut harness = harness();
    harness.run();

    assert!(
        !harness
            .get_by_label("下载并安装")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label("检查更新");

    harness
        .get_by_role_and_label(Role::TextInput, "浏览器根目录")
        .set_text_value_accesskit(r"D:\Browsers\Firefox");
    harness.run();

    assert!(
        !harness
            .get_by_label("下载并安装")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn version_status_uses_semantic_tone_and_keeps_missing_state_distinct() {
    use crate::domain::version::VersionRelation;

    assert_eq!(
        relation_status(false, true, VersionRelation::Unknown),
        Some(("无法比较", StatusTone::Error))
    );
    assert_eq!(
        relation_status(true, true, VersionRelation::Same),
        Some(("与上游版本相同", StatusTone::Success))
    );
    assert_eq!(
        relation_status(true, true, VersionRelation::UpstreamNewer),
        Some(("上游有新版本", StatusTone::Info))
    );
    assert_eq!(
        relation_status(true, true, VersionRelation::UpstreamOlder),
        Some(("上游版本较旧", StatusTone::Warning))
    );
}

#[test]
fn detected_same_version_uses_replace_label_and_requires_confirmation() {
    let (_directory, mut harness) = firefox_harness_with_versions("158.0.1", "158.0.1");
    harness.run();

    harness.get_by_label("与上游版本相同");
    harness.get_by_label("重新下载并替换").click();
    harness.run();
    harness.get_by_label("版本与上游相同");
    harness.get_by_label("继续替换");
    harness.get_by_label("保留当前版本").click();
    harness.run();

    assert!(harness.query_by_label("版本与上游相同").is_none());
    harness.get_by_label("已取消同版本替换");
}

#[test]
fn upstream_direction_drives_upgrade_and_replacement_labels() {
    let (_directory, mut newer) = firefox_harness_with_versions("158.0.1", "159.0.0");
    newer.run();
    newer.get_by_label("上游有新版本");
    newer.get_by_label("下载并升级");

    let (_directory, mut older) = firefox_harness_with_versions("158.0.1", "157.0.9");
    older.run();
    older.get_by_label("上游版本较旧");
    older.get_by_label("下载上游版本");
}

#[test]
fn upstream_check_is_scoped_by_release_channel() {
    let directory = tempdir().unwrap();
    let browser = directory.path().join("Iceweasel");
    let program = browser.join("App");
    fs::create_dir_all(&program).unwrap();
    fs::write(program.join("iceweasel.exe"), b"fixture").unwrap();
    fs::write(
        program.join("application.ini"),
        "[App]\r\nName=Iceweasel\r\nVersion=158.0.1\r\n",
    )
    .unwrap();
    let mut settings = Settings {
        selected_browser: TargetId::Iceweasel,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Iceweasel).unwrap();
    config.browser_root = Some(browser);
    config.browser_directory = Some(program);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.state_mut().latest_versions.insert(
        (TargetId::Iceweasel, ReleaseChannel::Release, CpuBuild::X64),
        UpstreamCheck {
            version: "158.0.1".to_owned(),
            channel: ReleaseChannel::Release,
            cpu_build: CpuBuild::X64,
            source: MetadataSource::Official,
            checked_at: 0,
        },
    );
    harness.run();

    harness.get_by_label("尚未检查上游版本");
    harness.get_by_label("检查并安装/升级");

    harness.state_mut().state.update_draft(|settings| {
        settings
            .targets
            .get_mut(&TargetId::Iceweasel)
            .unwrap()
            .channel = ReleaseChannel::Release;
    });
    harness.run();

    harness.get_by_label("与上游版本相同");
}

#[test]
fn missing_browser_directory_does_not_show_receipt_as_current_version() {
    let directory = tempdir().unwrap();
    let mut settings = Settings::default();
    let config = settings.targets.get_mut(&TargetId::Firefox).unwrap();
    let root = directory.path().join("deleted-firefox");
    config.browser_root = Some(root.clone());
    config.browser_directory = Some(root.join("App"));
    config.installed_version = Some("157.0.1".to_owned());
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.state_mut().latest_versions.insert(
        (TargetId::Firefox, ReleaseChannel::Stable, CpuBuild::X64),
        UpstreamCheck {
            version: "157.0.1".to_owned(),
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            source: MetadataSource::Official,
            checked_at: 0,
        },
    );
    harness.run();

    harness.get_by_label("目录不存在");
    harness.get_by_label("未安装");
    harness.get_by_label("下载并安装");
    assert!(harness.query_by_label("157.0.1（上次安装）").is_none());
}

#[test]
fn existing_program_directory_without_browser_exe_is_marked_incomplete() {
    let directory = tempdir().unwrap();
    let mut settings = Settings::default();
    let config = settings.targets.get_mut(&TargetId::Firefox).unwrap();
    let root = directory.path().join("broken-firefox");
    let program = root.join("App");
    fs::create_dir_all(&program).unwrap();
    config.browser_root = Some(root);
    config.browser_directory = Some(program);
    config.installed_version = Some("157.0.1".to_owned());
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness.get_by_label("安装不完整");
    harness.get_by_label("无法比较");
    assert!(harness.query_by_label("157.0.1（上次安装）").is_none());
}

#[test]
fn network_editor_adds_a_valid_https_accelerator() {
    let mut harness = harness();
    harness.run();
    let network = harness.get_by_label("全局设置");
    let overview = harness.get_by_role_and_label(Role::Button, "浏览器").rect();
    assert!(network.rect().min.y > overview.max.y);
    assert!(network.rect().max.x > overview.max.x);
    network.click();
    harness.run();

    assert!(harness.query_by_label("浏览器").is_none());
    assert!(
        harness
            .query_by_label("以下网络设置应用于全部浏览器，不属于当前选中的单个浏览器。")
            .is_none()
    );
    let accelerators = harness.get_by_label("启用内置下载加速站").rect();
    let custom = harness
        .get_by_role_and_label(Role::TextInput, "加速站前缀")
        .rect();
    let proxy = harness
        .get_by_role_and_label(Role::TextInput, "代理地址")
        .rect();
    assert!(accelerators.min.y < custom.min.y);
    assert!(custom.min.y < proxy.min.y);
    let proxy_section = harness.get_by_label("代理与超时").rect();
    assert!(
        proxy_section.width() >= 60.0,
        "proxy title must not be squeezed by its header control"
    );
    let timeout = harness
        .get_by_role_and_label(Role::SpinButton, "超时")
        .rect();
    assert!(
        (timeout.center().y - proxy_section.center().y).abs() < 8.0,
        "timeout must share the proxy card header row"
    );
    assert!(
        (timeout.min.x - proxy.min.x).abs() < 0.5,
        "timeout and proxy inputs must share the same left edge: {} vs {}",
        timeout.min.x,
        proxy.min.x
    );
    let cache_section = harness.get_by_label("下载缓存").rect();
    assert!((proxy_section.min.y - cache_section.min.y).abs() < 0.5);
    assert!(proxy_section.max.x < cache_section.min.x);
    let clear_cache = harness.get_by_label("清理下载缓存").rect();
    assert!(
        (proxy.max.y - clear_cache.max.y).abs() < 0.5,
        "proxy and cache cards must keep equal content heights"
    );
    let config_section = harness.get_by_label("配置位置").rect();
    assert!(config_section.min.y > proxy_section.max.y.max(cache_section.max.y));
    let user_config = harness.get_by_label("系统用户目录（默认）").rect();
    let portable_config = harness.get_by_label("程序目录（便携）").rect();
    assert!(
        (user_config.center().y - portable_config.center().y).abs() < 8.0,
        "configuration location choices must share one row"
    );
    let config_file_label = harness.get_by_label("当前配置文件").rect();
    let config_path = harness.state().state.config_path().display().to_string();
    let config_path_label = harness.get_by_label(&config_path).rect();
    assert!(
        config_path_label.min.x > config_file_label.max.x,
        "current configuration path must use the remaining row width"
    );
    assert!(
        config_path_label.width() > 300.0,
        "current configuration path must not be confined to a narrow column"
    );
    harness
        .get_by_role_and_label(Role::TextInput, "加速站前缀")
        .set_text_value_accesskit("https://mirror.example/");
    harness.run();
    harness.get_by_label("添加").click();
    harness.run();

    let network = &harness.state().state().draft().network;
    assert_eq!(network.accelerators.len(), 1);
    assert_eq!(network.accelerators[0].prefix, "https://mirror.example/");
    assert_eq!(
        harness.state().state().saved().network.accelerators,
        network.accelerators
    );

    harness.get_by_label("全局设置").click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, "浏览器");
    harness.get_by_label("Chrome++");
}

#[test]
fn global_settings_fit_and_align_in_english() {
    let mut harness = harness();
    harness
        .state_mut()
        .state
        .update_draft(|settings| settings.language = Language::English);
    harness.run();
    harness.get_by_label("Global settings").click();
    harness.run();

    assert!(
        harness.query_by_role(Role::ScrollBar).is_none(),
        "English global settings must fit without scrolling"
    );
    let timeout = harness
        .get_by_role_and_label(Role::SpinButton, "Timeout")
        .rect();
    let proxy = harness
        .get_by_role_and_label(Role::TextInput, "Proxy address")
        .rect();
    assert!((timeout.min.x - proxy.min.x).abs() < 0.5);
    harness.get_by_label("Current config file");
    set_language(Language::Chinese);
}

#[test]
fn global_settings_switches_manager_config_between_user_and_program_directories() {
    let directory = tempdir().unwrap();
    let tool_root = directory.path().join("Tool");
    fs::create_dir_all(&tool_root).unwrap();
    let user_path = directory
        .path()
        .join("Local/browser-harbor/BrowserHarbor.json");
    let portable_path = tool_root.join("BrowserHarbor.json");
    let state = AppState::with_config_paths(
        Settings::default(),
        ConfigStore::new(user_path.clone()),
        tool_root,
        user_path.clone(),
        portable_path.clone(),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness.get_by_label("全局设置").click();
    harness.run();

    harness
        .get_by_role_and_label(Role::RadioButton, "程序目录（便携）")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "程序目录（便携）")
        .click();
    harness.run();

    assert!(portable_path.is_file());
    assert!(harness.state().state().is_portable_config());
    harness.get_by_label_contains("配置位置已切换");

    harness
        .get_by_role_and_label(Role::RadioButton, "系统用户目录（默认）")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "系统用户目录（默认）")
        .click();
    harness.run();

    assert!(user_path.is_file());
    assert!(!portable_path.exists());
    assert!(!harness.state().state().is_portable_config());
}

#[test]
fn chromeplus_full_exposes_common_ini_controls() {
    let mut harness = harness();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("Chrome")
        .click();
    harness.run();

    harness.get_by_label("Chrome++").click();
    harness.run();

    harness.get_by_label("当前浏览器：Chrome");
    assert!(
        harness
            .query_by_role_and_label(Role::ComboBox, "浏览器")
            .is_none()
    );
    harness.get_by_label("双击关闭标签");
    harness.get_by_label("右键关闭标签");
    harness.get_by_label("保留最后一个标签");
    harness.get_by_label("标签栏滚轮切换");
    harness.get_by_label("按住右键滚轮切换");
    harness.get_by_label("地址栏新标签打开");
    harness.get_by_label("书签在新标签打开");
    harness.get_by_label("禁用指定新标签页");
    harness.get_by_label("悬停自动切换标签");
    harness.get_by_label("新标签匹配");
    harness.get_by_label("启动命令");
    harness.get_by_label("退出命令");
    harness.get_by_label("老板键");
    harness.get_by_label("翻译键");
    harness.get_by_label("免系统验证查看密码");
    harness.get_by_label("启用 win32k");
    harness.get_by_label("忽略企业策略");
    harness.get_by_label("抑制错误升级提示");
    harness.get_by_label("按键映射");
    let integration = harness.get_by_label("Chrome++ 集成").rect();
    let configuration = harness.get_by_label("配置文件").rect();
    harness.get_by_label("Chrome++ 源码与 GPL-3.0 许可证");
    let mode_row = harness.get_by_label("模式").rect();
    let status_row = harness.get_by_label("状态").rect();
    let source_row = harness.get_by_label("来源").rect();
    assert!((mode_row.min.x - status_row.min.x).abs() < 1.0);
    assert!((mode_row.min.x - source_row.min.x).abs() < 1.0);
    assert!(mode_row.center().y < status_row.center().y);
    assert!(status_row.center().y < source_row.center().y);
    assert!((integration.center().y - configuration.center().y).abs() < 1.0);
    let version = harness.get_by_label("—").rect();
    let check = harness.get_by_label("检查更新").rect();
    assert!(integration.max.x < version.min.x && version.max.x < check.min.x);
    assert!((integration.center().y - version.center().y).abs() < 1.0);
    assert!((integration.center().y - check.center().y).abs() < 1.0);
    let load = harness.get_by_label("从 INI 载入").rect();
    let open = harness.get_by_label("打开配置文件").rect();
    let more = harness.get_by_label("更多操作…").rect();
    let window_right = 560.0;
    assert!(load.min.x > integration.max.x);
    assert!(load.width() > 180.0);
    assert!((load.min.x - open.min.x).abs() < 1.0);
    assert!((load.min.x - more.min.x).abs() < 1.0);
    assert!(load.min.y < open.min.y && open.min.y < more.min.y);
    assert!(more.max.x <= window_right);
    harness.get_by_label("更多操作…").click();
    harness.run();
    harness.get_by_label("打开所在文件夹");
    harness.get_by_label("备份当前 INI");
    harness.get_by_label("恢复默认配置");
}

#[test]
fn chromeplus_header_fits_in_english() {
    let directory = tempdir().unwrap();
    let settings = Settings {
        selected_browser: TargetId::Chrome,
        language: Language::English,
        ..Settings::default()
    };
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness.get_by_label("Chrome++").click();
    harness.run();

    let integration = harness.get_by_label("Chrome++ integration").rect();
    let configuration = harness.get_by_label("Configuration file").rect();
    let version = harness.get_by_label("—").rect();
    let check = harness.get_by_label("Check for updates").rect();
    assert!(integration.width() > 100.0);
    assert!(integration.max.x < version.min.x && version.max.x < check.min.x);
    assert!((integration.center().y - version.center().y).abs() < 1.0);
    assert!((integration.center().y - check.center().y).abs() < 1.0);
    assert!((integration.center().y - configuration.center().y).abs() < 1.0);

    set_language(Language::Chinese);
}

#[test]
fn uninstalled_chromeplus_does_not_show_stored_version() {
    let directory = tempdir().unwrap();
    let browser = directory.path().join("Chrome");
    let program = browser.join("App");
    fs::create_dir_all(&program).unwrap();
    fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(browser);
    config.browser_directory = Some(program);
    config.chrome_plus_version = Some("1.18.2".to_owned());
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.state_mut().active_tab = DetailTab::ChromePlus;
    harness
        .state_mut()
        .chrome_plus_latest
        .insert(TargetId::Chrome, "1.18.3".to_owned());
    harness.run();

    harness.get_by_label("尚未安装");
    harness.get_by_label("—");
    assert!(harness.query_by_label("v1.18.2").is_none());
    assert!(harness.query_by_label("更新 Chrome++ 到 1.18.3").is_none());
    let config = &harness.state().state().draft().targets[&TargetId::Chrome];
    assert!(
        harness
            .state()
            .chrome_plus_pending_update(TargetId::Chrome, config)
            .is_none()
    );
}

#[test]
fn chromeplus_update_blocks_when_multiple_target_instances_are_running() {
    use std::process::{Child, Command, Stdio};

    struct Children(Vec<Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    let directory = tempdir().unwrap();
    let browser = directory.path().join("Chrome");
    let program = browser.join("App");
    fs::create_dir_all(&program).unwrap();
    let executable = program.join("chrome.exe");
    fs::copy(std::env::var_os("COMSPEC").unwrap(), &executable).unwrap();
    write_ini(
        &program.join("chrome++.ini"),
        "[general]\r\ncommand_line=\r\n",
        IniEncoding::Utf16Le,
    )
    .unwrap();
    let _children = Children(
        (0..2)
            .map(|_| {
                Command::new(&executable)
                    .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect(),
    );
    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(browser);
    config.browser_directory = Some(program);
    config.data_directory = Some(r"D:\Browser\Chrome\Profiles".into());
    config.profile_directory = Some(r"D:\Browser\Chrome\Profiles".into());
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    let app = harness.state_mut();
    app.active_tab = DetailTab::ChromePlus;
    app.chrome_plus_versions
        .insert(TargetId::Chrome, Some("1.18.2".to_owned()));
    app.chrome_plus_latest
        .insert(TargetId::Chrome, "1.18.3".to_owned());
    harness.run();

    harness.get_by_label("更新 Chrome++ 到 1.18.3").click();
    harness.run();

    harness.get_by_label("检测到 2 个 Chrome 进程；请手工关闭所有 Chrome 实例后再更新 Chrome++");
    assert!(harness.state().operation.is_none());
}

#[test]
fn chromeplus_update_ignores_other_browser_programs() {
    use std::process::{Child, Command, Stdio};

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let directory = tempdir().unwrap();
    let browser = directory.path().join("Chrome");
    let program = browser.join("App");
    fs::create_dir_all(&program).unwrap();
    fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
    write_ini(
        &program.join("chrome++.ini"),
        "[general]\r\ncommand_line=\r\n",
        IniEncoding::Utf16Le,
    )
    .unwrap();
    let other = directory.path().join("Helium/App");
    fs::create_dir_all(&other).unwrap();
    let other_executable = other.join("helium.exe");
    fs::copy(std::env::var_os("COMSPEC").unwrap(), &other_executable).unwrap();
    let _other_child = ChildGuard(
        Command::new(&other_executable)
            .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(browser);
    config.browser_directory = Some(program);
    config.data_directory = Some(r"D:\Browser\Chrome\Profiles".into());
    config.profile_directory = Some(r"D:\Browser\Chrome\Profiles".into());
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    let app = harness.state_mut();
    app.active_tab = DetailTab::ChromePlus;
    app.chrome_plus_versions
        .insert(TargetId::Chrome, Some("1.18.2".to_owned()));
    app.chrome_plus_latest
        .insert(TargetId::Chrome, "1.18.3".to_owned());
    harness.run();

    harness.get_by_label("更新 Chrome++ 到 1.18.3").click();
    harness.run();

    harness.get_by_label("Chrome++ 1.18.2 → 1.18.3");
    assert!(harness.state().operation.is_none());
}

#[test]
fn browser_upgrade_prompt_names_both_version_transitions() {
    let mut harness = harness();
    harness.state_mut().browser_upgrade_prompt = Some(super::BrowserUpgradePrompt {
        target: TargetId::Firefox,
        current_browser: "159.0".to_owned(),
        upstream_browser: "160.0".to_owned(),
        chrome_plus_current: "1.18.2".to_owned(),
        chrome_plus_latest: "1.18.3".to_owned(),
        replace_same_version: false,
    });
    harness.run();

    harness.get_by_label("同时更新 Chrome++");
    harness.get_by_label("浏览器：159.0 → 160.0");
    harness.get_by_label("Chrome++：1.18.2 → 1.18.3");
}

#[test]
fn chromeplus_edits_autosave_manager_state_and_ini() {
    let directory = tempdir().unwrap();
    let browser = directory.path().join("Chrome");
    let program = browser.join("App");
    fs::create_dir_all(&program).unwrap();
    fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
    let ini = program.join("chrome++.ini");
    write_ini(&ini, "[general]\r\ncommand_line=\r\n", IniEncoding::Utf16Le).unwrap();
    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(browser);
    config.browser_directory = Some(program);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness.get_by_label("Chrome++").click();
    harness.run();

    harness
        .get_by_role_and_label(Role::TextInput, "附加参数")
        .set_text_value_accesskit("--autosave-test");
    harness.run();

    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Chrome]
            .chrome_plus
            .command_line,
        "--autosave-test"
    );
    let (ini_text, _) = read_ini(&ini).unwrap();
    assert!(ini_text.contains("--autosave-test"));
}

#[test]
fn chromeplus_installed_version_is_probed_from_version_dll() {
    let directory = tempdir().unwrap();
    let browser = directory.path().join("Chrome");
    let program = browser.join("App");
    fs::create_dir_all(&program).unwrap();
    fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
    fs::copy(
        std::env::current_exe().unwrap(),
        program.join("version.dll"),
    )
    .unwrap();
    write_ini(
        &program.join("chrome++.ini"),
        "[general]\r\n",
        IniEncoding::Utf16Le,
    )
    .unwrap();
    let expected = crate::runtime::chromeplus::installed_version(&program).unwrap();
    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(browser);
    config.browser_directory = Some(program);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness.get_by_label("Chrome++").click();
    harness.run();

    harness.get_by_label(&format!("v{expected}"));
    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Chrome]
            .chrome_plus_version
            .as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn experimental_chromium_targets_expose_full_chromeplus() {
    for (browser, target) in [
        ("Brave", TargetId::Brave),
        ("Helium", TargetId::Helium),
        ("Ungoogled Chromium", TargetId::UngoogledChromium),
    ] {
        let mut harness = harness();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "浏览器")
            .click();
        harness.run();
        harness
            .get_by_role(Role::ListBox)
            .get_by_label(browser)
            .click();
        harness.run();
        assert_eq!(harness.state().state().selected_target(), target);
        harness.get_by_label("Chrome++").click();
        harness.run();

        harness.get_by_label("启动命令");
        harness.get_by_label("禁用指定新标签页");
        harness.get_by_label("免系统验证查看密码");
        harness.get_by_label("按键映射");
    }
}

#[test]
fn chrome_plus_update_button_reflects_the_last_check() {
    assert_eq!(chrome_plus_update_button_label(None, None), "检查更新");
    assert_eq!(
        chrome_plus_update_button_label(Some("1.18.2"), Some("1.18.2")),
        "已最新"
    );
    assert_eq!(
        chrome_plus_update_button_label(Some("1.18.1"), Some("1.18.2")),
        "可更新"
    );
    assert_eq!(
        chrome_plus_update_button_label(Some("1.18.2"), Some("1.18.2-beta")),
        "检查更新"
    );
}

#[test]
fn chromeplus_update_hover_separates_detection_from_installation() {
    let hover = chrome_plus_update_hover(Language::Chinese, "1.18.2", "1.18.3");
    assert!(hover.contains("点击后才会下载并替换"));
    assert!(hover.contains("完成后版本才会变化"));
}

#[test]
fn every_directory_field_has_a_folder_picker() {
    let mut harness = harness();
    harness.run();

    let labels = ["浏览器根目录", "程序目录", "用户数据目录", "缓存目录"];
    for label in labels {
        assert_eq!(
            harness
                .get_by_role_and_label(Role::Button, &format!("选择{label}"))
                .accesskit_node()
                .role(),
            Role::Button
        );
    }
    let label_left = labels.map(|label| {
        harness
            .get_by_role_and_label(Role::Label, label)
            .rect()
            .min
            .x
    });
    let input_left = labels.map(|label| {
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .rect()
            .min
            .x
    });
    assert!(
        label_left
            .iter()
            .all(|left| (left - label_left[0]).abs() < 0.5)
    );
    assert!(
        input_left
            .iter()
            .all(|left| (left - input_left[0]).abs() < 0.5),
        "{input_left:?}"
    );
    assert!(harness.query_by_label("Profile 位置").is_none());
    let shortcut = harness.get_by_label("快捷方式：开");
    assert!(!shortcut.accesskit_node().is_disabled());
    let input_right = labels.map(|label| {
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .rect()
            .max
            .x
    });
    assert!(
        input_right[1..]
            .iter()
            .all(|right| (right - shortcut.rect().max.x).abs() < 1.0),
        "shortcut action must share lower directory fields' right edge: shortcut={}, inputs={input_right:?}",
        shortcut.rect().max.x
    );
}

#[test]
fn separate_profile_location_is_shown_only_for_targets_that_need_it() {
    let directory = tempdir().unwrap();
    let settings = Settings {
        selected_browser: TargetId::Floorp,
        ..Settings::default()
    };
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness.get_by_role_and_label(Role::TextInput, "Profile 位置");
    assert!(harness.query_by_label("Profile 目录").is_none());
}

#[test]
fn shared_user_data_edit_updates_the_internal_profile_path() {
    let directory = tempdir().unwrap();
    let settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "用户数据目录")
        .set_text_value_accesskit(r"D:\ChromeData");
    harness.run();
    let config = &harness.state().state().saved().targets[&TargetId::Chrome];
    assert_eq!(config.data_directory, Some(r"D:\ChromeData".into()));
    assert_eq!(config.profile_directory, Some(r"D:\ChromeData".into()));
}

#[test]
fn shortcut_auto_creation_is_on_by_default_and_can_be_disabled() {
    let mut harness = harness();
    harness.run();

    harness.get_by_label("快捷方式：开").click();
    harness.run();

    harness.get_by_label("快捷方式：关");
    assert!(!harness.state().state().saved().targets[&TargetId::Firefox].auto_create_shortcut);
}

#[test]
fn skykakapo_uses_app_and_shows_profile_followed_cache() {
    let mut harness = harness();
    harness.run();
    harness
        .get_by_role_and_label(Role::ComboBox, "浏览器")
        .click();
    harness.run();
    harness
        .get_by_role(Role::ListBox)
        .get_by_label("SkyKakapo")
        .click();
    harness.run();

    assert_eq!(
        harness
            .get_by_role_and_label(Role::TextInput, "程序目录")
            .accesskit_node()
            .value(),
        Some(".\\SkyKakapo\\App".to_owned())
    );
    assert!(
        harness
            .query_by_role_and_label(Role::TextInput, "缓存目录")
            .is_none()
    );
    harness.get_by_label("跟随 PortableData");
}

#[test]
fn auto_shortcut_creation_respects_the_enabled_flag() {
    let root = tempdir().unwrap();
    let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    let program = root.path().join("App");
    fs::create_dir_all(&program).unwrap();
    let executable = program.join("firefox.exe");
    fs::copy(
        PathBuf::from(windows).join(r"System32\notepad.exe"),
        &executable,
    )
    .unwrap();

    assert_eq!(
        create_auto_shortcut(TargetId::Firefox, false, root.path(), &program).unwrap(),
        None
    );
    assert!(!root.path().join("Firefox.lnk").exists());

    let shortcut = create_auto_shortcut(TargetId::Firefox, true, root.path(), &program).unwrap();
    assert_eq!(shortcut, Some(root.path().join("Firefox.lnk")));
}

#[test]
fn browser_root_change_is_saved_immediately() {
    let mut harness = harness();
    harness.run();
    let field = harness.get_by_role_and_label(Role::TextInput, "浏览器根目录");
    field.set_text_value_accesskit(r"D:\draft");
    harness.run();
    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Firefox].browser_root,
        Some(PathBuf::from(r"D:\draft"))
    );
    let config = &harness.state().state().saved().targets[&TargetId::Firefox];
    assert_eq!(
        config.browser_directory,
        Some(PathBuf::from(r"D:\draft\App"))
    );
    assert_eq!(
        config.data_directory,
        Some(PathBuf::from(r"D:\draft\Profiles"))
    );
    assert_eq!(
        config.profile_directory,
        Some(PathBuf::from(r"D:\draft\Profiles"))
    );
    assert_eq!(config.cache_directory, None);
    assert_eq!(
        harness.state().state().draft(),
        harness.state().state().saved()
    );
}

#[test]
fn browser_root_hint_follows_the_selected_target() {
    assert_eq!(
        super::browser_root_hint(TargetId::Helium),
        r"例如 .\Helium 或 D:\Tools\Browser\Helium"
    );
    assert_eq!(
        super::browser_root_hint(TargetId::Chrome),
        r"例如 .\Chrome 或 D:\Tools\Browser\Chrome"
    );
}

#[test]
fn configuration_diagnostics_open_from_path_section() {
    let directory = tempdir().unwrap();
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

    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(root);
    config.derive_paths_from_root(TargetId::Chrome);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "检查配置")
        .click();
    harness.run();

    harness.get_by_role_and_label(Role::Label, "检查配置");
    assert!(harness.query_by_label("检查结果：正常").is_some());
    assert!(harness.query_by_label("Profile 位置").is_none());
}

#[test]
fn chromeplus_path_repair_requires_confirmation_and_creates_backup() {
    let directory = tempdir().unwrap();
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
        format!(
            "[general]\r\ndata_dir={}\r\ncache_dir=none\r\n",
            root.join("Profiles").display()
        ),
    )
    .unwrap();
    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(root.clone());
    config.derive_paths_from_root(TargetId::Chrome);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "检查配置")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "修复 Chrome++ 路径")
        .click();
    harness.run();

    assert!(
        harness
            .query_by_label("将备份 chrome++.ini 并只修复路径键。")
            .is_some()
    );
    harness
        .get_by_role_and_label(Role::Button, "确认修复")
        .click();
    harness.run();

    assert!(harness.query_by_label("检查结果：正常").is_some());
    let ini = fs::read_to_string(program.join("chrome++.ini")).unwrap();
    assert!(ini.contains(r"data_dir=%app%\..\Profiles"));
    assert!(
        fs::read_dir(root.join("backups"))
            .unwrap()
            .any(|entry| entry.unwrap().file_type().unwrap().is_file())
    );
}

#[test]
fn iceweasel_portable_paths_can_be_repaired_from_health_dialog() {
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
    fs::copy(
        std::env::current_exe().unwrap(),
        program.join("portable64.dll"),
    )
    .unwrap();
    fs::write(
        program.join("application.ini"),
        "[App]\r\nVersion=159.0\r\n",
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
    let mut settings = Settings {
        selected_browser: TargetId::Iceweasel,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Iceweasel).unwrap();
    config.browser_root = Some(root.clone());
    config.derive_paths_from_root(TargetId::Iceweasel);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "检查配置")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "修复便携配置")
        .click();
    harness.run();

    assert!(
        harness
            .query_by_label("将备份 portable.ini 并只修复路径键。")
            .is_some()
    );
    harness
        .get_by_role_and_label(Role::Button, "确认修复")
        .click();
    harness.run();

    assert!(harness.query_by_label("检查结果：正常").is_some());
    let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
    assert!(ini.contains(r"PortableDataPath=..\Profiles"));
    assert!(
        fs::read_dir(root.join("backups"))
            .unwrap()
            .any(|entry| entry.unwrap().file_type().unwrap().is_file())
    );
}

#[test]
fn relocation_preview_does_not_save_until_applied() {
    let directory = tempdir().unwrap();
    let old_root = directory.path().join("OldChrome");
    let new_root = directory.path().join("NewChrome");
    fs::create_dir_all(new_root.join("App")).unwrap();
    fs::copy(
        std::env::current_exe().unwrap(),
        new_root.join("App/chrome.exe"),
    )
    .unwrap();
    let mut settings = Settings {
        selected_browser: TargetId::Chrome,
        ..Settings::default()
    };
    let config = settings.targets.get_mut(&TargetId::Chrome).unwrap();
    config.browser_root = Some(old_root.clone());
    config.derive_paths_from_root(TargetId::Chrome);
    let state = AppState::new(
        settings,
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness.state_mut().open_health_dialog();
    harness.state_mut().stage_relocation(new_root.clone());

    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Chrome].browser_root,
        Some(old_root)
    );
    assert!(
        harness
            .state()
            .health_dialog
            .as_ref()
            .and_then(|dialog| dialog.relocation_warning.as_deref())
            .is_some()
    );

    harness.state_mut().apply_staged_relocation();

    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Chrome].browser_root,
        Some(new_root)
    );
}

#[test]
fn derived_paths_survive_configuration_reload() {
    let directory = tempdir().unwrap();
    let config_path = directory.path().join("BrowserHarbor.json");
    let state = AppState::new(Settings::default(), ConfigStore::new(config_path.clone()));
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness
        .get_by_role_and_label(Role::TextInput, "浏览器根目录")
        .set_text_value_accesskit(r"D:\Persisted\Firefox");
    harness.run();
    drop(harness);

    let reloaded = ConfigStore::new(config_path).load().unwrap();
    let config = &reloaded.targets[&TargetId::Firefox];
    assert_eq!(
        config.browser_root,
        Some(PathBuf::from(r"D:\Persisted\Firefox"))
    );
    assert_eq!(
        config.browser_directory,
        Some(PathBuf::from(r"D:\Persisted\Firefox\App"))
    );
    assert_eq!(
        config.data_directory,
        Some(PathBuf::from(r"D:\Persisted\Firefox\Profiles"))
    );
    assert_eq!(
        config.profile_directory,
        Some(PathBuf::from(r"D:\Persisted\Firefox\Profiles"))
    );
    assert_eq!(config.cache_directory, None);
}

#[test]
fn every_edit_is_saved_without_a_save_button() {
    let directory = tempdir().unwrap();
    let state = AppState::new(
        Settings::default(),
        ConfigStore::new(directory.path().join("BrowserHarbor.json")),
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, "浏览器根目录")
        .set_text_value_accesskit(r"D:\saved");
    harness.run();
    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Firefox].browser_root,
        Some(PathBuf::from(r"D:\saved"))
    );

    harness
        .get_by_role_and_label(Role::TextInput, "浏览器根目录")
        .set_text_value_accesskit(r"D:\changed");
    harness.run();

    assert_eq!(
        harness.state().state().saved().targets[&TargetId::Firefox].browser_root,
        Some(PathBuf::from(r"D:\changed"))
    );
    assert_eq!(
        harness.state().state().draft(),
        harness.state().state().saved()
    );
    assert!(harness.query_by_label("保存").is_none());
}

#[test]
fn malformed_config_is_visible_and_autosave_is_blocked() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("BrowserHarbor.json");
    fs::write(&path, b"{broken").unwrap();
    let store = ConfigStore::new(path);
    let error = store.load().unwrap_err().to_string();
    let state = AppState::new(Settings::default(), store);
    let mut harness = harness_for(BrowserHarborApp::with_startup_error(state, error));

    harness.run();

    harness.get_by_label_contains("配置加载失败");
    assert!(harness.query_by_label("保存").is_none());
    let field = harness.get_by_role_and_label(Role::TextInput, "浏览器根目录");
    field.set_text_value_accesskit(r"D:\must-not-overwrite");
    harness.run();
    assert_eq!(
        fs::read(directory.path().join("BrowserHarbor.json")).unwrap(),
        b"{broken"
    );
}

#[test]
fn theme_choice_is_saved_immediately() {
    let mut harness = harness();
    harness.run();

    harness.get_by_label("外观：跟随系统").click();
    harness.run();
    harness.get_by_label("深色").click();
    harness.run();

    assert_eq!(harness.state().state().draft().theme, ThemeMode::Dark);
    assert_eq!(harness.state().state().saved().theme, ThemeMode::Dark);
    harness.get_by_label("外观：深色");
}

#[test]
fn language_switch_localizes_ui_and_persists() {
    let directory = tempdir().unwrap();
    let config_path = directory.path().join("BrowserHarbor.json");
    let state = AppState::new(Settings::default(), ConfigStore::new(config_path.clone()));
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "界面语言: 中文")
        .click();
    harness.run();
    harness.get_by_label("English").click();
    harness.run();

    assert_eq!(harness.state().state().saved().language, Language::English);
    harness.get_by_role_and_label(Role::Button, "Browser");
    harness.get_by_role_and_label(Role::ComboBox, "Browser");
    harness.get_by_role_and_label(Role::TextInput, "Browser root");
    harness.get_by_label("Appearance: Follow system");
    assert!(harness.query_by_label("浏览器根目录").is_none());

    let labels = [
        "Browser root",
        "Program directory",
        "User data directory",
        "Cache directory",
    ];
    let label_left = labels.map(|label| {
        harness
            .get_by_role_and_label(Role::Label, label)
            .rect()
            .min
            .x
    });
    let input_left = labels.map(|label| {
        harness
            .get_by_role_and_label(Role::TextInput, label)
            .rect()
            .min
            .x
    });
    assert!(
        label_left
            .iter()
            .all(|left| (left - label_left[0]).abs() < 0.5)
    );
    assert!(
        input_left
            .iter()
            .all(|left| (left - input_left[0]).abs() < 0.5),
        "{input_left:?}"
    );

    let reloaded = ConfigStore::new(config_path).load().unwrap();
    assert_eq!(reloaded.language, Language::English);
    set_language(Language::Chinese);
}

#[test]
fn theme_configuration_keeps_system_default_and_styles_both_appearances() {
    let ctx = eframe::egui::Context::default();
    configure_context(&ctx, ThemeMode::System);

    assert_eq!(
        ctx.options(|options| options.theme_preference),
        eframe::egui::ThemePreference::System
    );
    let style = ctx.style_of(eframe::egui::Theme::Light);
    assert_eq!(
        style.text_styles[&eframe::egui::TextStyle::Body].size,
        BODY_FONT_SIZE
    );
    assert_eq!(style.visuals.selection.bg_fill, LIGHT_PALETTE.accent_soft);
    assert!(!style.visuals.dark_mode);
    assert!(ctx.style_of(eframe::egui::Theme::Dark).visuals.dark_mode);
}

#[test]
fn egui_default_fonts_are_not_embedded() {
    assert!(
        eframe::egui::FontDefinitions::builtin_font_names().is_empty(),
        "default font assets would regress the release size budget"
    );
}

#[test]
fn legacy_mismatched_profile_checks_healthy_without_manual_reselect() {
    let directory = tempdir().unwrap();
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

    let config_path = directory.path().join("BrowserHarbor.json");
    let store = ConfigStore::new(config_path.clone());
    let mut value = serde_json::to_value(Settings::default()).unwrap();
    value["schemaVersion"] = serde_json::json!(10);
    value["selectedBrowser"] = serde_json::json!("chrome");
    value["targets"]["chrome"]["browserRoot"] = serde_json::json!(root.to_string_lossy());
    value["targets"]["chrome"]["browserDirectory"] = serde_json::json!(program.to_string_lossy());
    value["targets"]["chrome"]["dataDirectory"] =
        serde_json::json!(root.join("Profiles").to_string_lossy());
    value["targets"]["chrome"]["profileDirectory"] =
        serde_json::json!(root.join("LegacyProfile").to_string_lossy());
    fs::write(&config_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let loaded = store.load().unwrap();
    let state = AppState::new(loaded, store);
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "检查配置")
        .click();
    harness.run();

    harness.get_by_role_and_label(Role::Label, "检查配置");
    assert!(harness.query_by_label("检查结果：正常").is_some());
}

#[test]
fn global_settings_switch_shows_permission_guidance_when_access_denied() {
    let directory = tempdir().unwrap();
    let tool_root = directory.path().join("Tool");
    fs::create_dir_all(&tool_root).unwrap();
    let user_path = directory
        .path()
        .join("Local/browser-harbor/BrowserHarbor.json");
    let portable_path = tool_root.join("BrowserHarbor.json");
    let temp_file = tool_root.join("BrowserHarbor.json.tmp");
    fs::write(&temp_file, b"read-only-stub").unwrap();
    let mut permissions = fs::metadata(&temp_file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&temp_file, permissions).unwrap();

    let state = AppState::with_config_paths(
        Settings::default(),
        ConfigStore::new(user_path.clone()),
        tool_root,
        user_path,
        portable_path,
    );
    let mut harness = harness_for(BrowserHarborApp::new(state));
    harness.run();
    harness.get_by_label("全局设置").click();
    harness.run();

    harness
        .get_by_role_and_label(Role::RadioButton, "程序目录（便携）")
        .scroll_to_me();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "程序目录（便携）")
        .click();
    harness.run();

    harness.get_by_label_contains("当前目录受系统权限保护");
}
