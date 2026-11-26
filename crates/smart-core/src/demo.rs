//! Demo disks built from synthetic pages, for UI work on machines without real S.M.A.R.T.
//! access (e.g. WSL or a VM). Covers Good / Caution / Bad for ATA HDD, ATA SSD and NVMe.

use crate::assemble::{apply_ata, apply_nvme};
use crate::model::Disk;
use crate::synth::{identify_page, nvme_health_page, nvme_identify_page, smart_pages, NvmeSpec};

fn base(id: &str, iface: &str, mounts: &[&str], mode: Option<&str>) -> Disk {
    Disk {
        id: id.into(),
        dev_path: format!("/dev/{id}"),
        interface: iface.into(),
        mount_points: mounts.iter().map(|s| s.to_string()).collect(),
        transfer_mode: mode.map(String::from),
        ..Default::default()
    }
}

fn nvme(id: &str, model: &str, serial: &str, fw: &str, size: u64, mounts: &[&str], spec: NvmeSpec) -> Disk {
    let mut d = base(id, "NVM Express", mounts, Some("PCIe 4.0 x4 | PCIe 4.0 x4"));
    d.size_bytes = size;
    apply_nvme(&mut d, Some(&nvme_identify_page(model, serial, fw, 0x0002_0000)), &nvme_health_page(&spec)).expect("demo nvme");
    d
}

fn ata(id: &str, iface: &str, mounts: &[&str], ident: Vec<u8>, attrs: &[(u8, u8, u8, u8, u64)]) -> Disk {
    let mut d = base(id, iface, mounts, None);
    let (v, t) = smart_pages(attrs);
    apply_ata(&mut d, &ident, Some((&v, &t)), Some(false)).expect("demo ata");
    d
}

/// 1 GB = 1953.125 NVMe data units (1 unit = 512 000 bytes).
const fn gb_to_units(gb: u64) -> u64 {
    gb * 15_625 / 8
}

