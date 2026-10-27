use std::{
    fs,
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

use super::{RuntimeError, download_artifact, io_error};
use crate::{
    domain::{runtime::LibportableRelease, target::TargetId},
    infra::ini::{IniUpdate, update_ini},
    services::http::{HttpClient, github_api_candidates},
};

const LIBPORTABLE_LATEST: &str = "https://api.github.com/repos/adonais/libportable/releases/latest";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

pub async fn resolve_latest(http: &HttpClient) -> Result<LibportableRelease, RuntimeError> {
    let release = http
        .get_json_candidates::<GitHubRelease>(&github_api_candidates(LIBPORTABLE_LATEST))
        .await?;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == "portable_bin.7z")
        .ok_or(RuntimeError::MissingComponent)?;
    Ok(LibportableRelease {
        version: release.tag_name,
        artifact: download_artifact(
            asset.name,
            asset.browser_download_url,
            Some(asset.size),
            asset.digest,
        ),
    })
}

pub struct LibportablePrepareRequest<'a> {
    pub runtime_root: &'a Path,
    pub staging_program: &'a Path,
    pub final_program: &'a Path,
    pub product: TargetId,
    pub product_version: &'a str,
    pub data_dir: &'a Path,
    pub cache_dir: Option<&'a Path>,
    pub backup_dir: &'a Path,
}

pub fn prepare(request: LibportablePrepareRequest<'_>) -> Result<(), RuntimeError> {
    if !is_compatible(request.product, request.product_version) {
        return Err(RuntimeError::UnsupportedLibportableVersion {
            product: request.product.display_name().to_owned(),
            version: request.product_version.to_owned(),
        });
    }
    let portable_dll = request.runtime_root.join("portable64.dll");
    let upcheck = request.runtime_root.join("upcheck64.exe");
    let mozglue = request.staging_program.join("mozglue.dll");
    if !portable_dll.is_file() || !upcheck.is_file() || !mozglue.is_file() {
        return Err(RuntimeError::MissingComponent);
    }

    fs::create_dir_all(request.backup_dir)
        .map_err(|source| io_error(request.backup_dir, source))?;
    let backup = request.backup_dir.join("mozglue.dll.original");
    if !backup.exists() {
        fs::copy(&mozglue, &backup).map_err(|source| io_error(&backup, source))?;
    }
    let original = fs::read(&mozglue).map_err(|source| io_error(&mozglue, source))?;
    let target_dll = request.staging_program.join("portable64.dll");
    let target_helper = request.staging_program.join("upcheck64.exe");
    fs::copy(&portable_dll, &target_dll).map_err(|source| io_error(&target_dll, source))?;
    fs::copy(&upcheck, &target_helper).map_err(|source| io_error(&target_helper, source))?;

    let helper_result = run_helper(&target_helper, request.staging_program);
    if let Err(error) = helper_result {
        let _ = fs::write(&mozglue, original);
        return Err(error);
    }

    let final_helper = request.staging_program.join("upcheck.exe");
    fs::rename(&target_helper, &final_helper).map_err(|source| io_error(&final_helper, source))?;
    write_portable_ini(
        request.staging_program,
        request.final_program,
        request.data_dir,
        request.cache_dir,
    )?;
    if !mozglue.is_file()
        || !target_dll.is_file()
        || !final_helper.is_file()
        || !request.staging_program.join("portable.ini").is_file()
    {
        return Err(RuntimeError::MissingComponent);
    }
    Ok(())
}

pub fn is_compatible(product: TargetId, version: &str) -> bool {
    let mut parts = version
        .trim_start_matches('v')
        .split(['.', '-'])
        .filter_map(|part| {
            part.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<u64>()
                .ok()
        });
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    match product {
        TargetId::Firefox | TargetId::Floorp | TargetId::LibreWolf => {
            (115..=158).contains(&major)
        }
        TargetId::Zen => major == 1 && minor == 23,
        _ => false,
    }
}

