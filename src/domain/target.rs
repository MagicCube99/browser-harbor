use serde::{Deserialize, Serialize};

use super::{
    release::{CpuBuild, ProgramPlacement, ProviderKind, ReleaseChannel},
    runtime::RuntimeKind,
    version::VersionSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowserFamily {
    Chromium,
    Firefox,
}

impl BrowserFamily {
    pub const ALL: [Self; 2] = [Self::Chromium, Self::Firefox];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SupportLevel {
    Standard,
    Experimental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetSpec {
    pub target: TargetId,
    pub display_name: &'static str,
    pub family: BrowserFamily,
    pub support_level: SupportLevel,
    pub provider: ProviderKind,
    pub runtime: RuntimeKind,
    pub default_channel: ReleaseChannel,
    pub channels: &'static [ReleaseChannel],
    pub default_cpu_build: CpuBuild,
    pub cpu_builds: &'static [CpuBuild],
    pub placement: ProgramPlacement,
    pub layout: TargetLayout,
    pub version_source: VersionSource,
    pub executable_candidates: &'static [&'static str],
    pub preserved_program_files: &'static [&'static str],
    pub display_beta: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetLayout {
    pub program: Option<&'static str>,
    pub package_prefix: Option<&'static str>,
    pub data: &'static str,
    pub profile: &'static str,
    pub cache: &'static str,
}

const APP_PROFILES_LAYOUT: TargetLayout = TargetLayout {
    program: Some("App"),
    package_prefix: None,
    data: "Profiles",
    profile: "Profiles",
    cache: r"Profiles\Cache",
};
const STABLE: &[ReleaseChannel] = &[ReleaseChannel::Stable];
const ICEWEASEL_CHANNELS: &[ReleaseChannel] = &[ReleaseChannel::Esr, ReleaseChannel::Release];
const X64_ONLY: &[CpuBuild] = &[CpuBuild::X64];
const THORIUM_BUILDS: &[CpuBuild] = &[
    CpuBuild::Sse3,
    CpuBuild::Sse4,
    CpuBuild::Avx,
    CpuBuild::Avx2,
    CpuBuild::Avx512,
];
const CHROME_PLUS_FILES: &[&str] = &["version.dll", "chrome++.ini"];
const FIREFOX_RUNTIME_FILES: &[&str] = &["portable.ini"];
const CHROMIUM_VERSION: VersionSource = VersionSource::ExecutableResource;
const HELIUM_VERSION: VersionSource = VersionSource::ExecutableFileVersion;
const BRAVE_VERSION: VersionSource = VersionSource::BraveProductVersion;
const FIREFOX_VERSION: VersionSource = VersionSource::ApplicationIni;

const TARGET_SPECS: [TargetSpec; 14] = [
    TargetSpec {
        target: TargetId::Brave,
        display_name: "Brave",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Experimental,
        provider: ProviderKind::GitHubRelease,
        runtime: RuntimeKind::ChromePlusFull,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("App"),
            package_prefix: Some("App"),
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: BRAVE_VERSION,
        executable_candidates: &["brave.exe", "chrome.exe"],
        preserved_program_files: CHROME_PLUS_FILES,
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Catsxp,
        display_name: "Catsxp",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::Catsxp,
        runtime: RuntimeKind::CatsxpLauncher,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("App"),
            package_prefix: Some("App"),
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: CHROMIUM_VERSION,
        executable_candidates: &["catsxp.exe", "chrome.exe"],
        preserved_program_files: &["StartupParm.txt"],
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Chrome,
        display_name: "Chrome",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::ChromeOmaha,
        runtime: RuntimeKind::ChromePlusFull,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::App,
        layout: APP_PROFILES_LAYOUT,
        version_source: CHROMIUM_VERSION,
        executable_candidates: &["chrome.exe"],
        preserved_program_files: CHROME_PLUS_FILES,
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Edge,
        display_name: "Edge",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::EdgeEnterprise,
        runtime: RuntimeKind::ChromePlusFull,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::EdgeVersioned,
        layout: TargetLayout {
            program: Some(r"App\Microsoft\Edge\Application"),
            package_prefix: None,
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: CHROMIUM_VERSION,
        executable_candidates: &["msedge.exe"],
        preserved_program_files: CHROME_PLUS_FILES,
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Firefox,
        display_name: "Firefox",
        family: BrowserFamily::Firefox,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::Mozilla,
        runtime: RuntimeKind::Libportable,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::App,
        layout: APP_PROFILES_LAYOUT,
        version_source: FIREFOX_VERSION,
        executable_candidates: &["firefox.exe"],
        preserved_program_files: FIREFOX_RUNTIME_FILES,
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Floorp,
        display_name: "Floorp",
        family: BrowserFamily::Firefox,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::GitHubRelease,
        runtime: RuntimeKind::FloorpLauncher,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("app"),
            package_prefix: None,
            data: "data",
            profile: "data",
            cache: "data",
        },
        version_source: FIREFOX_VERSION,
        executable_candidates: &["floorp.exe"],
        preserved_program_files: &["floorp-portable.yml"],
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Helium,
        display_name: "Helium",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Experimental,
        provider: ProviderKind::GitHubRelease,
        runtime: RuntimeKind::ChromePlusFull,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("App"),
            package_prefix: Some("App"),
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: HELIUM_VERSION,
        executable_candidates: &["chrome.exe", "helium.exe"],
        preserved_program_files: CHROME_PLUS_FILES,
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Iceweasel,
        display_name: "Iceweasel",
        family: BrowserFamily::Firefox,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::GitHubRelease,
        runtime: RuntimeKind::IceweaselNative,
        default_channel: ReleaseChannel::Esr,
        channels: ICEWEASEL_CHANNELS,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("App"),
            package_prefix: None,
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: FIREFOX_VERSION,
        executable_candidates: &["iceweasel.exe", "firefox.exe"],
        preserved_program_files: &["portable.ini"],
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::LibreWolf,
        display_name: "LibreWolf",
        family: BrowserFamily::Firefox,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::LibreWolf,
        runtime: RuntimeKind::LibreWolfLauncher,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("LibreWolf"),
            package_prefix: None,
            data: "Profiles",
            profile: "Profiles",
            cache: "Profiles",
        },
        version_source: FIREFOX_VERSION,
        executable_candidates: &["librewolf.exe"],
        preserved_program_files: &[],
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::SkyKakapo,
        display_name: "SkyKakapo",
        family: BrowserFamily::Firefox,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::SkyKakapo,
        runtime: RuntimeKind::SkyKakapoNative,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::Sse3,
        cpu_builds: &[CpuBuild::Sse3],
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("App"),
            package_prefix: Some("App"),
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: FIREFOX_VERSION,
        executable_candidates: &["skykakapo.exe", "firefox.exe"],
        preserved_program_files: &["tmemutil.ini"],
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Thorium,
        display_name: "Thorium",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::GitHubRelease,
        runtime: RuntimeKind::ChromePlusFull,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::Sse3,
        cpu_builds: THORIUM_BUILDS,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("BIN"),
            package_prefix: None,
            data: "USER_DATA",
            profile: "USER_DATA",
            cache: "USER_DATA",
        },
        version_source: CHROMIUM_VERSION,
        executable_candidates: &["thorium.exe", "chrome.exe"],
        preserved_program_files: CHROME_PLUS_FILES,
        display_beta: true,
    },
    TargetSpec {
        target: TargetId::Turbo,
        display_name: "Turbo",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::Turbo,
        runtime: RuntimeKind::TurboLauncher,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: None,
            package_prefix: None,
            data: "User Data",
            profile: "User Data",
            cache: "User Data",
        },
        version_source: CHROMIUM_VERSION,
        executable_candidates: &["turbo.exe", "chrome.exe"],
        preserved_program_files: &[],
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::UngoogledChromium,
        display_name: "Ungoogled Chromium",
        family: BrowserFamily::Chromium,
        support_level: SupportLevel::Experimental,
        provider: ProviderKind::GitHubRelease,
        runtime: RuntimeKind::ChromePlusFull,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some("App"),
            package_prefix: Some("App"),
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: CHROMIUM_VERSION,
        executable_candidates: &["chrome.exe"],
        preserved_program_files: CHROME_PLUS_FILES,
        display_beta: false,
    },
    TargetSpec {
        target: TargetId::Zen,
        display_name: "Zen",
        family: BrowserFamily::Firefox,
        support_level: SupportLevel::Standard,
        provider: ProviderKind::GitHubRelease,
        runtime: RuntimeKind::Libportable,
        default_channel: ReleaseChannel::Stable,
        channels: STABLE,
        default_cpu_build: CpuBuild::X64,
        cpu_builds: X64_ONLY,
        placement: ProgramPlacement::NativePackage,
        layout: TargetLayout {
            program: Some(r"App\core"),
            package_prefix: Some("App"),
            data: "Profiles",
            profile: "Profiles",
            cache: r"Profiles\Cache",
        },
        version_source: FIREFOX_VERSION,
        executable_candidates: &["zen.exe"],
        preserved_program_files: FIREFOX_RUNTIME_FILES,
        display_beta: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromePlusMode {
    Disabled,
    DataOnly,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetId {
    Brave,
    Catsxp,
    Chrome,
    Edge,
    Firefox,
    Floorp,
    Helium,
    Iceweasel,
    LibreWolf,
    SkyKakapo,
    Thorium,
    Turbo,
    UngoogledChromium,
    Zen,
}

impl TargetId {
    pub const ALL: [Self; 14] = [
        Self::Brave,
        Self::Catsxp,
        Self::Chrome,
        Self::Edge,
        Self::Firefox,
        Self::Floorp,
        Self::Helium,
        Self::Iceweasel,
        Self::LibreWolf,
        Self::SkyKakapo,
        Self::Thorium,
        Self::Turbo,
        Self::UngoogledChromium,
        Self::Zen,
    ];

    pub const fn sort_key(self) -> &'static str {
        match self {
            Self::Brave => "brave",
            Self::Catsxp => "catsxp",
            Self::Chrome => "chrome",
            Self::Edge => "edge",
            Self::Firefox => "firefox",
            Self::Floorp => "floorp",
            Self::Helium => "helium",
            Self::Iceweasel => "iceweasel",
            Self::LibreWolf => "librewolf",
            Self::SkyKakapo => "skykakapo",
            Self::Thorium => "thorium",
            Self::Turbo => "turbo",
            Self::UngoogledChromium => "ungoogled_chromium",
            Self::Zen => "zen",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Brave => "Brave",
            Self::Catsxp => "Catsxp",
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Firefox => "Firefox",
            Self::Floorp => "Floorp",
            Self::Helium => "Helium",
            Self::Iceweasel => "Iceweasel",
            Self::LibreWolf => "LibreWolf",
            Self::SkyKakapo => "SkyKakapo",
            Self::Thorium => "Thorium",
            Self::Turbo => "Turbo",
            Self::UngoogledChromium => "Ungoogled Chromium",
            Self::Zen => "Zen",
        }
    }

    pub const fn family(self) -> BrowserFamily {
        match self {
            Self::Brave
            | Self::Catsxp
            | Self::Chrome
            | Self::Edge
            | Self::Helium
            | Self::Thorium
            | Self::Turbo
            | Self::UngoogledChromium => BrowserFamily::Chromium,
            Self::Firefox
            | Self::Floorp
            | Self::Iceweasel
            | Self::LibreWolf
            | Self::SkyKakapo
            | Self::Zen => BrowserFamily::Firefox,
        }
    }

    pub const fn support_level(self) -> SupportLevel {
        match self {
            Self::Brave | Self::Helium | Self::UngoogledChromium => SupportLevel::Experimental,
            _ => SupportLevel::Standard,
        }
    }

    pub const fn chrome_plus_mode(self) -> ChromePlusMode {
        match self {
            Self::Brave
            | Self::Chrome
            | Self::Edge
            | Self::Helium
            | Self::Thorium
            | Self::UngoogledChromium => ChromePlusMode::Full,
            _ => ChromePlusMode::Disabled,
        }
    }

    pub const fn cache_follows_profile(self) -> bool {
        matches!(
            self,
            Self::Brave
                | Self::Chrome
                | Self::Edge
                | Self::Firefox
                | Self::Floorp
                | Self::Helium
                | Self::Iceweasel
                | Self::LibreWolf
                | Self::SkyKakapo
                | Self::Thorium
                | Self::UngoogledChromium
                | Self::Zen
        )
    }

    pub const fn supports_custom_cache(self) -> bool {
        !matches!(self, Self::Floorp | Self::LibreWolf | Self::SkyKakapo)
    }

    pub const fn spec(self) -> &'static TargetSpec {
        let index = match self {
            Self::Brave => 0,
            Self::Catsxp => 1,
            Self::Chrome => 2,
            Self::Edge => 3,
            Self::Firefox => 4,
            Self::Floorp => 5,
            Self::Helium => 6,
            Self::Iceweasel => 7,
            Self::LibreWolf => 8,
            Self::SkyKakapo => 9,
            Self::Thorium => 10,
            Self::Turbo => 11,
            Self::UngoogledChromium => 12,
            Self::Zen => 13,
        };
        &TARGET_SPECS[index]
    }
}

