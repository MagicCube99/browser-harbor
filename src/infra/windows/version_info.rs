//! Safe wrapper around Windows file version resources.

use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr};

use thiserror::Error;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

#[derive(Debug, Error)]
pub enum VersionInfoError {
    #[error("file contains no readable Windows version resource: {0}")]
    Unavailable(String),
}

pub fn executable_product_version(path: &Path) -> Result<String, VersionInfoError> {
    executable_version(
        path,
        &["ProductVersion", "FileVersion"],
        fixed_product_version,
    )
}

pub fn executable_file_version(path: &Path) -> Result<String, VersionInfoError> {
    executable_version(path, &["FileVersion"], fixed_file_version)
}

pub fn executable_product_name(path: &Path) -> Result<String, VersionInfoError> {
    executable_version(path, &["ProductName"], |_| None)
}

fn executable_version(
    path: &Path,
    fields: &[&str],
    fixed_fallback: fn(&[u8]) -> Option<String>,
) -> Result<String, VersionInfoError> {
    if !path.is_file() {
        return Err(VersionInfoError::Unavailable(path.display().to_string()));
    }
    let path_wide = wide(path.as_os_str());
    let size = unsafe { GetFileVersionInfoSizeW(path_wide.as_ptr(), ptr::null_mut()) };
    if size == 0 {
        return Err(VersionInfoError::Unavailable(path.display().to_string()));
    }
    let mut data = vec![0u8; size as usize];
    let loaded = unsafe {
        GetFileVersionInfoW(
            path_wide.as_ptr(),
            0,
            size,
            data.as_mut_ptr().cast::<c_void>(),
        )
    };
    if loaded == 0 {
        return Err(VersionInfoError::Unavailable(path.display().to_string()));
    }

    let languages = translations(&data).unwrap_or_else(|| vec![(0x0409, 0x04b0)]);
    for (language, code_page) in languages {
        for field in fields {
            if let Some(value) = string_value(&data, language, code_page, field)
                && !value.trim().is_empty()
            {
                return Ok(value);
            }
        }
    }
    fixed_fallback(&data).ok_or_else(|| VersionInfoError::Unavailable(path.display().to_string()))
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn query(data: &[u8], name: &str) -> Option<(*const c_void, u32)> {
    let name = wide(std::ffi::OsStr::new(name));
    let mut buffer = ptr::null_mut();
    let mut length = 0u32;
    let found = unsafe {
        VerQueryValueW(
            data.as_ptr().cast(),
            name.as_ptr(),
            &mut buffer,
            &mut length,
        )
    };
    (found != 0 && !buffer.is_null()).then_some((buffer.cast_const(), length))
}

fn translations(data: &[u8]) -> Option<Vec<(u16, u16)>> {
    let (buffer, length) = query(data, r"\VarFileInfo\Translation")?;
    let bytes = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), length as usize) };
    let translations = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|entry| {
            (
                u16::from_le_bytes([entry[0], entry[1]]),
                u16::from_le_bytes([entry[2], entry[3]]),
            )
        })
        .collect::<Vec<_>>();
    (!translations.is_empty()).then_some(translations)
}

fn string_value(data: &[u8], language: u16, code_page: u16, field: &str) -> Option<String> {
    let block = format!(r"\StringFileInfo\{language:04x}{code_page:04x}\{field}");
    let (buffer, character_count) = query(data, &block)?;
    let characters =
        unsafe { std::slice::from_raw_parts(buffer.cast::<u16>(), character_count as usize) };
    let end = characters
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(characters.len());
    String::from_utf16(&characters[..end]).ok()
}

fn fixed_product_version(data: &[u8]) -> Option<String> {
    fixed_version(data, 16)
}

fn fixed_file_version(data: &[u8]) -> Option<String> {
    fixed_version(data, 8)
}

fn fixed_version(data: &[u8], offset: usize) -> Option<String> {
    let (buffer, length) = query(data, r"\")?;
    if length < 24 {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), length as usize) };
    let version_ms = u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?);
    let version_ls = u32::from_le_bytes(bytes.get(offset + 4..offset + 8)?.try_into().ok()?);
    Some(format!(
        "{}.{}.{}.{}",
        version_ms >> 16,
        version_ms & 0xffff,
        version_ls >> 16,
        version_ls & 0xffff
    ))
}

#[cfg(test)]
mod tests {
    use super::{executable_file_version, executable_product_name, executable_product_version};

    #[test]
    fn reads_a_product_version_from_a_windows_system_binary() {
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let path = std::path::PathBuf::from(windows).join(r"System32\notepad.exe");
        let version = executable_product_version(&path).unwrap();
        assert!(version.contains('.'), "unexpected version: {version}");
        assert!(version.chars().any(|character| character.is_ascii_digit()));
    }

    #[test]
    fn reads_a_file_version_from_a_windows_system_binary() {
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let path = std::path::PathBuf::from(windows).join(r"System32\notepad.exe");
        let version = executable_file_version(&path).unwrap();
        assert!(version.contains('.'), "unexpected version: {version}");
        assert!(version.chars().any(|character| character.is_ascii_digit()));
    }

    #[test]
    fn reads_a_product_name_from_a_windows_system_binary() {
        let windows = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let path = std::path::PathBuf::from(windows).join(r"System32\notepad.exe");
        let product_name = executable_product_name(&path).unwrap();
        assert!(!product_name.trim().is_empty());
    }
}
