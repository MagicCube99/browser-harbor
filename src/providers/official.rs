use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use quick_xml::{Reader, XmlVersion, events::Event};
use serde::Deserialize;
use serde_json::Value;
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
const EDGE_UPDATE_URL: &str = "https://edgeupdates.microsoft.com/api/products?view=enterprise";
const EDGE_CDP_UPDATE_URL: &str = "https://msedge.api.cdp.microsoft.com/api/v2/contents/Browser/namespaces/Default/names/msedge-stable-win-x64/versions/latest?action=select";
const EDGE_CDP_DOWNLOAD_URL: &str = "https://msedge.api.cdp.microsoft.com/api/v1.1/internal/contents/Browser/namespaces/Default/names/msedge-stable-win-x64/versions/{version}/files?action=GenerateDownloadInfo";
const FIREFOX_VERSIONS_URL: &str = "https://product-details.mozilla.org/1.0/firefox_versions.json";

pub async fn resolve_chrome(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    if channel != ReleaseChannel::Stable || cpu_build != CpuBuild::X64 {
        return Err(ProviderError::UnsupportedSelection);
    }
    let xml = http
        .post_xml(CHROME_UPDATE_URL, chrome_omaha_request())
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

pub async fn resolve_edge(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    if channel != ReleaseChannel::Stable || cpu_build != CpuBuild::X64 {
        return Err(ProviderError::UnsupportedSelection);
    }
    let cdp = async {
        let update = http
            .post_json::<Value>(
                EDGE_CDP_UPDATE_URL,
                &serde_json::json!({
                    "targetingAttributes": {
                        "IsInternalUser": true,
                        "Updater": "MicrosoftEdgeUpdate",
                        "UpdaterVersion": "1.3.183.29"
                    }
                }),
            )
            .await?;
        let version = update["ContentId"]["Version"]
            .as_str()
            .ok_or(ProviderError::NoCompatibleAsset)?
            .to_owned();
        let download_url = EDGE_CDP_DOWNLOAD_URL.replace("{version}", &version);
        let items = http
            .post_json::<Vec<Value>>(&download_url, &serde_json::json!({}))
            .await?;
        parse_edge_cdp(&version, &items)
    }
    .await;
    let parsed = match cdp {
        Ok(parsed) => parsed,
        Err(_) => {
            let value = http.get_json::<Value>(EDGE_UPDATE_URL).await?;
            parse_edge_enterprise(&value)?
        }
    };
    Ok(ReleaseDescriptor {
        target: TargetId::Edge,
        version: parsed.version,
        channel,
        cpu_build,
        provider: ProviderKind::EdgeEnterprise,
        artifact: Artifact {
            file_name: parsed.file_name,
            format: parsed.format,
            size: Some(parsed.size),
            integrity: Some(Integrity::Sha256(parsed.sha256)),
            urls: vec![parsed.url],
            signature_url: None,
        },
        placement: TargetId::Edge.spec().placement,
        inner_archive: parsed.inner_archive,
        source: parsed.source,
        published_at: parsed.published_at,
    })
}

pub async fn resolve_firefox(
    http: &HttpClient,
    channel: ReleaseChannel,
    cpu_build: CpuBuild,
) -> Result<ReleaseDescriptor, ProviderError> {
    if channel != ReleaseChannel::Stable || cpu_build != CpuBuild::X64 {
        return Err(ProviderError::UnsupportedSelection);
    }
    let versions = http
        .get_json::<FirefoxVersions>(FIREFOX_VERSIONS_URL)
        .await?;
    let version = versions.latest_firefox_version;
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
}

#[derive(Debug)]
struct ChromeArtifact {
    version: String,
    file_name: String,
    size: u64,
    sha256: String,
    urls: Vec<String>,
}

fn chrome_omaha_request() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<request protocol="3.0" updater="Omaha" updaterversion="1.3.36.372" shell_version="1.3.36.352" ismachine="0" sessionid="{11111111-1111-1111-1111-111111111111}" installsource="taggedmi" requestid="{11111111-1111-1111-1111-111111111111}" dedup="cr" domainjoined="0">
  <hw physmemory="16" sse="1" sse2="1" sse3="1" ssse3="1" sse41="1" sse42="1" avx="1"/>
  <os platform="win" version="10.0.26100.1742" arch="x64"/>
  <app version="" appid="{8A69D345-D564-463C-AFF1-A69D9E530F96}" ap="x64-stable">
    <updatecheck/>
    <data name="install" index="empty"/>
  </app>
</request>"#
        .to_owned()
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

#[derive(Debug)]
struct EdgeArtifact {
    version: String,
    file_name: String,
    size: u64,
    sha256: String,
    url: String,
    published_at: Option<String>,
    format: ArchiveFormat,
    source: String,
    inner_archive: Option<String>,
}

fn parse_edge_cdp(version: &str, items: &[Value]) -> Result<EdgeArtifact, ProviderError> {
    let item = items
        .iter()
        .max_by_key(|item| item["SizeInBytes"].as_u64().unwrap_or(0))
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let url = official_https(&string_field(item, "Url")?, is_microsoft_download)
        .ok_or(ProviderError::NoCompatibleAsset)?;
    if !is_microsoft_download(&url) {
        return Err(ProviderError::NoCompatibleAsset);
    }
    let mut file_name = item["FileId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("MicrosoftEdgeEnterpriseX64")
        .to_owned();
    if !file_name.to_ascii_lowercase().ends_with(".exe") {
        file_name.push_str(".exe");
    }
    Ok(EdgeArtifact {
        version: version.to_owned(),
        file_name,
        size: item["SizeInBytes"]
            .as_u64()
            .ok_or(ProviderError::NoCompatibleAsset)?,
        sha256: normalize_sha256(
            item["Hashes"]["Sha256"]
                .as_str()
                .ok_or(ProviderError::NoCompatibleAsset)?,
        )?,
        url,
        published_at: None,
        format: ArchiveFormat::SevenZip,
        source: EDGE_CDP_UPDATE_URL.to_owned(),
        inner_archive: Some("MSEDGE.7Z".to_owned()),
    })
}

fn parse_edge_enterprise(value: &Value) -> Result<EdgeArtifact, ProviderError> {
    let stable = value
        .as_array()
        .and_then(|products| {
            products
                .iter()
                .find(|product| product["Product"] == "Stable")
        })
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let release = stable["Releases"]
        .as_array()
        .and_then(|releases| {
            releases.iter().find(|release| {
                release["Platform"] == "Windows" && release["Architecture"] == "x64"
            })
        })
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let artifact = release["Artifacts"]
        .as_array()
        .and_then(|artifacts| {
            artifacts
                .iter()
                .find(|artifact| artifact["ArtifactName"] == "msi")
        })
        .ok_or(ProviderError::NoCompatibleAsset)?;
    let url = official_https(&string_field(artifact, "Location")?, is_microsoft_download)
        .ok_or(ProviderError::NoCompatibleAsset)?;
    Ok(EdgeArtifact {
        version: string_field(release, "ProductVersion")?,
        file_name: file_name_from_url(&url)?,
        size: artifact["SizeInBytes"]
            .as_u64()
            .ok_or(ProviderError::NoCompatibleAsset)?,
        sha256: string_field(artifact, "Hash")?.to_ascii_lowercase(),
        url,
        published_at: release["PublishedTime"].as_str().map(str::to_owned),
        format: ArchiveFormat::Msi,
        source: EDGE_UPDATE_URL.to_owned(),
        inner_archive: None,
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

fn string_field(value: &Value, field: &str) -> Result<String, ProviderError> {
    value[field]
        .as_str()
        .map(str::to_owned)
        .ok_or(ProviderError::NoCompatibleAsset)
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

fn is_microsoft_download(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|url| {
            url.host_str().map(|host| {
                host.ends_with(".microsoft.com")
                    || host == "msedge.sb.tlu.dl.delivery.mp.microsoft.com"
            })
        })
        .unwrap_or(false)
}

fn official_https(url: &str, trusted: impl Fn(&str) -> bool) -> Option<String> {
    if !trusted(url) {
        return None;
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let rest = rest.replace(
            "msedge.b.tlu.dl.delivery.mp.microsoft.com",
            "msedge.sb.tlu.dl.delivery.mp.microsoft.com",
        );
        Some(format!("https://{rest}"))
    } else {
        Some(url.to_owned())
    }
}

fn normalize_sha256(value: &str) -> Result<String, ProviderError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(value.to_ascii_lowercase());
    }
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| ProviderError::NoCompatibleAsset)?;
    if bytes.len() != 32 {
        return Err(ProviderError::NoCompatibleAsset);
    }
    Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use serde_json::json;

    use super::{parse_chrome_omaha, parse_edge_cdp, parse_edge_enterprise, parse_sha256sums};

    #[test]
    fn omaha_parser_extracts_version_size_hash_and_official_urls() {
        let xml = r#"<?xml version="1.0"?>
<response><app><updatecheck><urls><url codebase="https://dl.google.com/release2/chrome/abc/"/></urls><manifest version="156.0.8078.48"/><packages><package name="156.0.8078.48_chrome_installer_uncompressed.exe" size="527126440" hash_sha256="84e13e72c71a3b7d767c115e8e94ec81cb563f9bf9a782407a54afceaa37f55a"/></packages></updatecheck></app></response>"#;
        let parsed = parse_chrome_omaha(xml).unwrap();

        assert_eq!(parsed.version, "156.0.8078.48");
        assert_eq!(parsed.size, 527_126_440);
        assert_eq!(parsed.sha256.len(), 64);
        assert_eq!(parsed.urls.len(), 1);
    }

    #[test]
    fn edge_parser_selects_windows_x64_stable_msi() {
        let value = json!([{
            "Product": "Stable",
            "Releases": [{
                "Platform": "Windows",
                "Architecture": "x64",
                "ProductVersion": "156.0.4314.42",
                "PublishedTime": "2026-10-20T00:00:00",
                "Artifacts": [{
                    "ArtifactName": "msi",
                    "Location": "https://msedge.sf.dl.delivery.mp.microsoft.com/file/edge.msi",
                    "Hash": "324D226C5ADE08C2776645CFC2A7B12AF9C08214ABF18C6499410CD8ED92D1B3",
                    "SizeInBytes": 215257088
                }]
            }]
        }]);
        let parsed = parse_edge_enterprise(&value).unwrap();

        assert_eq!(parsed.version, "156.0.4314.42");
        assert_eq!(parsed.file_name, "edge.msi");
        assert_eq!(parsed.sha256.len(), 64);
    }

    #[test]
    fn edge_cdp_parser_normalizes_base64_hash_and_exe_name() {
        let hash = base64::engine::general_purpose::STANDARD.encode([0x42u8; 32]);
        let items = vec![json!({
            "FileId": "MicrosoftEdgeEnterpriseX64",
            "Url": "https://msedge.sf.dl.delivery.mp.microsoft.com/file/setup",
            "SizeInBytes": 180000000,
            "Hashes": { "Sha256": hash }
        })];
        let parsed = parse_edge_cdp("156.0.4314.42", &items).unwrap();
        assert_eq!(parsed.file_name, "MicrosoftEdgeEnterpriseX64.exe");
        assert_eq!(parsed.sha256, "42".repeat(32));
        assert_eq!(
            parsed.format,
            crate::domain::release::ArchiveFormat::SevenZip
        );
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
