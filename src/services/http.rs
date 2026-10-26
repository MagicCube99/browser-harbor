//! Safe HTTP metadata and artifact transport.

use std::{collections::HashSet, sync::Arc, time::Duration};

use reqwest::{Client, Response, Url};
use serde::de::DeserializeOwned;

use super::ServiceError;
use crate::domain::{
    release::Artifact,
    settings::{Accelerator, NetworkSettings},
};

const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;
const USER_AGENT: &str = concat!("BrowserHarbor/", env!("CARGO_PKG_VERSION"), " Windows");
const BUILTIN_ACCELERATORS: &[&str] = &[
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
    "https://github.akams.cn/",
];

#[derive(Clone)]
pub struct HttpClient {
    direct: Client,
    fallback: Client,
    direct_hosts: Arc<HashSet<String>>,
}

impl HttpClient {
    pub fn new(settings: &NetworkSettings) -> Result<Self, ServiceError> {
        let (direct, fallback) = Self::download_clients(settings)?;
        Ok(Self {
            direct,
            fallback,
            direct_hosts: Arc::new(accelerator_hosts(settings)),
        })
    }

    pub fn download_client(settings: &NetworkSettings) -> Result<Client, ServiceError> {
        base_builder(settings, false)?
            .build()
            .map_err(ServiceError::Network)
    }

    pub fn download_clients(settings: &NetworkSettings) -> Result<(Client, Client), ServiceError> {
        let direct = base_builder(settings, true)?
            .build()
            .map_err(ServiceError::Network)?;
        let fallback = base_builder(settings, false)?
            .build()
            .map_err(ServiceError::Network)?;
        Ok((direct, fallback))
    }

    pub fn client(&self) -> &Client {
        &self.fallback
    }

