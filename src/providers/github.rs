use std::cmp::Reverse;

use serde::Deserialize;

use super::ProviderError;
use crate::{
    domain::{
        release::{
            ArchiveFormat, Artifact, CpuBuild, Integrity, ProviderKind, ReleaseChannel,
            ReleaseDescriptor,
        },
        target::TargetId,
    },
    services::http::{HttpClient, github_api_candidates},
};

const RELEASES_PER_PAGE: u32 = 30;

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

pub async fn resolve(
    http: &HttpClient,
    target: TargetId,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    let (owner, repository) = github_repository(target);
    let releases = if target == TargetId::Iceweasel {
        let url = format!(
            "https://api.github.com/repos/{owner}/{repository}/releases?per_page={RELEASES_PER_PAGE}"
        );
        http.get_json_candidates::<Vec<GitHubRelease>>(&github_api_candidates(&url))
            .await?
    } else {
        let url = format!("https://api.github.com/repos/{owner}/{repository}/releases/latest");
        vec![
            http.get_json_candidates::<GitHubRelease>(&github_api_candidates(&url))
                .await?,
        ]
    };
    let release = select_release(releases, target, channel)?;
    let asset =
        select_asset(&release, target, cpu_build).ok_or(ProviderError::NoCompatibleAsset)?;
    let integrity = resolve_integrity(http, &release, asset).await?;
    let format =
        ArchiveFormat::from_file_name(&asset.name).ok_or(ProviderError::NoCompatibleAsset)?;
    let version = version_from_tag(target, &release.tag_name);
    Ok(ReleaseDescriptor {
        target,
        version,
        channel,
        cpu_build,
        provider: ProviderKind::GitHubRelease,
        artifact: Artifact {
            file_name: asset.name.clone(),
            format,
            size: Some(asset.size),
            integrity,
            urls: vec![asset.browser_download_url.clone()],
            signature_url: None,
        },
        placement: target.spec().placement,
        inner_archive: None,
        source: release.html_url.clone(),
        published_at: release.published_at.clone(),
    })
}

fn github_repository(target: TargetId) -> (&'static str, &'static str) {
    match target {
        TargetId::Brave => ("brave", "brave-browser"),
        TargetId::Floorp => ("Floorp-Projects", "Floorp-Portable-v2"),
        TargetId::Helium => ("imputnet", "helium-windows"),
        TargetId::Iceweasel => ("adonais", "iceweasel"),
        TargetId::Thorium => ("gz83", "thorium"),
        TargetId::UngoogledChromium => ("ungoogled-software", "ungoogled-chromium-windows"),
        TargetId::Zen => ("zen-browser", "desktop"),
        _ => ("", ""),
    }
}

fn select_release(
    releases: Vec<GitHubRelease>,
    target: TargetId,
    channel: ReleaseChannel,
) -> Result<GitHubRelease, ProviderError> {
    let mut releases = releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .collect::<Vec<_>>();
    if target != TargetId::Iceweasel {
        return releases.pop().ok_or(ProviderError::NoCompatibleAsset);
    }
    releases.sort_by_key(|release| Reverse(version_key(&release.tag_name)));
    releases
        .into_iter()
        .find(|release| match channel {
            ReleaseChannel::Esr => release.tag_name.to_ascii_lowercase().contains("esr"),
            ReleaseChannel::Release => !release.tag_name.to_ascii_lowercase().contains("esr"),
            ReleaseChannel::Stable => false,
        })
        .ok_or(ProviderError::NoCompatibleAsset)
}

fn select_asset(
    release: &GitHubRelease,
    target: TargetId,
    cpu_build: CpuBuild,
) -> Option<&GitHubAsset> {
    let version = version_from_tag(target, &release.tag_name);
    release.assets.iter().find(|asset| match target {
        TargetId::Brave => {
            asset.name == format!("brave-v{version}-win32-x64.zip")
                && !asset.name.contains("symbols")
                && !asset.name.contains("origin")
        }
        TargetId::Floorp => asset.name.ends_with(".portable.7z"),
        TargetId::Helium => {
            asset.name.contains("x64-windows.zip") && !asset.name.contains("installer")
        }
        TargetId::Iceweasel => asset.name.contains("win64") && asset.name.ends_with(".7z"),
        TargetId::Thorium => {
            asset.name.starts_with("Thorium_")
                && asset
                    .name
                    .contains(&format!("_{}_", cpu_build.display_name()))
                && asset.name.ends_with(".zip")
                && !asset.name.contains("mini_installer")
        }
        TargetId::UngoogledChromium => asset.name.ends_with("_windows_x64.zip"),
        TargetId::Zen => asset.name == "zen.installer.exe",
        _ => false,
    })
}

