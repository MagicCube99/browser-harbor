//! Immutable browser release identity.

use serde::{Deserialize, Serialize};

use super::target::TargetId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    ChromeOmaha,
    EdgeEnterprise,
    Mozilla,
    GitHubRelease,
    LibreWolf,
    Catsxp,
    SkyKakapo,
    Turbo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    Stable,
    Esr,
    Release,
}

impl ReleaseChannel {
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Esr => "ESR",
            Self::Release => "Release",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum CpuBuild {
    #[default]
    X64,
    Sse3,
    Sse4,
    Avx,
    Avx2,
    Avx512,
}

impl CpuBuild {
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::Sse3 => "SSE3",
            Self::Sse4 => "SSE4",
            Self::Avx => "AVX",
            Self::Avx2 => "AVX2",
            Self::Avx512 => "AVX512",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
    Msi,
}

impl ArchiveFormat {
    pub fn from_file_name(file_name: &str) -> Option<Self> {
        let lower = file_name.to_ascii_lowercase();
        if lower.ends_with(".zip") {
            Some(Self::Zip)
        } else if lower.ends_with(".7z") || lower.ends_with(".exe") {
            Some(Self::SevenZip)
        } else if lower.ends_with(".msi") {
            Some(Self::Msi)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "algorithm", content = "value", rename_all = "lowercase")]
pub enum Integrity {
    Sha256(String),
    Sha512(String),
}

impl Integrity {
    pub fn from_digest(digest: &str) -> Option<Self> {
        let (algorithm, value) = digest.split_once(':')?;
        match algorithm.to_ascii_lowercase().as_str() {
            "sha256" if value.len() == 64 => Some(Self::Sha256(value.to_ascii_lowercase())),
            "sha512" if value.len() == 128 => Some(Self::Sha512(value.to_ascii_lowercase())),
            _ => None,
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Self::Sha256(value) | Self::Sha512(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub file_name: String,
    pub format: ArchiveFormat,
    pub size: Option<u64>,
    pub integrity: Option<Integrity>,
    pub urls: Vec<String>,
    pub signature_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramPlacement {
    App,
    EdgeVersioned,
    NativePackage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDescriptor {
    pub target: TargetId,
    pub version: String,
    pub channel: ReleaseChannel,
    pub cpu_build: CpuBuild,
    pub provider: ProviderKind,
    pub artifact: Artifact,
    pub placement: ProgramPlacement,
    #[serde(default)]
    pub inner_archive: Option<String>,
    pub source: String,
    pub published_at: Option<String>,
}

impl ReleaseDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.version.trim().is_empty() {
            return Err("release version is empty".to_owned());
        }
        if self.artifact.file_name.is_empty()
            || self.artifact.file_name.contains(['/', '\\'])
            || self.artifact.file_name == "."
            || self.artifact.file_name == ".."
        {
            return Err("artifact file name is unsafe".to_owned());
        }
        if self.artifact.urls.is_empty() {
            return Err("artifact has no source URL".to_owned());
        }
        if self.artifact.urls.iter().any(|url| {
            let lower = url.to_ascii_lowercase();
            !(lower.starts_with("https://") || lower.starts_with("http://"))
        }) {
            return Err("artifact URL is not HTTP(S)".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ArchiveFormat, Artifact, Integrity, ProgramPlacement, ReleaseDescriptor};
    use crate::domain::{
        release::{CpuBuild, ProviderKind, ReleaseChannel},
        target::TargetId,
    };

    fn descriptor() -> ReleaseDescriptor {
        ReleaseDescriptor {
            target: TargetId::Brave,
            version: "1.0.0".to_owned(),
            channel: ReleaseChannel::Stable,
            cpu_build: CpuBuild::X64,
            provider: ProviderKind::GitHubRelease,
            artifact: Artifact {
                file_name: "brave.zip".to_owned(),
                format: ArchiveFormat::Zip,
                size: Some(10),
                integrity: Some(Integrity::Sha256("a".repeat(64))),
                urls: vec!["https://example.com/brave.zip".to_owned()],
                signature_url: None,
            },
            placement: ProgramPlacement::App,
            inner_archive: None,
            source: "fixture".to_owned(),
            published_at: None,
        }
    }

    #[test]
    fn release_descriptor_rejects_path_traversal_file_names() {
        let mut release = descriptor();
        release.artifact.file_name = "../browser.zip".to_owned();

        assert!(release.validate().is_err());
    }

    #[test]
    fn release_descriptor_accepts_a_valid_https_artifact() {
        assert!(descriptor().validate().is_ok());
    }
}