    pub async fn get_text(&self, url: &str) -> Result<String, ServiceError> {
        let bytes = self.get_bytes(url).await?;
        String::from_utf8(bytes).map_err(|_| ServiceError::InvalidUrl)
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, ServiceError> {
        let bytes = self.get_bytes(url).await?;
        serde_json::from_slice(&bytes).map_err(ServiceError::InvalidResponse)
    }

    pub async fn get_json_candidates<T: DeserializeOwned>(
        &self,
        urls: &[String],
    ) -> Result<T, ServiceError> {
        let mut last = None;
        for url in urls {
            match self.get_json(url).await {
                Ok(value) => return Ok(value),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or(ServiceError::InvalidUrl))
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, ServiceError> {
        let parsed = Url::parse(url).map_err(|_| ServiceError::InvalidUrl)?;
        let response = self
            .client_for(&parsed)
            .get(parsed.clone())
            .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
            .send()
            .await
            .map_err(|error| ServiceError::Network(error.without_url()))?;
        let response = ensure_success(response, &parsed)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_METADATA_BYTES as u64)
        {
            return Err(ServiceError::ResponseTooLarge);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ServiceError::Network(error.without_url()))?;
        if bytes.len() > MAX_METADATA_BYTES {
            return Err(ServiceError::ResponseTooLarge);
        }
        Ok(bytes.to_vec())
    }

    pub async fn post_xml(&self, url: &str, body: String) -> Result<String, ServiceError> {
        let parsed = Url::parse(url).map_err(|_| ServiceError::InvalidUrl)?;
        let response = self
            .client_for(&parsed)
            .post(parsed.clone())
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/xml; charset=utf-8",
            )
            .body(body)
            .send()
            .await
            .map_err(|error| ServiceError::Network(error.without_url()))?;
        ensure_success(response, &parsed)?
            .text()
            .await
            .map_err(|error| ServiceError::Network(error.without_url()))
    }

    pub async fn post_json<T: DeserializeOwned>(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<T, ServiceError> {
        let parsed = Url::parse(url).map_err(|_| ServiceError::InvalidUrl)?;
        let response = self
            .client_for(&parsed)
            .post(parsed.clone())
            .header(
                reqwest::header::USER_AGENT,
                "Microsoft Edge Update/1.3.183.29;winhttp",
            )
            .json(body)
            .send()
            .await
            .map_err(|error| ServiceError::Network(error.without_url()))?;
        let bytes = ensure_success(response, &parsed)?
            .bytes()
            .await
            .map_err(|error| ServiceError::Network(error.without_url()))?;
        serde_json::from_slice(&bytes).map_err(ServiceError::InvalidResponse)
    }

    fn client_for(&self, url: &Url) -> &Client {
        if url_uses_accelerator(url, &self.direct_hosts) {
            &self.direct
        } else {
            &self.fallback
        }
    }
}

fn base_builder(
    settings: &NetworkSettings,
    direct_only: bool,
) -> Result<reqwest::ClientBuilder, ServiceError> {
    let timeout = Duration::from_secs(settings.timeout_seconds.clamp(5, 300));
    let mut builder = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(10));
    if direct_only {
        builder = builder.no_proxy();
    } else if let Some(proxy) = settings
        .proxy
        .as_deref()
        .map(str::trim)
        .filter(|proxy| !proxy.is_empty())
    {
        let proxy = reqwest::Proxy::all(proxy).map_err(|_| ServiceError::InvalidUrl)?;
        builder = builder.proxy(proxy);
    }
    Ok(builder)
}

pub(crate) fn accelerator_hosts(settings: &NetworkSettings) -> HashSet<String> {
    BUILTIN_ACCELERATORS
        .iter()
        .copied()
        .chain(
            settings
                .accelerators
                .iter()
                .filter(|accelerator| accelerator.enabled)
                .map(|accelerator| accelerator.prefix.as_str()),
        )
        .filter_map(|prefix| Url::parse(prefix.trim()).ok())
        .filter_map(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        .collect()
}

pub(crate) fn url_uses_accelerator(url: &Url, direct_hosts: &HashSet<String>) -> bool {
    url.host_str()
        .is_some_and(|host| direct_hosts.contains(&host.to_ascii_lowercase()))
}

pub struct UrlRouter;

pub fn github_api_candidates(url: &str) -> Vec<String> {
    BUILTIN_ACCELERATORS
        .iter()
        .map(|prefix| format!("{prefix}{url}"))
        .chain([url.to_owned()])
        .collect()
}

impl UrlRouter {
    pub fn candidates(
        artifact: &Artifact,
        settings: &NetworkSettings,
    ) -> Result<Vec<Url>, ServiceError> {
        let mut candidates = Vec::new();
        let user_prefixes = settings
            .accelerators
            .iter()
            .filter(|accelerator| accelerator.enabled)
            .filter(|accelerator| accelerator.validate().is_ok())
            .map(|accelerator| accelerator.prefix.trim().to_owned())
            .collect::<Vec<_>>();

        for raw in &artifact.urls {
            let original = Url::parse(raw).map_err(|_| ServiceError::InvalidUrl)?;
            if !is_allowed_transport(&original) {
                continue;
            }
            if settings.accelerators_enabled && proxyable_host(&original) {
                for prefix in &user_prefixes {
                    if let Ok(url) = accelerated_url(prefix, &original) {
                        candidates.push(url);
                    }
                }
                for prefix in BUILTIN_ACCELERATORS {
                    if let Ok(url) = accelerated_url(prefix, &original) {
                        candidates.push(url);
                    }
                }
            }
            candidates.push(original);
        }

        let mut seen = HashSet::new();
        candidates.retain(|url| seen.insert(url.as_str().to_owned()));
        if candidates.is_empty() {
            return Err(ServiceError::InvalidUrl);
        }
        Ok(candidates)
    }

    pub fn validate_accelerators(accelerators: &[Accelerator]) -> Result<(), String> {
        for accelerator in accelerators {
            accelerator.validate()?;
        }
        Ok(())
    }
}

fn proxyable_host(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("github.com" | "raw.githubusercontent.com" | "objects.githubusercontent.com")
    )
}

fn is_allowed_transport(url: &Url) -> bool {
    url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1")))
}

fn accelerated_url(prefix: &str, original: &Url) -> Result<Url, ServiceError> {
    let prefix = prefix.trim_end_matches('/');
    Url::parse(&format!("{prefix}/{}", original.as_str())).map_err(|_| ServiceError::InvalidUrl)
}

fn ensure_success(response: Response, url: &Url) -> Result<Response, ServiceError> {
    if response.status().is_success() {
        return Ok(response);
    }
    Err(ServiceError::Status {
        status: response.status().as_u16(),
        host: url.host_str().unwrap_or("unknown").to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{UrlRouter, accelerator_hosts, github_api_candidates, url_uses_accelerator};
    use crate::domain::{
        release::{ArchiveFormat, Artifact, Integrity},
        settings::{Accelerator, NetworkSettings},
    };

    fn artifact(url: &str) -> Artifact {
        Artifact {
            file_name: "browser.zip".to_owned(),
            format: ArchiveFormat::Zip,
            size: None,
            integrity: Some(Integrity::Sha256("a".repeat(64))),
            urls: vec![url.to_owned()],
            signature_url: None,
        }
    }

    #[test]
    fn github_artifact_prefers_user_then_builtin_then_direct() {
        let mut settings = NetworkSettings::default();
        settings.accelerators.push(Accelerator {
            prefix: "https://user.example/".to_owned(),
            enabled: true,
        });

        let urls = UrlRouter::candidates(
            &artifact("https://github.com/owner/repo/releases/download/v1/file.zip"),
            &settings,
        )
        .unwrap();

        assert!(
            urls[0]
                .as_str()
                .starts_with("https://user.example/https://github.com/")
        );
        assert!(urls.iter().any(|url| {
            url.as_str()
                .starts_with("https://ghfast.top/https://github.com/")
        }));
        assert_eq!(
            urls.last().unwrap().as_str(),
            "https://github.com/owner/repo/releases/download/v1/file.zip"
        );
    }

    #[test]
    fn non_github_artifact_is_never_sent_through_a_third_party_prefix() {
        let mut settings = NetworkSettings::default();
        settings.accelerators.push(Accelerator {
            prefix: "https://user.example/".to_owned(),
            enabled: true,
        });

        let urls = UrlRouter::candidates(&artifact("https://dl.google.com/chrome.msi"), &settings)
            .unwrap();

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].as_str(), "https://dl.google.com/chrome.msi");
    }

    #[test]
    fn disabled_acceleration_uses_direct_url_only() {
        let settings = NetworkSettings {
            accelerators_enabled: false,
            ..NetworkSettings::default()
        };
        let urls = UrlRouter::candidates(
            &artifact("https://github.com/owner/repo/releases/download/v1/file.zip"),
            &settings,
        )
        .unwrap();

        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn github_metadata_tries_accelerators_before_direct() {
        let url = "https://api.github.com/repos/owner/repo/releases/latest";
        let candidates = github_api_candidates(url);

        assert_eq!(
            candidates,
            [
                format!("https://ghfast.top/{url}"),
                format!("https://gh-proxy.com/{url}"),
                format!("https://ghproxy.net/{url}"),
                format!("https://github.akams.cn/{url}"),
                url.to_owned(),
            ]
        );
    }

    #[test]
    fn only_accelerator_hosts_bypass_the_system_proxy() {
        let mut settings = NetworkSettings::default();
        settings.accelerators.push(Accelerator {
            prefix: "https://mirror.example/".to_owned(),
            enabled: true,
        });
        let hosts = accelerator_hosts(&settings);

        assert!(url_uses_accelerator(
            &"https://ghfast.top/https://github.com/owner/repo"
                .parse()
                .unwrap(),
            &hosts
        ));
        assert!(url_uses_accelerator(
            &"https://mirror.example/file".parse().unwrap(),
            &hosts
        ));
        assert!(!url_uses_accelerator(
            &"https://api.github.com/repos/owner/repo/releases/latest"
                .parse()
                .unwrap(),
            &hosts
        ));
        assert!(!url_uses_accelerator(
            &"https://dl.google.com/chrome.msi".parse().unwrap(),
            &hosts
        ));
    }
}
