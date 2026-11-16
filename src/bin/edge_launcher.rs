#![windows_subsystem = "windows"]

use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };
    let Some(current_dir) = current_exe.parent() else {
        return;
    };

    // Determine target msedge.exe and profiles directory based on current_dir
    let Some((real_edge, profiles_dir)) =
        resolve_from_application_dir(current_dir).or_else(|| resolve_from_root_dir(current_dir))
    else {
        return;
    };

    if !real_edge.is_file() {
        return;
    }

    let mut command = Command::new(&real_edge);
    command.arg(format!("--user-data-dir={}", profiles_dir.display()));

    // Forward all remaining command-line arguments (skip args[0] which is current_exe)
    for arg in std::env::args_os().skip(1) {
        command.arg(arg);
    }

    let _ = command.spawn();
}

fn resolve_from_application_dir(app_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    // app_dir = ...\App\Microsoft\Edge\Application
    let browser_root = app_dir.parent()?.parent()?.parent()?;
    let profiles = browser_root.join("Profiles");
    let real_edge = find_latest_version_edge(app_dir)?;
    Some((real_edge, profiles))
}

fn resolve_from_root_dir(root_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    // root_dir = ...\Edge
    let app_dir = root_dir.join(r"App\Microsoft\Edge\Application");
    let profiles = root_dir.join("Profiles");
    let real_edge = find_latest_version_edge(&app_dir)?;
    Some((real_edge, profiles))
}

fn find_latest_version_edge(app_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(app_dir).ok()?;
    let mut versions: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.chars().all(|c| c.is_ascii_digit() || c == '.') && name.contains('.') {
                let candidate = path.join("msedge.exe");
                if candidate.is_file() {
                    versions.push(candidate);
                }
            }
        }
    }
    versions.sort();
    versions.pop()
}
