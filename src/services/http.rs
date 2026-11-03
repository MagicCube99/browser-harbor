//! Safe HTTP metadata and artifact transport.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use reqwest::{Client, Response, Url};
use serde::de::DeserializeOwned;

use super::ServiceError;
use crate::domain::{
    release::Artifact,
    settings::{Accelerator, NetworkSettings},
};

const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;
const METADATA_RESOLUTION_BUDGET: Duration = Duration::from_secs(30);
const USER_AGENT: &str = concat!("BrowserHarbor/", env!("CARGO_PKG_VERSION"), " Windows");
const BUILTIN_ACCELERATORS: &[&str] = &[
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
    "https://github.akams.cn/",
];

pub(crate) async fn wait_for_cancellation(cancellation: &AtomicBool) {
    while !cancellation.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[derive(Clone)]
pub struct HttpClient {
    direct: Client,
    fallback: Client,
    direct_hosts: Arc<HashSet<String>>,
    github_token: Option<String>,
    cancellation: Option<Arc<AtomicBool>>,
    resolution_deadline: Option<tokio::time::Instant>,
}

impl HttpClient {
    pub fn new(settings: &NetworkSettings) -> Result<Self, ServiceError> {
        let direct = metadata_client(settings, true)?;
        let fallback = metadata_client(settings, false)?;
        Ok(Self {
            direct,
            fallback,
            direct_hosts: Arc::new(accelerator_hosts(settings)),
            github_token: std::env::var("GITHUB_TOKEN")
                .ok()
                .filter(|token| !token.trim().is_empty()),
            cancellation: None,
            resolution_deadline: None,
        })
    }

    pub fn download_client(settings: &NetworkSettings) -> Result<Client, ServiceError> {
        base_builder(settings, false)?
            // reqwest only enables gzip for this build; the other codecs are absent features.
            .no_gzip()
            .build()
            .map_err(ServiceError::Network)
    }

    pub fn download_clients(settings: &NetworkSettings) -> Result<(Client, Client), ServiceError> {
        let direct = base_builder(settings, true)?
            .no_gzip()
            .build()
            .map_err(ServiceError::Network)?;
        let fallback = base_builder(settings, false)?
            .no_gzip()
            .build()
            .map_err(ServiceError::Network)?;
        Ok((direct, fallback))
    }

    pub fn client(&self) -> &Client {
        &self.fallback
    }

    pub fn with_operation(mut self, cancellation: Arc<AtomicBool>) -> Self {
        self.cancellation = Some(cancellation);
        self.renew_resolution_budget()
    }

    pub fn renew_resolution_budget(&self) -> Self {
        let mut client = self.clone();
        client.resolution_deadline = Some(tokio::time::Instant::now() + METADATA_RESOLUTION_BUDGET);
        client
    }

    async fn wait<T>(
        &self,
        request: impl std::future::Future<Output = Result<T, ServiceError>>,
    ) -> Result<T, ServiceError> {
        tokio::select! {
            biased;
            _ = async {
                if let Some(cancellation) = &self.cancellation {
                    wait_for_cancellation(cancellation).await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => Err(ServiceError::Cancelled),
            _ = async {
                if let Some(deadline) = self.resolution_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => Err(ServiceError::ResolutionTimedOut),
            result = request => result,
        }
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
                Err(error @ (ServiceError::Cancelled | ServiceError::ResolutionTimedOut)) => {
                    return Err(error);
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or(ServiceError::InvalidUrl))
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, ServiceError> {
        self.wait(async {
            let parsed = Url::parse(url).map_err(|_| ServiceError::InvalidUrl)?;
            let mut request = self
                .client_for(&parsed)
                .get(parsed.clone())
                .header(reqwest::header::ACCEPT, "application/json, text/plain, */*");
            if parsed.host_str() == Some("api.github.com")
                && let Some(token) = &self.github_token
            {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .map_err(|error| transport_error(&parsed, error))?;
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
                .map_err(|error| transport_error(&parsed, error))?;
            if bytes.len() > MAX_METADATA_BYTES {
                return Err(ServiceError::ResponseTooLarge);
            }
            Ok(bytes.to_vec())
        })
        .await
    }

    pub async fn post_xml(&self, url: &str, body: String) -> Result<String, ServiceError> {
        self.wait(async {
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
                .map_err(|error| transport_error(&parsed, error))?;
            ensure_success(response, &parsed)?
                .text()
                .await
                .map_err(|error| transport_error(&parsed, error))
        })
        .await
    }

    pub async fn post_json<T: DeserializeOwned>(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<T, ServiceError> {
        self.wait(async {
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
                .map_err(|error| transport_error(&parsed, error))?;
            let bytes = ensure_success(response, &parsed)?
                .bytes()
                .await
                .map_err(|error| transport_error(&parsed, error))?;
            serde_json::from_slice(&bytes).map_err(ServiceError::InvalidResponse)
        })
        .await
    }

    fn client_for(&self, url: &Url) -> &Client {
        if url_uses_accelerator(url, &self.direct_hosts) {
            &self.direct
        } else {
            &self.fallback
        }
    }
}

pub(crate) fn transport_error(url: &Url, error: reqwest::Error) -> ServiceError {
    // Classify the source chain without exposing URLs, proxy credentials or signed query strings.
    let mut detail = String::new();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        detail.push_str(&cause.to_string().to_ascii_lowercase());
        source = cause.source();
    }
    let reason = if error.is_timeout() {
        "timeout"
    } else if detail.contains("proxy") {
        "proxy connection failed"
    } else if detail.contains("dns") || detail.contains("resolve") {
        "DNS resolution failed"
    } else if detail.contains("certificate") || detail.contains("tls") {
        "TLS certificate or handshake failed"
    } else if error.is_connect() {
        "connection failed"
    } else if error.is_redirect() {
        "redirect failed"
    } else if error.is_body() || error.is_decode() {
        "response body read failed"
    } else {
        "connection or response failed"
    };
    ServiceError::Transport {
        host: url.host_str().unwrap_or("unknown").to_owned(),
        reason: reason.to_owned(),
    }
}

fn metadata_client(settings: &NetworkSettings, direct_only: bool) -> Result<Client, ServiceError> {
    // Metadata must fail over promptly; artifact downloads keep the user's timeout.
    base_builder(settings, direct_only)?
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(settings.timeout_seconds.clamp(5, 8)))
        .build()
        .map_err(ServiceError::Network)
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
    vec![url.to_owned()]
}

pub fn signed_manifest_candidates(
    url: &str,
    settings: &NetworkSettings,
) -> Result<Vec<String>, ServiceError> {
    let original = Url::parse(url).map_err(|_| ServiceError::InvalidUrl)?;
    if !matches!(
        original.host_str(),
        Some("github.com" | "raw.githubusercontent.com")
    ) {
        return Err(ServiceError::InvalidUrl);
    }
    let mut candidates = Vec::new();
    if settings.accelerators_enabled {
        for accelerator in settings
            .accelerators
            .iter()
            .filter(|accelerator| accelerator.enabled)
            .filter(|accelerator| accelerator.validate().is_ok())
        {
            if let Ok(candidate) = accelerated_url(&accelerator.prefix, &original) {
                candidates.push(candidate.to_string());
            }
        }
        for prefix in BUILTIN_ACCELERATORS {
            if let Ok(candidate) = accelerated_url(prefix, &original) {
                candidates.push(candidate.to_string());
            }
        }
    }
    candidates.push(url.to_owned());
    Ok(candidates)
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
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    use super::{
        UrlRouter, accelerator_hosts, github_api_candidates, signed_manifest_candidates,
        url_uses_accelerator,
    };
    use crate::domain::{
        release::{ArchiveFormat, Artifact, Integrity},
        settings::{Accelerator, NetworkSettings},
    };

    #[tokio::test]
    async fn resolution_budget_covers_all_requests_and_cancellation_interrupts_stalled_io() {
        for cancel in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let (accepted, waiting) = std::sync::mpsc::channel();
            thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0; 2048];
                let _ = socket.read(&mut request);
                accepted.send(()).unwrap();
                thread::sleep(Duration::from_secs(2));
            });
            let signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let cancellation = std::sync::Arc::clone(&signal);
            thread::spawn(move || {
                waiting.recv().unwrap();
                if cancel {
                    thread::sleep(Duration::from_millis(25));
                    cancellation.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
            let mut http = super::HttpClient::new(&NetworkSettings::default()).unwrap();
            http.cancellation = Some(signal);
            http.resolution_deadline =
                Some(tokio::time::Instant::now() + Duration::from_millis(100));
            let result = tokio::time::timeout(
                Duration::from_millis(500),
                http.get_bytes(&format!("http://{address}/metadata")),
            )
            .await;
            if cancel {
                assert!(matches!(result, Ok(Err(super::ServiceError::Cancelled))));
            } else {
                assert!(matches!(
                    result,
                    Ok(Err(super::ServiceError::ResolutionTimedOut))
                ));
                let next = http.get_bytes(&format!("http://{address}/next")).await;
                assert!(matches!(next, Err(super::ServiceError::ResolutionTimedOut)));
            }
        }
    }

    #[tokio::test]
    async fn transport_errors_preserve_host_and_reason_without_url_secrets() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let http = super::HttpClient::new(&NetworkSettings::default()).unwrap();
        let error = http
            .get_bytes(&format!(
                "http://user:password@{address}/secret-path?token=secret-token"
            ))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("127.0.0.1"), "{error}");
        assert!(error.contains("connection"), "{error}");
        assert!(!error.contains("password"));
        assert!(!error.contains("secret-path"));
        assert!(!error.contains("secret-token"));
    }

    #[tokio::test]
    async fn metadata_total_deadline_stops_a_slow_continuous_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request).unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 36\r\nConnection: close\r\n\r\n")
                .unwrap();
            for _ in 0..36 {
                if socket.write_all(b"a").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(250));
            }
        });
        let http = super::HttpClient::new(&NetworkSettings::default()).unwrap();
        let result = http.get_bytes(&format!("http://{address}/metadata")).await;
        server.join().unwrap();
        assert!(
            matches!(result, Err(super::ServiceError::Transport { reason, .. }) if reason == "timeout"),
            "metadata must have a total deadline even when each read arrives on time"
        );
    }

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

    fn serve_response(content_encoding: Option<&'static str>, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            let (mut socket, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            return;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return,
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 2048];
            while request.len() < 64 * 1024 {
                match socket.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => return,
                }
            }
            let encoding = content_encoding
                .map(|value| format!("Content-Encoding: {value}\r\n"))
                .unwrap_or_default();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{encoding}Connection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes());
            let _ = socket.write_all(body);
        });
        format!("http://{address}/artifact")
    }

    #[tokio::test]
    async fn artifact_clients_preserve_encoded_responses_for_the_downloader_to_reject() {
        let settings = NetworkSettings::default();
        let (direct, fallback) = super::HttpClient::download_clients(&settings).unwrap();
        let clients = [
            (
                "download",
                super::HttpClient::download_client(&settings).unwrap(),
            ),
            ("direct", direct),
            ("fallback", fallback),
        ];
        for (name, client) in clients {
            for encoding in ["gzip", "br", "zstd", "deflate", "gzip, br"] {
                let url = serve_response(Some(encoding), b"artifact");
                let response = client.get(url).send().await.unwrap();
                assert_eq!(
                    response
                        .headers()
                        .get(reqwest::header::CONTENT_ENCODING)
                        .and_then(|value| value.to_str().ok()),
                    Some(encoding),
                    "{name} client transparently consumed {encoding}"
                );
            }
        }
    }

    #[tokio::test]
    async fn metadata_client_still_decodes_gzip() {
        const GZIP_HELLO: &[u8] =
            b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\x07\x00\x86\xa6\x10\x36\x05\x00\x00\x00";
        let url = serve_response(Some("gzip"), GZIP_HELLO);
        let http = super::HttpClient::new(&NetworkSettings::default()).unwrap();
        assert_eq!(http.get_bytes(&url).await.unwrap(), b"hello");
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
    fn github_metadata_uses_the_official_api_url_only() {
        let url = "https://api.github.com/repos/owner/repo/releases/latest";
        let candidates = github_api_candidates(url);

        assert_eq!(candidates, [url.to_owned()]);
    }

    #[test]
    fn disabled_acceleration_keeps_signed_manifest_on_the_official_url() {
        let settings = NetworkSettings {
            accelerators_enabled: false,
            ..NetworkSettings::default()
        };
        let url = "https://github.com/owner/repo/releases/latest/download/trusted-metadata.json";

        assert_eq!(
            signed_manifest_candidates(url, &settings).unwrap(),
            vec![url.to_owned()]
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