#[cfg(test)]
mod tests {
    use super::{BrowserFamily, SupportLevel, TargetId};

    #[test]
    fn all_targets_are_sorted_by_english_key() {
        let keys: Vec<_> = TargetId::ALL
            .iter()
            .copied()
            .map(TargetId::sort_key)
            .collect();

        assert_eq!(
            keys,
            [
                "brave",
                "catsxp",
                "chrome",
                "edge",
                "firefox",
                "floorp",
                "helium",
                "iceweasel",
                "librewolf",
                "skykakapo",
                "thorium",
                "turbo",
                "ungoogled_chromium",
                "zen",
            ]
        );
    }

    #[test]
    fn brand_names_do_not_have_localized_suffixes() {
        assert_eq!(TargetId::Catsxp.display_name(), "Catsxp");
        assert_eq!(TargetId::Turbo.display_name(), "Turbo");
    }

    #[test]
    fn browser_families_have_fixed_membership_and_order() {
        assert_eq!(
            BrowserFamily::ALL,
            [BrowserFamily::Chromium, BrowserFamily::Firefox]
        );

        let chromium: Vec<_> = TargetId::ALL
            .iter()
            .copied()
            .filter(|target| target.family() == BrowserFamily::Chromium)
            .map(TargetId::sort_key)
            .collect();
        let firefox: Vec<_> = TargetId::ALL
            .iter()
            .copied()
            .filter(|target| target.family() == BrowserFamily::Firefox)
            .map(TargetId::sort_key)
            .collect();

        assert_eq!(
            chromium,
            [
                "brave",
                "catsxp",
                "chrome",
                "edge",
                "helium",
                "thorium",
                "turbo",
                "ungoogled_chromium",
            ]
        );
        assert_eq!(
            firefox,
            [
                "firefox",
                "floorp",
                "iceweasel",
                "librewolf",
                "skykakapo",
                "zen",
            ]
        );
    }

