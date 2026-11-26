//! SCSI generic (SG_IO) transport with SAT ATA PASS-THROUGH (16) and (12) commands.
//! Works for native SATA (libata), SAS HBAs that implement SAT, and most USB-SATA bridges.

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;

#[repr(C)]
struct SgIoHdr {
    interface_id: libc::c_int,
    dxfer_direction: libc::c_int,
    cmd_len: u8,
    mx_sb_len: u8,
    iovec_count: u16,
    dxfer_len: u32,
    dxferp: *mut libc::c_void,
    cmdp: *const u8,
    sbp: *mut u8,
    timeout: u32,
    flags: u32,
    pack_id: libc::c_int,
    usr_ptr: *mut libc::c_void,
    status: u8,
    masked_status: u8,
    msg_status: u8,
    sb_len_wr: u8,
    host_status: u16,
    driver_status: u16,
    resid: libc::c_int,
    duration: u32,
    info: u32,
}

const SG_IO: libc::c_ulong = 0x2285;
const SG_DXFER_NONE: libc::c_int = -1;
const SG_DXFER_FROM_DEV: libc::c_int = -3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassThrough {
    Sat16,
    Sat12,
}

/// ATA task-file input for a 28-bit command.
#[derive(Clone, Copy, Default)]
pub struct AtaCmd {
    pub command: u8,
    pub feature: u8,
    pub count: u8,
    pub lba_low: u8,
    pub lba_mid: u8,
    pub lba_high: u8,
}

pub const IDENTIFY: AtaCmd = AtaCmd { command: 0xEC, feature: 0, count: 1, lba_low: 0, lba_mid: 0, lba_high: 0 };
pub const SMART_READ_DATA: AtaCmd = AtaCmd { command: 0xB0, feature: 0xD0, count: 1, lba_low: 0, lba_mid: 0x4F, lba_high: 0xC2 };
pub const SMART_READ_THRESHOLDS: AtaCmd = AtaCmd { command: 0xB0, feature: 0xD1, count: 1, lba_low: 1, lba_mid: 0x4F, lba_high: 0xC2 };
pub const SMART_RETURN_STATUS: AtaCmd = AtaCmd { command: 0xB0, feature: 0xDA, count: 0, lba_low: 0, lba_mid: 0x4F, lba_high: 0xC2 };

fn build_cdb(pt: PassThrough, c: &AtaCmd, data_in: bool) -> Vec<u8> {
    // protocol: 4 = PIO Data-In, 3 = Non-data
    let protocol: u8 = if data_in { 4 } else { 3 };
    // T_DIR=1 (from device), BYTE_BLOCK=1, T_LENGTH=2 (sector count field); CK_COND for non-data
    let flags: u8 = if data_in { 0x0E } else { 0x20 };
    match pt {
        PassThrough::Sat16 => vec![
            0x85, protocol << 1, flags, 0, c.feature, 0, c.count, 0, c.lba_low, 0, c.lba_mid, 0, c.lba_high, 0, c.command, 0,
        ],
        PassThrough::Sat12 => vec![0xA1, protocol << 1, flags, c.feature, c.count, c.lba_low, c.lba_mid, c.lba_high, 0, c.command, 0, 0],
    }
}

