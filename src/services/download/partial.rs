//! A locked owner record binds a resumable payload to one trusted artifact.

use crate::{
    domain::release::{Artifact, Integrity},
    infra::windows::file_move::move_file_without_replace,
    services::ServiceError,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema: u8,
    integrity: Option<Integrity>,
    size: Option<u64>,
    payload: String,
}

pub(super) struct PartialDownload {
    pub file: File,
    pub path: PathBuf,
    owner_file: File,
    owner_path: PathBuf,
}

impl PartialDownload {
    pub fn open(cache: &Path, artifact: &Artifact) -> Result<Self, ServiceError> {
        let key = resume_key(artifact);
        let owner_path = cache.join(format!("bh-{key}.resume.part"));
        let owner_file = match exclusive_options().open(&owner_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match initialize(cache, artifact, &key, &owner_path)? {
                    Some(initialized) => return Ok(initialized),
                    None => exclusive_options()
                        .open(&owner_path)
                        .map_err(|error| ServiceError::io(&owner_path, error))?,
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(ServiceError::io(&owner_path, error));
            }
            Err(error) => return Err(ServiceError::io(&owner_path, error)),
        };
        open_existing(cache, artifact, &key, &owner_path, owner_file)
    }

    pub fn len(&self) -> Result<u64, ServiceError> {
        self.file
            .metadata()
            .map(|m| m.len())
            .map_err(|e| ServiceError::io(&self.path, e))
    }

    pub fn reset(&mut self) -> Result<(), ServiceError> {
        self.file
            .set_len(0)
            .and_then(|()| self.file.seek(SeekFrom::Start(0)).map(|_| ()))
            .map_err(|e| ServiceError::io(&self.path, e))
    }

    pub fn publish(
        self,
        destination: &Path,
        artifact: &Artifact,
        cancellation: Option<&AtomicBool>,
    ) -> Result<PathBuf, ServiceError> {
        let Self {
            file,
            path,
            owner_file,
            owner_path,
        } = self;
        file.sync_all().map_err(|e| ServiceError::io(&path, e))?;
        drop(file);
        move_file_without_replace(&path, destination)
            .map_err(|error| ServiceError::io(&path, error))?;
        let mut published = exclusive_options()
            .open(destination)
            .map_err(|error| ServiceError::io(destination, error))?;
        require_regular_file(&published, destination)?;
        if let Err(error) =
            super::verify_open_file_with_cancel(&mut published, destination, artifact, cancellation)
        {
            drop(published);
            if let Err(rollback) = move_file_without_replace(destination, &path) {
                return Err(ServiceError::io(
                    &path,
                    io::Error::other(format!(
                        "final verification failed ({error}); restoring the payload failed: {rollback}"
                    )),
                ));
            }
            return Err(error);
        }
        drop(owner_file);
        let _ = fs::remove_file(owner_path);
        drop(published);
        Ok(destination.to_path_buf())
    }

    pub fn close(self, invalid: bool) {
        let remove = invalid || self.len().ok() == Some(0);
        let _ = self.file.sync_all();
        let Self {
            file,
            path,
            owner_file,
            owner_path,
        } = self;
        drop(file);
        if remove {
            let _ = fs::remove_file(path);
        }
        drop(owner_file);
        if remove {
            let _ = fs::remove_file(owner_path);
        }
    }
}

fn initialize(
    cache: &Path,
    artifact: &Artifact,
    key: &str,
    owner_path: &Path,
) -> Result<Option<PartialDownload>, ServiceError> {
    let (path, file) = create_payload(cache, key)?;
    let owner = Owner {
        schema: 1,
        integrity: artifact.integrity.clone(),
        size: artifact.size,
        payload: path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid_owner(owner_path))?
            .to_owned(),
    };
    let bytes = serde_json::to_vec(&owner).map_err(ServiceError::Cache)?;
    let (temporary_owner, mut temporary_file) = create_temporary_owner(cache, key)?;
    if let Err(error) = temporary_file
        .write_all(&bytes)
        .and_then(|()| temporary_file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&temporary_owner);
        let _ = fs::remove_file(&path);
        return Err(ServiceError::io(&temporary_owner, error));
    }
    drop(temporary_file);
    match move_file_without_replace(&temporary_owner, owner_path) {
        Ok(()) => {
            let owner_file = exclusive_options()
                .open(owner_path)
                .map_err(|error| ServiceError::io(owner_path, error))?;
            Ok(Some(PartialDownload {
                file,
                path,
                owner_file,
                owner_path: owner_path.to_path_buf(),
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            drop(file);
            let _ = fs::remove_file(&temporary_owner);
            let _ = fs::remove_file(&path);
            Ok(None)
        }
        Err(error) => {
            drop(file);
            let _ = fs::remove_file(&temporary_owner);
            let _ = fs::remove_file(&path);
            Err(ServiceError::io(owner_path, error))
        }
    }
}

fn create_payload(cache: &Path, key: &str) -> Result<(PathBuf, File), ServiceError> {
    for _ in 0..8 {
        let name = format!("bh-{key}-{}.part", random_nonce()?);
        let path = cache.join(name);
        match exclusive_options().create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ServiceError::io(&path, error)),
        }
    }
    Err(ServiceError::io(
        cache,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique partial payload",
        ),
    ))
}

