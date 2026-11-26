//! NVMe Identify Controller / SMART-Health log parsing and the admin-command ioctl.

use crate::attrs::NVME_ROWS;
use crate::model::{Attribute, Health, NvmeHealth};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NvmeIdentify {
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub version: Option<String>,
    pub features: Vec<String>,
}

fn ascii(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim().trim_matches('\0').trim().to_string()
}

fn le_u64_sat(b: &[u8]) -> u64 {
    // 128-bit little-endian counter, saturated to u64.
    let lo = u64::from_le_bytes(b[0..8].try_into().unwrap());
    let hi = u64::from_le_bytes(b[8..16].try_into().unwrap());
    if hi != 0 {
        u64::MAX
    } else {
        lo
    }
}

pub fn parse_identify_ctrl(b: &[u8]) -> Option<NvmeIdentify> {
    if b.len() < 4096 {
        return None;
    }
    let model = ascii(&b[24..64]);
    if model.is_empty() {
        return None;
    }
    let ver = u32::from_le_bytes(b[80..84].try_into().unwrap());
    let version = (ver != 0).then(|| {
        let (maj, min, ter) = (ver >> 16, (ver >> 8) & 0xFF, ver & 0xFF);
        if ter != 0 {
            format!("NVM Express {maj}.{min}.{ter}")
        } else {
            format!("NVM Express {maj}.{min}")
        }
    });
    let mut features = vec!["S.M.A.R.T.".to_string()];
    let oncs = u16::from_le_bytes([b[520], b[521]]);
    if oncs & (1 << 2) != 0 {
        features.push("TRIM".into());
    }
    if b[525] & 1 != 0 {
        features.push("VolatileWriteCache".into());
    }
    Some(NvmeIdentify { model, serial: ascii(&b[4..24]), firmware: ascii(&b[64..72]), version, features })
}

pub fn parse_health_log(b: &[u8]) -> Option<NvmeHealth> {
    if b.len() < 512 {
        return None;
    }
    let sensors = (0..8)
        .map(|i| u16::from_le_bytes([b[200 + i * 2], b[201 + i * 2]]))
        .filter(|&k| k != 0)
        .collect();
    Some(NvmeHealth {
        critical_warning: b[0],
        composite_temp_k: u16::from_le_bytes([b[1], b[2]]),
        available_spare: b[3],
        available_spare_threshold: b[4],
        percentage_used: b[5],
        data_units_read: le_u64_sat(&b[32..48]),
        data_units_written: le_u64_sat(&b[48..64]),
        host_read_commands: le_u64_sat(&b[64..80]),
        host_write_commands: le_u64_sat(&b[80..96]),
        controller_busy_time: le_u64_sat(&b[96..112]),
        power_cycles: le_u64_sat(&b[112..128]),
        power_on_hours: le_u64_sat(&b[128..144]),
        unsafe_shutdowns: le_u64_sat(&b[144..160]),
        media_errors: le_u64_sat(&b[160..176]),
        error_log_entries: le_u64_sat(&b[176..192]),
        warning_temp_time: u32::from_le_bytes(b[192..196].try_into().unwrap()),
        critical_temp_time: u32::from_le_bytes(b[196..200].try_into().unwrap()),
        temp_sensors_k: sensors,
    })
}

/// One NVMe data unit = 1000 * 512 bytes.
pub fn data_units_to_gb(units: u64) -> u64 {
    units.saturating_mul(512_000) / 1_000_000_000
}

pub fn temp_c(h: &NvmeHealth) -> Option<i32> {
    (h.composite_temp_k > 0).then(|| h.composite_temp_k as i32 - 273)
}

