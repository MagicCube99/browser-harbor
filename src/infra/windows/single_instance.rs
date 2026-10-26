//! One-process guard so concurrent windows cannot overwrite the same manager state.

use std::{ffi::OsStr, io, iter, os::windows::ffi::OsStrExt, ptr};

use thiserror::Error;
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
    System::Threading::CreateMutexW,
    UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, MB_ICONERROR, MB_OK, MessageBoxW, SW_RESTORE, SetForegroundWindow,
        ShowWindow,
    },
};

#[derive(Debug)]
pub struct SingleInstance {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl SingleInstance {
    pub fn acquire(name: &str) -> Result<Option<Self>, SingleInstanceError> {
        let name = wide(name);
        let handle = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            return Err(SingleInstanceError::Acquire(io::Error::last_os_error()));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return Ok(None);
        }
        Ok(Some(Self { handle }))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

pub fn focus_window(title: &str) -> bool {
    let title = wide(title);
    let window = unsafe { FindWindowW(ptr::null(), title.as_ptr()) };
    if window.is_null() {
        return false;
    }
    unsafe {
        if IsIconic(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        SetForegroundWindow(window) != 0
    }
}

pub fn show_error(title: &str, message: &str) {
    let title = wide(title);
    let message = wide(message);
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text)
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

#[derive(Debug, Error)]
pub enum SingleInstanceError {
    #[error("failed to create the Browser Harbor instance guard")]
    Acquire(#[source] io::Error),
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::SingleInstance;

    #[test]
    fn second_guard_is_rejected_until_the_first_is_dropped() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!(r"Local\BrowserHarborTest-{}-{unique}", std::process::id());

        let first = SingleInstance::acquire(&name).unwrap().unwrap();
        assert!(SingleInstance::acquire(&name).unwrap().is_none());

        drop(first);
        assert!(SingleInstance::acquire(&name).unwrap().is_some());
    }
}
