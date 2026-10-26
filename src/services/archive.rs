//! Bounded archive extraction for browser payloads.

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
};

use cab::Cabinet;
use msi::{Package, Select};
use sevenz_rust2::{ArchiveReader, Password};
use thiserror::Error;
use zip::ZipArchive;

use crate::domain::release::ArchiveFormat;

const MAX_ENTRIES: usize = 250_000;
const MAX_UNCOMPRESSED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const SEVEN_Z_SIGNATURE: &[u8; 6] = b"7z\xbc\xaf'\x1c";

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("unsupported archive format")]
    UnsupportedFormat,
    #[error("archive entry has an unsafe path: {0}")]
    UnsafePath(String),
    #[error("archive contains too many entries")]
    TooManyEntries,
    #[error("archive exceeds the extraction size limit")]
    TooLarge,
    #[error("archive contains an unsupported symbolic link: {0}")]
    SymbolicLink(String),
    #[error("archive contains no usable payload")]
    EmptyPayload,
    #[error("could not locate a 7z payload in the self-extracting file")]
    SevenZipPayloadMissing,
    #[error("could not locate an embedded MSI cabinet")]
    MsiCabinetMissing,
    #[error("MSI file table references an unknown cabinet file: {0}")]
    MsiUnknownFile(String),
    #[error("could not read MSI package")]
    Msi(#[source] io::Error),
    #[error("could not read MSI cabinet")]
    Cab(#[source] io::Error),
    #[error("could not read ZIP archive")]
    Zip(#[source] zip::result::ZipError),
    #[error("could not read 7z archive: {0}")]
    SevenZip(String),
    #[error("file operation failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub struct ArchiveService;

impl ArchiveService {
    pub fn extract(
        artifact_path: &Path,
        format: ArchiveFormat,
        destination: &Path,
    ) -> Result<(), ArchiveError> {
        fs::create_dir_all(destination).map_err(|source| io_error(destination, source))?;
        match format {
            ArchiveFormat::Zip => extract_zip(artifact_path, destination),
            ArchiveFormat::SevenZip => extract_seven_zip(artifact_path, destination),
            ArchiveFormat::Msi => extract_msi(artifact_path, destination),
        }
    }
}

pub fn safe_join(root: &Path, archive_path: &str) -> Result<PathBuf, ArchiveError> {
    let relative = Path::new(archive_path);
    if relative.is_absolute() {
        return Err(ArchiveError::UnsafePath(archive_path.to_owned()));
    }
    let mut output = PathBuf::from(root);
    for component in relative.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_string_lossy();
                if value.contains(':') {
                    return Err(ArchiveError::UnsafePath(archive_path.to_owned()));
                }
                output.push(value.as_ref());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ArchiveError::UnsafePath(archive_path.to_owned()));
            }
        }
    }
    Ok(output)
}

fn extract_zip(path: &Path, destination: &Path) -> Result<(), ArchiveError> {
    let file = File::open(path).map_err(|source| io_error(path, source))?;
    let mut archive = ZipArchive::new(file).map_err(ArchiveError::Zip)?;
    if archive.len() > MAX_ENTRIES {
        return Err(ArchiveError::TooManyEntries);
    }
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(ArchiveError::Zip)?;
        let name = entry.name().map_err(ArchiveError::Zip)?.into_owned();
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(ArchiveError::SymbolicLink(name.to_string()));
        }
        if entry.is_dir() {
            continue;
        }
        total = checked_total(total, entry.size())?;
        let output = safe_join(destination, &name)?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
        }
        let mut file = File::create(&output).map_err(|source| io_error(&output, source))?;
        io::copy(&mut entry, &mut file).map_err(|source| io_error(&output, source))?;
    }
    Ok(())
}

fn extract_seven_zip(path: &Path, destination: &Path) -> Result<(), ArchiveError> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("7z"))
    {
        let reader = File::open(path).map_err(|source| io_error(path, source))?;
        return extract_seven_zip_reader(reader, destination);
    }

    let offsets = find_7z_offsets(path)?;
    for offset in offsets {
        let file = File::open(path).map_err(|source| io_error(path, source))?;
        let reader = OffsetReader::new(file, offset)?;
        if let Ok(()) = extract_seven_zip_reader(reader, destination) {
            return Ok(());
        }
    }
    Err(ArchiveError::SevenZipPayloadMissing)
}

fn extract_seven_zip_reader<R: Read + Seek>(
    reader: R,
    destination: &Path,
) -> Result<(), ArchiveError> {
    let mut archive = ArchiveReader::new(reader, Password::empty())
        .map_err(|error| ArchiveError::SevenZip(error.to_string()))?;
    let mut count = 0usize;
    let mut total = 0u64;
    archive
        .for_each_entries(|entry, reader| {
            if entry.is_directory() {
                return Ok(true);
            }
            count += 1;
            if count > MAX_ENTRIES {
                return Err(sevenz_rust2::Error::from(io::Error::other(
                    "archive contains too many entries",
                )));
            }
            total = total
                .checked_add(entry.size())
                .filter(|total| *total <= MAX_UNCOMPRESSED_BYTES)
                .ok_or_else(|| io::Error::other("archive exceeds extraction size limit"))?;
            let output = safe_join(destination, entry.name())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&output)?;
            let mut limited = reader.take(entry.size().saturating_add(1));
            let copied = io::copy(&mut limited, &mut file)?;
            if copied != entry.size() {
                return Err(sevenz_rust2::Error::from(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "7z entry size mismatch",
                )));
            }
            Ok(true)
        })
        .map_err(|error| ArchiveError::SevenZip(error.to_string()))?;
    Ok(())
}

