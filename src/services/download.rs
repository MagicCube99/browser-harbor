//! Verified artifact download with mirror fallback.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_util::StreamExt;
use sha2::{Digest, Sha256, Sha512};

use super::{
    ServiceError,
    http::{UrlRouter, accelerator_hosts, url_uses_accelerator},
};
use crate::domain::{
    release::{Artifact, Integrity},
    settings::NetworkSettings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DownloadCacheUsage {
    pub files: u64,
    pub bytes: u64,
}

#[derive(Clone)]
pub struct Downloader {
    direct_client: reqwest::Client,
    fallback_client: reqwest::Client,
    cache_dir: PathBuf,
    cancellation: Arc<AtomicBool>,
}

impl Downloader {
    pub fn new(client: reqwest::Client, cache_dir: PathBuf, cancellation: Arc<AtomicBool>) -> Self {
        Self::with_fallback_clients(client.clone(), client, cache_dir, cancellation)
    }

    pub fn with_fallback_clients(
        direct_client: reqwest::Client,
        fallback_client: reqwest::Client,
        cache_dir: PathBuf,
        cancellation: Arc<AtomicBool>,
    ) -> Self {
        Self {
            direct_client,
            fallback_client,
            cache_dir,
            cancellation,
        }
    }

    pub fn cancellation_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancellation)
    }

    pub async fn download(
        &self,
        artifact: &Artifact,
        network: &NetworkSettings,
        mut progress: impl FnMut(DownloadProgress) + Send,
    ) -> Result<PathBuf, ServiceError> {
        fs::create_dir_all(&self.cache_dir)
            .map_err(|error| ServiceError::io(&self.cache_dir, error))?;
        let cached = self.cached_path(artifact);
        if cached.is_file() {
            match verify_artifact(&cached, artifact) {
                Ok(()) => return Ok(cached),
                Err(_) => {
                    fs::remove_file(&cached).map_err(|error| ServiceError::io(&cached, error))?;
                }
            }
        }

        let part = self
            .cache_dir
            .join(format!("{}.part", safe_file_name(&artifact.file_name)));
        let candidates = UrlRouter::candidates(artifact, network)?;
        let direct_hosts = accelerator_hosts(network);
        let mut last_error = None;
        for url in candidates {
            if self.cancellation.load(Ordering::Relaxed) {
                return Err(ServiceError::Cancelled);
            }
            match self
                .try_download(url, artifact, &part, &direct_hosts, &mut progress)
                .await
            {
                Ok(()) => {
                    if let Err(error) = verify_artifact(&part, artifact) {
                        let _ = fs::remove_file(&part);
                        last_error = Some(error);
                        continue;
                    }
                    fs::rename(&part, &cached).map_err(|error| ServiceError::io(&cached, error))?;
                    return Ok(cached);
                }
                Err(error) => {
                    let _ = fs::remove_file(&part);
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or(ServiceError::AllCandidatesFailed))
    }

    async fn try_download(
        &self,
        url: reqwest::Url,
        artifact: &Artifact,
        part: &Path,
        direct_hosts: &HashSet<String>,
        progress: &mut impl FnMut(DownloadProgress),
    ) -> Result<(), ServiceError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            if self.cancellation.load(Ordering::Relaxed) {
                return Err(ServiceError::Cancelled);
            }
            let client = if url_uses_accelerator(&url, direct_hosts) {
                &self.direct_client
            } else {
                &self.fallback_client
            };
            let response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|error| ServiceError::Network(error.without_url()))?;
            let status = response.status();
            if status.is_success() {
                return self
                    .write_response(response, artifact, part, progress)
                    .await;
            }
            let retryable = status.as_u16() == 429 || status.is_server_error();
            if retryable && attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(400 * attempt)).await;
                continue;
            }
            return Err(ServiceError::Status {
                status: status.as_u16(),
                host: url.host_str().unwrap_or("unknown").to_owned(),
            });
        }
    }

    async fn write_response(
        &self,
        response: reqwest::Response,
        artifact: &Artifact,
        part: &Path,
        progress: &mut impl FnMut(DownloadProgress),
    ) -> Result<(), ServiceError> {
        let total = response.content_length().or(artifact.size);
        if let (Some(expected), Some(actual)) = (artifact.size, response.content_length())
            && expected != actual
        {
            return Err(ServiceError::SizeMismatch { expected, actual });
        }

        let mut file = File::create(part).map_err(|error| ServiceError::io(part, error))?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0u64;
        let mut hasher = StreamingHasher::new(artifact.integrity.as_ref());
        while let Some(chunk) = stream.next().await {
            if self.cancellation.load(Ordering::Relaxed) {
                return Err(ServiceError::Cancelled);
            }
            let chunk = chunk.map_err(|error| ServiceError::Network(error.without_url()))?;
            file.write_all(&chunk)
                .map_err(|error| ServiceError::io(part, error))?;
            if let Some(hasher) = &mut hasher {
                hasher.update(&chunk);
            }
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            progress(DownloadProgress { downloaded, total });
        }
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|error| ServiceError::io(part, error))?;
        if let Some(expected) = artifact.size
            && expected != downloaded
        {
            return Err(ServiceError::SizeMismatch {
                expected,
                actual: downloaded,
            });
        }
        if let (Some(integrity), Some(hasher)) = (&artifact.integrity, hasher) {
            let actual = hasher.finish_hex();
            if !actual.eq_ignore_ascii_case(integrity.value()) {
                return Err(ServiceError::integrity_mismatch(integrity, actual));
            }
        }
        Ok(())
    }

    fn cached_path(&self, artifact: &Artifact) -> PathBuf {
        let name = artifact.integrity.as_ref().map_or_else(
            || safe_file_name(&artifact.file_name),
            |integrity| format!("{}.artifact", integrity.value()),
        );
        self.cache_dir.join(name)
    }
}

