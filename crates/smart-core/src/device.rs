//! Disk enumeration via sysfs and S.M.A.R.T. reading via ioctls (Linux only).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::assemble;
use crate::model::{Disk, DiskKind};
use crate::nvme::ioctl as nvme_ioctl;
use crate::sgio;

const SKIP_PREFIXES: &[&str] = &["loop", "ram", "zram", "dm-", "md", "sr", "fd", "nbd", "zd", "drbd", "bcache"];

fn read_trim(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn read_u64(p: impl AsRef<Path>) -> Option<u64> {
    read_trim(p)?.parse().ok()
}

/// Reads the unit serial number from the SCSI VPD page 0x80 cached by the kernel.
fn vpd_serial(dev: &Path) -> Option<String> {
    let b = fs::read(dev.join("vpd_pg80")).ok()?;
    if b.len() < 4 {
        return None;
    }
    let len = u16::from_be_bytes([b[2], b[3]]) as usize;
    let s = String::from_utf8_lossy(b.get(4..4 + len.min(b.len() - 4))?).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn hwmon_temp(dirs: &[PathBuf]) -> Option<i32> {
    for dir in dirs {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name();
            if name.to_string_lossy().starts_with("hwmon") {
                if let Some(m) = read_u64(e.path().join("temp1_input")) {
                    return Some((m / 1000) as i32);
                }
            }
        }
    }
    None
}

fn pcie_gen(speed: &str) -> Option<u8> {
    let gt: f32 = speed.split_whitespace().next()?.parse().ok()?;
    Some(match gt {
        x if x < 3.0 => 1,
        x if x < 6.0 => 2,
        x if x < 10.0 => 3,
        x if x < 20.0 => 4,
        x if x < 40.0 => 5,
        _ => 6,
    })
}

fn pcie_mode(pci: &Path) -> Option<String> {
    let fmt = |speed: &str, width: &str| {
        let g = read_trim(pci.join(speed)).and_then(|s| pcie_gen(&s))?;
        let w = read_trim(pci.join(width))?;
        Some(format!("PCIe {g}.0 x{w}"))
    };
    let cur = fmt("current_link_speed", "current_link_width");
    let max = fmt("max_link_speed", "max_link_width");
    match (cur, max) {
        (Some(c), Some(m)) => Some(format!("{c} | {m}")),
        (Some(c), None) => Some(c),
        (None, Some(m)) => Some(format!("---- | {m}")),
        _ => None,
    }
}

/// Maps every whole-disk kernel name to the mount points of its partitions (and of
/// device-mapper / md devices stacked on top of it).
fn mount_map() -> BTreeMap<String, Vec<String>> {
    fn top_disks(name: &str, depth: u8, out: &mut Vec<String>) {
        if depth > 8 {
            return;
        }
        let p = Path::new("/sys/class/block").join(name);
        let slaves = p.join("slaves");
        if let Ok(rd) = fs::read_dir(&slaves) {
            let mut any = false;
            for e in rd.flatten() {
                any = true;
                top_disks(&e.file_name().to_string_lossy(), depth + 1, out);
            }
            if any {
                return;
            }
        }
        if p.join("partition").exists() {
            if let Some(parent) = fs::canonicalize(&p).ok().and_then(|c| c.parent().map(|x| x.to_path_buf())) {
                if let Some(n) = parent.file_name() {
                    out.push(n.to_string_lossy().into_owned());
                    return;
                }
            }
        }
        out.push(name.to_string());
    }
    let unescape = |s: &str| s.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\");
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut lines = fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    // Swap partitions show up as "[SWAP]" like lsblk does.
    for l in fs::read_to_string("/proc/swaps").unwrap_or_default().lines().skip(1) {
        if let Some(dev) = l.split_whitespace().next() {
            lines.push_str(&format!("\n{dev} [SWAP] swap"));
        }
    }
    for line in lines.lines() {
        let mut it = line.split_whitespace();
        let (Some(src), Some(mp)) = (it.next(), it.next()) else { continue };
        if !src.starts_with("/dev/") {
            continue;
        }
        let Ok(real) = fs::canonicalize(src) else { continue };
        let Some(name) = real.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let mut disks = Vec::new();
        top_disks(&name, 0, &mut disks);
        for disk in disks {
            let v = map.entry(disk).or_default();
            let mp = unescape(mp);
            if !v.contains(&mp) {
                v.push(mp);
            }
        }
    }
    map
}

/// Lists whole disks from sysfs without touching the devices (works unprivileged).
pub fn enumerate() -> Vec<Disk> {
    let mounts = mount_map();
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir("/sys/block") else { return out };
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort_by_key(|a| natural_key(a));
    for name in names {
        if SKIP_PREFIXES.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let sys = Path::new("/sys/block").join(&name);
        let size = read_u64(sys.join("size")).unwrap_or(0) * 512;
        if size == 0 {
            continue;
        }
        let real = fs::canonicalize(&sys).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let dev = sys.join("device");
        let mut d = Disk {
            id: name.clone(),
            dev_path: format!("/dev/{name}"),
            size_bytes: size,
            removable: read_u64(sys.join("removable")) == Some(1),
            mount_points: mounts.get(&name).cloned().unwrap_or_default(),
            ..Default::default()
        };
        let rotational = read_u64(sys.join("queue/rotational")) == Some(1);
        d.rotation_rate = (!rotational).then_some(1);

        if name.starts_with("nvme") {
            d.kind = Some(DiskKind::Nvme);
            d.interface = "NVM Express".into();
            d.model = read_trim(dev.join("model")).unwrap_or_default();
            d.serial = read_trim(dev.join("serial")).unwrap_or_default();
            d.firmware = read_trim(dev.join("firmware_rev")).unwrap_or_default();
            d.transfer_mode = pcie_mode(&dev.join("device"));
            d.temperature_c = hwmon_temp(&[dev.clone(), dev.join("device/hwmon")]);
        } else {
            let vendor = read_trim(dev.join("vendor")).unwrap_or_default();
            let model = read_trim(dev.join("model")).unwrap_or_default();
            d.model = if vendor.is_empty() || vendor == "ATA" { model } else { format!("{vendor} {model}") };
            d.firmware = read_trim(dev.join("rev")).unwrap_or_default();
            d.serial = vpd_serial(&dev).or_else(|| read_trim(dev.join("serial"))).unwrap_or_default();
            d.temperature_c = hwmon_temp(&[dev.join("hwmon"), dev.clone()]);
            let usb = real.contains("/usb");
            d.interface = if name.starts_with("mmcblk") {
                "SD/MMC".into()
            } else if name.starts_with("vd") || real.contains("/virtio") {
                "VirtIO".into()
            } else if usb {
                "USB".into()
            } else if real.contains("/ata") || vendor == "ATA" {
                "Serial ATA".into()
            } else {
                "SCSI".into()
            };
            d.kind = Some(if d.interface == "Serial ATA" { DiskKind::Ata } else { DiskKind::Other });
        }
        out.push(d);
    }
    out
}

fn natural_key(s: &str) -> (String, u64, String) {
    let prefix: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
    let rest = &s[prefix.len()..];
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    (prefix, num.parse().unwrap_or(0), rest[num.len()..].to_string())
}

fn open_dev(path: &str) -> io::Result<File> {
    OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)
}

