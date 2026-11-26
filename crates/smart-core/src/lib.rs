//! smart-core: S.M.A.R.T. access and health evaluation for Linux and macOS block devices.

pub mod assemble;
pub mod ata;
pub mod attrs;
pub mod demo;
pub mod health;
pub mod model;
pub mod nvme;
pub mod synth;

#[cfg(target_os = "linux")]
pub mod device;
#[cfg(target_os = "linux")]
pub mod sgio;
#[cfg(target_os = "macos")]
pub mod macos;

pub use health::{evaluate, HealthConfig};
pub use model::{Attribute, Disk, DiskKind, Health, NvmeHealth};

/// Enumerates disks and, if `with_smart`, reads S.M.A.R.T. data from each one.
/// Health is evaluated with default thresholds; callers may re-evaluate with their own.
#[cfg(target_os = "linux")]
pub fn scan(with_smart: bool) -> Vec<Disk> {
    let mut disks = device::enumerate();
    for d in &mut disks {
        if with_smart {
            device::read_smart(d);
        } else {
            d.smart_error = Some("Not elevated: S.M.A.R.T. unavailable".into());
        }
        evaluate(d, &HealthConfig::default());
    }
    disks
}

#[cfg(target_os = "macos")]
pub fn scan(with_smart: bool) -> Vec<Disk> {
    let mut disks = macos::scan(with_smart);
    for d in &mut disks {
        evaluate(d, &HealthConfig::default());
    }
    disks
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn scan(_with_smart: bool) -> Vec<Disk> {
    Vec::new()
}

/// Whether this process can read S.M.A.R.T. without elevation.
#[cfg(target_os = "linux")]
pub fn is_privileged() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// macOS exposes S.M.A.R.T. through IOKit user clients that need no root.
#[cfg(target_os = "macos")]
pub fn is_privileged() -> bool {
    true
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn is_privileged() -> bool {
    false
}
