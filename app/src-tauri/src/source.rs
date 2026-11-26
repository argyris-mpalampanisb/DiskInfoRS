//! Where disk data comes from: demo pages, in-process (already root on Linux, always on
//! macOS), a privileged helper started through pkexec (Linux), or unprivileged
//! sysfs-only (read-only, Linux).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout};
#[cfg(target_os = "linux")]
use std::{
    env, fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::PathBuf,
    process::{Command, Stdio},
};

use serde::Deserialize;
use smart_core::Disk;

pub struct Helper {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub enum Source {
    Demo,
    Direct,
    Helper(Helper),
    ReadOnly,
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    disks: Vec<Disk>,
    error: Option<String>,
}

impl Source {
    pub fn initial(demo: bool) -> Source {
        if demo {
            Source::Demo
        } else if smart_core::is_privileged() {
            Source::Direct
        } else {
            Source::ReadOnly
        }
    }

    pub fn mode(&self) -> &'static str {
        match self {
            Source::Demo => "demo",
            Source::Direct if cfg!(target_os = "macos") => "native",
            Source::Direct => "root",
            Source::Helper(_) => "elevated",
            Source::ReadOnly => "readonly",
        }
    }

    pub fn scan(&mut self) -> Result<Vec<Disk>, String> {
        match self {
            Source::Demo => Ok(smart_core::demo::disks()),
            Source::Direct => Ok(smart_core::scan(true)),
            Source::ReadOnly => Ok(smart_core::scan(false)),
            Source::Helper(h) => match h.scan() {
                Ok(d) => Ok(d),
                Err(e) => {
                    // Helper died (e.g. killed); fall back so the UI keeps working.
                    *self = Source::ReadOnly;
                    Err(format!("Privileged helper stopped: {e}"))
                }
            },
        }
    }
}

impl Helper {
    fn scan(&mut self) -> Result<Vec<Disk>, String> {
        writeln!(self.stdin, "scan").and_then(|_| self.stdin.flush()).map_err(|e| e.to_string())?;
        let mut line = String::new();
        if self.stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("helper exited".into());
        }
        let r: Reply = serde_json::from_str(&line).map_err(|e| format!("bad helper reply: {e}"))?;
        if r.ok {
            Ok(r.disks)
        } else {
            Err(r.error.unwrap_or_else(|| "helper error".into()))
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "linux")]
fn bundled_helper() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let p = exe.parent().ok_or("no exe dir")?.join("diskinfo-helper");
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("helper not found at {}", p.display()))
    }
}

/// Inside an AppImage the program runs from a FUSE mount that root cannot read, so the
/// helper is copied to a private per-user runtime directory first.
#[cfg(target_os = "linux")]
fn runnable_helper() -> Result<PathBuf, String> {
    let src = bundled_helper()?;
    if env::var_os("APPIMAGE").is_none() {
        return Ok(src);
    }
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let base = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join(format!("diskinfors-{uid}"));
    if !dir.exists() {
        fs::DirBuilder::new().mode(0o700).create(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let meta = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(format!("refusing to use insecure directory {}", dir.display()));
    }
    let dst = dir.join("diskinfo-helper");
    let tmp = dir.join("diskinfo-helper.tmp");
    fs::copy(&src, &tmp).map_err(|e| format!("copy helper: {e}"))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dst).map_err(|e| e.to_string())?;
    Ok(dst)
}

/// macOS reads S.M.A.R.T. without root, so there is nothing to elevate.
#[cfg(not(target_os = "linux"))]
pub fn start_helper() -> Result<Helper, String> {
    Err("Elevation is only needed on Linux.".into())
}

/// Starts the helper as root via pkexec (prompts for a password through the desktop's
/// polkit agent). Blocks until the helper is ready or authorization fails.
#[cfg(target_os = "linux")]
pub fn start_helper() -> Result<Helper, String> {
    let helper = runnable_helper()?;
    let mut child = Command::new("pkexec")
        .arg(&helper)
        .arg("--serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "pkexec not found. Install polkit, or start DiskInfoRS with sudo.".to_string()
            } else {
                format!("pkexec: {e}")
            }
        })?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let mut line = String::new();
    let n = stdout.read_line(&mut line).map_err(|e| e.to_string())?;
    if n == 0 || !line.contains("\"ready\":true") {
        let status = child.wait().ok().and_then(|s| s.code());
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut e, &mut err);
        }
        return Err(match status {
            Some(126) => "Authorization was cancelled.".into(),
            Some(127) => format!("Not authorized (no polkit agent running?) {}", err.trim()),
            _ => format!("Helper failed to start (exit {status:?}) {}", err.trim()),
        });
    }
    Ok(Helper { child, stdin, stdout })
}
