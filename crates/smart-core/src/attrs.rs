//! Attribute naming and vendor detection. Names follow the commonly documented
//! meanings of ATA S.M.A.R.T. attribute IDs; vendors repurpose many IDs on SSDs.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vendor {
    Generic,
    Samsung,
    Micron, // Crucial / Micron
    Intel,
    WesternDigital, // WD / SanDisk SSD
    Kingston,       // Kingston / Phison
    Kioxia,         // Toshiba / Kioxia SSD
}

pub fn detect_vendor(model: &str, is_ssd: bool) -> Vendor {
    if !is_ssd {
        return Vendor::Generic;
    }
    let m = model.to_ascii_uppercase();
    let starts = |p: &str| m.starts_with(p);
    if m.contains("SAMSUNG") || starts("MZ") {
        Vendor::Samsung
    } else if m.contains("CRUCIAL") || starts("CT") || m.contains("MICRON") || starts("MTFD") {
        Vendor::Micron
    } else if m.contains("INTEL") || starts("SSDSC") {
        Vendor::Intel
    } else if m.contains("SANDISK") || starts("WDC WDS") || starts("WDS") || starts("WD ") {
        Vendor::WesternDigital
    } else if m.contains("KINGSTON") || starts("SA400") || starts("SUV") || starts("SKC") || m.contains("PHISON") {
        Vendor::Kingston
    } else if m.contains("TOSHIBA") || m.contains("KIOXIA") || starts("THNS") {
        Vendor::Kioxia
    } else {
        Vendor::Generic
    }
}

fn generic_name(id: u8) -> Option<&'static str> {
    Some(match id {
        0x01 => "Read Error Rate",
        0x02 => "Throughput Performance",
        0x03 => "Spin-Up Time",
        0x04 => "Start/Stop Count",
        0x05 => "Reallocated Sectors Count",
        0x06 => "Read Channel Margin",
        0x07 => "Seek Error Rate",
        0x08 => "Seek Time Performance",
        0x09 => "Power-On Hours",
        0x0A => "Spin Retry Count",
        0x0B => "Recalibration Retries",
        0x0C => "Power Cycle Count",
        0x0D => "Soft Read Error Rate",
        0x16 => "Current Helium Level",
        0x17 => "Helium Condition Lower",
        0x18 => "Helium Condition Upper",
        0xAA => "Available Reserved Space",
        0xAB => "SSD Program Fail Count",
        0xAC => "SSD Erase Fail Count",
        0xAD => "SSD Wear Leveling Count",
        0xAE => "Unexpected Power Loss Count",
        0xAF => "Power Loss Protection Failure",
        0xB1 => "Wear Range Delta",
        0xB3 => "Used Reserved Block Count Total",
        0xB4 => "Unused Reserved Block Count Total",
        0xB5 => "Program Fail Count Total",
        0xB6 => "Erase Fail Count",
        0xB7 => "SATA Downshift Error Count",
        0xB8 => "End-to-End Error",
        0xBB => "Reported Uncorrectable Errors",
        0xBC => "Command Timeout",
        0xBD => "High Fly Writes",
        0xBE => "Airflow Temperature",
        0xBF => "G-Sense Error Rate",
        0xC0 => "Power-off Retract Count",
        0xC1 => "Load/Unload Cycle Count",
        0xC2 => "Temperature",
        0xC3 => "Hardware ECC Recovered",
        0xC4 => "Reallocation Event Count",
        0xC5 => "Current Pending Sector Count",
        0xC6 => "Uncorrectable Sector Count",
        0xC7 => "UltraDMA CRC Error Count",
        0xC8 => "Write Error Rate",
        0xC9 => "Soft Read Error Rate",
        0xCA => "Data Address Mark Errors",
        0xCB => "Run Out Cancel",
        0xCC => "Soft ECC Correction",
        0xCD => "Thermal Asperity Rate",
        0xCE => "Flying Height",
        0xCF => "Spin High Current",
        0xD0 => "Spin Buzz",
        0xD1 => "Offline Seek Performance",
        0xDC => "Disk Shift",
        0xDD => "G-Sense Error Rate",
        0xDE => "Loaded Hours",
        0xDF => "Load/Unload Retry Count",
        0xE0 => "Load Friction",
        0xE1 => "Load/Unload Cycle Count",
        0xE2 => "Load 'In'-time",
        0xE3 => "Torque Amplification Count",
        0xE4 => "Power-Off Retract Cycle",
        0xE6 => "GMR Head Amplitude",
        0xE7 => "Life Left",
        0xE8 => "Endurance Remaining",
        0xE9 => "Media Wearout Indicator",
        0xEA => "Average Erase Count",
        0xEB => "Good Block Count",
        0xF0 => "Head Flying Hours",
        0xF1 => "Total LBAs Written",
        0xF2 => "Total LBAs Read",
        0xF3 => "Total LBAs Written Expanded",
        0xF4 => "Total LBAs Read Expanded",
        0xF9 => "NAND Writes (1GiB)",
        0xFA => "Read Error Retry Rate",
        0xFE => "Free Fall Protection",
        _ => return None,
    })
}