fn create_temporary_owner(cache: &Path, key: &str) -> Result<(PathBuf, File), ServiceError> {
    for _ in 0..8 {
        let name = format!("bh-{key}-{}.owner.part", random_nonce()?);
        let path = cache.join(name);
        match exclusive_options().create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ServiceError::io(&path, error)),
        }
    }
    Err(ServiceError::io(
        cache,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary resume record",
        ),
    ))
}

fn random_nonce() -> Result<String, ServiceError> {
    let mut nonce = [0; 16];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|error| ServiceError::io("OsRng", io::Error::other(error.to_string())))?;
    Ok(hex::encode(nonce))
}

fn open_existing(
    cache: &Path,
    artifact: &Artifact,
    key: &str,
    owner_path: &Path,
    mut owner_file: File,
) -> Result<PartialDownload, ServiceError> {
    require_regular_file(&owner_file, owner_path)?;
    if owner_file
        .metadata()
        .map_err(|e| ServiceError::io(owner_path, e))?
        .len()
        > 2048
    {
        return Err(invalid_owner(owner_path));
    }
    let mut bytes = Vec::new();
    owner_file
        .read_to_end(&mut bytes)
        .map_err(|e| ServiceError::io(owner_path, e))?;
    let owner: Owner = serde_json::from_slice(&bytes).map_err(|_| invalid_owner(owner_path))?;
    let prefix = format!("bh-{key}-");
    let nonce = owner
        .payload
        .strip_prefix(&prefix)
        .and_then(|s| s.strip_suffix(".part"));
    if owner.schema != 1
        || !integrity_matches(owner.integrity.as_ref(), artifact.integrity.as_ref())
        || owner.size != artifact.size
        || !nonce.is_some_and(|n| n.len() == 32 && n.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(invalid_owner(owner_path));
    }
    let path = cache.join(owner.payload);
    let file = match exclusive_options().open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => exclusive_options()
            .create_new(true)
            .open(&path)
            .map_err(|e| ServiceError::io(&path, e))?,
        Err(error) => return Err(ServiceError::io(&path, error)),
    };
    require_regular_file(&file, &path)?;
    Ok(PartialDownload {
        file,
        path,
        owner_file,
        owner_path: owner_path.to_path_buf(),
    })
}

fn resume_key(artifact: &Artifact) -> String {
    let integrity = match &artifact.integrity {
        Some(Integrity::Sha256(value)) => format!("sha256:{}", value.to_ascii_lowercase()),
        Some(Integrity::Sha512(value)) => format!("sha512:{}", value.to_ascii_lowercase()),
        None => "none".to_owned(),
    };
    let size = artifact
        .size
        .map_or_else(|| "none".to_owned(), |size| size.to_string());
    hex::encode(Sha256::digest(format!(
        "browser-harbor-resume-v1\0{integrity}\0{size}"
    )))
}

fn integrity_matches(left: Option<&Integrity>, right: Option<&Integrity>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(Integrity::Sha256(left)), Some(Integrity::Sha256(right)))
        | (Some(Integrity::Sha512(left)), Some(Integrity::Sha512(right))) => {
            left.eq_ignore_ascii_case(right)
        }
        _ => false,
    }
}

fn invalid_owner(path: &Path) -> ServiceError {
    ServiceError::io(
        path,
        io::Error::new(
            io::ErrorKind::InvalidData,
            "download resume record is invalid; retained for inspection",
        ),
    )
}

fn exclusive_options() -> OpenOptions {
    use std::os::windows::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    // A resume record is the per-artifact lock; opening the reparse point itself prevents link traversal.
    options
        .read(true)
        .write(true)
        .share_mode(0)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    options
}

