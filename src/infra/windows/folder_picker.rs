use std::path::{Path, PathBuf};

pub fn pick_folder(title: &str, current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if let Some(directory) = current.and_then(starting_directory) {
        dialog = dialog.set_directory(directory);
    }
    dialog.pick_folder()
}

pub fn pick_file(title: &str, current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter("浏览器安装包", &["zip", "7z", "exe", "msi"]);
    if let Some(directory) = current.and_then(starting_directory) {
        dialog = dialog.set_directory(directory);
    }
    dialog.pick_file()
}

fn starting_directory(path: &Path) -> Option<PathBuf> {
    existing_directory(path)
        .and_then(|directory| {
            (!has_same_stem_file(&directory))
                .then_some(directory.clone())
                .or_else(|| directory.parent().and_then(existing_directory))
        })
        .or_else(|| path.parent().and_then(existing_directory))
}

fn existing_directory(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path)
        .ok()
        .filter(|path| path.is_dir())
}

fn has_same_stem_file(directory: &Path) -> bool {
    let Some(name) = directory.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(parent) = directory.parent() else {
        return false;
    };
    parent.read_dir().is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.file_type().is_ok_and(|file_type| file_type.is_file())
                && entry
                    .path()
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.eq_ignore_ascii_case(name))
        })
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::starting_directory;

    #[test]
    fn folder_picker_starts_at_an_existing_canonical_directory() {
        let directory = tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();

        assert_eq!(
            starting_directory(&nested),
            Some(fs::canonicalize(&nested).unwrap())
        );
    }

    #[test]
    fn file_picker_falls_back_to_the_existing_parent() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("browser.zip");
        fs::write(&file, b"test").unwrap();

        assert_eq!(
            starting_directory(&file),
            Some(fs::canonicalize(directory.path()).unwrap())
        );
    }

    #[test]
    fn same_stem_file_uses_the_parent_directory_to_avoid_shell_ambiguity() {
        let directory = tempdir().unwrap();
        let nested = directory.path().join("Iceweasel");
        fs::create_dir(&nested).unwrap();
        fs::write(directory.path().join("Iceweasel.zip"), b"test").unwrap();

        assert_eq!(
            starting_directory(&nested),
            Some(fs::canonicalize(directory.path()).unwrap())
        );
    }
}