/// Rows for the attribute table, using CrystalDiskInfo's NVMe ID layout.
pub fn rows(h: &NvmeHealth) -> Vec<Attribute> {
    let vals: [u64; 15] = [
        h.critical_warning as u64,
        h.composite_temp_k as u64,
        h.available_spare as u64,
        h.available_spare_threshold as u64,
        h.percentage_used as u64,
        h.data_units_read,
        h.data_units_written,
        h.host_read_commands,
        h.host_write_commands,
        h.controller_busy_time,
        h.power_cycles,
        h.power_on_hours,
        h.unsafe_shutdowns,
        h.media_errors,
        h.error_log_entries,
    ];
    NVME_ROWS
        .iter()
        .zip(vals)
        .map(|(&(id, name), raw)| Attribute {
            id,
            name: name.to_string(),
            current: None,
            worst: None,
            threshold: None,
            raw,
            raw_hex: format!("{raw:012X}"),
            status: Health::Unknown,
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub mod ioctl {
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;

    #[repr(C)]
    #[derive(Default)]
    struct NvmePassthruCmd {
        opcode: u8,
        flags: u8,
        rsvd1: u16,
        nsid: u32,
        cdw2: u32,
        cdw3: u32,
        metadata: u64,
        addr: u64,
        metadata_len: u32,
        data_len: u32,
        cdw10: u32,
        cdw11: u32,
        cdw12: u32,
        cdw13: u32,
        cdw14: u32,
        cdw15: u32,
        timeout_ms: u32,
        result: u32,
    }

    // _IOWR('N', 0x41, struct nvme_admin_cmd), sizeof == 72
    const NVME_IOCTL_ADMIN_CMD: libc::c_ulong = 0xC048_4E41;

    fn admin(f: &File, opcode: u8, nsid: u32, cdw10: u32, buf: &mut [u8]) -> io::Result<()> {
        let mut cmd = NvmePassthruCmd {
            opcode,
            nsid,
            addr: buf.as_mut_ptr() as u64,
            data_len: buf.len() as u32,
            cdw10,
            timeout_ms: 5000,
            ..Default::default()
        };
        // SAFETY: cmd and buf outlive the call; layout matches the kernel UAPI struct.
        let r = unsafe { libc::ioctl(f.as_raw_fd(), NVME_IOCTL_ADMIN_CMD as _, &mut cmd as *mut NvmePassthruCmd) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        if r > 0 {
            return Err(io::Error::other(format!("NVMe status 0x{r:X}")));
        }
        Ok(())
    }

    pub fn identify_controller(f: &File) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; 4096];
        admin(f, 0x06, 0, 1, &mut buf)?;
        Ok(buf)
    }

    pub fn smart_log(f: &File) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; 512];
        let numd = (512 / 4 - 1) as u32;
        admin(f, 0x02, 0xFFFF_FFFF, 0x02 | (numd << 16), &mut buf)?;
        Ok(buf)
    }

    #[cfg(test)]
    #[test]
    fn struct_size() {
        assert_eq!(std::mem::size_of::<NvmePassthruCmd>(), 72);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{nvme_health_page, nvme_identify_page, NvmeSpec};

    #[test]
    fn parse_pages() {
        let id = parse_identify_ctrl(&nvme_identify_page("Samsung SSD 990 EVO Plus 4TB", "S7U9NU0Y926089N", "2B2QKXG7", 0x0002_0000)).unwrap();
        assert_eq!(id.model, "Samsung SSD 990 EVO Plus 4TB");
        assert_eq!(id.version.as_deref(), Some("NVM Express 2.0"));
        let h = parse_health_log(&nvme_health_page(&NvmeSpec {
            critical_warning: 0,
            temp_c: 29,
            spare: 100,
            spare_thr: 10,
            used: 4,
            units_read: 285_732_000,
            units_written: 296_921_000,
            power_cycles: 2339,
            power_on_hours: 2463,
            unsafe_shutdowns: 61,
            media_errors: 0,
        }))
        .unwrap();
        assert_eq!(temp_c(&h), Some(29));
        assert_eq!(h.power_on_hours, 2463);
        assert_eq!(data_units_to_gb(h.data_units_written), 152_023);
        assert_eq!(rows(&h).len(), 15);
        assert_eq!(h.temp_sensors_k.len(), 2);
    }
}