    #[test]
    fn only_unverified_chromium_forks_are_experimental() {
        let experimental: Vec<_> = TargetId::ALL
            .iter()
            .copied()
            .filter(|target| target.support_level() == SupportLevel::Experimental)
            .map(TargetId::sort_key)
            .collect();

        assert_eq!(experimental, ["brave", "helium", "ungoogled_chromium"]);
    }

    #[test]
    fn cache_policy_matches_runtime_capabilities() {
        let profile_default = TargetId::ALL
            .into_iter()
            .filter(|target| target.cache_follows_profile())
            .map(TargetId::sort_key)
            .collect::<Vec<_>>();
        assert_eq!(
            profile_default,
            [
                "brave",
                "chrome",
                "edge",
                "firefox",
                "floorp",
                "helium",
                "iceweasel",
                "librewolf",
                "skykakapo",
                "thorium",
                "ungoogled_chromium",
                "zen",
            ]
        );

        let no_custom_cache = TargetId::ALL
            .into_iter()
            .filter(|target| !target.supports_custom_cache())
            .map(TargetId::sort_key)
            .collect::<Vec<_>>();
        assert_eq!(no_custom_cache, ["floorp", "librewolf", "skykakapo"]);
    }

    #[test]
    fn every_target_has_a_consistent_target_spec() {
        for target in TargetId::ALL {
            let spec = target.spec();
            assert_eq!(spec.target, target);
            assert_eq!(spec.display_name, target.display_name());
            assert_eq!(spec.family, target.family());
            assert_eq!(spec.support_level, target.support_level());
            assert!(spec.channels.contains(&spec.default_channel));
            assert!(spec.cpu_builds.contains(&spec.default_cpu_build));
            assert!(!spec.executable_candidates.is_empty());
            assert!(!spec.layout.data.is_empty());
            assert!(!spec.layout.profile.is_empty());
            assert!(!spec.layout.cache.is_empty());
        }
    }

