//! Files only your user can read, in Akyba's own folder: where the trackers' tokens are kept.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

/// `name` in Akyba's folder (a test's folder, for a test).
pub fn path(name: &str) -> Option<PathBuf> {
    crate::settings::data_dir().map(|dir| dir.join(name))
}

/// What `name` holds, if it's there.
pub fn read(name: &str) -> Option<String> {
    path(name).and_then(|path| std::fs::read_to_string(path).ok())
}

/// Writes `contents` to `name` with permissions 0600, so only your user can read it.
pub fn write(name: &str, contents: &str) -> Result<(), String> {
    let path = path(name).ok_or("Akyba can't find its folder to save the token in.")?;
    let refused = |error: std::io::Error| format!("Akyba couldn't save the token: {error}");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(refused)?;
    }
    let mut file =
        std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path).map_err(refused)?;
    file.write_all(contents.as_bytes()).map_err(refused)?;
    // A file made by someone else first keeps its own permissions; make sure they're 0600.
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).map_err(refused)
}

pub fn remove(name: &str) {
    if let Some(path) = path(name) {
        let _ = std::fs::remove_file(path);
    }
}
