use std::{collections::BTreeSet, io::Cursor};

use base64::{Engine, engine::general_purpose::STANDARD};
use pgp::{
    composed::{Deserializable, DetachedSignature, SignedPublicKey},
    types::KeyDetails,
};
use regex_lite::Regex;
use serde::Deserialize;
use url::Url;

use super::ProviderError;
use crate::{
    domain::{
        release::{
            ArchiveFormat, Artifact, CpuBuild, Integrity, ProviderKind, ReleaseChannel,
            ReleaseDescriptor,
        },
        target::TargetId,
    },
    services::http::HttpClient,
};

const CATSXP_HOME: &str = "https://www.catsxp.com/";
const SKYKAKAPO_HOME: &str = "https://tete009.pages.dev/en-US/software";
const TURBO_UPDATE: &str = "https://tbrowser.cn/update/update.js";
const VIVALDI_VERSIONS: &str = "https://vivaldi.com/wp-content/vivaldi-versions.js";
const LIBREWOLF_WINDOWS: &str = "https://librewolf.net/installation/windows/";
const LIBREWOLF_RELEASES: &str =
    "https://librewolf.dev/api/v1/repos/librewolf/bsys6/releases?limit=1";
const LIBREWOLF_KEY: &str = include_str!("keys/librewolf.asc");
const LIBREWOLF_FINGERPRINT: &str = "662E3CDD6FE329002D0CA5BB40339DD82B12EF16";

pub async fn resolve_catsxp(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    validate_stable_x64(channel, cpu_build)?;
    let html = http.get_text(CATSXP_HOME).await?;
    let parsed = parse_catsxp(&html)?;
    Ok(ReleaseDescriptor {
        target: TargetId::Catsxp,
        version: parsed.version,
        channel,
        cpu_build,
        provider: ProviderKind::Catsxp,
        artifact: Artifact {
            file_name: parsed.file_name,
            format: ArchiveFormat::Zip,
            size: None,
            integrity: None,
            urls: parsed.urls,
            signature_url: None,
        },
        placement: TargetId::Catsxp.spec().placement,
        inner_archive: None,
        source: CATSXP_HOME.to_owned(),
        published_at: None,
    })
}

pub async fn resolve_skykakapo(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    if channel != ReleaseChannel::Stable || cpu_build != CpuBuild::Sse3 {
        return Err(ProviderError::UnsupportedSelection);
    }
    let html = http.get_text(SKYKAKAPO_HOME).await?;
    let parsed = parse_skykakapo(&html)?;
    Ok(ReleaseDescriptor {
        target: TargetId::SkyKakapo,
        version: parsed.version,
        channel,
        cpu_build,
        provider: ProviderKind::SkyKakapo,
        artifact: Artifact {
            file_name: parsed.file_name,
            format: ArchiveFormat::SevenZip,
            size: None,
            integrity: Some(Integrity::Sha256(parsed.sha256)),
            urls: vec![parsed.url],
            signature_url: None,
        },
        placement: TargetId::SkyKakapo.spec().placement,
        inner_archive: None,
        source: SKYKAKAPO_HOME.to_owned(),
        published_at: None,
    })
}

pub async fn resolve_turbo(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    validate_stable_x64(channel, cpu_build)?;
    let script = http.get_text(TURBO_UPDATE).await?;
    let parsed = parse_turbo(&script)?;
    Ok(ReleaseDescriptor {
        target: TargetId::Turbo,
        version: parsed.version,
        channel,
        cpu_build,
        provider: ProviderKind::Turbo,
        artifact: Artifact {
            file_name: parsed.file_name,
            format: ArchiveFormat::SevenZip,
            size: None,
            integrity: None,
            urls: vec![parsed.url],
            signature_url: None,
        },
        placement: TargetId::Turbo.spec().placement,
        inner_archive: None,
        source: TURBO_UPDATE.to_owned(),
        published_at: None,
    })
}

