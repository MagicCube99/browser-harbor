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

/// A process whose image path could be read; used for diagnostics only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub process_id: u32,
    pub name: String,
    pub path: PathBuf,
}

pub fn is_process_running(executable: &Path) -> Result<bool, ProcessError> {
    running_process_count(executable).map(|count| count > 0)
}

pub fn running_process_count(executable: &Path) -> Result<usize, ProcessError> {
    let target = normalized_path(executable);
    collect_processes(|process| normalized_path(&process.path) == target).map(|items| items.len())
}

pub fn running_process_count_in_directory(directory: &Path) -> Result<usize, ProcessError> {
    running_processes_in_directory(directory).map(|items| items.len())
}

/// Lists processes whose image lives inside `directory`, excluding this tool itself.
pub fn running_processes_in_directory(
    directory: &Path,
) -> Result<Vec<RunningProcess>, ProcessError> {
    let directory = normalized_path(directory);
    let prefix = format!("{}\\", directory.trim_end_matches('\\'));
    let current = std::process::id();
    collect_processes(|process| {
        process.process_id != current && normalized_path(&process.path).starts_with(&prefix)
    })
}

fn collect_processes(
    predicate: impl Fn(&RunningProcess) -> bool,
) -> Result<Vec<RunningProcess>, ProcessError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(ProcessError::Snapshot(std::io::Error::last_os_error()));
    }
    let result = (|| {
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
            return Ok(Vec::new());
        }
        let mut processes = Vec::new();
        loop {
            if let Some(process) = running_process(&entry)
                && predicate(&process)
            {
                processes.push(process);
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                return Ok(processes);
            }
        }
    })();
    unsafe {
        CloseHandle(snapshot);
    }
    result
}

fn running_process(entry: &PROCESSENTRY32W) -> Option<RunningProcess> {
    let path = process_path(entry.th32ProcessID)?;
    Some(RunningProcess {
        process_id: entry.th32ProcessID,
        name: process_name(&entry.szExeFile),
        path,
    })
}

fn process_name(executable: &[u16]) -> String {
    let length = executable
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(executable.len());
    String::from_utf16_lossy(&executable[..length])
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
    use std::{
        path::Path,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use super::{
        is_process_running, normalized_path, running_process_count,
        running_process_count_in_directory, running_processes_in_directory,
    };

    #[test]
    fn finds_the_current_test_executable_by_full_path() {
        let executable = std::env::current_exe().unwrap();
        assert!(is_process_running(&executable).unwrap());
    }

    #[test]
    fn counts_multiple_instances_of_the_same_executable() {
        let executable = std::env::var_os("COMSPEC").expect("Windows command processor is missing");
        let mut children = (0..2)
            .map(|_| {
                Command::new(&executable)
                    .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let deadline = Instant::now() + Duration::from_secs(5);
        let count = loop {
            let count = running_process_count(Path::new(&executable)).unwrap();
            if count >= 2 {
                break count;
            }
            assert!(
                Instant::now() < deadline,
                "expected two matching processes, found {count}"
            );
            thread::sleep(Duration::from_millis(50));
        };
        for child in &mut children {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(count >= 2);
    }

    #[test]
    fn counts_alternate_executables_inside_one_program_directory() {
        let directory = tempfile::tempdir().unwrap();
        let command = std::env::var_os("COMSPEC").expect("Windows command processor is missing");
        let first_path = directory.path().join("browser-a.exe");
        let second_path = directory.path().join("browser-b.exe");
        std::fs::copy(&command, &first_path).unwrap();
        std::fs::copy(&command, &second_path).unwrap();
        let mut children = [first_path, second_path]
            .iter()
            .map(|path| {
                Command::new(path)
                    .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let deadline = Instant::now() + Duration::from_secs(5);
        let count = loop {
            let count = running_process_count_in_directory(directory.path()).unwrap();
            if count >= 2 {
                break count;
            }
            assert!(
                Instant::now() < deadline,
                "expected two processes in the directory, found {count}"
            );
            thread::sleep(Duration::from_millis(50));
        };
        for child in &mut children {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(count >= 2);
    }

    #[test]
    fn reports_the_process_name_and_path_inside_the_directory() {
        let directory = tempfile::tempdir().unwrap();
        let command = std::env::var_os("COMSPEC").expect("Windows command processor is missing");
        let executable = directory.path().join("browser-a.exe");
        std::fs::copy(&command, &executable).unwrap();
        let mut child = Command::new(&executable)
            .args(["/C", "ping 127.0.0.1 -n 10 > nul"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let expected = normalized_path(&executable);
        let deadline = Instant::now() + Duration::from_secs(5);
        let processes = loop {
            let processes = running_processes_in_directory(directory.path()).unwrap();
            if processes
                .iter()
                .any(|process| normalized_path(&process.path) == expected)
            {
                break processes;
            }
            assert!(
                Instant::now() < deadline,
                "expected the started process to be reported"
            );
            thread::sleep(Duration::from_millis(50));
        };
        let _ = child.kill();
        let _ = child.wait();
        let process = processes
            .iter()
            .find(|process| normalized_path(&process.path) == expected)
            .unwrap();
        assert_eq!(process.name, "browser-a.exe");
        assert_ne!(process.process_id, std::process::id());
    }

    #[test]
    fn never_reports_the_current_process_as_running() {
        let executable = std::env::current_exe().unwrap();
        let directory = executable.parent().unwrap();
        let processes = running_processes_in_directory(directory).unwrap();
        assert!(
            processes
                .iter()
                .all(|process| process.process_id != std::process::id())
        );
    }
}