async fn resolve_integrity(
    http: &HttpClient,
    release: &GitHubRelease,
    asset: &GitHubAsset,
) -> Result<Option<Integrity>, ProviderError> {
    if let Some(digest) = asset.digest.as_deref().and_then(Integrity::from_digest) {
        return Ok(Some(digest));
    }
    if let Some(sidecar) = release
        .assets
        .iter()
        .find(|candidate| candidate.name == format!("{}.sha256", asset.name))
    {
        let text = http.get_text(&sidecar.browser_download_url).await?;
        let digest = text
            .split_whitespace()
            .next()
            .and_then(|value| Integrity::from_digest(&format!("sha256:{value}")));
        if digest.is_some() {
            return Ok(digest);
        }
    }
    Ok(None)
}

fn version_from_tag(target: TargetId, tag: &str) -> String {
    match target {
        TargetId::Thorium => tag.trim_start_matches('M').to_owned(),
        TargetId::UngoogledChromium => tag
            .trim_start_matches('v')
            .split('-')
            .next()
            .unwrap_or_default()
            .to_owned(),
        _ => tag.trim_start_matches('v').to_owned(),
    }
}

fn version_key(tag: &str) -> Vec<u64> {
    tag.trim_start_matches('v')
        .trim_end_matches("esr")
        .split(['.', '-', 'b'])
        .filter_map(|part| part.parse::<u64>().ok())
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::{GitHubAsset, GitHubRelease, select_asset, select_release, version_from_tag};
    use crate::domain::{
        release::{CpuBuild, ReleaseChannel},
        target::TargetId,
    };

    fn release(tag: &str, assets: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.to_owned(),
            draft: false,
            prerelease: false,
            published_at: None,
            html_url: "https://github.com/example/release".to_owned(),
            assets: assets
                .iter()
                .map(|name| GitHubAsset {
                    name: (*name).to_owned(),
                    size: 1,
                    digest: Some(format!("sha256:{}", "a".repeat(64))),
                    browser_download_url: format!("https://github.com/file/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn target_asset_selectors_choose_portable_x64_payloads() {
        let cases = [
            (
                TargetId::Brave,
                release("v1.2.3", &["brave-v1.2.3-win32-x64.zip"]),
                CpuBuild::X64,
                "brave-v1.2.3-win32-x64.zip",
            ),
            (
                TargetId::Helium,
                release("0.1", &["helium_0.1_x64-windows.zip"]),
                CpuBuild::X64,
                "helium_0.1_x64-windows.zip",
            ),
            (
                TargetId::Thorium,
                release(
                    "M156.0",
                    &[
                        "thorium-browser_156.0_AVX2.zip",
                        "Thorium_AVX2_156.0.zip",
                        "thorium_AVX2_156.0_mini_installer.exe",
                    ],
                ),
                CpuBuild::Avx2,
                "Thorium_AVX2_156.0.zip",
            ),
            (
                TargetId::Zen,
                release("1.0", &["zen.installer.exe"]),
                CpuBuild::X64,
                "zen.installer.exe",
            ),
        ];
        for (target, release, cpu, expected) in cases {
            assert_eq!(select_asset(&release, target, cpu).unwrap().name, expected);
        }
    }

    #[test]
    fn iceweasel_channel_selection_keeps_product_lines_separate() {
        let releases = vec![
            release("v158.0.1", &[]),
            release("v115.44.0esr", &[]),
            release("v157.0.1", &[]),
        ];
        let esr =
            select_release(releases.clone(), TargetId::Iceweasel, ReleaseChannel::Esr).unwrap();
        let mainstream =
            select_release(releases, TargetId::Iceweasel, ReleaseChannel::Release).unwrap();
        assert_eq!(esr.tag_name, "v115.44.0esr");
        assert_eq!(mainstream.tag_name, "v158.0.1");
    }

    #[test]
    fn ungoogled_package_revision_is_not_part_of_the_browser_version() {
        assert_eq!(
            version_from_tag(TargetId::UngoogledChromium, "155.0.8059.84-1.1"),
            "155.0.8059.84"
        );
    }
}
