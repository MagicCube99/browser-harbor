//! Ed25519-verified release metadata used when official APIs are unavailable.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::{
    release::{CpuBuild, ReleaseChannel, ReleaseDescriptor},
    runtime::{ChromePlusRelease, LibportableRelease},
    target::TargetId,
};

pub const METADATA_FILE: &str = "trusted-metadata.json";
pub const SIGNATURE_FILE: &str = "trusted-metadata.sig";
pub const NO_EXPIRY: u64 = 253_402_300_799;
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrustedMetadata {
    pub schema_version: u32,
    pub generated_at: u64,
    pub expires_at: u64,
    pub releases: Vec<ReleaseDescriptor>,
    #[serde(default)]
    pub chrome_plus: Option<ChromePlusRelease>,
    #[serde(default)]
    pub libportable: Option<LibportableRelease>,
}

impl TrustedMetadata {
    pub fn load_embedded() -> Result<Self, TrustedMetadataError> {
        verify_with_public_key(
            include_bytes!("../../assets/trusted-metadata.json"),
            include_str!("../../assets/trusted-metadata.sig"),
            include_str!("../../assets/metadata-public-key.hex"),
        )
    }

    pub fn load_cached(cache_dir: &Path) -> Result<Option<Self>, TrustedMetadataError> {
        let metadata_path = cache_dir.join(METADATA_FILE);
        let signature_path = cache_dir.join(SIGNATURE_FILE);
        if !metadata_path.is_file() || !signature_path.is_file() {
            return Ok(None);
        }
        let metadata = fs::read(&metadata_path).map_err(|source| TrustedMetadataError::Io {
            path: metadata_path,
            source,
        })?;
        let signature =
            fs::read_to_string(&signature_path).map_err(|source| TrustedMetadataError::Io {
                path: signature_path,
                source,
            })?;
        verify_with_public_key(&metadata, &signature, embedded_public_key()).map(Some)
    }

    pub fn store(
        &self,
        metadata_bytes: &[u8],
        signature: &str,
        cache_dir: &Path,
    ) -> Result<(), TrustedMetadataError> {
        fs::create_dir_all(cache_dir).map_err(|source| TrustedMetadataError::Io {
            path: cache_dir.to_path_buf(),
            source,
        })?;
        write_atomic(&cache_dir.join(METADATA_FILE), metadata_bytes)?;
        write_atomic(&cache_dir.join(SIGNATURE_FILE), signature.trim().as_bytes())
    }

    pub fn release(
        &self,
        target: TargetId,
        channel: ReleaseChannel,
        cpu_build: CpuBuild,
    ) -> Option<&ReleaseDescriptor> {
        self.releases.iter().find(|release| {
            release.target == target && release.channel == channel && release.cpu_build == cpu_build
        })
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at < unix_now()
    }
}

pub fn verify_with_public_key(
    metadata: &[u8],
    signature: &str,
    public_key: &str,
) -> Result<TrustedMetadata, TrustedMetadataError> {
    let key = decode_public_key(public_key)?;
    let signature = decode_signature(signature)?;
    key.verify_strict(metadata, &signature)
        .map_err(|_| TrustedMetadataError::InvalidSignature)?;
    let metadata: TrustedMetadata =
        serde_json::from_slice(metadata).map_err(TrustedMetadataError::Json)?;
    metadata.validate()?;
    Ok(metadata)
}

impl TrustedMetadata {
    fn validate(&self) -> Result<(), TrustedMetadataError> {
        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(TrustedMetadataError::UnsupportedSchema(self.schema_version));
        }
        if self.generated_at > self.expires_at {
            return Err(TrustedMetadataError::InvalidValidity);
        }
        let mut keys = HashSet::new();
        for release in &self.releases {
            release
                .validate()
                .map_err(TrustedMetadataError::InvalidDescriptor)?;
            require_integrity(release)?;
            let key = (release.target, release.channel, release.cpu_build);
            if !keys.insert(key) {
                return Err(TrustedMetadataError::DuplicateRelease {
                    target: release.target,
                    channel: release.channel,
                    cpu_build: release.cpu_build,
                });
            }
        }
        if let Some(chrome_plus) = &self.chrome_plus {
            require_artifact_integrity(&chrome_plus.artifact, "Chrome++")?;
            require_artifact_integrity(&chrome_plus.setdll_artifact, "Chrome++ setdll")?;
        }
        if let Some(libportable) = &self.libportable {
            require_artifact_integrity(&libportable.artifact, "Libportable")?;
        }
        Ok(())
    }
}

fn require_integrity(release: &ReleaseDescriptor) -> Result<(), TrustedMetadataError> {
    require_artifact_integrity(
        &release.artifact,
        &format!("{} release", release.target.display_name()),
    )
}

fn require_artifact_integrity(
    artifact: &crate::domain::release::Artifact,
    name: &str,
) -> Result<(), TrustedMetadataError> {
    if artifact.integrity.is_some() {
        Ok(())
    } else {
        Err(TrustedMetadataError::MissingIntegrity(name.to_owned()))
    }
}

