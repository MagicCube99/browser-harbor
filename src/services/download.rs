//! Verified artifact download with mirror fallback.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_RANGE, RANGE, RETRY_AFTER};
use sha2::{Digest, Sha256, Sha512};

use super::{
    ServiceError,
    http::{
        UrlRouter, accelerator_hosts, transport_error, url_uses_accelerator, wait_for_cancellation,
    },
};
use crate::domain::{
    release::{Artifact, Integrity},
    settings::NetworkSettings,
};

mod partial;
mod routes;
use partial::PartialDownload;
#[cfg(test)]
pub(super) mod test_server;

const MAX_INLINE_RETRY_DELAY: Duration = Duration::from_secs(2);

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
        if self.cancellation.load(Ordering::Relaxed) {
            return Err(ServiceError::Cancelled);
        }
        fs::create_dir_all(&self.cache_dir)
            .map_err(|error| ServiceError::io(&self.cache_dir, error))?;
        let valid_integrity = artifact.integrity.as_ref().is_some_and(|digest| {
            let length = match digest {
                Integrity::Sha256(_) => 64,
                Integrity::Sha512(_) => 128,
            };
            digest.value().len() == length && digest.value().bytes().all(|b| b.is_ascii_hexdigit())
        });
        if !valid_integrity {
            return Err(ServiceError::InvalidIntegrity);
        }
        let cached = self.cached_path(artifact);
        if cached.is_file() {
            let mut file = File::open(&cached).map_err(|error| ServiceError::io(&cached, error))?;
            match verify_open_file_with_cancel(
                &mut file,
                &cached,
                artifact,
                Some(&self.cancellation),
            ) {
                Ok(()) => return Ok(cached),
                Err(_) => {
                    fs::remove_file(&cached).map_err(|error| ServiceError::io(&cached, error))?;
                }
            }
        }

        let mut part = PartialDownload::open(&self.cache_dir, artifact)?;
        if artifact
            .size
            .is_some_and(|size| part.len().ok() == Some(size))
        {
            match self.hash_partial(&mut part, artifact) {
                Ok(_) => return part.publish(&cached, artifact, Some(&self.cancellation)),
                Err(ServiceError::HashMismatch { .. }) => part.reset()?,
                Err(error) => {
                    part.close(false);
                    return Err(error);
                }
            }
        } else if artifact
            .size
            .is_some_and(|size| part.len().ok().is_some_and(|length| length > size))
        {
            part.reset()?;
        }
        let candidates = UrlRouter::candidates(artifact, network)?;
        let direct_hosts = accelerator_hosts(network);
        let candidates = match routes::order_candidates(
            &self.direct_client,
            candidates,
            &direct_hosts,
            artifact.size,
            &self.cancellation,
        )
        .await
        {
            Ok(candidates) => candidates,
            Err(error) => {
                part.close(false);
                return Err(error);
            }
        };
        let mut last_error = None;
        for url in candidates {
            if self.cancellation.load(Ordering::Relaxed) {
                part.close(false);
                return Err(ServiceError::Cancelled);
            }
            let observed_url = url.clone();
            match self
                .try_download(url, artifact, &mut part, &direct_hosts, &mut progress)
                .await
            {
                Ok(()) => {
                    routes::record(&observed_url, true);
                    return part.publish(&cached, artifact, Some(&self.cancellation));
                }
                Err(error) => {
                    if matches!(error, ServiceError::Cancelled) {
                        part.close(false);
                        return Err(error);
                    }
                    routes::record(&observed_url, false);
                    if matches!(
                        error,
                        ServiceError::SizeMismatch { .. } | ServiceError::HashMismatch { .. }
                    ) {
                        part.reset()?;
                    }
                    if matches!(error, ServiceError::Io { .. }) {
                        part.close(false);
                        return Err(error);
                    }
                    last_error = Some(error);
                }
            }
        }
        let error = last_error.unwrap_or(ServiceError::AllCandidatesFailed);
        part.close(false);
        Err(error)
    }

    async fn try_download(
        &self,
        url: reqwest::Url,
        artifact: &Artifact,
        part: &mut PartialDownload,
        direct_hosts: &HashSet<String>,
        progress: &mut impl FnMut(DownloadProgress),
    ) -> Result<(), ServiceError> {
        let mut last_error = ServiceError::AllCandidatesFailed;
        let mut full_retry_used = false;
        for attempt in 1..=3 {
            if self.cancellation.load(Ordering::Relaxed) {
                return Err(ServiceError::Cancelled);
            }
            let client = if url_uses_accelerator(&url, direct_hosts) {
                &self.direct_client
            } else {
                &self.fallback_client
            };
            let retained = part.len()?;
            let offset = if artifact.size.is_some() { retained } else { 0 };
            if artifact.size == Some(offset) && offset > 0 {
                self.hash_partial(part, artifact)?;
                return Ok(());
            }
            if offset > 0 {
                progress(DownloadProgress {
                    downloaded: offset,
                    total: artifact.size,
                });
            }
            let mut request = client.get(url.clone()).header(ACCEPT_ENCODING, "identity");
            if offset > 0 {
                request = request.header(RANGE, format!("bytes={offset}-"));
            }
            let mirror = url_uses_accelerator(&url, direct_hosts);
            let result = tokio::select! {
                biased;
                _ = wait_for_cancellation(&self.cancellation) => return Err(ServiceError::Cancelled),
                _ = tokio::time::sleep(Duration::from_secs(6)), if mirror => Err(ServiceError::Transport {host: url.host_str().unwrap_or("unknown").into(), reason: "mirror response timed out".into()}),
                result = request.send() => result.map_err(|error| transport_error(&url, error)),
            };
            let response = match result {
                Ok(response) => response,
                Err(error) if attempt < 3 => {
                    last_error = error;
                    self.wait_before_retry(backoff_delay(attempt)).await?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let status = response.status();
            if status.as_u16() == 206
                && (offset == 0 || !valid_content_range(&response, offset, artifact.size))
            {
                if offset == 0 {
                    return Err(ServiceError::InvalidRange);
                }
                drop(response);
                return Err(ServiceError::InvalidRange);
            }
            if matches!(status.as_u16(), 200 | 206) {
                let write_offset = if status.as_u16() == 206 { offset } else { 0 };
                match self
                    .write_response(response, artifact, part, write_offset, mirror, progress)
                    .await
                {
                    Ok(()) => return Ok(()),
                    Err(error) if retryable_download_error(&error) && attempt < 3 => {
                        last_error = error;
                        self.wait_before_retry(backoff_delay(attempt)).await?;
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            if status.as_u16() == 416 && offset > 0 {
                let matching_total = valid_unsatisfied_range(&response, artifact.size);
                drop(response);
                if matching_total && !full_retry_used {
                    full_retry_used = true;
                    part.reset()?;
                    continue;
                }
                return Err(ServiceError::InvalidRange);
            }
            let retryable = status.as_u16() == 429 || status.is_server_error();
            if retryable && attempt < 3 {
                last_error = ServiceError::Status {
                    status: status.as_u16(),
                    host: url.host_str().unwrap_or("unknown").into(),
                };
                let retry_after = retry_after(&response);
                if retry_after.is_some_and(|delay| delay > MAX_INLINE_RETRY_DELAY) {
                    return Err(last_error);
                }
                let delay = retry_after.unwrap_or_else(|| backoff_delay(attempt));
                self.wait_before_retry(delay).await?;
                continue;
            }
            return Err(ServiceError::Status {
                status: status.as_u16(),
                host: url.host_str().unwrap_or("unknown").to_owned(),
            });
        }
        Err(last_error)
    }

    async fn wait_before_retry(&self, delay: Duration) -> Result<(), ServiceError> {
        tokio::select! {
            biased;
            _ = wait_for_cancellation(&self.cancellation) => Err(ServiceError::Cancelled),
            _ = tokio::time::sleep(delay) => Ok(()),
        }
    }

    async fn write_response(
        &self,
        response: reqwest::Response,
        artifact: &Artifact,
        part: &mut PartialDownload,
        offset: u64,
        mirror: bool,
        progress: &mut impl FnMut(DownloadProgress),
    ) -> Result<(), ServiceError> {
        if !has_identity_content_encoding(&response) {
            return Err(ServiceError::InvalidRange);
        }
        if offset == 0 && response.headers().contains_key(CONTENT_RANGE) {
            return Err(ServiceError::InvalidRange);
        }
        let total = artifact.size.or_else(|| {
            response
                .content_length()
                .and_then(|length| offset.checked_add(length))
        });
        let response_url = response.url().clone();
        if let (Some(expected), Some(actual)) = (artifact.size, response.content_length())
            && Some(expected) != offset.checked_add(actual)
        {
            return Err(ServiceError::SizeMismatch {
                expected,
                actual: offset.saturating_add(actual),
            });
        }

        if offset == 0 {
            part.reset()?;
        }
        let mut stream = response.bytes_stream();
        let mut downloaded = offset;
        let mut hasher = self.hash_prefix(part, artifact)?;
        loop {
            let chunk = tokio::select! {
                biased;
                _ = wait_for_cancellation(&self.cancellation) => return Err(ServiceError::Cancelled),
                _ = tokio::time::sleep(Duration::from_secs(8)), if mirror => return Err(ServiceError::Transport {host: response_url.host_str().unwrap_or("unknown").into(), reason: "mirror transfer stalled".into()}),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|error| transport_error(&response_url, error))?;
            let next = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or(ServiceError::ResponseTooLarge)?;
            if let Some(expected) = total
                && next > expected
            {
                return Err(ServiceError::SizeMismatch {
                    expected,
                    actual: next,
                });
            }
            part.file
                .write_all(&chunk)
                .map_err(|error| ServiceError::io(&part.path, error))?;
            if let Some(hasher) = &mut hasher {
                hasher.update(&chunk);
            }
            downloaded = next;
            progress(DownloadProgress { downloaded, total });
        }
        part.file
            .flush()
            .and_then(|()| part.file.sync_all())
            .map_err(|error| ServiceError::io(&part.path, error))?;
        if let Some(expected) = artifact.size
            && expected != downloaded
        {
            return Err(ServiceError::Transport {
                host: response_url.host_str().unwrap_or("unknown").into(),
                reason: format!(
                    "response body ended after {downloaded} of {expected} trusted bytes"
                ),
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

    fn hash_prefix(
        &self,
        part: &mut PartialDownload,
        artifact: &Artifact,
    ) -> Result<Option<StreamingHasher>, ServiceError> {
        part.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| ServiceError::io(&part.path, e))?;
        let mut hasher = StreamingHasher::new(artifact.integrity.as_ref());
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            if self.cancellation.load(Ordering::Relaxed) {
                return Err(ServiceError::Cancelled);
            }
            let read = part
                .file
                .read(&mut buffer)
                .map_err(|e| ServiceError::io(&part.path, e))?;
            if read == 0 {
                break;
            }
            if let Some(hasher) = &mut hasher {
                hasher.update(&buffer[..read]);
            }
        }
        Ok(hasher)
    }

    fn hash_partial(
        &self,
        part: &mut PartialDownload,
        artifact: &Artifact,
    ) -> Result<(), ServiceError> {
        if let (Some(hasher), Some(integrity)) = (
            self.hash_prefix(part, artifact)?,
            artifact.integrity.as_ref(),
        ) {
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
            |integrity| format!("{}.artifact", integrity.value().to_ascii_lowercase()),
        );
        self.cache_dir.join(name)
    }
}

fn retryable_download_error(error: &ServiceError) -> bool {
    matches!(
        error,
        ServiceError::Network(_) | ServiceError::Transport { .. }
    )
}

fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_millis(400 << (attempt.saturating_sub(1).min(2)))
}

fn valid_content_range(response: &reqwest::Response, offset: u64, expected: Option<u64>) -> bool {
    let mut values = response.headers().get_all(CONTENT_RANGE).iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Some((start, end, total)) = value.to_str().ok().and_then(parse_content_range) else {
        return false;
    };
    start == offset
        && end.checked_add(1) == Some(total)
        && expected.is_none_or(|expected| expected == total)
        && response
            .content_length()
            .is_none_or(|length| length == total - start)
}

fn has_identity_content_encoding(response: &reqwest::Response) -> bool {
    let mut values = response.headers().get_all(CONTENT_ENCODING).iter();
    match values.next() {
        None => true,
        Some(value) => {
            values.next().is_none()
                && value
                    .to_str()
                    .is_ok_and(|value| value.eq_ignore_ascii_case("identity"))
        }
    }
}

fn valid_unsatisfied_range(response: &reqwest::Response, expected: Option<u64>) -> bool {
    let mut values = response.headers().get_all(CONTENT_RANGE).iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    value
        .to_str()
        .ok()
        .and_then(|value| value.strip_prefix("bytes */"))
        .and_then(|total| total.parse::<u64>().ok())
        .is_some_and(|total| expected == Some(total))
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let mut values = response.headers().get_all(RETRY_AFTER).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let value = value.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    parse_http_date(value).map(|deadline| Duration::from_secs(deadline.saturating_sub(now)))
}

fn parse_http_date(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let _weekday = parts.next()?;
    let day = parts.next()?;
    let month = parts.next()?;
    let year = parts.next()?.parse::<u32>().ok()?;
    let time = parts.next()?;
    if !parts.next()?.eq_ignore_ascii_case("GMT") || parts.next().is_some() || year < 1970 {
        return None;
    }
    if day.len() != 2 {
        return None;
    }
    let day = day.parse::<u32>().ok()?;
    let month = match month {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    if day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let mut clock = time.split(':');
    let hour = clock.next()?.parse::<u32>().ok()?;
    let minute = clock.next()?.parse::<u32>().ok()?;
    let second = clock.next()?.parse::<u32>().ok()?;
    if clock.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year as i64, month, day);
    u64::try_from(days)
        .ok()?
        .checked_mul(86_400)?
        .checked_add(u64::from(hour * 3_600 + minute * 60 + second))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(mut year: i64, month: u32, day: u32) -> i64 {
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = end.parse::<u64>().ok()?;
    let total = total.parse::<u64>().ok()?;
    (start <= end && end < total).then_some((start, end, total))
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
    let mut file = File::open(path).map_err(|error| ServiceError::io(path, error))?;
    verify_open_file(&mut file, path, artifact)
}

pub(super) fn verify_open_file(
    file: &mut File,
    path: &Path,
    artifact: &Artifact,
) -> Result<(), ServiceError> {
    verify_open_file_with_cancel(file, path, artifact, None)
}

pub(super) fn verify_open_file_with_cancel(
    file: &mut File,
    path: &Path,
    artifact: &Artifact,
    cancellation: Option<&AtomicBool>,
) -> Result<(), ServiceError> {
    let length = file
        .metadata()
        .map_err(|error| ServiceError::io(path, error))?
        .len();
    if let Some(expected) = artifact.size
        && length != expected
    {
        return Err(ServiceError::SizeMismatch {
            expected,
            actual: length,
        });
    }
    if let Some(integrity) = &artifact.integrity {
        file.rewind()
            .map_err(|error| ServiceError::io(path, error))?;
        let mut hasher =
            StreamingHasher::new(Some(integrity)).ok_or(ServiceError::AllCandidatesFailed)?;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            if cancellation.is_some_and(|cancelled| cancelled.load(Ordering::Relaxed)) {
                return Err(ServiceError::Cancelled);
            }
            let read = std::io::Read::read(file, &mut buffer)
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
    #[test]
    fn range_parser_rejects_inconsistent_and_overflowing_ranges() {
        for value in [
            "bytes 9-3/12",
            "bytes 0-12/12",
            "bytes 0-1/*",
            "bytes */12",
            "bytes 18446744073709551616-20/22",
            "items 0-2/3",
        ] {
            assert!(super::parse_content_range(value).is_none(), "{value}");
        }
        assert_eq!(
            super::parse_content_range("bytes 3-11/12"),
            Some((3, 11, 12))
        );
    }

    #[test]
    fn retry_after_http_date_uses_utc_epoch_seconds() {
        assert_eq!(
            super::parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(1_445_412_480)
        );
    }
    use std::{
        fs,
        io::Write,
        sync::{Arc, atomic::AtomicBool},
        thread,
    };

    use sha2::{Digest, Sha256, Sha512};
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
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let status = status.to_owned();
        thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let _ = super::test_server::read_request(&mut socket);
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let mut response = header.into_bytes();
            response.extend_from_slice(&body);
            super::test_server::write_all(&mut socket, &response).unwrap();
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
    async fn cancellation_interrupts_waiting_for_headers_and_the_next_chunk() {
        for send_headers in [false, true] {
            let (listener, address) = super::test_server::TestListener::bind().unwrap();
            let (accepted, waiting) = std::sync::mpsc::channel();
            let server = thread::spawn(move || {
                let mut socket = listener.accept().unwrap();
                let _ = super::test_server::read_request(&mut socket);
                if send_headers {
                    super::test_server::write_all(
                        &mut socket,
                        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
                }
                accepted.send(()).unwrap();
                thread::sleep(std::time::Duration::from_secs(2));
            });
            let cancellation = Arc::new(AtomicBool::new(false));
            let signal = Arc::clone(&cancellation);
            thread::spawn(move || {
                waiting.recv().unwrap();
                thread::sleep(std::time::Duration::from_millis(25));
                signal.store(true, std::sync::atomic::Ordering::Relaxed);
            });
            let directory = tempdir().unwrap();
            let downloader = Downloader::new(
                reqwest::Client::new(),
                directory.path().to_path_buf(),
                cancellation,
            );
            let release = artifact(format!("http://{address}/artifact.zip"), b"x");
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(500),
                downloader.download(&release, &NetworkSettings::default(), |_| {}),
            )
            .await;
            assert!(
                matches!(result, Ok(Err(crate::services::ServiceError::Cancelled))),
                "cancel must interrupt stalled network I/O"
            );
            assert!(
                !std::fs::read_dir(directory.path())
                    .unwrap()
                    .flatten()
                    .any(
                        |entry| entry.path().extension().and_then(std::ffi::OsStr::to_str)
                            == Some("part")
                    )
            );
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn reuses_one_verified_artifact_for_later_targets() {
        let bytes = b"shared chromeplus runtime".to_vec();
        let url = serve_once(bytes.clone(), "200 OK");
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().to_path_buf(),
            Arc::new(AtomicBool::new(false)),
        );
        let artifact = artifact(url, &bytes);

        let first = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        let second = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(std::fs::read(second).unwrap(), bytes);
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
        assert!(
            !std::fs::read_dir(directory.path())
                .unwrap()
                .flatten()
                .any(
                    |entry| entry.path().extension().and_then(std::ffi::OsStr::to_str)
                        == Some("part")
                )
        );
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

    #[tokio::test]
    async fn resumes_a_partial_download_with_a_valid_range_response() {
        let full = b"resumable browser artifact".to_vec();
        let split = 9usize;
        let server_full = full.clone();
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let expected_range = format!("bytes={split}-");
        let server = thread::spawn(move || {
            let mut first = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut first).unwrap();
            assert!(!String::from_utf8_lossy(&request).contains("Range:"));
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_full.len()
            )
            .into_bytes();
            response.extend_from_slice(&server_full[..split]);
            super::test_server::write_all(&mut first, &response).unwrap();
            drop(first);

            let mut second = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut second).unwrap();
            assert!(String::from_utf8_lossy(&request).contains(&expected_range));
            let remaining = &server_full[split..];
            let mut response = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                remaining.len(),
                split,
                server_full.len() - 1,
                server_full.len()
            )
            .into_bytes();
            response.extend_from_slice(remaining);
            super::test_server::write_all(&mut second, &response).unwrap();
        });
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().to_path_buf(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(
                &artifact(format!("http://{address}/artifact.zip"), &full),
                &NetworkSettings::default(),
                |_| {},
            )
            .await
            .unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), full);
    }

    #[tokio::test]
    async fn resumes_and_verifies_a_sha512_artifact() {
        let full = b"sha512 resumable artifact".to_vec();
        let split = 7usize;
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let directory = tempdir().unwrap();
        let mut artifact = artifact(format!("http://{address}/artifact.zip"), &full);
        artifact.integrity = Some(Integrity::Sha512(hex::encode(Sha512::digest(&full))));
        let mut partial = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        partial.file.write_all(&full[..split]).unwrap();
        partial.close(false);
        let expected_range = format!("bytes={split}-");
        let server_full = full.clone();
        let server = thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(String::from_utf8_lossy(&request).contains(&expected_range));
            let remaining = &server_full[split..];
            let mut response = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                remaining.len(),
                split,
                server_full.len() - 1,
                server_full.len()
            )
            .into_bytes();
            response.extend_from_slice(remaining);
            super::test_server::write_all(&mut socket, &response).unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), full);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn restarts_when_a_server_ignores_the_resume_range() {
        let full = b"restart from zero after ignored range".to_vec();
        let split = 7usize;
        let server_full = full.clone();
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let server = thread::spawn(move || {
            let mut first = listener.accept().unwrap();
            let _ = super::test_server::read_request(&mut first).unwrap();
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_full.len()
            )
            .into_bytes();
            response.extend_from_slice(&server_full[..split]);
            super::test_server::write_all(&mut first, &response).unwrap();
            drop(first);

            let mut second = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut second).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range:")
            );
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_full.len()
            )
            .into_bytes();
            response.extend_from_slice(&server_full);
            super::test_server::write_all(&mut second, &response).unwrap();
        });
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().to_path_buf(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(
                &artifact(format!("http://{address}/artifact.zip"), &full),
                &NetworkSettings::default(),
                |_| {},
            )
            .await
            .unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), full);
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

    #[tokio::test]
    async fn a_matching_416_allows_one_full_retry_without_appending_unverified_bytes() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let artifact = artifact(format!("http://{address}/artifact.zip"), bytes);
        let mut saved = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        saved.file.write_all(b"abc").unwrap();
        saved.close(false);
        let server = thread::spawn(move || {
            let mut first = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut first).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut first,
                b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */12\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            drop(first);
            let mut second = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut second).unwrap();
            assert!(
                !String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("\r\nrange:")
            );
            super::test_server::write_all(
                &mut second,
                b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nabcdefghijkl",
            )
            .unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn a_mismatched_416_keeps_the_prefix_and_switches_candidates() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (bad, bad_address) = super::test_server::TestListener::bind().unwrap();
        let (good, good_address) = super::test_server::TestListener::bind().unwrap();
        let mut artifact = artifact(format!("http://{bad_address}/bad"), bytes);
        artifact.urls.push(format!("http://{good_address}/good"));
        let mut saved = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        saved.file.write_all(b"abc").unwrap();
        saved.close(false);
        let bad_server = thread::spawn(move || {
            let mut socket = bad.accept().unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */13\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        });
        let good_server = thread::spawn(move || {
            let mut socket = good.accept().unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 9\r\nContent-Range: bytes 3-11/12\r\nConnection: close\r\n\r\ndefghijkl",
            )
            .unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        bad_server.join().unwrap();
        good_server.join().unwrap();
    }

    #[tokio::test]
    async fn duplicate_content_range_headers_are_rejected_without_touching_the_prefix() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let artifact = artifact(format!("http://{address}/artifact.zip"), bytes);
        let mut saved = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        saved.file.write_all(b"abc").unwrap();
        saved.close(false);
        let server = thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let _ = super::test_server::read_request(&mut socket).unwrap();
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 9\r\nContent-Range: bytes 3-11/12\r\nContent-Range: bytes 3-11/12\r\nConnection: close\r\n\r\ndefghijkl",
            )
            .unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let result = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await;
        assert!(matches!(
            result,
            Err(crate::services::ServiceError::InvalidRange)
        ));
        let retained = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        assert_eq!(retained.len().unwrap(), 3);
        retained.close(false);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn invalid_range_keeps_the_prefix_and_switches_candidates() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (bad, bad_address) = super::test_server::TestListener::bind().unwrap();
        let (good, good_address) = super::test_server::TestListener::bind().unwrap();
        let mut artifact = artifact(format!("http://{bad_address}/bad"), bytes);
        artifact.urls.push(format!("http://{good_address}/good"));
        let mut partial = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        partial.file.write_all(b"abc").unwrap();
        partial.close(false);

        let bad_server = thread::spawn(move || {
            let mut socket = bad.accept().unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 8\r\nContent-Range: bytes 4-11/12\r\nConnection: close\r\n\r\nefghijkl",
            )
            .unwrap();
        });
        let good_server = thread::spawn(move || {
            let mut socket = good.accept().unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 9\r\nContent-Range: bytes 3-11/12\r\nConnection: close\r\n\r\ndefghijkl",
            )
            .unwrap();
        });

        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        bad_server.join().unwrap();
        good_server.join().unwrap();
    }

    #[tokio::test]
    async fn unknown_trusted_size_never_sends_a_resume_range() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let mut artifact = artifact(format!("http://{address}/artifact.zip"), bytes);
        artifact.size = None;
        let mut saved = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        saved.file.write_all(b"abc").unwrap();
        saved.close(false);
        let server = thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(
                !String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("\r\nrange:")
            );
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nabcdefghijkl",
            )
            .unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn truncated_body_retains_the_prefix_and_resumes_it() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let artifact = artifact(format!("http://{address}/artifact.zip"), bytes);
        let server = thread::spawn(move || {
            let mut first = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut first).unwrap();
            assert!(
                !String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("\r\nrange:")
            );
            super::test_server::write_all(
                &mut first,
                b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nabc",
            )
            .unwrap();
            drop(first);
            let mut second = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut second).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut second,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 9\r\nContent-Range: bytes 3-11/12\r\nConnection: close\r\n\r\ndefghijkl",
            )
            .unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn clean_eof_before_trusted_size_is_retryable_and_preserves_the_prefix() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let artifact = artifact(format!("http://{address}/artifact.zip"), bytes);
        let server = thread::spawn(move || {
            let mut first = listener.accept().unwrap();
            let _ = super::test_server::read_request(&mut first).unwrap();
            super::test_server::write_all(
                &mut first,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nabc",
            )
            .unwrap();
            drop(first);
            let mut second = listener.accept().unwrap();
            let request = super::test_server::read_request(&mut second).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut second,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 9\r\nContent-Range: bytes 3-11/12\r\nConnection: close\r\n\r\ndefghijkl",
            )
            .unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn interrupted_first_candidate_resumes_on_the_second() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (bad, bad_address) = super::test_server::TestListener::bind().unwrap();
        let (good, good_address) = super::test_server::TestListener::bind().unwrap();
        let mut artifact = artifact(format!("http://{bad_address}/bad"), bytes);
        artifact.urls.push(format!("http://{good_address}/good"));
        let bad_server = thread::spawn(move || {
            let mut socket = bad.accept().unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(
                !String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("\r\nrange:")
            );
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nabc",
            )
            .unwrap();
        });
        let good_server = thread::spawn(move || {
            let mut socket = good.accept_for(std::time::Duration::from_secs(3)).unwrap();
            let request = super::test_server::read_request(&mut socket).unwrap();
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("range: bytes=3-")
            );
            super::test_server::write_all(
                &mut socket,
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 9\r\nContent-Range: bytes 3-11/12\r\nConnection: close\r\n\r\ndefghijkl",
            )
            .unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_millis(200))
                .build()
                .unwrap(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        bad_server.join().unwrap();
        good_server.join().unwrap();
    }

    #[tokio::test]
    async fn retries_connection_failures_three_times_before_succeeding() {
        let bytes = b"verified after retries";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let url = format!("http://{address}/artifact.zip");
        let server = thread::spawn(move || {
            for attempt in 0..3 {
                let mut socket = listener.accept().unwrap();
                let _ = super::test_server::read_request(&mut socket);
                if attempt < 2 {
                    drop(socket);
                } else {
                    let mut response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    )
                    .into_bytes();
                    response.extend_from_slice(bytes);
                    super::test_server::write_all(&mut socket, &response).unwrap();
                }
            }
        });
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact(url, bytes), &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn short_retry_after_is_respected_before_retrying() {
        let bytes = b"respect retry after";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let url = format!("http://{address}/artifact.zip");
        let server = thread::spawn(move || {
            let mut first = listener.accept().unwrap();
            let _ = super::test_server::read_request(&mut first).unwrap();
            super::test_server::write_all(
                &mut first,
                b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nRetry-After: 1\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            let mut second = listener
                .accept_for(std::time::Duration::from_secs(2))
                .unwrap();
            let _ = super::test_server::read_request(&mut second).unwrap();
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .into_bytes();
            response.extend_from_slice(bytes);
            super::test_server::write_all(&mut second, &response).unwrap();
        });
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let started = std::time::Instant::now();
        let path = downloader
            .download(&artifact(url, bytes), &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert!(started.elapsed() >= std::time::Duration::from_millis(900));
        assert_eq!(fs::read(path).unwrap(), bytes);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn long_retry_after_switches_to_another_candidate_without_waiting() {
        let bytes = b"use another candidate";
        let (bad, bad_address) = super::test_server::TestListener::bind().unwrap();
        let (good, good_address) = super::test_server::TestListener::bind().unwrap();
        let mut artifact = artifact(format!("http://{bad_address}/bad"), bytes);
        artifact.urls.push(format!("http://{good_address}/good"));
        let bad_server = thread::spawn(move || {
            while let Ok(mut socket) = bad.accept() {
                let _ = super::test_server::read_request(&mut socket).unwrap();
                super::test_server::write_all(
                    &mut socket,
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nRetry-After: 5\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            }
        });
        let good_server = thread::spawn(move || {
            let mut socket = good.accept().unwrap();
            let _ = super::test_server::read_request(&mut socket).unwrap();
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .into_bytes();
            response.extend_from_slice(bytes);
            super::test_server::write_all(&mut socket, &response).unwrap();
        });
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            downloader.download(&artifact, &NetworkSettings::default(), |_| {}),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        good_server.join().unwrap();
        bad_server.join().unwrap();
    }

    #[tokio::test]
    async fn cancellation_interrupts_transport_retry_backoff() {
        let bytes = b"retry backoff";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let url = format!("http://{address}/artifact.zip");
        let cancellation = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancellation);
        let (failed_once, first_failure) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            for attempt in 0..3 {
                let Ok(mut socket) = listener.accept_for(std::time::Duration::from_millis(500))
                else {
                    break;
                };
                let _ = super::test_server::read_request(&mut socket);
                if attempt == 0 {
                    failed_once.send(()).unwrap();
                }
            }
        });
        let canceller = thread::spawn(move || {
            first_failure.recv().unwrap();
            thread::sleep(std::time::Duration::from_millis(25));
            signal.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let directory = tempdir().unwrap();
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            cancellation,
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            downloader.download(&artifact(url, bytes), &NetworkSettings::default(), |_| {}),
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            Err(crate::services::ServiceError::Cancelled)
        ));
        canceller.join().unwrap();
        server.join().unwrap();
    }

    #[tokio::test]
    async fn a_complete_retained_payload_is_verified_without_network_access() {
        let directory = tempdir().unwrap();
        let artifact = artifact("https://invalid.invalid/not-requested".into(), b"complete");
        let mut partial = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        partial.file.write_all(b"complete").unwrap();
        partial.close(false);
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            downloader.download(&artifact, &NetworkSettings::default(), |_| {}),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), b"complete");
    }

    #[tokio::test]
    async fn corrupt_retained_prefix_never_passes_the_final_digest_or_poisons_next_source() {
        let directory = tempdir().unwrap();
        let bytes = b"abcdefghijkl";
        let (listener, address) = super::test_server::TestListener::bind().unwrap();
        let url = format!("http://{address}/bad-prefix");
        let mut artifact = artifact(url, bytes);
        artifact.urls.push(serve_once(bytes.to_vec(), "200 OK"));
        let mut partial = super::PartialDownload::open(directory.path(), &artifact).unwrap();
        partial.file.write_all(b"BAD").unwrap();
        partial.close(false);
        let server = thread::spawn(move || {
            let mut socket = listener.accept().unwrap();
            let _ = super::test_server::read_request(&mut socket);
            super::test_server::write_all(&mut socket, b"HTTP/1.1 206 Partial Content\r\nContent-Length: 9\r\nContent-Range: bytes 3-11/12\r\nConnection: close\r\n\r\ndefghijkl").unwrap();
        });
        let downloader = Downloader::new(
            reqwest::Client::new(),
            directory.path().into(),
            Arc::new(AtomicBool::new(false)),
        );
        let path = downloader
            .download(&artifact, &NetworkSettings::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        server.join().unwrap();
    }
}
