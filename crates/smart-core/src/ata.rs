//! Parsing of ATA IDENTIFY DEVICE data and S.M.A.R.T. READ DATA / READ THRESHOLDS pages.

use crate::attrs::{ata_attribute_name, detect_vendor, host_io_attributes, Vendor};
use crate::model::{Attribute, Health};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identify {
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub sectors: u64,
    pub rotation_rate: Option<u16>,
    pub sata_max_gen: Option<u8>,
    pub sata_cur_gen: Option<u8>,
    pub standard: Option<String>,
    pub features: Vec<String>,
    pub smart_supported: bool,
    pub smart_enabled: bool,
}

fn word(buf: &[u8], w: usize) -> u16 {
    u16::from_le_bytes([buf[w * 2], buf[w * 2 + 1]])
}

/// ATA strings are stored with the two bytes of each word swapped.
fn ata_string(buf: &[u8], first_word: usize, last_word: usize) -> String {
    let mut out = Vec::with_capacity((last_word - first_word + 1) * 2);
    for w in first_word..=last_word {
        out.push(buf[w * 2 + 1]);
        out.push(buf[w * 2]);
    }
    String::from_utf8_lossy(&out).trim().trim_matches('\0').trim().to_string()
}

/// A 512-byte page with an integrity byte is valid when all bytes sum to 0 mod 256.
pub fn checksum_ok(buf: &[u8]) -> bool {
    buf.len() >= 512 && buf[..512].iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

/// Field present if the "word is valid" pattern (bit 15 = 0, bit 14 = 1) is set.
fn valid_word(w: u16) -> bool {
    w != 0xFFFF && (w & 0xC000) == 0x4000
}

pub fn parse_identify(buf: &[u8]) -> Option<Identify> {
    if buf.len() < 512 {
        return None;
    }
    // Word 255: integrity word; if signature 0xA5 is present the checksum must hold.
    if buf[510] == 0xA5 && !checksum_ok(buf) {
        return None;
    }
    let mut id = Identify {
        serial: ata_string(buf, 10, 19),
        firmware: ata_string(buf, 23, 26),
        model: ata_string(buf, 27, 46),
        ..Default::default()
    };
    if id.model.is_empty() {
        return None;
    }

    let w83 = word(buf, 83);
    let lba48 = valid_word(w83) && (w83 & (1 << 10)) != 0;
    id.sectors = if lba48 {
        (0..4).fold(0u64, |acc, i| acc | ((word(buf, 100 + i) as u64) << (16 * i)))
    } else {
        word(buf, 60) as u64 | ((word(buf, 61) as u64) << 16)
    };

    let rr = word(buf, 217);
    id.rotation_rate = match rr {
        1 => Some(1),
        0x0401..=0xFFFE => Some(rr),
        _ => None,
    };

    let w76 = word(buf, 76);
    if w76 != 0 && w76 != 0xFFFF {
        id.sata_max_gen = if w76 & (1 << 3) != 0 {
            Some(3)
        } else if w76 & (1 << 2) != 0 {
            Some(2)
        } else if w76 & (1 << 1) != 0 {
            Some(1)
        } else {
            None
        };
        let w77 = word(buf, 77);
        if w77 != 0xFFFF {
            let cur = ((w77 >> 1) & 0x7) as u8;
            if (1..=3).contains(&cur) {
                id.sata_cur_gen = Some(cur);
            }
        }
    }

    // Major version (word 80) and transport (word 222).
    let w80 = word(buf, 80);
    let ata = if w80 != 0 && w80 != 0xFFFF {
        [(12, "ACS-5"), (11, "ACS-4"), (10, "ACS-3"), (9, "ACS-2"), (8, "ATA8-ACS"), (7, "ATA/ATAPI-7"), (6, "ATA/ATAPI-6")]
            .iter()
            .find(|(bit, _)| w80 & (1 << bit) != 0)
            .map(|(_, s)| *s)
    } else {
        None
    };
    let w222 = word(buf, 222);
    let sata = if (w222 >> 12) == 0x1 {
        [(10, "SATA 3.5"), (9, "SATA 3.4"), (8, "SATA 3.3"), (7, "SATA 3.2"), (6, "SATA 3.1"), (5, "SATA 3.0"), (4, "SATA 2.6"), (3, "SATA 2.5"), (2, "SATA II: Extensions"), (1, "SATA 1.0a")]
            .iter()
            .find(|(bit, _)| w222 & (1 << bit) != 0)
            .map(|(_, s)| *s)
    } else {
        None
    };
    id.standard = match (ata, sata) {
        (Some(a), Some(s)) => Some(format!("{a} | {s}")),
        (Some(a), None) => Some(a.to_string()),
        (None, Some(s)) => Some(s.to_string()),
        _ => None,
    };

    let w82 = word(buf, 82);
    let w85 = word(buf, 85);
    let w87 = word(buf, 87);
    id.smart_supported = valid_word(w83) && (w82 & 1) != 0;
    id.smart_enabled = id.smart_supported && (!valid_word(w87) || (w85 & 1) != 0);
    let mut f = Vec::new();
    if id.smart_supported {
        f.push("S.M.A.R.T.");
    }
    if valid_word(w83) && w83 & (1 << 3) != 0 {
        f.push("APM");
    }
    if valid_word(w83) && w83 & (1 << 9) != 0 {
        f.push("AAM");
    }
    if lba48 {
        f.push("48bit LBA");
    }
    if w76 != 0xFFFF && w76 & (1 << 8) != 0 {
        f.push("NCQ");
    }
    if word(buf, 169) & 1 != 0 {
        f.push("TRIM");
    }
    if word(buf, 78) & (1 << 8) != 0 && w76 != 0xFFFF {
        f.push("DevSleep");
    }
    if valid_word(w83) && w82 & (1 << 5) != 0 {
        f.push("VolatileWriteCache");
    }
    id.features = f.into_iter().map(String::from).collect();
    Some(id)
}

pub fn sata_gen_label(gen: u8) -> &'static str {
    match gen {
        1 => "SATA/150",
        2 => "SATA/300",
        3 => "SATA/600",
        _ => "----",
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawAttr {
    pub id: u8,
    pub flags: u16,
    pub current: u8,
    pub worst: u8,
    pub raw: [u8; 6],
}

pub fn parse_smart_values(buf: &[u8]) -> Vec<RawAttr> {
    let mut out = Vec::new();
    if buf.len() < 362 {
        return out;
    }
    for i in 0..30 {
        let e = &buf[2 + i * 12..2 + (i + 1) * 12];
        if e[0] == 0 {
            continue;
        }
        out.push(RawAttr {
            id: e[0],
            flags: u16::from_le_bytes([e[1], e[2]]),
            current: e[3],
            worst: e[4],
            raw: [e[5], e[6], e[7], e[8], e[9], e[10]],
        });
    }
    out
}

pub fn parse_smart_thresholds(buf: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    if buf.len() < 362 {
        return out;
    }
    for i in 0..30 {
        let e = &buf[2 + i * 12..2 + (i + 1) * 12];
        if e[0] != 0 {
            out.push((e[0], e[1]));
        }
    }
    out
}

fn raw_u64(raw: &[u8; 6]) -> u64 {
    raw.iter().rev().fold(0u64, |acc, b| (acc << 8) | *b as u64)
}

fn raw_hex(raw: &[u8; 6]) -> String {
    raw.iter().rev().map(|b| format!("{b:02X}")).collect()
}

/// Derived values computed from an ATA attribute set.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AtaDerived {
    pub temperature_c: Option<i32>,
    pub power_on_hours: Option<u64>,
    pub power_on_count: Option<u64>,
    pub host_writes_gb: Option<u64>,
    pub host_reads_gb: Option<u64>,
    pub nand_writes_gb: Option<u64>,
}

pub fn build_attributes(model: &str, is_ssd: bool, values: &[RawAttr], thresholds: &[(u8, u8)]) -> (Vec<Attribute>, AtaDerived) {
    let vendor = detect_vendor(model, is_ssd);
    let attrs: Vec<Attribute> = values
        .iter()
        .map(|v| {
            let thr = thresholds.iter().find(|(id, _)| *id == v.id).map(|(_, t)| *t);
            Attribute {
                id: v.id,
                name: ata_attribute_name(vendor, v.id),
                current: Some(v.current),
                worst: Some(v.worst),
                threshold: thr,
                raw: raw_u64(&v.raw),
                raw_hex: raw_hex(&v.raw),
                status: Health::Unknown,
            }
        })
        .collect();

    let get = |id: u8| values.iter().find(|v| v.id == id);
    let mut d = AtaDerived::default();

    for id in [0xC2u8, 0xBE, 0xE7] {
        if id == 0xE7 && is_ssd {
            continue; // E7 is "life left" on SSDs
        }
        if let Some(a) = get(id) {
            let t = a.raw[0] as i32;
            if (1..=120).contains(&t) {
                d.temperature_c = Some(t);
                break;
            }
        }
    }
    if let Some(a) = get(0x09) {
        d.power_on_hours = Some(u32::from_le_bytes([a.raw[0], a.raw[1], a.raw[2], a.raw[3]]) as u64);
    }
    if let Some(a) = get(0x0C) {
        d.power_on_count = Some(u32::from_le_bytes([a.raw[0], a.raw[1], a.raw[2], a.raw[3]]) as u64);
    }
    for &(w, r, unit) in host_io_attributes(vendor) {
        if let Some(a) = get(w) {
            d.host_writes_gb = Some(raw_u64(&a.raw).saturating_mul(unit) / 1_000_000_000);
            if let Some(ra) = r.and_then(get) {
                d.host_reads_gb = Some(raw_u64(&ra.raw).saturating_mul(unit) / 1_000_000_000);
            }
            break;
        }
    }
    let nand = match vendor {
        Vendor::Intel => get(0xF9).map(|a| raw_u64(&a.raw) * (1 << 30)),
        Vendor::Kingston | Vendor::Kioxia | Vendor::WesternDigital => get(0xE9).map(|a| raw_u64(&a.raw) * (1 << 30)),
        _ => None,
    };
    d.nand_writes_gb = nand.map(|b| b / 1_000_000_000);
    (attrs, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{identify_page, smart_pages};

    #[test]
    fn identify_roundtrip() {
        let buf = identify_page("TOSHIBA MQ04UBB400", "357DT1W9T", "JU003U", 7_814_037_168, 5400, 3, 3);
        assert!(checksum_ok(&buf));
        let id = parse_identify(&buf).unwrap();
        assert_eq!(id.model, "TOSHIBA MQ04UBB400");
        assert_eq!(id.serial, "357DT1W9T");
        assert_eq!(id.firmware, "JU003U");
        assert_eq!(id.sectors, 7_814_037_168);
        assert_eq!(id.rotation_rate, Some(5400));
        assert_eq!(id.sata_max_gen, Some(3));
        assert!(id.features.iter().any(|f| f == "S.M.A.R.T."));
        assert!(id.features.iter().any(|f| f == "48bit LBA"));
        assert!(id.smart_supported);
    }

    #[test]
    fn bad_checksum_rejected() {
        let mut buf = identify_page("X", "1", "1", 1000, 1, 3, 3);
        buf[100] ^= 0xFF;
        assert!(parse_identify(&buf).is_none());
    }

    #[test]
    fn smart_page_parse() {
        let (vals, thr) = smart_pages(&[
            (0x05, 100, 100, 50, 0),
            (0x09, 100, 100, 0, 183),
            (0xC2, 100, 100, 0, 0x002D_0012_001E), // 30 C, min/max in high bytes
        ]);
        assert!(checksum_ok(&vals));
        let v = parse_smart_values(&vals);
        let t = parse_smart_thresholds(&thr);
        assert_eq!(v.len(), 3);
        let (attrs, d) = build_attributes("TOSHIBA MQ04UBB400", false, &v, &t);
        assert_eq!(attrs[0].name, "Reallocated Sectors Count");
        assert_eq!(attrs[0].threshold, Some(50));
        assert_eq!(attrs[2].raw_hex, "002D0012001E");
        assert_eq!(d.temperature_c, Some(30));
        assert_eq!(d.power_on_hours, Some(183));
    }

    #[test]
    fn samsung_host_writes() {
        // 1_000_000_000 LBAs * 512 = 512 GB
        let (vals, thr) = smart_pages(&[(0xF1, 99, 99, 0, 1_000_000_000), (0xB1, 97, 97, 0, 30)]);
        let (_, d) = build_attributes("Samsung SSD 870 EVO 1TB", true, &parse_smart_values(&vals), &parse_smart_thresholds(&thr));
        assert_eq!(d.host_writes_gb, Some(512));
    }
}