fn describe(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::PermissionDenied => "Permission denied (needs root / CAP_SYS_RAWIO)".into(),
        _ => e.to_string(),
    }
}

/// Reads S.M.A.R.T. for one enumerated disk, filling fields in place.
pub fn read_smart(d: &mut Disk) {
    if d.interface == "SD/MMC" || d.interface == "VirtIO" {
        d.smart_error = Some(format!("{} devices do not provide S.M.A.R.T.", d.interface));
        return;
    }
    let f = match open_dev(&d.dev_path) {
        Ok(f) => f,
        Err(e) => {
            d.smart_error = Some(describe(&e));
            return;
        }
    };
    let res = if d.kind == Some(DiskKind::Nvme) { read_nvme(&f, d) } else { read_ata(&f, d) };
    if let Err(e) = res {
        d.smart_error = Some(e);
    }
}

fn read_nvme(f: &File, d: &mut Disk) -> Result<(), String> {
    let id = nvme_ioctl::identify_controller(f).ok();
    let log = nvme_ioctl::smart_log(f).map_err(|e| describe(&e))?;
    assemble::apply_nvme(d, id.as_deref(), &log)
}

fn read_ata(f: &File, d: &mut Disk) -> Result<(), String> {
    let (pt, identify) = sgio::probe(f).map_err(|e| {
        if e.kind() == io::ErrorKind::PermissionDenied {
            describe(&e)
        } else {
            format!("ATA pass-through not supported ({e})")
        }
    })?;
    let was_usb = d.interface == "USB";
    let smart = match (sgio::read_512(f, pt, &sgio::SMART_READ_DATA), sgio::read_512(f, pt, &sgio::SMART_READ_THRESHOLDS)) {
        (Ok(v), Ok(t)) => Some((v, t)),
        (Ok(v), Err(_)) => Some((v, vec![0u8; 512])),
        (Err(e), _) => {
            d.smart_error = Some(format!("SMART READ DATA failed: {}", describe(&e)));
            None
        }
    };
    let status = smart.as_ref().and_then(|_| sgio::smart_threshold_exceeded(f, pt));
    // Prefer IDENTIFY strings over the (often truncated) SCSI INQUIRY strings.
    assemble::apply_ata(d, &identify, smart.as_ref().map(|(v, t)| (v.as_slice(), t.as_slice())), status)?;
    if was_usb {
        d.interface = "USB (Serial ATA)".into();
    }
    Ok(())
}
