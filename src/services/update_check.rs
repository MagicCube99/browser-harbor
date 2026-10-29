//! Lightweight Browser Harbor release discovery.

use serde::Deserialize;

use super::{ServiceError, http::HttpClient};
use crate::domain::{settings::NetworkSettings, version::normalize_version};

const GITEE_LATEST_RELEASE: &str =
    "https://gitee.com/api/v5/repos/MagicCube99/browser-harbor/releases/latest";
const GITHUB_LATEST_RELEASE: &str =
    "https://api.github.com/repos/MagicCube99/browser-harbor/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestAppRelease {
    pub version: String,
}

#[derive(Deserialize)]
struct GiteeRelease {
    tag_name: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

pub async fn latest_release(network: &NetworkSettings) -> Result<LatestAppRelease, ServiceError> {
    let http = HttpClient::new(network)?;
    if let Ok(release) = http.get_json::<GiteeRelease>(GITEE_LATEST_RELEASE).await {
        return release_from_tag(&release.tag_name);
    }
    match http.get_json::<GitHubRelease>(GITHUB_LATEST_RELEASE).await {
        Ok(release) if !release.draft && !release.prerelease => release_from_tag(&release.tag_name),
        Ok(_) => Err(ServiceError::InvalidUrl),
        Err(error) => Err(error),
    }
}

fn release_from_tag(tag: &str) -> Result<LatestAppRelease, ServiceError> {
    let version = normalize_version(tag);
    if version.is_empty() {
        return Err(ServiceError::InvalidUrl);
    }
    Ok(LatestAppRelease { version })
}

#[cfg(test)]
mod tests {
    use super::release_from_tag;

    #[test]
    fn release_tags_are_normalized() {
        assert_eq!(release_from_tag("v0.6.2").unwrap().version, "0.6.2");
        assert!(release_from_tag(" ").is_err());
    }
}
