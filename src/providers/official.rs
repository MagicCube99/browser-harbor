use std::collections::BTreeMap;

use quick_xml::{Reader, XmlVersion, events::Event};
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

const CHROME_UPDATE_URL: &str = "https://tools.google.com/service/update2";
const FIREFOX_VERSIONS_URL: &str = "https://product-details.mozilla.org/1.0/firefox_versions.json";

pub async fn resolve_chrome(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    let channel_tag = match channel {
        ReleaseChannel::Stable => "x64-stable",
        ReleaseChannel::Beta => "x64-beta",
        _ => return Err(ProviderError::UnsupportedSelection),
    };
    if cpu_build != CpuBuild::X64 {
        return Err(ProviderError::UnsupportedSelection);
    }
    let xml = http
        .post_xml(CHROME_UPDATE_URL, chrome_omaha_request(channel_tag))
        .await?;
    let parsed = parse_chrome_omaha(&xml)?;
    Ok(ReleaseDescriptor {
        target: TargetId::Chrome,
        version: parsed.version,
        channel,
        cpu_build,
        provider: ProviderKind::ChromeOmaha,
        artifact: Artifact {
            file_name: parsed.file_name,
            format: ArchiveFormat::SevenZip,
            size: Some(parsed.size),
            integrity: Some(Integrity::Sha256(parsed.sha256)),
            urls: parsed.urls,
            signature_url: None,
        },
        placement: TargetId::Chrome.spec().placement,
        inner_archive: None,
        source: CHROME_UPDATE_URL.to_owned(),
        published_at: None,
    })
}

pub async fn resolve_firefox(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    if !matches!(
        channel,
        ReleaseChannel::Stable | ReleaseChannel::Esr | ReleaseChannel::Beta
    ) || cpu_build != CpuBuild::X64
    {
        return Err(ProviderError::UnsupportedSelection);
    }
    let versions = http
        .get_json::<FirefoxVersions>(FIREFOX_VERSIONS_URL)
        .await?;
    let version = match channel {
        ReleaseChannel::Stable => versions.latest_firefox_version,
        ReleaseChannel::Beta => versions.latest_firefox_devel_version,
        ReleaseChannel::Esr => versions.firefox_esr,
        _ => return Err(ProviderError::UnsupportedSelection),
    };
    let sums_url = format!(
        "https://download-installer.cdn.mozilla.net/pub/firefox/releases/{version}/SHA256SUMS"
    );
    let sums = http.get_text(&sums_url).await?;
    let file_name = format!("Firefox Setup {version}.exe");
    let relative = format!("win64/zh-CN/{file_name}");
    let sha256 = parse_sha256sums(&sums, &relative).ok_or(ProviderError::NoCompatibleAsset)?;
    let encoded = relative.replace(' ', "%20");
    let url = format!(
        "https://download-installer.cdn.mozilla.net/pub/firefox/releases/{version}/{encoded}"
    );
    Ok(ReleaseDescriptor {
        target: TargetId::Firefox,
        version,
        channel,
        cpu_build,
        provider: ProviderKind::Mozilla,
        artifact: Artifact {
            file_name,
            format: ArchiveFormat::SevenZip,
            size: None,
            integrity: Some(Integrity::Sha256(sha256)),
            urls: vec![url],
            signature_url: None,
        },
        placement: TargetId::Firefox.spec().placement,
        inner_archive: None,
        source: sums_url,
        published_at: None,
    })
}

#[derive(Debug, Deserialize)]
struct FirefoxVersions {
    #[serde(rename = "LATEST_FIREFOX_VERSION")]
    latest_firefox_version: String,
    #[serde(rename = "LATEST_FIREFOX_DEVEL_VERSION")]
    latest_firefox_devel_version: String,
    #[serde(rename = "FIREFOX_ESR")]
    firefox_esr: String,
}

#[derive(Debug)]
struct ChromeArtifact {
    version: String,
    file_name: String,
    size: u64,
    sha256: String,
    urls: Vec<String>,
}

fn chrome_omaha_request(channel_tag: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<request protocol="3.0" updater="Omaha" updaterversion="1.3.36.372" shell_version="1.3.36.352" ismachine="0" sessionid="{{11111111-1111-1111-1111-111111111111}}" installsource="taggedmi" requestid="{{11111111-1111-1111-1111-111111111111}}" dedup="cr" domainjoined="0">
  <hw physmemory="16" sse="1" sse2="1" sse3="1" ssse3="1" sse41="1" sse42="1" avx="1"/>
  <os platform="win" version="10.0.26100.1742" arch="x64"/>
  <app version="" appid="{{8A69D345-D564-463C-AFF1-A69D9E530F96}}" ap="{channel_tag}">
    <updatecheck/>
    <data name="install" index="empty"/>
  </app>
</request>"#
    )
}

