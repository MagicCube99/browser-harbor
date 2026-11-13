//! Content-bound channel identity for Chromium packages with generic ProductName resources.

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};

use super::version_probe::probe_local_version;
use crate::domain::{
    release::{InstalledChannelIdentity, ReleaseChannel},
    target::TargetId,
};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub identity: InstalledChannelIdentity,
    program: PathBuf,
    files: [PathBuf; 2],
    stamp: [(u64, SystemTime); 2],
}

fn core_name(target: TargetId) -> Option<&'static str> {
    match target {
        TargetId::Chrome | TargetId::Brave => Some("chrome.dll"),
        TargetId::Edge => Some("msedge.dll"),
        _ => None,
    }
}

fn files(target: TargetId, program: &Path, version: &str) -> io::Result<Option<[PathBuf; 2]>> {
    let Some(core) = core_name(target) else {
        return Ok(None);
    };
    let probe = match probe_local_version(target, program) {
        Ok(probe) if probe.version == version => probe,
        _ => return Ok(None),
    };
    let Some(parent) = probe.executable.parent() else {
        return Ok(None);
    };
    let mut candidates = vec![parent.join(version).join(core), parent.join(core)];
    // Brave's product version can differ from its Chromium version directory.
    if let Ok(file_version) =
        crate::infra::windows::version_info::executable_file_version(&probe.executable)
    {
        candidates.push(parent.join(file_version).join(core));
    }
    let core = candidates.into_iter().find(|path| path.is_file());
    Ok(core.map(|core| [probe.executable, core]))
}

fn stamp(files: &[PathBuf; 2]) -> io::Result<[(u64, SystemTime); 2]> {
    let mut result = [(0, SystemTime::UNIX_EPOCH); 2];
    for (index, path) in files.iter().enumerate() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::other(
                "channel identity requires regular program files",
            ));
        }
        result[index] = (metadata.len(), metadata.modified()?);
    }
    Ok(result)
}

fn hash(path: &Path, cancellation: &AtomicBool) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if cancellation.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "channel identity check cancelled",
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

pub fn capture(
    target: TargetId,
    program: &Path,
    version: &str,
    channel: ReleaseChannel,
) -> io::Result<Option<InstalledChannelIdentity>> {
    let Some(files) = files(target, program, version)? else {
        return Ok(None);
    };
    let before = stamp(&files)?;
    let cancellation = AtomicBool::new(false);
    let identity = InstalledChannelIdentity {
        target,
        version: version.to_owned(),
        channel,
        executable_sha256: hash(&files[0], &cancellation)?,
        core_sha256: hash(&files[1], &cancellation)?,
    };
    if before != stamp(&files)? {
        return Err(io::Error::other(
            "program files changed while recording their channel",
        ));
    }
    Ok(Some(identity))
}

pub fn observe(
    target: TargetId,
    program: &Path,
    identity: Option<&InstalledChannelIdentity>,
    cancellation: &AtomicBool,
) -> io::Result<Option<Observation>> {
    let Some(identity) = identity.filter(|value| value.target == target) else {
        return Ok(None);
    };
    let Some(files) = files(target, program, &identity.version)? else {
        return Ok(None);
    };
    let before = stamp(&files)?;
    if hash(&files[0], cancellation)? != identity.executable_sha256
        || hash(&files[1], cancellation)? != identity.core_sha256
        || before != stamp(&files)?
    {
        return Ok(None);
    }
    Ok(Some(Observation {
        identity: identity.clone(),
        program: program.to_owned(),
        files,
        stamp: before,
    }))
}

impl Observation {
    // Only use directly with the receipt returned by a successful verified install.
    pub fn from_installed(
        program: &Path,
        identity: InstalledChannelIdentity,
    ) -> io::Result<Option<Self>> {
        let Some(files) = files(identity.target, program, &identity.version)? else {
            return Ok(None);
        };
        let stamp = stamp(&files)?;
        Ok(Some(Self {
            identity,
            program: program.to_owned(),
            files,
            stamp,
        }))
    }

