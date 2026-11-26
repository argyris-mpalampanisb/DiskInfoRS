//! S.M.A.R.T. history in CrystalDiskInfo's on-disk format:
//!   Smart/<Model><Serial>/Smart.ini      FIRST / latest / THRESHOLD sections
//!   Smart/<Model><Serial>/<Series>.csv   "YYYY/MM/DD HH:MM:SS,value", appended on change
//! An existing CrystalDiskInfo `Smart` folder can be copied in and keeps working.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use smart_core::{Disk, DiskKind};

use crate::ini::Ini;

fn now() -> String {
    chrono::Local::now().format("%Y/%m/%d %H:%M:%S").to_string()
}

fn computer_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: buf is writable for its full length; gethostname NUL-terminates on success
    // (the last byte stays 0 even if the name is truncated).
    if unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len() - 1) } != 0 {
        return String::new();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
    // macOS reports "name.local"; CrystalDiskInfo stores the short computer name.
    name.strip_suffix(".local").map(str::to_string).unwrap_or(name)
}

/// (series name, value) pairs recorded for a disk, matching CDI's naming.
fn series(d: &Disk) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = Vec::new();
    v.push(("HealthStatus".into(), d.health.cdi_code() as u64));
    if let Some(t) = d.temperature_c.filter(|t| *t > 0) {
        v.push(("Temperature".into(), t as u64));
    }
    if let Some(x) = d.power_on_hours {
        v.push(("PowerOnHours".into(), x));
    }
    if let Some(x) = d.power_on_count {
        v.push(("PowerOnCount".into(), x));
    }
    if let Some(x) = d.health_percent {
        v.push(("Life".into(), x as u64));
    }
    if let Some(x) = d.host_writes_gb {
        v.push(("HostWrites".into(), x));
    }
    if let Some(x) = d.host_reads_gb {
        v.push(("HostReads".into(), x));
    }
    if let Some(x) = d.nand_writes_gb {
        v.push(("NandWrites".into(), x));
    }
    if d.kind == Some(DiskKind::Nvme) {
        if let Some(h) = &d.nvme {
            v.push(("ReallocatedSectorsCount".into(), 100u64.saturating_sub(h.available_spare as u64)));
        }
        return v;
    }
    let raw = |id: u8| d.attributes.iter().find(|a| a.id == id).map(|a| a.raw & 0xFFFF_FFFF);
    for (id, name) in [(0x05, "ReallocatedSectorsCount"), (0xC4, "ReallocationEventCount"), (0xC5, "CurrentPendingSectorCount"), (0xC6, "UncorrectableSectorCount")] {
        if let Some(r) = raw(id) {
            v.push((name.into(), r));
        }
    }
    for a in &d.attributes {
        if let Some(c) = a.current {
            v.push((format!("{:02X}", a.id), c as u64));
        }
    }
    v
}

fn last_value(csv: &Path) -> Option<String> {
    let s = fs::read_to_string(csv).ok()?;
    s.lines().rev().find(|l| !l.trim().is_empty())?.split(',').nth(1).map(|v| v.trim().to_string())
}

/// Records the current state of every disk with readable S.M.A.R.T. data.
pub fn record(smart_root: &Path, disks: &[Disk]) -> std::io::Result<()> {
    let date = now();
    for d in disks.iter().filter(|d| d.smart_available && !d.serial.is_empty()) {
        let key = d.history_key();
        let dir = smart_root.join(&key);
        fs::create_dir_all(&dir)?;
        let ini_path = dir.join("Smart.ini");
        let mut ini = Ini::load(&ini_path);
        ini.set("PC", "ComputerName", computer_name());
        let first = format!("{key}FIRST");
        let s = series(d);
        let new_first = !ini.has_section(&first);
        for sect in [Some(key.as_str()), new_first.then_some(first.as_str())].into_iter().flatten() {
            ini.set(sect, "Date", &date);
            for (name, value) in &s {
                ini.set(sect, name, value);
            }
        }
        let thr = format!("{key}THRESHOLD");
        for a in &d.attributes {
            if let Some(t) = a.threshold {
                ini.set(&thr, &format!("{:02X}", a.id), t);
            }
        }
        ini.save(&ini_path)?;

        for (name, value) in s.iter().filter(|(n, _)| n != "HealthStatus") {
            let csv = dir.join(format!("{name}.csv"));
            if last_value(&csv).as_deref() == Some(value.to_string().as_str()) {
                continue;
            }
            let mut f = OpenOptions::new().create(true).append(true).open(&csv)?;
            writeln!(f, "{date},{value}")?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
pub struct Point {
    pub t: i64,
    pub v: f64,
}

fn parse_date(s: &str) -> Option<i64> {
    let dt = chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y/%m/%d %H:%M:%S").ok()?;
    Some(dt.and_local_timezone(chrono::Local).earliest()?.timestamp())
}

fn disk_dir(smart_root: &Path, key: &str) -> Option<PathBuf> {
    // Never let a key escape the Smart folder.
    if key.is_empty() || key.contains('/') || key.contains("..") {
        return None;
    }
    Some(smart_root.join(key))
}

pub fn list_series(smart_root: &Path, key: &str) -> Vec<String> {
    let Some(dir) = disk_dir(smart_root, key) else { return Vec::new() };
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".csv")).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    // Named series first, then attribute IDs.
    v.sort_by_key(|n| (n.len() <= 2, n.clone()));
    v
}

pub fn read_series(smart_root: &Path, key: &str, series: &str) -> Vec<Point> {
    let valid = !series.is_empty() && series.chars().all(|c| c.is_ascii_alphanumeric());
    let Some(dir) = disk_dir(smart_root, key).filter(|_| valid) else { return Vec::new() };
    fs::read_to_string(dir.join(format!("{series}.csv")))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (d, v) = l.split_once(',')?;
            Some(Point { t: parse_date(d)?, v: v.trim().parse().ok()? })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_read() {
        let root = std::env::temp_dir().join(format!("diskinfors-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut disks = smart_core::demo::disks();
        for d in &mut disks {
            smart_core::evaluate(d, &Default::default());
        }
        record(&root, &disks).unwrap();
        record(&root, &disks).unwrap(); // unchanged values must not be appended twice
        let key = disks[2].history_key();
        assert_eq!(key, "TOSHIBA MQ04UBB400357DT1W9T");
        let pts = read_series(&root, &key, "Temperature");
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].v, 30.0);
        assert!(list_series(&root, &key).contains(&"0A".to_string()));
        assert!(read_series(&root, "../etc", "passwd").is_empty());
        let ini = Ini::load(&root.join(&key).join("Smart.ini"));
        assert_eq!(ini.get(&format!("{key}THRESHOLD"), "0A"), Some("30"));
        let _ = fs::remove_dir_all(&root);
    }
}
