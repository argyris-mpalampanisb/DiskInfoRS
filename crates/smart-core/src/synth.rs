//! Builders for synthetic IDENTIFY / SMART / NVMe pages. Used by unit tests and by
//! `--demo` mode, so the demo disks run through exactly the same parsers as real hardware.

fn set_word(buf: &mut [u8], w: usize, v: u16) {
    buf[w * 2..w * 2 + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_string(buf: &mut [u8], first_word: usize, len_words: usize, s: &str) {
    let mut bytes = s.as_bytes().to_vec();
    bytes.resize(len_words * 2, b' ');
    for w in 0..len_words {
        buf[(first_word + w) * 2] = bytes[w * 2 + 1];
        buf[(first_word + w) * 2 + 1] = bytes[w * 2];
    }
}

fn seal(buf: &mut [u8]) {
    let sum = buf[..511].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    buf[511] = 0u8.wrapping_sub(sum);
}

/// `rpm == 1` means SSD.
pub fn identify_page(model: &str, serial: &str, fw: &str, sectors: u64, rpm: u16, sata_max: u8, sata_cur: u8) -> Vec<u8> {
    let mut b = vec![0u8; 512];
    put_string(&mut b, 10, 10, serial);
    put_string(&mut b, 23, 4, fw);
    put_string(&mut b, 27, 20, model);
    set_word(&mut b, 60, sectors.min(0x0FFF_FFFF) as u16);
    set_word(&mut b, 61, (sectors.min(0x0FFF_FFFF) >> 16) as u16);
    let gens = (1..=sata_max).fold(0u16, |a, g| a | (1 << g));
    set_word(&mut b, 76, gens | (1 << 8));
    set_word(&mut b, 77, (sata_cur as u16) << 1);
    set_word(&mut b, 80, 0x0F00); // up to ACS-4
    set_word(&mut b, 82, 1 | (1 << 5));
    set_word(&mut b, 83, 0x4000 | (1 << 10) | if rpm != 1 { 1 << 3 } else { 0 });
    set_word(&mut b, 84, 0x4000);
    set_word(&mut b, 85, 1 | (1 << 5));
    set_word(&mut b, 87, 0x4000);
    for i in 0..4 {
        set_word(&mut b, 100 + i, (sectors >> (16 * i)) as u16);
    }
    if rpm == 1 {
        set_word(&mut b, 169, 1);
    }
    set_word(&mut b, 217, rpm);
    set_word(&mut b, 222, 0x1000 | (1 << 5) | (1 << 6));
    b[510] = 0xA5;
    seal(&mut b);
    b
}

/// Entries are `(id, current, worst, threshold, raw48)`. Returns (values page, thresholds page).
pub fn smart_pages(entries: &[(u8, u8, u8, u8, u64)]) -> (Vec<u8>, Vec<u8>) {
    let mut v = vec![0u8; 512];
    let mut t = vec![0u8; 512];
    v[0] = 0x10;
    t[0] = 0x10;
    for (i, &(id, cur, worst, thr, raw)) in entries.iter().take(30).enumerate() {
        let o = 2 + i * 12;
        v[o] = id;
        v[o + 1] = 0x33; // flags: prefailure | online
        v[o + 3] = cur;
        v[o + 4] = worst;
        v[o + 5..o + 11].copy_from_slice(&raw.to_le_bytes()[..6]);
        t[o] = id;
        t[o + 1] = thr;
    }
    seal(&mut v);
    seal(&mut t);
    (v, t)
}

pub struct NvmeSpec {
    pub critical_warning: u8,
    pub temp_c: i32,
    pub spare: u8,
    pub spare_thr: u8,
    pub used: u8,
    pub units_read: u64,
    pub units_written: u64,
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
}

pub fn nvme_health_page(s: &NvmeSpec) -> Vec<u8> {
    let mut b = vec![0u8; 512];
    b[0] = s.critical_warning;
    b[1..3].copy_from_slice(&((s.temp_c + 273) as u16).to_le_bytes());
    b[3] = s.spare;
    b[4] = s.spare_thr;
    b[5] = s.used;
    let put = |b: &mut Vec<u8>, off: usize, v: u64| b[off..off + 8].copy_from_slice(&v.to_le_bytes());
    put(&mut b, 32, s.units_read);
    put(&mut b, 48, s.units_written);
    put(&mut b, 64, s.units_read * 20);
    put(&mut b, 80, s.units_written * 12);
    put(&mut b, 96, s.power_on_hours / 3);
    put(&mut b, 112, s.power_cycles);
    put(&mut b, 128, s.power_on_hours);
    put(&mut b, 144, s.unsafe_shutdowns);
    put(&mut b, 160, s.media_errors);
    b[200..202].copy_from_slice(&((s.temp_c + 273) as u16).to_le_bytes());
    b[202..204].copy_from_slice(&((s.temp_c + 278) as u16).to_le_bytes());
    b
}

pub fn nvme_identify_page(model: &str, serial: &str, fw: &str, version: u32) -> Vec<u8> {
    let mut b = vec![0u8; 4096];
    let pad = |s: &str, n: usize| {
        let mut v = s.as_bytes().to_vec();
        v.resize(n, b' ');
        v
    };
    b[4..24].copy_from_slice(&pad(serial, 20));
    b[24..64].copy_from_slice(&pad(model, 40));
    b[64..72].copy_from_slice(&pad(fw, 8));
    b[80..84].copy_from_slice(&version.to_le_bytes());
    b[520] = 1 << 2; // ONCS: Dataset Management (TRIM)
    b[525] = 1; // VWC present
    b
}
