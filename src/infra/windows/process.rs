//! Windows process-path checks used only to protect file replacement.

use std::path::{Path, PathBuf};

use thiserror::Error;
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
    },
};

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("could not inspect running processes")]
    Snapshot(#[source] std::io::Error),
}

pub fn is_process_running(executable: &Path) -> Result<bool, ProcessError> {
    let target = normalized_path(executable);
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(ProcessError::Snapshot(std::io::Error::last_os_error()));
    }
    let result = (|| {
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            return Ok(false);
        }
        loop {
            if let Some(path) = process_path(entry.th32ProcessID)
                && normalized_path(&path) == target
            {
                return Ok(true);
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                return Ok(false);
            }
        }
    })();
    unsafe {
        CloseHandle(snapshot);
    }
    result
}

fn process_path(process_id: u32) -> Option<PathBuf> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    let success = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
    unsafe {
        CloseHandle(handle);
    }
    if success == 0 {
        return None;
    }
    buffer.truncate(size as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

fn normalized_path(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = canonical.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.replace('/', r"\").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::is_process_running;

    #[test]
    fn finds_the_current_test_executable_by_full_path() {
        let executable = std::env::current_exe().unwrap();
        assert!(is_process_running(&executable).unwrap());
    }
}