fn run_helper(helper: &Path, working_directory: &Path) -> Result<(), RuntimeError> {
    let mut child = Command::new(helper)
        .arg("-dll")
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|source| io_error(helper, source))?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|source| io_error(helper, source))?
        {
            return if status.success() {
                Ok(())
            } else {
                Err(RuntimeError::HelperFailed(status.code().unwrap_or(-1)))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RuntimeError::HelperTimeout);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub(crate) fn write_portable_ini(
    program_dir: &Path,
    final_program_dir: &Path,
    data_dir: &Path,
    cache_dir: Option<&Path>,
) -> Result<(), RuntimeError> {
    let ini_path = program_dir.join("portable.ini");
    let template = if ini_path.is_file() {
        fs::read_to_string(&ini_path).map_err(|source| io_error(&ini_path, source))?
    } else {
        "[General]\r\nPortable=1\r\nUpdate=0\r\n\r\n[Env]\r\nTmpDataPath=\r\n".to_owned()
    };
    let data_path = super::portable_relative_path(final_program_dir, data_dir)
        .unwrap_or_else(|| data_dir.display().to_string());
    let cache_path = cache_dir.map(|path| {
        super::portable_relative_path(final_program_dir, path)
            .unwrap_or_else(|| path.display().to_string())
    });
    let updated = update_ini(
        &template,
        &[
            IniUpdate {
                section: "general",
                key: "Portable",
                value: "1",
            },
            IniUpdate {
                section: "general",
                key: "PortableDataPath",
                value: &data_path,
            },
            IniUpdate {
                section: "general",
                key: "Update",
                value: "0",
            },
            IniUpdate {
                section: "env",
                key: "TmpDataPath",
                value: cache_path.as_deref().unwrap_or_default(),
            },
        ],
    );
    fs::write(&ini_path, updated).map_err(|source| io_error(&ini_path, source))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{is_compatible, write_portable_ini};
    use crate::domain::target::TargetId;

    #[test]
    fn libportable_version_gate_is_target_specific() {
        assert!(is_compatible(TargetId::Firefox, "158.0.1"));
        assert!(is_compatible(TargetId::Floorp, "158.0.1"));
        assert!(is_compatible(TargetId::LibreWolf, "158.0.1"));
        assert!(is_compatible(TargetId::Zen, "1.23.4b"));
        assert!(!is_compatible(TargetId::Floorp, "159.0"));
        assert!(!is_compatible(TargetId::LibreWolf, "159.0"));
        assert!(!is_compatible(TargetId::Zen, "159.0"));
        assert!(!is_compatible(TargetId::Chrome, "158.0.1"));
    }

    #[test]
    fn portable_ini_blank_cache_follows_portable_data_path() {
        let directory = tempdir().unwrap();
        let program = directory.path().join("App");
        fs::create_dir_all(&program).unwrap();
        write_portable_ini(&program, &program, &directory.path().join("Profiles"), None).unwrap();

        let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\Profiles"));
        assert!(ini.lines().any(|line| line.trim() == "TmpDataPath="));
        assert!(!ini.contains(r"\Cache"));
    }

    #[test]
    fn portable_ini_custom_cache_uses_explicit_temporary_data_root() {
        let directory = tempdir().unwrap();
        let program = directory.path().join("App");
        fs::create_dir_all(&program).unwrap();
        write_portable_ini(
            &program,
            &program,
            &directory.path().join("Profiles"),
            Some(&directory.path().join("Cache")),
        )
        .unwrap();

        let ini = fs::read_to_string(program.join("portable.ini")).unwrap();
        assert!(ini.contains(r"TmpDataPath=..\Cache"));
    }

    #[test]
    fn portable_ini_paths_are_relative_to_the_final_program() {
        let directory = tempdir().unwrap();
        write_portable_ini(
            directory.path(),
            Path::new(r"D:\Browsers\Zen\App\core"),
            Path::new(r"D:\Browsers\Zen\Profiles"),
            None,
        )
        .unwrap();

        let ini = fs::read_to_string(directory.path().join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\..\Profiles"));
        assert!(ini.lines().any(|line| line.trim() == "TmpDataPath="));
    }

    #[test]
    fn portable_ini_uses_relative_path_across_browser_branches_on_same_volume() {
        let directory = tempdir().unwrap();
        write_portable_ini(
            directory.path(),
            Path::new(r"D:\Browsers\Firefox\App"),
            Path::new(r"D:\Shared\Firefox Profiles"),
            None,
        )
        .unwrap();

        let ini = fs::read_to_string(directory.path().join("portable.ini")).unwrap();
        assert!(ini.contains(r"PortableDataPath=..\..\..\Shared\Firefox Profiles"));
    }
}
