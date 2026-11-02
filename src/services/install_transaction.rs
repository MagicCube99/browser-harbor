//! A write-ahead list of replacements; unfinished originals remain recoverable.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

const JOURNAL: &str = "transaction.json";

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    name: String,
    had_previous: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct FileTransaction {
    destination: PathBuf,
    entries: Vec<Entry>,
    #[serde(skip)]
    backup: PathBuf,
}

impl FileTransaction {
    pub fn new(backup: &Path, destination: &Path) -> io::Result<Self> {
        fs::create_dir_all(backup)?;
        let state = Self {
            destination: destination.to_path_buf(),
            entries: Vec::new(),
            backup: backup.to_path_buf(),
        };
        state.persist()?;
        Ok(state)
    }

    fn persist(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        let temporary = self.backup.join("transaction.tmp");
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, self.backup.join(JOURNAL))
    }

    pub fn replace(&mut self, name: &std::ffi::OsStr, staged: Option<&Path>) -> io::Result<()> {
        let name = name
            .to_str()
            .ok_or_else(|| io::Error::other("replacement name is not Unicode"))?;
        if !single_name(name) {
            return Err(io::Error::other("unsafe replacement name"));
        }
        let destination = self.destination.join(name);
        if self.backup.join(name).exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "replacement backup already exists",
            ));
        }
        let had_previous = destination.exists();
        self.entries.push(Entry {
            name: name.to_owned(),
            had_previous,
        });
        self.persist()?;
        if had_previous {
            fs::rename(&destination, self.backup.join(name))?;
        }
        if let Some(staged) = staged {
            fs::rename(staged, destination)?;
        }
        Ok(())
    }

    pub fn rollback(&mut self) -> io::Result<()> {
        while let Some(entry) = self.entries.last() {
            let saved = self.backup.join(&entry.name);
            let destination = self.destination.join(&entry.name);
            if saved.exists() || !entry.had_previous {
                remove_if_present(&destination)?;
            }
            if saved.exists() {
                fs::rename(saved, destination)?;
            }
            self.entries.pop();
            self.persist()?;
        }
        fs::remove_file(self.backup.join(JOURNAL))
    }
}

pub(super) fn recover_work(work: &Path, root: &Path, protected: &[&Path]) -> io::Result<()> {
    for name in ["backup", "native-backup", "replacement-backup"] {
        let backup = work.join(name);
        let journal = backup.join(JOURNAL);
        if !journal.is_file() {
            continue;
        }
        reject_links(root, &journal)?;
        if fs::metadata(&journal)?.len() > 1024 * 1024 {
            return Err(io::Error::other("installation journal is too large"));
        }
        let bytes = fs::read(&journal)?;
        let mut state: FileTransaction =
            serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        let relative = state
            .destination
            .strip_prefix(root)
            .map_err(|_| io::Error::other("recovery destination escaped browser root"))?;
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(io::Error::other("unsafe recovery destination"));
        }
        if state.entries.len() > 10000 {
            return Err(io::Error::other("too many recovery entries"));
        }
        let mut names = std::collections::HashSet::new();
        for entry in &state.entries {
            if !single_name(&entry.name) {
                return Err(io::Error::other("unsafe recovery entry"));
            }
            if !names.insert(entry.name.to_ascii_lowercase()) || entry.name.starts_with(".pbm-") {
                return Err(io::Error::other("duplicate or reserved recovery entry"));
            }
            let destination = state.destination.join(&entry.name);
            if protected
                .iter()
                .any(|path| paths_overlap(path, &destination))
            {
                return Err(io::Error::other(
                    "recovery overlaps user data; preserve backup for manual recovery",
                ));
            }
            reject_links(root, &destination)?;
            reject_links(root, &backup.join(&entry.name))?;
        }
        state.backup = backup;
        state.rollback()?;
    }
    Ok(())
}

pub(super) fn has_journal(work: &Path) -> bool {
    ["backup", "native-backup", "replacement-backup"]
        .iter()
        .any(|name| work.join(name).join(JOURNAL).exists())
}

pub(super) fn finish_work(work: &Path) -> io::Result<()> {
    for name in ["backup", "native-backup", "replacement-backup"] {
        let path = work.join(name).join(JOURNAL);
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn single_name(name: &str) -> bool {
    !name.is_empty()
        && name != JOURNAL
        && name != "transaction.tmp"
        && !name.contains(['/', '\\', ':'])
        && !name.ends_with(['.', ' '])
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        )
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    let identity = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let left = identity(left);
    let right = identity(right);
    left == right
        || left.starts_with(&format!("{right}\\"))
        || right.starts_with(&format!("{left}\\"))
}

pub(super) fn reject_links(root: &Path, path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("path escaped browser root"))?;
    let mut current = root.to_path_buf();
    if fs::symlink_metadata(root)?.file_attributes() & 0x400 != 0 {
        return Err(io::Error::other(
            "reparse points are not allowed in the browser root",
        ));
    }
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(io::Error::other("unsafe path component"));
        }
        if component
            .as_os_str()
            .to_string_lossy()
            .ends_with(['.', ' '])
        {
            return Err(io::Error::other("ambiguous Windows path component"));
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_attributes() & 0x400 != 0 => {
                return Err(io::Error::other(
                    "reparse points are not allowed in installation recovery",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::other(
            "refusing to remove a linked recovery path",
        )),
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