    #[test]
    fn chromium_uses_executable_version_and_firefox_uses_application_ini() {
        use super::{BrowserFamily, VersionSource};

        for target in TargetId::ALL {
            let expected = match target.family() {
                BrowserFamily::Chromium if target == super::TargetId::Helium => {
                    VersionSource::ExecutableFileVersion
                }
                BrowserFamily::Chromium if target == super::TargetId::Brave => {
                    VersionSource::BraveProductVersion
                }
                BrowserFamily::Chromium => VersionSource::ExecutableResource,
                BrowserFamily::Firefox => VersionSource::ApplicationIni,
            };
            assert_eq!(
                target.spec().version_source,
                expected,
                "{}",
                target.sort_key()
            );
        }
    }

    #[test]
    fn every_target_declares_its_real_portable_layout() {
        let cases = [
            (
                TargetId::Brave,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::Catsxp,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::Chrome,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::Edge,
                Some(r"App\Microsoft\Edge\Application"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::Firefox,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (TargetId::Floorp, Some("app"), "data", "data", "data"),
            (
                TargetId::Helium,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::Iceweasel,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::LibreWolf,
                Some("LibreWolf"),
                "Profiles",
                "Profiles",
                "Profiles",
            ),
            (
                TargetId::SkyKakapo,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::Thorium,
                Some("BIN"),
                "USER_DATA",
                "USER_DATA",
                "USER_DATA",
            ),
            (TargetId::Turbo, None, "User Data", "User Data", "User Data"),
            (
                TargetId::UngoogledChromium,
                Some("App"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
            (
                TargetId::Zen,
                Some(r"App\core"),
                "Profiles",
                "Profiles",
                r"Profiles\Cache",
            ),
        ];

        for (target, program, data, profile, cache) in cases {
            let layout = target.spec().layout;
            assert_eq!(layout.program, program, "{} program", target.sort_key());
            assert_eq!(layout.data, data, "{} data", target.sort_key());
            assert_eq!(layout.profile, profile, "{} profile", target.sort_key());
            assert_eq!(layout.cache, cache, "{} cache", target.sort_key());
            assert_eq!(
                layout.package_prefix,
                match target {
                    TargetId::Brave
                    | TargetId::Catsxp
                    | TargetId::Helium
                    | TargetId::SkyKakapo
                    | TargetId::UngoogledChromium
                    | TargetId::Zen => Some("App"),
                    _ => None,
                },
                "{} package prefix",
                target.sort_key()
            );
        }
    }

    #[test]
    fn iceweasel_and_thorium_expose_target_specific_choices() {
        use super::ReleaseChannel;

        assert_eq!(
            TargetId::Iceweasel.spec().channels,
            &[ReleaseChannel::Esr, ReleaseChannel::Release]
        );
        assert_eq!(
            TargetId::Iceweasel.spec().default_channel,
            ReleaseChannel::Esr
        );
        assert_eq!(TargetId::Thorium.spec().cpu_builds.len(), 5);
        assert_eq!(
            TargetId::Thorium.spec().default_cpu_build,
            super::CpuBuild::Sse3
        );
        assert!(TargetId::Thorium.spec().display_beta);
    }
}
