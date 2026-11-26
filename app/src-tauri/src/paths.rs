//! Portable data directory resolution.
//!
//! Like CrystalDiskInfo's portable edition, settings (`DiskInfo.ini`) and history (`Smart/`)
//! live next to the program: beside the `.AppImage` file, or beside the executable.
//! If that folder is not writable we fall back to `$XDG_CONFIG_HOME/DiskInfoRS`.
//!
//! macOS: writing into a signed `.app` bundle (or littering `/Applications`) is not an
//! option, so the default is `~/Library/Application Support/DiskInfoRS`. Portable mode is
//! used when a `DiskInfo.ini` or `Smart` folder already exists next to `DiskInfoRS.app`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    if let Some(d) = env::var_os("DISKINFORS_DATA_DIR") {
        return PathBuf::from(d);
    }
    let exe_dir = env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));

    // macOS app bundle: <dir>/DiskInfoRS.app/Contents/MacOS/diskinfors
    if let Some(bundle_parent) = exe_dir.as_deref().and_then(app_bundle_parent) {
        let portable = settings_file(&bundle_parent).exists() || smart_dir(&bundle_parent).is_dir();
        if portable && writable(&bundle_parent) {
            return bundle_parent;
        }
        return user_dir();
    }

    let base = env::var_os("APPIMAGE").map(PathBuf::from).and_then(|p| p.parent().map(Path::to_path_buf)).or(exe_dir);
    if let Some(b) = base {
        if writable(&b) {
            return b;
        }
    }
    user_dir()
}

/// For `.../X.app/Contents/MacOS`, the folder that contains `X.app`.
fn app_bundle_parent(exe_dir: &Path) -> Option<PathBuf> {
    let contents = exe_dir.parent()?;
    let app = contents.parent()?;
    let is_bundle = exe_dir.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    is_bundle.then(|| app.parent().map(Path::to_path_buf)).flatten()
}

/// Per-user fallback: XDG config dir on Linux, Application Support on macOS.
fn user_dir() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else {
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".config")))
    };
    let d = base.unwrap_or_else(|| PathBuf::from(".")).join("DiskInfoRS");
    let _ = fs::create_dir_all(&d);
    d
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(".diskinfors-write-test");
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

pub fn settings_file(dir: &Path) -> PathBuf {
    dir.join("DiskInfo.ini")
}

pub fn smart_dir(dir: &Path) -> PathBuf {
    dir.join("Smart")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_detection() {
        assert_eq!(
            app_bundle_parent(Path::new("/Users/me/Tools/DiskInfoRS.app/Contents/MacOS")),
            Some(PathBuf::from("/Users/me/Tools"))
        );
        assert_eq!(app_bundle_parent(Path::new("/opt/diskinfors/bin")), None);
        assert_eq!(app_bundle_parent(Path::new("/tmp/x/Contents/MacOS")), None);
    }
}
