//! Browser Root shortcut creation through the Windows Shell.

use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use thiserror::Error;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Error)]
pub enum ShortcutError {
    #[error("browser root does not exist: {0}")]
    BrowserRoot(PathBuf),
    #[error("browser executable does not exist: {0}")]
    Executable(PathBuf),
    #[error("could not start Windows PowerShell: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("Windows Shell could not create the shortcut: {0}")]
    PowerShell(String),
    #[error("Windows Shell reported success but did not create {0}")]
    Missing(PathBuf),
}

pub fn create_browser_shortcut(
    browser_root: &Path,
    display_name: &str,
    executable: &Path,
) -> Result<PathBuf, ShortcutError> {
    if !browser_root.is_dir() {
        return Err(ShortcutError::BrowserRoot(browser_root.to_path_buf()));
    }
    if !executable.is_file() {
        return Err(ShortcutError::Executable(executable.to_path_buf()));
    }
    let shortcut = browser_root.join(format!("{display_name}.lnk"));
    let shell = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = Command::new(shell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "$ErrorActionPreference='Stop'; \
             $link=(New-Object -ComObject WScript.Shell).CreateShortcut($env:BROWSER_HARBOR_LINK); \
             $link.TargetPath=$env:BROWSER_HARBOR_TARGET; \
             $link.WorkingDirectory=$env:BROWSER_HARBOR_WORKING_DIR; \
             $link.IconLocation=$env:BROWSER_HARBOR_TARGET + ',0'; \
             $link.Save()",
        ])
        .env("BROWSER_HARBOR_LINK", &shortcut)
        .env("BROWSER_HARBOR_TARGET", executable)
        .env(
            "BROWSER_HARBOR_WORKING_DIR",
            executable.parent().unwrap_or(browser_root),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(ShortcutError::Spawn)?;
    if !output.status.success() {
        return Err(ShortcutError::PowerShell(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    if !shortcut.is_file() {
        return Err(ShortcutError::Missing(shortcut));
    }
    Ok(shortcut)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::tempdir;

    use super::create_browser_shortcut;

    #[test]
    fn creates_shortcut_in_browser_root() {
        let root = tempdir().unwrap();
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let executable = root.path().join("chrome.exe");
        fs::copy(
            PathBuf::from(windows).join(r"System32\notepad.exe"),
            &executable,
        )
        .unwrap();

        let shortcut = create_browser_shortcut(root.path(), "Chrome", &executable).unwrap();

        assert_eq!(shortcut, root.path().join("Chrome.lnk"));
        assert!(fs::metadata(shortcut).unwrap().len() > 0);
    }
}