/// Executes an ATA command. Returns the sense buffer on success.
fn exec(f: &File, pt: PassThrough, cmd: &AtaCmd, data: Option<&mut [u8]>) -> io::Result<[u8; 32]> {
    let data_in = data.is_some();
    let cdb = build_cdb(pt, cmd, data_in);
    let mut sense = [0u8; 32];
    let (ptr, len) = match data {
        Some(d) => (d.as_mut_ptr() as *mut libc::c_void, d.len() as u32),
        None => (std::ptr::null_mut(), 0),
    };
    let mut hdr = SgIoHdr {
        interface_id: b'S' as libc::c_int,
        dxfer_direction: if data_in { SG_DXFER_FROM_DEV } else { SG_DXFER_NONE },
        cmd_len: cdb.len() as u8,
        mx_sb_len: sense.len() as u8,
        iovec_count: 0,
        dxfer_len: len,
        dxferp: ptr,
        cmdp: cdb.as_ptr(),
        sbp: sense.as_mut_ptr(),
        timeout: 10_000,
        flags: 0,
        pack_id: 0,
        usr_ptr: std::ptr::null_mut(),
        status: 0,
        masked_status: 0,
        msg_status: 0,
        sb_len_wr: 0,
        host_status: 0,
        driver_status: 0,
        resid: 0,
        duration: 0,
        info: 0,
    };
    // SAFETY: all pointers in hdr reference live buffers for the duration of the call.
    let r = unsafe { libc::ioctl(f.as_raw_fd(), SG_IO as _, &mut hdr as *mut SgIoHdr) };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }
    if hdr.host_status != 0 {
        return Err(io::Error::other(format!("SG_IO host status 0x{:X}", hdr.host_status)));
    }
    // CHECK CONDITION is expected with CK_COND / descriptor sense; reject real errors.
    if hdr.status != 0 {
        let key = sense_key(&sense);
        if !(key == 0 || key == 1) {
            let (asc, ascq) = asc_ascq(&sense);
            return Err(io::Error::other(format!("SCSI sense key {key:X} ASC {asc:02X}/{ascq:02X}")));
        }
    }
    Ok(sense)
}

fn sense_key(s: &[u8]) -> u8 {
    match s[0] & 0x7F {
        0x72 | 0x73 => s[1] & 0x0F,
        0x70 | 0x71 => s[2] & 0x0F,
        _ => 0,
    }
}

fn asc_ascq(s: &[u8]) -> (u8, u8) {
    match s[0] & 0x7F {
        0x72 | 0x73 => (s[2], s[3]),
        0x70 | 0x71 => (s[12], s[13]),
        _ => (0, 0),
    }
}

pub fn read_512(f: &File, pt: PassThrough, cmd: &AtaCmd) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; 512];
    exec(f, pt, cmd, Some(&mut buf))?;
    if buf.iter().all(|&b| b == 0) {
        return Err(io::Error::other("device returned an empty page"));
    }
    Ok(buf)
}

/// Finds a pass-through variant that answers IDENTIFY. Returns it with the IDENTIFY page.
pub fn probe(f: &File) -> io::Result<(PassThrough, Vec<u8>)> {
    let mut last = io::Error::other("no ATA pass-through");
    for pt in [PassThrough::Sat16, PassThrough::Sat12] {
        match read_512(f, pt, &IDENTIFY) {
            Ok(b) => return Ok((pt, b)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// SMART RETURN STATUS: `Some(true)` if the drive reports a threshold exceeded condition.
pub fn smart_threshold_exceeded(f: &File, pt: PassThrough) -> Option<bool> {
    let s = exec(f, pt, &SMART_RETURN_STATUS, None).ok()?;
    // Descriptor sense with ATA Status Return descriptor (code 0x09) at offset 8.
    if (s[0] & 0x7F) == 0x72 && s[8] == 0x09 {
        let (mid, high) = (s[8 + 9], s[8 + 11]);
        return match (mid, high) {
            (0x4F, 0xC2) => Some(false),
            (0xF4, 0x2C) => Some(true),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdb_layout() {
        let c = build_cdb(PassThrough::Sat16, &SMART_READ_DATA, true);
        assert_eq!(c, [0x85, 0x08, 0x0E, 0, 0xD0, 0, 1, 0, 0, 0, 0x4F, 0, 0xC2, 0, 0xB0, 0]);
        let c = build_cdb(PassThrough::Sat12, &IDENTIFY, true);
        assert_eq!(c.len(), 12);
        assert_eq!(c[9], 0xEC);
    }

    #[test]
    fn hdr_size() {
        // sizeof(sg_io_hdr_t) on 64-bit Linux
        #[cfg(target_pointer_width = "64")]
        assert_eq!(std::mem::size_of::<SgIoHdr>(), 88);
    }
}