pub fn download_cache_usage(cache_dir: &Path) -> std::io::Result<DownloadCacheUsage> {
    let mut usage = DownloadCacheUsage::default();
    let entries = match fs::read_dir(cache_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(usage),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() && is_download_cache_file(&entry.path()) {
            usage.files += 1;
            usage.bytes = usage.bytes.saturating_add(entry.metadata()?.len());
        }
    }
    Ok(usage)
}

pub fn clear_download_cache(cache_dir: &Path) -> std::io::Result<DownloadCacheUsage> {
    let mut removed = DownloadCacheUsage::default();
    let entries = match fs::read_dir(cache_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(removed),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() && is_download_cache_file(&path) {
            let bytes = entry.metadata()?.len();
            fs::remove_file(&path)?;
            removed.files += 1;
            removed.bytes = removed.bytes.saturating_add(bytes);
        }
    }
    Ok(removed)
}

fn is_download_cache_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("artifact" | "part")
    )
}

enum StreamingHasher {
    Sha256(Sha256),
    Sha512(Sha512),
}

impl StreamingHasher {
    fn new(integrity: Option<&Integrity>) -> Option<Self> {
        match integrity {
            Some(Integrity::Sha256(_)) => Some(Self::Sha256(Sha256::new())),
            Some(Integrity::Sha512(_)) => Some(Self::Sha512(Sha512::new())),
            None => None,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha256(hasher) => hasher.update(bytes),
            Self::Sha512(hasher) => hasher.update(bytes),
        }
    }

    fn finish_hex(self) -> String {
        match self {
            Self::Sha256(hasher) => hex::encode(hasher.finalize()),
            Self::Sha512(hasher) => hex::encode(hasher.finalize()),
        }
    }
}

pub fn verify_artifact(path: &Path, artifact: &Artifact) -> Result<(), ServiceError> {
    let metadata = fs::metadata(path).map_err(|error| ServiceError::io(path, error))?;
    if let Some(expected) = artifact.size
        && metadata.len() != expected
    {
        return Err(ServiceError::SizeMismatch {
            expected,
            actual: metadata.len(),
        });
    }
    if let Some(integrity) = &artifact.integrity {
        let mut file = File::open(path).map_err(|error| ServiceError::io(path, error))?;
        let mut hasher =
            StreamingHasher::new(Some(integrity)).ok_or(ServiceError::AllCandidatesFailed)?;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = std::io::Read::read(&mut file, &mut buffer)
                .map_err(|error| ServiceError::io(path, error))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let actual = hasher.finish_hex();
        if !actual.eq_ignore_ascii_case(integrity.value()) {
            return Err(ServiceError::integrity_mismatch(integrity, actual));
        }
    }
    Ok(())
}

