//! Timestamped release descriptor cache.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::ServiceError;
use crate::domain::{
    release::{CpuBuild, ReleaseChannel, ReleaseDescriptor},
    target::TargetId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheEntry {
    checked_at: u64,
    descriptor: ReleaseDescriptor,
}

#[derive(Debug)]
pub struct ReleaseCache {
    path: PathBuf,
    entries: BTreeMap<String, CacheEntry>,
}

impl ReleaseCache {
    pub fn load(path: PathBuf) -> Result<Self, ServiceError> {
        let entries = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(ServiceError::Cache)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(ServiceError::io(&path, error)),
        };
        Ok(Self { path, entries })
    }

    pub fn get_fresh(
        &self,
        target: TargetId,
        channel: ReleaseChannel,
        cpu_build: CpuBuild,
        max_age_seconds: u64,
    ) -> Option<&ReleaseDescriptor> {
        self.entries
            .get(&key(target, channel, cpu_build))
            .filter(|entry| now_seconds().saturating_sub(entry.checked_at) <= max_age_seconds)
            .map(|entry| &entry.descriptor)
    }

    pub fn get_any(
        &self,
        target: TargetId,
        channel: ReleaseChannel,
        cpu_build: CpuBuild,
    ) -> Option<&ReleaseDescriptor> {
        self.entries
            .get(&key(target, channel, cpu_build))
            .map(|entry| &entry.descriptor)
    }

    pub fn get_any_with_checked_at(
        &self,
        target: TargetId,
        channel: ReleaseChannel,
        cpu_build: CpuBuild,
    ) -> Option<(&ReleaseDescriptor, u64)> {
        self.entries
            .get(&key(target, channel, cpu_build))
            .map(|entry| (&entry.descriptor, entry.checked_at))
    }

    pub fn store(&mut self, descriptor: ReleaseDescriptor) -> Result<(), ServiceError> {
        self.entries.insert(
            key(descriptor.target, descriptor.channel, descriptor.cpu_build),
            CacheEntry {
                checked_at: now_seconds(),
                descriptor,
            },
        );
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| ServiceError::io(parent, error))?;
        }
        let bytes = serde_json::to_vec_pretty(&self.entries).map_err(ServiceError::Cache)?;
        atomic_write(&self.path, &bytes)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn key(target: TargetId, channel: ReleaseChannel, cpu_build: CpuBuild) -> String {
    format!("{target:?}:{channel:?}:{cpu_build:?}")
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ServiceError> {
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|error| ServiceError::io(&temp, error))?;
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        ServiceError::io(path, error)
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::ReleaseCache;
    use crate::domain::release::{
        ArchiveFormat, Artifact, CpuBuild, Integrity, ProgramPlacement, ProviderKind,
        ReleaseChannel, ReleaseDescriptor,
    };
    use crate::domain::target::TargetId;

    fn descriptor() -> ReleaseDescriptor {
        ReleaseDescriptor {
            target: TargetId::Chrome,
            version: "1.0.0".to_owned(),
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            provider: ProviderKind::ChromeOmaha,
            artifact: Artifact {
                file_name: "chrome.exe".to_owned(),
                format: ArchiveFormat::SevenZip,
                size: None,
                integrity: Some(Integrity::Sha256("0".repeat(64))),
                urls: vec!["https://dl.google.com/chrome.exe".to_owned()],
                signature_url: None,
            },
            placement: ProgramPlacement::App,
            inner_archive: None,
            source: "fixture".to_owned(),
            published_at: None,
        }
    }

    #[test]
    fn release_descriptor_round_trips_through_cache() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("releases.json");
        let mut cache = ReleaseCache::load(path.clone()).unwrap();
        cache.store(descriptor()).unwrap();

        let loaded = ReleaseCache::load(path).unwrap();

        assert_eq!(
            loaded
                .get_fresh(TargetId::Chrome, ReleaseChannel::Stable, CpuBuild::X64, 60)
                .unwrap(),
            &descriptor()
        );
        let (cached, checked_at) = loaded
            .get_any_with_checked_at(TargetId::Chrome, ReleaseChannel::Stable, CpuBuild::X64)
            .unwrap();
        assert_eq!(cached, &descriptor());
        assert!(checked_at > 0);
    }
}