pub async fn resolve_vivaldi(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    validate_stable_x64(channel, cpu_build)?;
    let script = http.get_text(VIVALDI_VERSIONS).await?;
    let parsed = parse_vivaldi(&script)?;
    Ok(ReleaseDescriptor {
        target: TargetId::Vivaldi,
        version: parsed.version,
        channel,
        cpu_build,
        provider: ProviderKind::Vivaldi,
        artifact: Artifact {
            file_name: parsed.file_name,
            format: ArchiveFormat::SevenZip,
            size: None,
            integrity: None,
            urls: vec![parsed.url],
            signature_url: None,
        },
        placement: TargetId::Vivaldi.spec().placement,
        inner_archive: Some("vivaldi.7z".to_owned()),
        source: VIVALDI_VERSIONS.to_owned(),
        published_at: None,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct VivaldiArtifact {
    version: String,
    file_name: String,
    url: String,
}

fn parse_vivaldi(script: &str) -> Result<VivaldiArtifact, ProviderError> {
    let json_str = script
        .split_once('=')
        .map(|(_, right)| right.trim().trim_end_matches(';'))
        .unwrap_or(script);
    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|error| ProviderError::Parse(error.to_string()))?;
    let version = value
        .get("vivaldi_version_number")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ProviderError::Parse("missing vivaldi_version_number".to_owned()))?
        .to_owned();
    let file_name = value
        .get("vivaldi_version_win64")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Vivaldi.{version}.x64.exe"));
    let url = format!("https://downloads.vivaldi.com/stable/{file_name}");
    Ok(VivaldiArtifact {
        version,
        file_name,
        url,
    })
}

pub async fn resolve_librewolf(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    validate_stable_x64(channel, cpu_build)?;
    let forge = match http.get_json::<Vec<ForgeRelease>>(LIBREWOLF_RELEASES).await {
        Ok(releases) => parse_forge_release(&releases),
        Err(error) => Err(ProviderError::Service(error)),
    };
    let parsed = match forge {
        Ok(parsed) => parsed,
        Err(_) => {
            let html = http.get_text(LIBREWOLF_WINDOWS).await?;
            let parsed = parse_librewolf(&html)?;
            ForgeArtifact {
                version: parsed.version,
                file_name: parsed.file_name,
                url: parsed.url.clone(),
                hash_url: format!("{}.sha256sum", parsed.url),
                signature_url: format!("{}.sig", parsed.url),
                published_at: None,
            }
        }
    };
    let hash_text = http.get_text(&parsed.hash_url).await?;
    let sha256 = hash_text
        .split_whitespace()
        .next()
        .filter(|value| value.len() == 64)
        .map(str::to_ascii_lowercase)
        .ok_or(ProviderError::NoCompatibleAsset)?;
    Ok(ReleaseDescriptor {
        target: TargetId::LibreWolf,
        version: parsed.version,
        channel,
        cpu_build,
        provider: ProviderKind::LibreWolf,
        artifact: Artifact {
            file_name: parsed.file_name,
            format: ArchiveFormat::Zip,
            size: None,
            integrity: Some(Integrity::Sha256(sha256)),
            urls: vec![parsed.url.clone()],
            signature_url: Some(parsed.signature_url.clone()),
        },
        placement: TargetId::LibreWolf.spec().placement,
        inner_archive: None,
        source: LIBREWOLF_RELEASES.to_owned(),
        published_at: parsed.published_at,
    })
}

pub fn verify_librewolf_artifact_signature(
    content: &[u8],
    signature: &[u8],
) -> Result<bool, ProviderError> {
    let (key, _) = SignedPublicKey::from_armor_single(Cursor::new(LIBREWOLF_KEY.as_bytes()))
        .map_err(|error| ProviderError::Parse(error.to_string()))?;
    let fingerprint = key
        .fingerprint()
        .to_string()
        .replace(' ', "")
        .to_ascii_uppercase();
    if fingerprint != LIBREWOLF_FINGERPRINT {
        return Err(ProviderError::Parse(
            "LibreWolf signing key fingerprint changed".to_owned(),
        ));
    }
    let signature = DetachedSignature::from_bytes(Cursor::new(signature))
        .map_err(|error| ProviderError::Parse(error.to_string()))?;
    if signature.verify(&key, content).is_ok() {
        return Ok(true);
    }
    Ok(key
        .public_subkeys
        .iter()
        .any(|subkey| signature.verify(subkey, content).is_ok()))
}

#[derive(Debug, PartialEq, Eq)]
struct WebsiteArtifact {
    version: String,
    file_name: String,
    url: String,
    sha256: String,
    urls: Vec<String>,
}

fn parse_catsxp(html: &str) -> Result<WebsiteArtifact, ProviderError> {
    let encoded = Regex::new(r#"data-url="([A-Za-z0-9+/=]+)""#)
        .map_err(|error| ProviderError::Parse(error.to_string()))?;
    let mut urls = BTreeSet::new();
    for capture in encoded.captures_iter(html) {
        let Some(decoded) = decode_catsxp_url(&capture[1]) else {
            continue;
        };
        if decoded.contains("/catsxp_portable/win_x64/portable_x64_release_")
            && decoded.ends_with(".zip")
        {
            urls.insert(decoded);
        }
    }
    if urls.is_empty() {
        return Err(ProviderError::NoCompatibleAsset);
    }
    let urls = urls.into_iter().collect::<Vec<_>>();
    let primary = urls
        .iter()
        .find(|url| url.starts_with("https://b2.catsxp.com/"))
        .or_else(|| urls.first())
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let file_name = file_name_from_url(primary)?;
    let version = file_name
        .strip_prefix("portable_x64_release_")
        .and_then(|value| value.strip_suffix(".zip"))
        .map(|value| value.replace('_', "."))
        .ok_or(ProviderError::NoCompatibleAsset)?;
    Ok(WebsiteArtifact {
        version,
        file_name,
        url: primary.clone(),
        sha256: String::new(),
        urls,
    })
}

#[derive(Debug, Deserialize)]
struct ForgeRelease {
    tag_name: String,
    published_at: Option<String>,
    assets: Vec<ForgeAsset>,
}

#[derive(Debug, Deserialize)]
struct ForgeAsset {
    name: String,
    browser_download_url: String,
}

struct ForgeArtifact {
    version: String,
    file_name: String,
    url: String,
    hash_url: String,
    signature_url: String,
    published_at: Option<String>,
}

fn parse_forge_release(releases: &[ForgeRelease]) -> Result<ForgeArtifact, ProviderError> {
    let release = releases.first().ok_or(ProviderError::NoCompatibleAsset)?;
    let asset = release
        .assets
        .iter()
        .find(|asset| {
            asset.name.contains("-windows-x86_64-portable.zip")
                && !asset.name.ends_with(".sha256sum")
                && !asset.name.ends_with(".sig")
        })
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let version = release
        .tag_name
        .rsplit_once('-')
        .map_or(release.tag_name.as_str(), |(version, _)| version)
        .to_owned();
    Ok(ForgeArtifact {
        version,
        file_name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        hash_url: format!("{}.sha256sum", asset.browser_download_url),
        signature_url: format!("{}.sig", asset.browser_download_url),
        published_at: release.published_at.clone(),
    })
}

fn parse_skykakapo(html: &str) -> Result<WebsiteArtifact, ProviderError> {
    let file_pattern =
        Regex::new(r#"skykakapo-([0-9.]+)-[A-Za-z0-9._-]+\.win64-sse3[A-Za-z0-9._-]*\.7z"#)
            .map_err(|error| ProviderError::Parse(error.to_string()))?;
    let hash_pattern = Regex::new(r"(?i)SHA256</abbr>:\s*([0-9a-f]{64})")
        .map_err(|error| ProviderError::Parse(error.to_string()))?;
    let file_capture = file_pattern
        .captures(html)
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let file_name = file_capture[0].to_owned();
    let version = file_capture[1].to_owned();
    let sha256 = hash_pattern
        .captures(html)
        .map(|capture| capture[1].to_ascii_lowercase())
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let url =
        format!("https://sourceforge.net/projects/skykakapo/files/{version}/{file_name}/download");
    Ok(WebsiteArtifact {
        version,
        file_name,
        url: url.clone(),
        sha256,
        urls: vec![url],
    })
}

fn parse_turbo(script: &str) -> Result<WebsiteArtifact, ProviderError> {
    let version_pattern = Regex::new(r#"ver\s*:\s*['"]([^'"]+)['"]"#)
        .map_err(|error| ProviderError::Parse(error.to_string()))?;
    let url_pattern = Regex::new(r#"portable_url\s*:\s*['"]([^'"]+)['"]"#)
        .map_err(|error| ProviderError::Parse(error.to_string()))?;
    let version = version_pattern
        .captures(script)
        .map(|capture| capture[1].to_owned())
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let url = url_pattern
        .captures(script)
        .map(|capture| capture[1].to_owned())
        .ok_or(ProviderError::NoCompatibleAsset)?;
    validate_official_host(&url, "dl.tbrowser.cn")?;
    Ok(WebsiteArtifact {
        version,
        file_name: file_name_from_url(&url)?,
        url: url.clone(),
        sha256: String::new(),
        urls: vec![url],
    })
}

fn parse_librewolf(html: &str) -> Result<WebsiteArtifact, ProviderError> {
    let pattern = Regex::new(
        r#"https://dl\.librewolf\.net/librewolf/([^/"]+)/librewolf-[^"]+-windows-x86_64-portable\.zip"#,
    )
    .map_err(|error| ProviderError::Parse(error.to_string()))?;
    let capture = pattern
        .captures(html)
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let url = capture[0].to_owned();
    let package_version = capture[1].to_owned();
    let version = package_version
        .rsplit_once('-')
        .map_or(package_version.as_str(), |(version, _)| version)
        .to_owned();
    Ok(WebsiteArtifact {
        version,
        file_name: file_name_from_url(&url)?,
        url: url.clone(),
        sha256: String::new(),
        urls: vec![url],
    })
}

fn decode_catsxp_url(encoded: &str) -> Option<String> {
    let decoded = STANDARD.decode(encoded).ok()?;
    let mut transformed = decoded
        .into_iter()
        .map(|byte| byte.wrapping_sub(1))
        .collect::<Vec<_>>();
    transformed.reverse();
    String::from_utf8(transformed).ok()
}

fn file_name_from_url(url: &str) -> Result<String, ProviderError> {
    Url::parse(url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(str::to_owned)
        })
        .filter(|name| !name.is_empty())
        .ok_or(ProviderError::NoCompatibleAsset)
}

fn validate_official_host(url: &str, expected: &str) -> Result<(), ProviderError> {
    if Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| host == expected)
    {
        Ok(())
    } else {
        Err(ProviderError::NoCompatibleAsset)
    }
}

fn validate_stable_x64(channel: ReleaseChannel, cpu_build: CpuBuild) -> Result<(), ProviderError> {
    if channel == ReleaseChannel::Stable && cpu_build == CpuBuild::X64 {
        Ok(())
    } else {
        Err(ProviderError::UnsupportedSelection)
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};

    use super::{parse_catsxp, parse_librewolf, parse_skykakapo, parse_turbo, parse_vivaldi};

    fn encode_catsxp(url: &str) -> String {
        let mut bytes = url
            .as_bytes()
            .iter()
            .map(|byte| byte.wrapping_add(1))
            .collect::<Vec<_>>();
        bytes.reverse();
        STANDARD.encode(bytes)
    }

    #[test]
    fn catsxp_decoder_selects_x64_release_portable() {
        let url =
            "https://b2.catsxp.com/catsxp_portable/win_x64/portable_x64_release_156_6_10_4.zip";
        let html = format!(
            r#"<span class="version-value">156.0.8078.50</span><a data-url="{}">download</a>"#,
            encode_catsxp(url)
        );
        let parsed = parse_catsxp(&html).unwrap();
        assert_eq!(parsed.version, "156.6.10.4");
        assert_eq!(parsed.url, url);
    }

    #[test]
    fn skykakapo_parser_extracts_name_version_and_hash() {
        let html = r#"<a>skykakapo-158.0.1-2026102011.en-US.win64-sse3-cspgo.7z</a>
<li><abbr title="Secure Hash Algorithm 256">SHA256</abbr>: 2609eea734321e0001fced49895c9d61e366f0410fedc76603a0d74597ae498f</li>"#;
        let parsed = parse_skykakapo(html).unwrap();
        assert_eq!(parsed.version, "158.0.1");
        assert!(parsed.url.starts_with("https://sourceforge.net/"));
    }

    #[test]
    fn turbo_parser_uses_script_portable_url() {
        let script = r#"_callback({ ver: '154.0.8037.104', portable_url: 'https://dl.tbrowser.cn/download/Turbo_154.0.8037.104_portable.7z' })"#;
        let parsed = parse_turbo(script).unwrap();
        assert_eq!(parsed.version, "154.0.8037.104");
        assert!(parsed.file_name.ends_with(".7z"));
    }

    #[test]
    fn librewolf_parser_selects_official_x64_portable() {
        let html = r#"<a href="https://dl.librewolf.net/librewolf/158.0.1-2/librewolf-158.0.1-2-windows-x86_64-portable.zip">Download</a>"#;
        let parsed = parse_librewolf(html).unwrap();
        assert_eq!(parsed.version, "158.0.1");
        assert_eq!(
            parsed.url,
            "https://dl.librewolf.net/librewolf/158.0.1-2/librewolf-158.0.1-2-windows-x86_64-portable.zip"
        );
    }

    #[test]
    fn vivaldi_parser_extracts_version_and_download_url() {
        let script = r#"var vivaldi_versions = {"vivaldi_version_number":"8.2.4133.94","vivaldi_version_win64":"Vivaldi.8.2.4133.94.x64.exe"};"#;
        let parsed = parse_vivaldi(script).unwrap();
        assert_eq!(parsed.version, "8.2.4133.94");
        assert_eq!(parsed.file_name, "Vivaldi.8.2.4133.94.x64.exe");
        assert_eq!(
            parsed.url,
            "https://downloads.vivaldi.com/stable/Vivaldi.8.2.4133.94.x64.exe"
        );
    }
}