fn extract_msi(path: &Path, destination: &Path) -> Result<(), ArchiveError> {
    let file = File::open(path).map_err(|source| io_error(path, source))?;
    let mut package = Package::open(file).map_err(ArchiveError::Msi)?;
    let tables = MsiTables::read(&mut package)?;
    let cabinets = package
        .streams()
        .filter(|stream| stream.to_ascii_lowercase().ends_with(".cab"))
        .collect::<Vec<_>>();
    if cabinets.is_empty() {
        return Err(ArchiveError::MsiCabinetMissing);
    }

    let mut total = 0u64;
    let mut files = 0usize;
    for cabinet_name in cabinets {
        let stream = package
            .read_stream(&cabinet_name)
            .map_err(ArchiveError::Msi)?;
        let mut cabinet = Cabinet::new(stream).map_err(ArchiveError::Cab)?;
        let entries = cabinet
            .folder_entries()
            .flat_map(|folder| folder.file_entries())
            .map(|entry| (entry.name().to_owned(), entry.uncompressed_size() as u64))
            .collect::<Vec<_>>();
        for (entry_name, declared) in entries {
            files += 1;
            if files > MAX_ENTRIES {
                return Err(ArchiveError::TooManyEntries);
            }
            let relative = tables
                .files
                .get(&entry_name.to_ascii_lowercase())
                .or_else(|| tables.files.get(&entry_name))
                .ok_or_else(|| ArchiveError::MsiUnknownFile(entry_name.clone()))?;
            let mut data = cabinet.read_file(&entry_name).map_err(ArchiveError::Cab)?;
            total = checked_total(total, declared)?;
            let output = safe_join(destination, &relative.to_string_lossy())?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
            }
            let mut target = File::create(&output).map_err(|source| io_error(&output, source))?;
            io::copy(&mut data, &mut target).map_err(|source| io_error(&output, source))?;
        }
    }
    if files == 0 {
        return Err(ArchiveError::EmptyPayload);
    }
    Ok(())
}

struct MsiTables {
    files: HashMap<String, PathBuf>,
}

impl MsiTables {
    fn read<F: Read + Seek>(package: &mut Package<F>) -> Result<Self, ArchiveError> {
        let mut file_components = HashMap::new();
        let mut file_names = HashMap::new();
        for row in package
            .select_rows(Select::table("File"))
            .map_err(ArchiveError::Msi)?
        {
            let Some(file) = row["File"].as_str() else {
                continue;
            };
            if let Some(component) = row["Component_"].as_str() {
                file_components.insert(file.to_owned(), component.to_owned());
            }
            if let Some(name) = row["FileName"].as_str() {
                file_names.insert(file.to_owned(), parse_msi_name(name));
            }
        }

        let mut component_directories = HashMap::new();
        for row in package
            .select_rows(Select::table("Component"))
            .map_err(ArchiveError::Msi)?
        {
            if let (Some(component), Some(directory)) =
                (row["Component"].as_str(), row["Directory_"].as_str())
            {
                component_directories.insert(component.to_owned(), directory.to_owned());
            }
        }

        let mut directory_parents = HashMap::new();
        let mut directory_names = HashMap::new();
        for row in package
            .select_rows(Select::table("Directory"))
            .map_err(ArchiveError::Msi)?
        {
            let Some(directory) = row["Directory"].as_str() else {
                continue;
            };
            if let Some(parent) = row["Directory_Parent"].as_str() {
                directory_parents.insert(directory.to_owned(), parent.to_owned());
            }
            if let Some(name) = row["DefaultDir"].as_str() {
                directory_names.insert(directory.to_owned(), parse_msi_directory(name));
            }
        }

        let mut files = HashMap::new();
        for (file, file_name) in file_names {
            let Some(directory) = file_components
                .get(&file)
                .and_then(|component| component_directories.get(component))
            else {
                continue;
            };
            let mut parts = Vec::new();
            let mut current = Some(directory.as_str());
            while let Some(id) = current {
                if let Some(name) = directory_names.get(id)
                    && name != "."
                    && !name.is_empty()
                {
                    parts.push(name.clone());
                }
                current = directory_parents.get(id).map(String::as_str);
            }
            parts.reverse();
            let mut path = PathBuf::new();
            for part in parts {
                path.push(part);
            }
            path.push(file_name);
            files.insert(file.clone(), path);
            files.insert(file.to_ascii_lowercase(), files[&file].clone());
        }
        Ok(Self { files })
    }
}

