use std::{ffi::OsStr, io, os::windows::ffi::OsStrExt, path::Path};

use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

pub fn move_file_without_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide_null(from.as_os_str());
    let to = wide_null(to.as_os_str());
    // MoveFileExW without MOVEFILE_REPLACE_EXISTING is the no-overwrite atomic rename.
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}
