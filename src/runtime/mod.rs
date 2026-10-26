pub mod chromeplus;
pub mod libportable;
pub mod native;

use std::path::PathBuf;
use std::{fs, io, path::Path};

use thiserror::Error;

use crate::infra::ini::IniFileError;
use crate::infra::windows::version_info::VersionInfoError;
use crate::{domain::release::Artifact, providers::ProviderError, services::ServiceError};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    Ini(#[from] IniFileError),
    #[error(transparent)]
    VersionInfo(#[from] VersionInfoError),
    #[error("runtime component is missing from the verified payload")]
    MissingComponent,
    #[error("runtime helper failed with exit code {0}")]
    HelperFailed(i32),
    #[error("runtime helper timed out")]
    HelperTimeout,
    #[error("Libportable does not support {product} version {version}")]
    UnsupportedLibportableVersion { product: String, version: String },
    #[error("Chrome++ key mapping is invalid on line {line}: {value}")]
    InvalidChromePlusKeyMapping { line: usize, value: String },
    #[error("runtime preparation failed for {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> RuntimeError {
    RuntimeError::Io {
        path: path.into(),
        source,
    }
}

pub(crate) fn download_artifact(
    file_name: String,
    url: String,
    size: Option<u64>,
    digest: Option<String>,
) -> Artifact {
    Artifact {
        format: crate::domain::release::ArchiveFormat::SevenZip,
        integrity: digest
            .as_deref()
            .and_then(crate::domain::release::Integrity::from_digest),
        file_name,
        size,
        urls: vec![url],
        signature_url: None,
    }
}

pub(crate) fn pe_imports_dll(path: &Path, expected: &str) -> io::Result<bool> {
    let data = fs::read(path)?;
    let read_u16 = |offset: usize| -> io::Result<u16> {
        data.get(offset..offset + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_le_bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated PE header"))
    };
    let read_u32 = |offset: usize| -> io::Result<u32> {
        data.get(offset..offset + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated PE header"))
    };
    if data.get(..2) != Some(b"MZ") {
        return Ok(false);
    }
    let pe = read_u32(0x3c)? as usize;
    if data.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Ok(false);
    }
    let sections = read_u16(pe + 6)? as usize;
    let optional_size = read_u16(pe + 20)? as usize;
    let optional = pe.checked_add(24).ok_or_else(invalid_pe)?;
    let data_directories = match read_u16(optional)? {
        0x20b => optional + 112,
        0x10b => optional + 96,
        _ => return Ok(false),
    };
    let imports = data_directories + 8;
    let import_rva = read_u32(imports)? as usize;
    let import_size = read_u32(imports + 4)? as usize;
    if import_rva == 0 || import_size < 20 {
        return Ok(false);
    }

    struct Section {
        virtual_address: usize,
        virtual_size: usize,
        raw_offset: usize,
        raw_size: usize,
    }
    let section_table = optional.checked_add(optional_size).ok_or_else(invalid_pe)?;
    let mut mapped = Vec::with_capacity(sections);
    for index in 0..sections {
        let header = section_table
            .checked_add(index.checked_mul(40).ok_or_else(invalid_pe)?)
            .ok_or_else(invalid_pe)?;
        mapped.push(Section {
            virtual_size: read_u32(header + 8)? as usize,
            virtual_address: read_u32(header + 12)? as usize,
            raw_size: read_u32(header + 16)? as usize,
            raw_offset: read_u32(header + 20)? as usize,
        });
    }
    let rva_to_offset = |rva: usize| -> io::Result<usize> {
        for section in &mapped {
            let span = section.virtual_size.max(section.raw_size);
            if rva >= section.virtual_address && rva < section.virtual_address.saturating_add(span)
            {
                let delta = rva - section.virtual_address;
                if delta >= section.raw_size {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "RVA points to uninitialized PE data",
                    ));
                }
                return section.raw_offset.checked_add(delta).ok_or_else(invalid_pe);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "RVA is outside PE sections",
        ))
    };
    let descriptor_offset = rva_to_offset(import_rva)?;
    for index in 0..(import_size / 20).min(4096) {
        let descriptor = descriptor_offset
            .checked_add(index.saturating_mul(20))
            .ok_or_else(invalid_pe)?;
        let fields = data
            .get(descriptor..descriptor + 20)
            .ok_or_else(invalid_pe)?;
        if fields.iter().all(|byte| *byte == 0) {
            break;
        }
        let name_rva = u32::from_le_bytes(fields[12..16].try_into().unwrap_or_default());
        let name_offset = rva_to_offset(name_rva as usize)?;
        let name = data
            .get(name_offset..)
            .ok_or_else(invalid_pe)?
            .split(|byte| *byte == 0)
            .next()
            .unwrap_or_default();
        if name.eq_ignore_ascii_case(expected.as_bytes()) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn is_pe_x64(path: &Path) -> io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = fs::File::open(path)?;
    let mut dos = [0u8; 64];
    if file.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return Ok(false);
    }
    let offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap_or_default()) as u64;
    file.seek(SeekFrom::Start(offset))?;
    let mut headers = [0u8; 48];
    if file.read_exact(&mut headers).is_err() || &headers[..4] != b"PE\0\0" {
        return Ok(false);
    }
    let machine = u16::from_le_bytes([headers[4], headers[5]]);
    let optional_magic = u16::from_le_bytes([headers[24], headers[25]]);
    Ok(machine == 0x8664 && optional_magic == 0x20b)
}

pub(crate) fn portable_relative_path(base: &Path, target: &Path) -> Option<String> {
    if !base.is_absolute() || !target.is_absolute() {
        return None;
    }
    let base_components = base.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();
    let common = base_components
        .iter()
        .zip(&target_components)
        .take_while(|(left, right)| {
            left.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
        })
        .count();
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in common..base_components.len() {
        relative.push("..");
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }
    Some(relative.display().to_string())
}

fn invalid_pe() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid PE structure")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{pe_imports_dll, portable_relative_path};

    #[test]
    fn pe_import_parser_reads_the_current_test_executable() {
        let executable = std::env::current_exe().unwrap();
        assert!(pe_imports_dll(&executable, "KERNEL32.dll").unwrap());
        assert!(!pe_imports_dll(&executable, "not-present.dll").unwrap());
    }

    #[test]
    fn portable_relative_path_keeps_same_drive_paths_relocatable() {
        assert_eq!(
            portable_relative_path(
                Path::new(r"D:\Browsers\Chrome\App"),
                Path::new(r"D:\Browsers\Chrome\Profiles"),
            ),
            Some(r"..\Profiles".to_owned())
        );
        assert_eq!(
            portable_relative_path(
                Path::new(r"D:\Browsers\Edge\App\Microsoft\Edge\Application\156.0.0.0"),
                Path::new(r"D:\Browsers\Edge\Profiles"),
            ),
            Some(r"..\..\..\..\..\Profiles".to_owned())
        );
    }

    #[test]
    fn portable_relative_path_refuses_cross_drive_paths() {
        assert_eq!(
            portable_relative_path(
                Path::new(r"C:\Browsers\Chrome\App"),
                Path::new(r"D:\Profiles"),
            ),
            None
        );
    }
}