fn safe_file_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, atomic::AtomicBool},
        thread,
    };

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use super::{Downloader, clear_download_cache, download_cache_usage};
    use crate::domain::{
        release::{ArchiveFormat, Artifact, Integrity},
        settings::NetworkSettings,
    };

    fn artifact(url: String, bytes: &[u8]) -> Artifact {
        Artifact {
            file_name: "fixture.zip".to_owned(),
            format: ArchiveFormat::Zip,
            size: Some(bytes.len() as u64),
            integrity: Some(Integrity::Sha256(hex::encode(Sha256::digest(bytes)))),
            urls: vec![url],
            signature_url: None,
        }
    }

    fn serve_once(body: Vec<u8>, status: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request);
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).unwrap();
            socket.write_all(&body).unwrap();
        });
        format!("http://{address}/artifact.zip")
    }

    #[tokio::test]
    async fn downloads_and_verifies_an_artifact() {
        let bytes = b"verified browser".to_vec();
        let url = serve_once(bytes.clone(), "200 OK");
        let directory = tempdir().unwrap();
        let client = reqwest::Client::new();
        let downloader = Downloader::new(
            client,
            directory.path().to_path_buf(),
            Arc::new(AtomicBool::new(false)),
        );

        let path = downloader
            .download(&artifact(url, &bytes), &NetworkSettings::default(), |_| {})
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[tokio::test]
    async fn hash_mismatch_removes_partial_file() {
        let bytes = b"tampered".to_vec();
        let url = serve_once(bytes.clone(), "200 OK");
        let directory = tempdir().unwrap();
        let client = reqwest::Client::new();
        let downloader = Downloader::new(
            client,
            directory.path().to_path_buf(),
            Arc::new(AtomicBool::new(false)),
        );
        let mut release = artifact(url, &bytes);
        release.integrity = Some(Integrity::Sha256("0".repeat(64)));

        assert!(
            downloader
                .download(&release, &NetworkSettings::default(), |_| {})
                .await
                .is_err()
        );
        assert!(!directory.path().join("fixture.zip.part").exists());
    }

    #[tokio::test]
    async fn hash_mismatch_switches_to_the_next_candidate() {
        let bad = b"tampered".to_vec();
        let good = b"verified browser".to_vec();
        let bad_url = serve_once(bad, "200 OK");
        let good_url = serve_once(good.clone(), "200 OK");
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().to_path_buf(),
            Arc::new(AtomicBool::new(false)),
        );
        let mut release = artifact(good_url, &good);
        release.urls.insert(0, bad_url);

        let path = downloader
            .download(&release, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), good);
    }

    #[test]
    fn cache_cleanup_keeps_metadata_and_unrelated_files() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("browser.artifact"), b"browser").unwrap();
        std::fs::write(directory.path().join("partial.zip.part"), b"partial").unwrap();
        std::fs::write(directory.path().join("trusted-metadata.json"), b"metadata").unwrap();
        std::fs::write(directory.path().join("notes.txt"), b"keep").unwrap();

        assert_eq!(
            download_cache_usage(directory.path()).unwrap(),
            super::DownloadCacheUsage {
                files: 2,
                bytes: 14,
            }
        );
        assert_eq!(
            clear_download_cache(directory.path()).unwrap(),
            super::DownloadCacheUsage {
                files: 2,
                bytes: 14,
            }
        );
        assert!(directory.path().join("trusted-metadata.json").is_file());
        assert!(directory.path().join("notes.txt").is_file());
        assert_eq!(
            download_cache_usage(directory.path()).unwrap(),
            super::DownloadCacheUsage::default()
        );
    }
}