fn parse_msi_name(value: &str) -> String {
    value
        .split_once('|')
        .map_or(value, |(_, long)| long)
        .to_owned()
}

fn parse_msi_directory(value: &str) -> String {
    let target = value.split_once(':').map_or(value, |(target, _)| target);
    parse_msi_name(target)
}

fn checked_total(total: u64, entry_size: u64) -> Result<u64, ArchiveError> {
    total
        .checked_add(entry_size)
        .filter(|total| *total <= MAX_UNCOMPRESSED_BYTES)
        .ok_or(ArchiveError::TooLarge)
}

fn find_7z_offsets(path: &Path) -> Result<Vec<u64>, ArchiveError> {
    let mut file = File::open(path).map_err(|source| io_error(path, source))?;
    let mut buffer = vec![0u8; 1024 * 1024 + SEVEN_Z_SIGNATURE.len()];
    let mut absolute = 0u64;
    let mut offsets = Vec::new();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| io_error(path, source))?;
        if read == 0 {
            break;
        }
        for (index, window) in buffer[..read].windows(SEVEN_Z_SIGNATURE.len()).enumerate() {
            if window == SEVEN_Z_SIGNATURE {
                offsets.push(absolute + index as u64);
                if offsets.len() >= 32 {
                    return Ok(offsets);
                }
            }
        }
        if read < SEVEN_Z_SIGNATURE.len() {
            break;
        }
        let overlap = SEVEN_Z_SIGNATURE.len() - 1;
        file.seek(SeekFrom::Current(-(overlap as i64)))
            .map_err(|source| io_error(path, source))?;
        absolute = absolute
            .saturating_add(read as u64)
            .saturating_sub(overlap as u64);
    }
    Ok(offsets)
}

struct OffsetReader<R> {
    inner: R,
    base: u64,
}

impl<R: Read + Seek> OffsetReader<R> {
    fn new(mut inner: R, base: u64) -> Result<Self, ArchiveError> {
        inner
            .seek(SeekFrom::Start(base))
            .map_err(|source| ArchiveError::Io {
                path: PathBuf::new(),
                source,
            })?;
        Ok(Self { inner, base })
    }
}

impl<R: Read + Seek> Read for OffsetReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl<R: Read + Seek> Seek for OffsetReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        match position {
            SeekFrom::Start(offset) => self.inner.seek(SeekFrom::Start(self.base + offset)),
            SeekFrom::Current(delta) => self.inner.seek(SeekFrom::Current(delta)),
            SeekFrom::End(delta) => self.inner.seek(SeekFrom::End(delta)),
        }
        .and_then(|absolute| {
            absolute
                .checked_sub(self.base)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before archive"))
        })
    }
}

fn io_error(path: &Path, source: io::Error) -> ArchiveError {
    ArchiveError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
    };

    use sevenz_rust2::ArchiveWriter;
    use tempfile::tempdir;
    use walkdir::WalkDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{ArchiveService, safe_join};
    use crate::domain::release::ArchiveFormat;

    #[test]
    fn safe_join_rejects_parent_and_absolute_paths() {
        let root = tempdir().unwrap();
        assert!(safe_join(root.path(), r"folder\file.txt").is_ok());
        assert!(safe_join(root.path(), r"..\escape.txt").is_err());
        assert!(safe_join(root.path(), r"C:\escape.txt").is_err());
    }

    #[test]
    fn zip_extraction_rejects_path_traversal() {
        let directory = tempdir().unwrap();
        let archive_path = directory.path().join("fixture.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("../escape.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"escape").unwrap();
        writer.finish().unwrap();

        let destination = directory.path().join("out");
        assert!(ArchiveService::extract(&archive_path, ArchiveFormat::Zip, &destination).is_err());
        assert!(!directory.path().join("escape.txt").exists());
    }

    #[test]
    fn zip_extraction_writes_normal_payload() {
        let directory = tempdir().unwrap();
        let archive_path = directory.path().join("fixture.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("APP/chrome.exe", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"fixture").unwrap();
        writer.finish().unwrap();

        let destination = directory.path().join("out");
        ArchiveService::extract(&archive_path, ArchiveFormat::Zip, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("APP/chrome.exe")).unwrap(),
            b"fixture"
        );
    }

    #[test]
    fn self_extracting_seven_zip_payload_is_found() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("firefox.exe"), b"fixture").unwrap();
        let archive_path = directory.path().join("payload.7z");
        let mut writer = ArchiveWriter::create(&archive_path).unwrap();
        writer.push_source_path(&source, |_| true).unwrap();
        writer.finish().unwrap();

        let sfx_path = directory.path().join("installer.exe");
        let mut sfx = File::create(&sfx_path).unwrap();
        sfx.write_all(b"MZ synthetic installer prefix").unwrap();
        sfx.write_all(&fs::read(&archive_path).unwrap()).unwrap();

        let destination = directory.path().join("out");
        ArchiveService::extract(&sfx_path, ArchiveFormat::SevenZip, &destination).unwrap();
        assert!(
            WalkDir::new(&destination)
                .into_iter()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name() == "firefox.exe")
        );
    }
}