fn parse_chrome_omaha(xml: &str) -> Result<ChromeArtifact, ProviderError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut version = None;
    let mut package_name = None;
    let mut size = None;
    let mut sha256 = None;
    let mut codebases = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| ProviderError::Parse(error.to_string()))?
        {
            Event::Start(element) | Event::Empty(element) => {
                let attributes = element
                    .attributes()
                    .with_checks(false)
                    .filter_map(Result::ok)
                    .filter_map(|attribute| {
                        let key = attribute.key.as_ref().to_owned();
                        let value = attribute
                            .normalized_value(XmlVersion::Implicit1_0)
                            .ok()?
                            .into_owned();
                        Some((key, value))
                    })
                    .collect::<BTreeMap<_, _>>();
                match element.name().as_ref() {
                    "manifest" => version = attributes.get("version").cloned(),
                    "package" => {
                        package_name = attributes.get("name").cloned();
                        size = attributes.get("size").and_then(|value| value.parse().ok());
                        sha256 = attributes.get("hash_sha256").cloned();
                    }
                    "url" => {
                        if let Some(codebase) = attributes.get("codebase") {
                            codebases.push(codebase.clone());
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    let package_name = package_name.ok_or(ProviderError::NoCompatibleAsset)?;
    let urls = codebases
        .into_iter()
        .map(|base| format!("{base}{package_name}"))
        .filter_map(|url| official_https(&url, is_google_download))
        .collect::<Vec<_>>();
    if urls.is_empty() {
        return Err(ProviderError::NoCompatibleAsset);
    }
    Ok(ChromeArtifact {
        version: version.ok_or(ProviderError::NoCompatibleAsset)?,
        file_name: package_name
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .ok_or(ProviderError::NoCompatibleAsset)?,
        size: size.ok_or(ProviderError::NoCompatibleAsset)?,
        sha256: sha256.ok_or(ProviderError::NoCompatibleAsset)?,
        urls,
    })
}

fn parse_sha256sums(text: &str, file_name: &str) -> Option<String> {
    for line in text.lines() {
        let Some((hash, name)) = line.split_once("  ") else {
            continue;
        };
        if name == file_name && hash.len() == 64 {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

fn is_google_download(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|url| {
            url.host_str().map(|host| {
                host == "dl.google.com"
                    || host == "www.google.com"
                    || host.ends_with(".gvt1.com")
                    || host.ends_with(".google.com")
            })
        })
        .unwrap_or(false)
}

fn official_https(url: &str, trusted: impl Fn(&str) -> bool) -> Option<String> {
    if !trusted(url) {
        return None;
    }
    if let Some(rest) = url.strip_prefix("http://") {
        Some(format!("https://{rest}"))
    } else {
        Some(url.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{chrome_omaha_request, parse_chrome_omaha, parse_sha256sums};

    #[test]
    fn omaha_parser_extracts_version_size_hash_and_official_urls() {
        let xml = r#"<?xml version="1.0"?>
<response><app><updatecheck><urls><url codebase="https://dl.google.com/release2/chrome/abc/"/></urls><manifest version="156.0.8078.48"/><packages><package name="156.0.8078.48_chrome_installer_uncompressed.exe" size="527126440" hash_sha256="84e13e72c71a3b7d767c115e8e94ec81cb563f9bf9a782407a54afceaa37f55a"/></packages></updatecheck></app></response>"#;
        let parsed = parse_chrome_omaha(xml).unwrap();

        assert_eq!(parsed.version, "156.0.8078.48");
        assert_eq!(parsed.size, 527_126_440);
        assert_eq!(parsed.sha256.len(), 64);
        assert_eq!(parsed.urls.len(), 1);
        assert!(chrome_omaha_request("x64-beta").contains(r#"ap="x64-beta""#));
    }

    #[test]
    fn firefox_sums_parser_matches_paths_with_spaces() {
        let sums = "b6b75a7cc12adf3c22b93d4f2691e2784024bdb083393df5acce89ae84dee0f6  win64/zh-CN/Firefox Setup 158.0.1.exe";
        assert_eq!(
            parse_sha256sums(sums, "win64/zh-CN/Firefox Setup 158.0.1.exe").unwrap(),
            "b6b75a7cc12adf3c22b93d4f2691e2784024bdb083393df5acce89ae84dee0f6"
        );
    }
}
