//! Microsoft Edge native launcher deployment and configuration.

use std::{fs, path::Path};

use super::{RuntimeError, io_error};

pub const EDGE_LAUNCHER_EXE: &[u8] = include_bytes!("../../assets/edge_launcher.exe");

pub fn deploy_launcher(target_path: &Path) -> Result<(), RuntimeError> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    }
    fs::write(target_path, EDGE_LAUNCHER_EXE).map_err(|source| io_error(target_path, source))?;
    Ok(())
}

pub fn clean_legacy_injections(version_dir: &Path) -> Result<(), RuntimeError> {
    let legacy_version_dll = version_dir.join("version.dll");
    if legacy_version_dll.is_file() {
        let _ = fs::remove_file(&legacy_version_dll);
    }
    let legacy_disabled_dll = version_dir.join("version.dll.disabled");
    if legacy_disabled_dll.is_file() {
        let _ = fs::remove_file(&legacy_disabled_dll);
    }
    let legacy_ini = version_dir.join("chrome++.ini");
    if legacy_ini.is_file() {
        let _ = fs::remove_file(&legacy_ini);
    }
    let legacy_msedgepp_dll = version_dir.join("msedge++.dll");
    if legacy_msedgepp_dll.is_file() {
        let _ = fs::remove_file(&legacy_msedgepp_dll);
    }
    let legacy_msedgepp_ini = version_dir.join("msedge++.ini");
    if legacy_msedgepp_ini.is_file() {
        let _ = fs::remove_file(&legacy_msedgepp_ini);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EDGE_LAUNCHER_EXE, deploy_launcher};
    use tempfile::tempdir;

    #[test]
    fn deploys_valid_edge_launcher() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("Edge.exe");
        deploy_launcher(&target).unwrap();
        assert!(target.is_file());
        assert_eq!(std::fs::read(&target).unwrap(), EDGE_LAUNCHER_EXE);
    }
}