fn require_regular_file(file: &File, path: &Path) -> Result<(), ServiceError> {
    use std::os::windows::fs::MetadataExt;
    let metadata = file.metadata().map_err(|e| ServiceError::io(path, e))?;
    if !metadata.is_file()
        || metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    {
        return Err(invalid_owner(path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn artifact() -> Artifact {
        Artifact {
            file_name: "browser.zip".into(),
            format: crate::domain::release::ArchiveFormat::Zip,
            size: Some(8),
            integrity: Some(Integrity::Sha256("a".repeat(64))),
            urls: vec![],
            signature_url: None,
        }
    }

    fn artifact_for(bytes: &[u8]) -> Artifact {
        Artifact {
            file_name: "browser.zip".into(),
            format: crate::domain::release::ArchiveFormat::Zip,
            size: Some(bytes.len() as u64),
            integrity: Some(Integrity::Sha256(hex::encode(Sha256::digest(bytes)))),
            urls: vec![],
            signature_url: None,
        }
    }

    #[test]
    fn retained_payload_is_locked_and_only_the_matching_artifact_can_resume_it() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact();
        let mut first = PartialDownload::open(dir.path(), &artifact).unwrap();
        first.file.write_all(b"prefix").unwrap();
        assert!(PartialDownload::open(dir.path(), &artifact).is_err());
        assert!(fs::write(&first.path, b"clobber").is_err());
        assert!(fs::remove_file(&first.path).is_err());
        let path = first.path.clone();
        first.close(false);
        let mut second = PartialDownload::open(dir.path(), &artifact).unwrap();
        assert_eq!(second.path, path);
        let mut bytes = Vec::new();
        second.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"prefix");
        let mut different = artifact.clone();
        different.integrity = Some(Integrity::Sha256("b".repeat(64)));
        let other = PartialDownload::open(dir.path(), &different).unwrap();
        assert_ne!(other.path, path);
        other.close(false);
        second.close(false);
    }

    #[test]
    fn an_unowned_partial_and_a_tampered_owner_record_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact();
        let foreign = dir.path().join("browser.zip.part");
        fs::write(&foreign, b"unrelated").unwrap();
        let first = PartialDownload::open(dir.path(), &artifact).unwrap();
        assert_ne!(first.path, foreign);
        let record = first.owner_path.clone();
        drop(first);
        fs::write(&record, b"{}").unwrap();
        assert!(PartialDownload::open(dir.path(), &artifact).is_err());
        assert_eq!(fs::read(&record).unwrap(), b"{}");
        assert_eq!(fs::read(&foreign).unwrap(), b"unrelated");
        super::super::clear_download_cache(dir.path()).unwrap();
        assert!(!record.exists());
        assert!(!foreign.exists());
    }

    #[test]
    fn a_missing_payload_after_explicit_cache_cleanup_restarts_safely() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact();
        let first = PartialDownload::open(dir.path(), &artifact).unwrap();
        let path = first.path.clone();
        drop(first);
        fs::remove_file(&path).unwrap();
        let second = PartialDownload::open(dir.path(), &artifact).unwrap();
        assert_eq!(second.len().unwrap(), 0);
        second.close(false);
    }

    #[test]
    fn digest_case_does_not_split_resume_identity() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact();
        let first = PartialDownload::open(dir.path(), &artifact).unwrap();
        let path = first.path.clone();
        drop(first);
        let mut uppercase = artifact.clone();
        uppercase.integrity = Some(Integrity::Sha256("A".repeat(64)));
        let second = PartialDownload::open(dir.path(), &uppercase).unwrap();
        assert_eq!(second.path, path);
        second.close(false);
    }

    #[test]
    fn publish_never_replaces_an_existing_destination_or_loses_the_payload() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"verified payload";
        let artifact = artifact_for(bytes);
        let mut partial = PartialDownload::open(dir.path(), &artifact).unwrap();
        partial.file.write_all(bytes).unwrap();
        let source = partial.path.clone();
        let destination = dir.path().join("existing.artifact");
        fs::write(&destination, b"occupied").unwrap();
        assert!(partial.publish(&destination, &artifact, None).is_err());
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert_eq!(fs::read(&destination).unwrap(), b"occupied");
    }

    #[test]
    fn interrupted_owner_initialization_does_not_block_the_next_open() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact();
        let key = resume_key(&artifact);
        let stale_owner = dir
            .path()
            .join(format!("bh-{key}-{}.owner.part", "1".repeat(32)));
        let orphan_payload = dir.path().join(format!("bh-{key}-{}.part", "2".repeat(32)));
        fs::write(&stale_owner, b"partial owner record").unwrap();
        fs::write(&orphan_payload, b"orphan payload").unwrap();

        let opened = PartialDownload::open(dir.path(), &artifact).unwrap();
        assert_ne!(opened.path, orphan_payload);
        assert_eq!(fs::read(&stale_owner).unwrap(), b"partial owner record");
        assert_eq!(fs::read(&orphan_payload).unwrap(), b"orphan payload");
        opened.close(false);
    }

    #[test]
    fn publish_does_not_report_success_before_final_identity_verification() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = artifact_for(b"verified payload");
        let mut partial = PartialDownload::open(dir.path(), &artifact).unwrap();
        partial.file.write_all(b"tampered payload").unwrap();
        let destination = dir.path().join("verified.artifact");
        let source = partial.path.clone();
        assert!(matches!(
            partial.publish(&destination, &artifact, None),
            Err(ServiceError::HashMismatch { .. })
        ));
        assert_eq!(fs::read(source).unwrap(), b"tampered payload");
        assert!(!destination.exists());
    }

    #[test]
    fn cancelled_final_verification_restores_the_payload() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"verified payload";
        let artifact = artifact_for(bytes);
        let mut partial = PartialDownload::open(dir.path(), &artifact).unwrap();
        partial.file.write_all(bytes).unwrap();
        let source = partial.path.clone();
        let destination = dir.path().join("verified.artifact");
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        assert!(matches!(
            partial.publish(&destination, &artifact, Some(&cancelled)),
            Err(ServiceError::Cancelled)
        ));
        assert_eq!(fs::read(source).unwrap(), bytes);
        assert!(!destination.exists());
    }
}