pub fn disks() -> Vec<Disk> {
    vec![
        nvme(
            "nvme0n1",
            "Samsung SSD 990 EVO Plus 4TB",
            "S7U9NU0Y926089N",
            "2B2QKXG7",
            4_000_787_030_016,
            &["/", "/boot/efi"],
            NvmeSpec {
                critical_warning: 0,
                temp_c: 29,
                spare: 100,
                spare_thr: 10,
                used: 4,
                units_read: gb_to_units(146_295),
                units_written: gb_to_units(152_024),
                power_cycles: 2339,
                power_on_hours: 2463,
                unsafe_shutdowns: 61,
                media_errors: 0,
            },
        ),
        nvme(
            "nvme1n1",
            "SAMSUNG MZVL21T0HCLR-00BL2",
            "S64NNX0T828778",
            "CXV8501Q",
            1_024_209_543_168,
            &["/home"],
            NvmeSpec {
                critical_warning: 0,
                temp_c: 38,
                spare: 100,
                spare_thr: 10,
                used: 2,
                units_read: 61_204_331,
                units_written: 48_771_092,
                power_cycles: 1874,
                power_on_hours: 6120,
                unsafe_shutdowns: 112,
                media_errors: 0,
            },
        ),
        ata(
            "sda",
            "USB (Serial ATA)",
            &["/media/backup"],
            identify_page("TOSHIBA MQ04UBB400", "357DT1W9T", "JU003U", 7_814_037_168, 5400, 3, 3),
            &[
                (0x01, 100, 100, 50, 0),
                (0x02, 100, 100, 50, 0),
                (0x03, 100, 100, 1, 1563),
                (0x04, 100, 100, 0, 412),
                (0x05, 100, 100, 50, 0),
                (0x07, 100, 100, 50, 0),
                (0x08, 100, 100, 50, 0),
                (0x09, 100, 100, 0, 1450),
                (0x0A, 111, 100, 30, 0),
                (0x0C, 100, 100, 0, 183),
                (0xBF, 100, 100, 0, 0),
                (0xC0, 100, 100, 0, 37),
                (0xC1, 100, 100, 0, 2216),
                (0xC2, 100, 100, 0, 0x0027_0012_001E),
                (0xC4, 100, 100, 0, 0),
                (0xC5, 100, 100, 0, 0),
                (0xC6, 100, 100, 0, 0),
                (0xC7, 200, 200, 0, 0),
                (0xDC, 100, 100, 0, 0),
                (0xDE, 100, 100, 0, 1310),
                (0xDF, 100, 100, 0, 0),
                (0xE0, 100, 100, 0, 0),
                (0xE2, 100, 100, 0, 228),
                (0xF0, 100, 100, 1, 0),
            ],
        ),
        ata(
            "sdb",
            "Serial ATA",
            &["/srv/data"],
            identify_page("CT1000MX500SSD1", "2219E6A1B2C3", "M3CR046", 1_953_525_168, 1, 3, 3),
            &[
                (0x01, 100, 100, 0, 0),
                (0x05, 100, 100, 10, 0),
                (0x09, 100, 100, 0, 15_904),
                (0x0C, 100, 100, 0, 1_022),
                (0xAB, 100, 100, 0, 0),
                (0xAC, 100, 100, 0, 0),
                (0xAD, 87, 87, 0, 197),
                (0xAE, 100, 100, 0, 88),
                (0xB4, 0, 0, 0, 29),
                (0xB7, 100, 100, 0, 0),
                (0xB8, 100, 100, 0, 0),
                (0xBB, 100, 100, 0, 0),
                (0xC2, 66, 49, 0, 0x0033_0000_0022),
                (0xC4, 100, 100, 0, 0),
                (0xC5, 100, 100, 0, 2),
                (0xC6, 100, 100, 0, 0),
                (0xC7, 100, 100, 0, 0),
                (0xCA, 87, 87, 1, 13),
                (0xCE, 100, 100, 0, 0),
                (0xD2, 100, 100, 0, 0),
                (0xF6, 100, 100, 0, 61_843_993_210),
                (0xF7, 100, 100, 0, 1_811_203_448),
                (0xF8, 100, 100, 0, 3_377_115_041),
            ],
        ),
        ata(
            "sdc",
            "Serial ATA",
            &[],
            identify_page("WDC WD20EZRX-00D8PB0", "WD-WCC4M1XXXXXX", "80.00A80", 3_907_029_168, 5400, 3, 2),
            &[
                (0x01, 200, 200, 51, 14),
                (0x03, 176, 172, 21, 4183),
                (0x04, 99, 99, 0, 1460),
                (0x05, 30, 30, 140, 1840),
                (0x07, 200, 200, 0, 0),
                (0x09, 37, 37, 0, 46_221),
                (0x0A, 100, 100, 0, 0),
                (0x0B, 100, 100, 0, 0),
                (0x0C, 99, 99, 0, 1_455),
                (0xC0, 200, 200, 0, 610),
                (0xC1, 1, 1, 0, 1_802_644),
                (0xC2, 112, 101, 0, 38),
                (0xC4, 1, 1, 0, 230),
                (0xC5, 200, 200, 0, 17),
                (0xC6, 200, 200, 0, 9),
                (0xC7, 200, 200, 0, 0),
                (0xC8, 200, 200, 0, 3),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crate::health::{evaluate, HealthConfig};
    use crate::model::Health;

    #[test]
    fn demo_health_spread() {
        let mut ds = super::disks();
        for d in &mut ds {
            evaluate(d, &HealthConfig::default());
        }
        let h: Vec<Health> = ds.iter().map(|d| d.health).collect();
        assert_eq!(h, [Health::Good, Health::Good, Health::Good, Health::Caution, Health::Bad]);
        assert_eq!(ds[0].host_writes_gb, Some(152_024));
        assert_eq!(ds[2].temperature_c, Some(30));
        assert_eq!(ds[3].health_percent, Some(87));
        assert_eq!(ds[3].host_writes_gb, Some(31_664));
    }
}
