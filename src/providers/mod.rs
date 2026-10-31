//! Official release metadata adapters.

mod github;
mod official;
mod website;

use thiserror::Error;

use crate::{
    domain::{
        release::{CpuBuild, ProviderKind, ReleaseChannel, ReleaseDescriptor},
        target::TargetId,
    },
    services::{ServiceError, http::HttpClient},
};

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error("release selection is not supported for the target")]
    UnsupportedSelection,
    #[error("provider response could not be parsed: {0}")]
    Parse(String),
    #[error("provider did not publish a compatible x64 artifact")]
    NoCompatibleAsset,
    #[error("release descriptor is invalid: {0}")]
    InvalidDescriptor(String),
}

pub struct ProviderResolver {
    http: HttpClient,
}

impl ProviderResolver {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }

    pub async fn resolve(
        &self,
        target: TargetId,
        channel: ReleaseChannel,
        cpu_build: CpuBuild,
    ) -> Result<ReleaseDescriptor, ProviderError> {
        let spec = target.spec();
        if !spec.channels.contains(&channel) || !spec.cpu_builds.contains(&cpu_build) {
            return Err(ProviderError::UnsupportedSelection);
        }
        let descriptor = match spec.provider {
            ProviderKind::ChromeOmaha => {
                official::resolve_chrome(&self.http, channel, cpu_build).await?
            }
            ProviderKind::EdgeEnterprise => {
                official::resolve_edge(&self.http, channel, cpu_build).await?
            }
            ProviderKind::Mozilla => {
                official::resolve_firefox(&self.http, channel, cpu_build).await?
            }
            ProviderKind::GitHubRelease => {
                github::resolve(&self.http, target, channel, cpu_build).await?
            }
            ProviderKind::LibreWolf => {
                website::resolve_librewolf(&self.http, channel, cpu_build).await?
            }
            ProviderKind::Catsxp => website::resolve_catsxp(&self.http, channel, cpu_build).await?,
            ProviderKind::SkyKakapo => {
                website::resolve_skykakapo(&self.http, channel, cpu_build).await?
            }
            ProviderKind::Turbo => website::resolve_turbo(&self.http, channel, cpu_build).await?,
            ProviderKind::Vivaldi => {
                website::resolve_vivaldi(&self.http, channel, cpu_build).await?
            }
        };
        descriptor
            .validate()
            .map_err(ProviderError::InvalidDescriptor)?;
        Ok(descriptor)
    }
}

pub fn verify_artifact_signature(
    target: TargetId,
    content: &[u8],
    signature: &[u8],
) -> Result<bool, ProviderError> {
    match target {
        TargetId::LibreWolf => website::verify_librewolf_artifact_signature(content, signature),
        _ => Ok(true),
    }
}