fn vendor_name(vendor: Vendor, id: u8) -> Option<&'static str> {
    Some(match (vendor, id) {
        (Vendor::Samsung, 0xB1) => "Wear Leveling Count",
        (Vendor::Samsung, 0xB2) => "Used Reserved Block Count (Chip)",
        (Vendor::Samsung, 0xB3) => "Used Reserved Block Count (Total)",
        (Vendor::Samsung, 0xB4) => "Unused Reserved Block Count (Total)",
        (Vendor::Samsung, 0xB5) => "Program Fail Count (Total)",
        (Vendor::Samsung, 0xB6) => "Erase Fail Count (Total)",
        (Vendor::Samsung, 0xB7) => "Runtime Bad Block (Total)",
        (Vendor::Samsung, 0xBB) => "Uncorrectable Error Count",
        (Vendor::Samsung, 0xC3) => "ECC Error Rate",
        (Vendor::Samsung, 0xC7) => "CRC Error Count",
        (Vendor::Samsung, 0xEB) => "POR Recovery Count",

        (Vendor::Micron, 0xAA) => "Grown Failing Block Count",
        (Vendor::Micron, 0xAD) => "Average Block-Erase Count",
        (Vendor::Micron, 0xB4) => "Unused Reserved Block Count",
        (Vendor::Micron, 0xB7) => "SATA Interface Downshift",
        (Vendor::Micron, 0xB8) => "Error Correction Count",
        (Vendor::Micron, 0xC5) => "Current Pending ECC Count",
        (Vendor::Micron, 0xC6) => "SMART Off-line Scan Uncorrectable Error Count",
        (Vendor::Micron, 0xCA) => "Percent Lifetime Remaining",
        (Vendor::Micron, 0xCE) => "Write Error Rate",
        (Vendor::Micron, 0xD2) => "Successful RAIN Recovery Count",
        (Vendor::Micron, 0xF6) => "Total Host Sector Writes",
        (Vendor::Micron, 0xF7) => "Host Program Page Count",
        (Vendor::Micron, 0xF8) => "FTL Program Page Count",

        (Vendor::Intel, 0xBE) => "Temperature Case",
        (Vendor::Intel, 0xC0) => "Unsafe Shutdown Count",
        (Vendor::Intel, 0xE1) => "Host Writes (32MiB)",
        (Vendor::Intel, 0xE2) => "Timed Workload Media Wear",
        (Vendor::Intel, 0xE3) => "Timed Workload Host Read/Write Ratio",
        (Vendor::Intel, 0xE4) => "Timed Workload Timer",
        (Vendor::Intel, 0xE8) => "Available Reserved Space",
        (Vendor::Intel, 0xF1) => "Host Writes (32MiB)",
        (Vendor::Intel, 0xF2) => "Host Reads (32MiB)",

        (Vendor::WesternDigital, 0xA9) => "Percent Lifetime Remaining",
        (Vendor::WesternDigital, 0xE6) => "Media Wearout Indicator",
        (Vendor::WesternDigital, 0xE8) => "Available Reserved Space",
        (Vendor::WesternDigital, 0xE9) => "NAND GB Written (TLC)",
        (Vendor::WesternDigital, 0xEA) => "NAND GB Written (SLC)",
        (Vendor::WesternDigital, 0xF1) => "Host Writes (GiB)",
        (Vendor::WesternDigital, 0xF2) => "Host Reads (GiB)",

        (Vendor::Kingston, 0xE7) => "SSD Life Left",
        (Vendor::Kingston, 0xE9) => "Lifetime Writes to Flash (GiB)",
        (Vendor::Kingston, 0xF1) => "Lifetime Writes from Host (GiB)",
        (Vendor::Kingston, 0xF2) => "Lifetime Reads from Host (GiB)",

        (Vendor::Kioxia, 0xAD) => "Erase Count",
        (Vendor::Kioxia, 0xE9) => "NAND Writes (GiB)",
        (Vendor::Kioxia, 0xF1) => "Host Writes (GiB)",
        (Vendor::Kioxia, 0xF2) => "Host Reads (GiB)",
        _ => return None,
    })
}