fn decode_public_key(public_key: &str) -> Result<VerifyingKey, TrustedMetadataError> {
    let bytes = hex::decode(public_key.trim()).map_err(|_| TrustedMetadataError::InvalidKey)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| TrustedMetadataError::InvalidKey)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| TrustedMetadataError::InvalidKey)
}

fn decode_signature(signature: &str) -> Result<Signature, TrustedMetadataError> {
    let bytes =
        hex::decode(signature.trim()).map_err(|_| TrustedMetadataError::InvalidSignature)?;
    Signature::from_slice(&bytes).map_err(|_| TrustedMetadataError::InvalidSignature)
}

fn embedded_public_key() -> &'static str {
    include_str!("../../assets/metadata-public-key.hex")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), TrustedMetadataError> {
    let temp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("tmp")
    ));
    fs::write(&temp, bytes).map_err(|source| TrustedMetadataError::Io {
        path: temp.clone(),
        source,
    })?;
    fs::rename(&temp, path).map_err(|source| {
        let _ = fs::remove_file(&temp);
        TrustedMetadataError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[derive(Debug, Error)]
pub enum TrustedMetadataError {
    #[error("trusted metadata public key is invalid")]
    InvalidKey,
    #[error("trusted metadata signature is invalid")]
    InvalidSignature,
    #[error("trusted metadata JSON is invalid")]
    Json(#[source] serde_json::Error),
    #[error("trusted metadata schema {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("trusted metadata validity window is invalid")]
    InvalidValidity,
    #[error("trusted release descriptor is invalid: {0}")]
    InvalidDescriptor(String),
    #[error("trusted metadata entry {0} has no integrity value")]
    MissingIntegrity(String),
    #[error("trusted metadata contains a duplicate release: {target:?}/{channel:?}/{cpu_build:?}")]
    DuplicateRelease {
        target: TargetId,
        channel: ReleaseChannel,
        cpu_build: CpuBuild,
    },
    #[error("trusted metadata file operation failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use rand_core::OsRng;

    use super::{TrustedMetadata, verify_with_public_key};
    use crate::domain::{
        release::{
            ArchiveFormat, Artifact, CpuBuild, Integrity, ProgramPlacement, ProviderKind,
            ReleaseChannel, ReleaseDescriptor,
        },
        target::TargetId,
    };

    fn metadata() -> TrustedMetadata {
        TrustedMetadata {
            schema_version: 1,
            generated_at: 1,
            expires_at: 2,
            releases: vec![ReleaseDescriptor {
                target: TargetId::Brave,
                version: "1.0.0".to_owned(),
                channel: ReleaseChannel::Stable,
                cpu_build: CpuBuild::X64,
                provider: ProviderKind::GitHubRelease,
                artifact: Artifact {
                    file_name: "brave.zip".to_owned(),
                    format: ArchiveFormat::Zip,
                    size: Some(1),
                    integrity: Some(Integrity::Sha256("a".repeat(64))),
                    urls: vec!["https://github.com/example/brave.zip".to_owned()],
                    signature_url: None,
                },
                placement: ProgramPlacement::NativePackage,
                inner_archive: None,
                source: "test".to_owned(),
                published_at: None,
            }],
            chrome_plus: None,
            libportable: None,
        }
    }

    #[test]
    fn signed_metadata_round_trips_and_finds_a_release() {
        let key = SigningKey::generate(&mut OsRng);
        let bytes = serde_json::to_vec(&metadata()).unwrap();
        let signature = hex::encode(key.sign(&bytes).to_bytes());
        let public_key = hex::encode(key.verifying_key().to_bytes());

        let parsed = verify_with_public_key(&bytes, &signature, &public_key).unwrap();
        assert!(
            parsed
                .release(TargetId::Brave, ReleaseChannel::Stable, CpuBuild::X64)
                .is_some()
        );
    }

    #[test]
    fn changed_metadata_fails_signature_verification() {
        let key = SigningKey::generate(&mut OsRng);
        let bytes = serde_json::to_vec(&metadata()).unwrap();
        let signature = hex::encode(key.sign(&bytes).to_bytes());
        let public_key = hex::encode(key.verifying_key().to_bytes());
        let changed = [bytes, b" ".to_vec()].concat();

        assert!(verify_with_public_key(&changed, &signature, &public_key).is_err());
    }

    #[test]
    fn signed_metadata_rejects_missing_integrity() {
        let key = SigningKey::generate(&mut OsRng);
        let mut value = metadata();
        value.releases[0].artifact.integrity = None;
        let bytes = serde_json::to_vec(&value).unwrap();
        let signature = hex::encode(key.sign(&bytes).to_bytes());
        let public_key = hex::encode(key.verifying_key().to_bytes());

        assert!(verify_with_public_key(&bytes, &signature, &public_key).is_err());
    }

    #[test]
    fn embedded_metadata_is_signed_and_covers_every_installable_target() {
        let metadata = TrustedMetadata::load_embedded().unwrap();

        assert_eq!(metadata.releases.len(), 19);
        assert!(metadata.chrome_plus.is_some());
        assert!(metadata.libportable.is_some());
        assert!(metadata.expires_at > metadata.generated_at);
    }
}
