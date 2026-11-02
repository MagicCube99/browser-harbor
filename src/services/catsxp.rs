//! Verified Catsxp package identity supplements, but never replaces, the PE version.

use super::version_probe::probe_local_version;
use crate::domain::release::{CatsxpBuildIdentity, ReleaseChannel};
use crate::domain::target::TargetId;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub program: PathBuf,
    pub build: CatsxpBuildIdentity,
    stamp: [(u64, SystemTime); 2],
}

pub fn base_version(version: &str, channel: ReleaseChannel) -> Option<String> {
    let parts = version.split('.').collect::<Vec<_>>();
    let count = match channel {
        ReleaseChannel::Stable => 4,
        ReleaseChannel::Beta => 5,
        _ => return None,
    };
    if parts.len() != count
        || parts
            .iter()
            .any(|part| part.is_empty() || part.parse::<u32>().is_err())
    {
        return None;
    }
    Some(parts[..4].join("."))
}

fn paths(program: &Path, raw: &str) -> [PathBuf; 2] {
    [
        program.join("catsxp.exe"),
        program.join(raw).join("chrome.dll"),
    ]
}

fn stamp(program: &Path, raw: &str) -> io::Result<[(u64, SystemTime); 2]> {
    if base_version(raw, ReleaseChannel::Stable).as_deref() != Some(raw) {
        return Err(io::Error::other("invalid Catsxp PE version"));
    }
    let mut result = [(0, SystemTime::UNIX_EPOCH); 2];
    for (index, path) in paths(program, raw).iter().enumerate() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::other(
                "Catsxp program file is not a regular file",
            ));
        }
        result[index] = (metadata.len(), metadata.modified()?);
    }
    Ok(result)
}

fn hash(path: &Path, cancellation: Option<&AtomicBool>) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancellation.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Catsxp identity check cancelled",
            ));
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn capture_files(
    program: &Path,
    raw: &str,
    version: &str,
    channel: ReleaseChannel,
) -> io::Result<CatsxpBuildIdentity> {
    if base_version(version, channel).as_deref() != Some(raw) {
        return Err(io::Error::other(
            "Catsxp package version does not match its PE version",
        ));
    }
    let files = paths(program, raw);
    Ok(CatsxpBuildIdentity {
        version: version.to_owned(),
        channel,
        executable_sha256: hash(&files[0], None)?,
        core_sha256: hash(&files[1], None)?,
    })
}

fn verify_files(
    program: &Path,
    raw: &str,
    build: &CatsxpBuildIdentity,
    cancellation: Option<&AtomicBool>,
) -> io::Result<bool> {
    if base_version(&build.version, build.channel).as_deref() != Some(raw) {
        return Ok(false);
    }
    let files = paths(program, raw);
    Ok(hash(&files[0], cancellation)? == build.executable_sha256
        && hash(&files[1], cancellation)? == build.core_sha256)
}

pub fn capture(
    program: &Path,
    version: &str,
    channel: ReleaseChannel,
) -> io::Result<Option<CatsxpBuildIdentity>> {
    // A foreign descriptor without a Catsxp version cannot establish identity.
    if base_version(version, channel).is_none() {
        return Ok(None);
    }
    let probe = probe_local_version(TargetId::Catsxp, program).map_err(io::Error::other)?;
    capture_files(program, &probe.version, version, channel).map(Some)
}

impl Observation {
    pub fn from_installed(program: &Path, build: CatsxpBuildIdentity) -> io::Result<Self> {
        let raw = base_version(&build.version, build.channel)
            .ok_or_else(|| io::Error::other("invalid Catsxp build identity"))?;
        Ok(Self {
            program: program.to_owned(),
            stamp: stamp(program, &raw)?,
            build,
        })
    }

    pub fn is_current(&self, program: &Path, raw: &str) -> bool {
        self.program == program
            && base_version(&self.build.version, self.build.channel).as_deref() == Some(raw)
            && stamp(program, raw).is_ok_and(|value| value == self.stamp)
    }
}

pub fn observe(
    program: &Path,
    build: Option<&CatsxpBuildIdentity>,
    cancellation: &AtomicBool,
) -> io::Result<Option<Observation>> {
    let Some(build) = build else { return Ok(None) };
    let Ok(probe) = probe_local_version(TargetId::Catsxp, program) else {
        return Ok(None);
    };
    let before = stamp(program, &probe.version)?;
    if !verify_files(program, &probe.version, build, Some(cancellation))? {
        return Ok(None);
    }
    let observation = Observation::from_installed(program, build.clone())?;
    Ok((observation.stamp == before).then_some(observation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::release::ReleaseChannel;

    #[test]
    fn catsxp_identity_rejects_changed_core_and_a_different_pe_version() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("156.6.11.1")).unwrap();
        std::fs::write(root.path().join("catsxp.exe"), b"exe").unwrap();
        std::fs::write(root.path().join("156.6.11.1/chrome.dll"), b"core").unwrap();
        let identity = capture_files(
            root.path(),
            "156.6.11.1",
            "156.6.11.1.3",
            ReleaseChannel::Beta,
        )
        .unwrap();
        assert!(verify_files(root.path(), "156.6.11.1", &identity, None).unwrap());
        assert!(!verify_files(root.path(), "156.6.11.2", &identity, None).unwrap());
        std::fs::write(root.path().join("156.6.11.1/chrome.dll"), b"changed core").unwrap();
        assert!(!verify_files(root.path(), "156.6.11.1", &identity, None).unwrap());
    }

    #[test]
    fn catsxp_identity_rejects_changed_executable_and_cancellation() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("156.6.10.5")).unwrap();
        std::fs::write(root.path().join("catsxp.exe"), b"exe").unwrap();
        std::fs::write(root.path().join("156.6.10.5/chrome.dll"), b"core").unwrap();
        let identity = capture_files(
            root.path(),
            "156.6.10.5",
            "156.6.10.5",
            ReleaseChannel::Stable,
        )
        .unwrap();
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        assert!(verify_files(root.path(), "156.6.10.5", &identity, Some(&cancelled)).is_err());
        std::fs::write(root.path().join("catsxp.exe"), b"different exe").unwrap();
        assert!(!verify_files(root.path(), "156.6.10.5", &identity, None).unwrap());
        assert!(base_version("156.6.11.1.3", ReleaseChannel::Stable).is_none());
        assert!(base_version("156.6.11.1", ReleaseChannel::Beta).is_none());
        assert!(base_version("../escape", ReleaseChannel::Beta).is_none());
    }
}