pub fn ata_attribute_name(vendor: Vendor, id: u8) -> String {
    vendor_name(vendor, id)
        .or_else(|| generic_name(id))
        .unwrap_or("Vendor Specific")
        .to_string()
}

/// Bytes per unit of the "host writes"/"host reads" attributes, by vendor.
/// Returns `(write_id, read_id, bytes_per_unit)`.
pub fn host_io_attributes(vendor: Vendor) -> &'static [(u8, Option<u8>, u64)] {
    const GIB: u64 = 1 << 30;
    match vendor {
        Vendor::Samsung | Vendor::Generic => &[(0xF1, Some(0xF2), 512)],
        Vendor::Micron => &[(0xF6, None, 512), (0xF1, Some(0xF2), 512)],
        Vendor::Intel => &[(0xF1, Some(0xF2), 32 << 20), (0xE1, None, 32 << 20)],
        Vendor::WesternDigital | Vendor::Kingston | Vendor::Kioxia => &[(0xF1, Some(0xF2), GIB)],
    }
}

/// Attribute (by ID) whose *normalized current value* is the remaining life in percent.
pub fn life_attribute(vendor: Vendor) -> &'static [u8] {
    match vendor {
        Vendor::Samsung => &[0xB1],
        Vendor::Micron => &[0xCA, 0xAD],
        Vendor::Intel => &[0xE9, 0xE8],
        Vendor::WesternDigital => &[0xA9, 0xE6],
        Vendor::Kingston => &[0xE7, 0xA9],
        Vendor::Kioxia => &[0xAD, 0xE9],
        Vendor::Generic => &[0xE7, 0xA9, 0xE9, 0xCA, 0xB1, 0xAD],
    }
}

/// Synthesized NVMe "attribute" names, using CrystalDiskInfo's ID layout.
pub const NVME_ROWS: &[(u8, &str)] = &[
    (0x01, "Critical Warning"),
    (0x02, "Composite Temperature"),
    (0x03, "Available Spare"),
    (0x04, "Available Spare Threshold"),
    (0x05, "Percentage Used"),
    (0x06, "Data Units Read"),
    (0x07, "Data Units Written"),
    (0x08, "Host Read Commands"),
    (0x09, "Host Write Commands"),
    (0x0A, "Controller Busy Time"),
    (0x0B, "Power Cycles"),
    (0x0C, "Power On Hours"),
    (0x0D, "Unsafe Shutdowns"),
    (0x0E, "Media and Data Integrity Errors"),
    (0x0F, "Number of Error Information Log Entries"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_detection() {
        assert_eq!(detect_vendor("Samsung SSD 870 EVO 1TB", true), Vendor::Samsung);
        assert_eq!(detect_vendor("CT1000MX500SSD1", true), Vendor::Micron);
        assert_eq!(detect_vendor("TOSHIBA MQ04UBB400", false), Vendor::Generic);
        assert_eq!(detect_vendor("WDC WDS100T2B0A", true), Vendor::WesternDigital);
    }

    #[test]
    fn names() {
        assert_eq!(ata_attribute_name(Vendor::Generic, 0x05), "Reallocated Sectors Count");
        assert_eq!(ata_attribute_name(Vendor::Samsung, 0xB1), "Wear Leveling Count");
        assert_eq!(ata_attribute_name(Vendor::Generic, 0x99), "Vendor Specific");
    }
}
