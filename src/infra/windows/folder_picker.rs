use std::path::{Path, PathBuf};

pub fn pick_folder(title: &str, current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if let Some(directory) = current
        .filter(|path| path.is_dir())
        .or_else(|| current.and_then(Path::parent).filter(|path| path.is_dir()))
    {
        dialog = dialog.set_directory(directory);
    }
    dialog.pick_folder()
}

pub fn pick_file(title: &str, current: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter("浏览器安装包", &["zip", "7z", "exe", "msi"]);
    if let Some(directory) = current
        .filter(|path| path.is_dir())
        .or_else(|| current.and_then(Path::parent).filter(|path| path.is_dir()))
    {
        dialog = dialog.set_directory(directory);
    }
    dialog.pick_file()
}