    pub fn is_current(&self, target: TargetId, program: &Path, version: &str) -> bool {
        self.identity.target == target
            && self.identity.version == version
            && self.program == program
            && stamp(&self.files).is_ok_and(|stamp| stamp == self.stamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn beta_identity_requires_matching_executable_core_and_version() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path();
        std::fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        let version =
            crate::services::version_probe::probe_local_version(TargetId::Chrome, program)
                .unwrap()
                .version;
        std::fs::create_dir(program.join(&version)).unwrap();
        let core = program.join(&version).join("chrome.dll");
        std::fs::write(&core, b"beta core").unwrap();
        let identity = capture(TargetId::Chrome, program, &version, ReleaseChannel::Beta)
            .unwrap()
            .unwrap();
        let cancellation = AtomicBool::new(false);
        let observation = observe(TargetId::Chrome, program, Some(&identity), &cancellation)
            .unwrap()
            .unwrap();
        assert_eq!(observation.identity.channel, ReleaseChannel::Beta);
        assert!(observation.is_current(TargetId::Chrome, program, &version));
        assert!(!observation.is_current(TargetId::Chrome, program, "different version"));
        assert!(
            observe(TargetId::Edge, program, Some(&identity), &cancellation)
                .unwrap()
                .is_none()
        );

        std::fs::write(&core, b"stable core replacing beta").unwrap();
        assert!(!observation.is_current(TargetId::Chrome, program, &version));
        assert!(
            observe(TargetId::Chrome, program, Some(&identity), &cancellation)
                .unwrap()
                .is_none()
        );
        std::fs::write(&core, b"beta core").unwrap();
        std::fs::write(
            program.join("chrome.exe"),
            b"externally replaced executable",
        )
        .unwrap();
        assert!(
            observe(TargetId::Chrome, program, Some(&identity), &cancellation)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn edge_and_brave_channel_records_follow_the_actual_program_layout() {
        for (target, executable, core, versioned) in [
            (TargetId::Edge, "msedge.exe", "msedge.dll", true),
            (TargetId::Brave, "brave.exe", "chrome.dll", false),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let version = crate::infra::windows::version_info::executable_product_version(
                &std::env::current_exe().unwrap(),
            )
            .unwrap();
            let program = if versioned {
                directory.path().join(&version)
            } else {
                directory.path().to_path_buf()
            };
            std::fs::create_dir_all(&program).unwrap();
            std::fs::copy(std::env::current_exe().unwrap(), program.join(executable)).unwrap();
            let observed_version =
                crate::services::version_probe::probe_local_version(target, &program)
                    .unwrap()
                    .version;
            std::fs::write(program.join(core), b"beta core").unwrap();
            let identity = capture(
                target,
                directory.path(),
                &observed_version,
                ReleaseChannel::Beta,
            )
            .unwrap()
            .unwrap();
            let cancellation = AtomicBool::new(false);
            let observation = observe(target, directory.path(), Some(&identity), &cancellation)
                .unwrap()
                .unwrap();
            assert_eq!(observation.identity.channel, ReleaseChannel::Beta);
            std::fs::write(program.join(core), b"external replacement").unwrap();
            assert!(
                observe(target, directory.path(), Some(&identity), &cancellation)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn identity_without_a_record_or_with_cancelled_check_is_not_assumed() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path();
        let cancellation = AtomicBool::new(false);
        assert!(
            observe(TargetId::Chrome, program, None, &cancellation)
                .unwrap()
                .is_none()
        );
        std::fs::copy(std::env::current_exe().unwrap(), program.join("chrome.exe")).unwrap();
        let version =
            crate::services::version_probe::probe_local_version(TargetId::Chrome, program)
                .unwrap()
                .version;
        std::fs::create_dir(program.join(&version)).unwrap();
        std::fs::write(program.join(&version).join("chrome.dll"), b"core").unwrap();
        let identity = capture(TargetId::Chrome, program, &version, ReleaseChannel::Beta)
            .unwrap()
            .unwrap();
        assert_eq!(
            observe(
                TargetId::Chrome,
                program,
                Some(&identity),
                &AtomicBool::new(true)
            )
            .unwrap_err()
            .kind(),
            std::io::ErrorKind::Interrupted
        );
        let mut stale = identity;
        stale.version = "159.0.8154.6".to_owned();
        assert!(
            observe(TargetId::Chrome, program, Some(&stale), &cancellation)
                .unwrap()
                .is_none()
        );
    }
}
