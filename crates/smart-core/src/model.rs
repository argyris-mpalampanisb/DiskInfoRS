use serde::{Deserialize, Serialize};

/// Overall health verdict, ordered from best to worst so `max()` gives the worst.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Health {
    Good,
    #[default]
    Unknown,
    Caution,
    Bad,
}

impl Health {
    /// Numeric code used by CrystalDiskInfo's `Smart.ini` (`HealthStatus`).
    pub fn cdi_code(self) -> u8 {
        match self {
            Health::Good => 1,
            Health::Caution => 2,
            Health::Bad => 3,
            Health::Unknown => 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiskKind {
    Ata,
    Nvme,
    Other,
}

/// One row of the S.M.A.R.T. table. For NVMe disks the rows are synthesized from the
/// SMART/Health log page (IDs 01..0F like CrystalDiskInfo) and only `raw` is meaningful.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Attribute {
    pub id: u8,
    pub name: String,
    pub current: Option<u8>,
    pub worst: Option<u8>,
    pub threshold: Option<u8>,
    /// Raw value: 48-bit for ATA, up to 64-bit (saturated) for NVMe.
    pub raw: u64,
    /// Raw bytes as hex, most significant first (12 hex digits for ATA).
    pub raw_hex: String,
    pub status: Health,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct NvmeHealth {
    pub critical_warning: u8,
    pub composite_temp_k: u16,
    pub available_spare: u8,
    pub available_spare_threshold: u8,
    pub percentage_used: u8,
    pub data_units_read: u64,
    pub data_units_written: u64,
    pub host_read_commands: u64,
    pub host_write_commands: u64,
    pub controller_busy_time: u64,
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    pub error_log_entries: u64,
    pub warning_temp_time: u32,
    pub critical_temp_time: u32,
    pub temp_sensors_k: Vec<u16>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Disk {
    /// Kernel name, e.g. `sda`, `nvme0n1`.
    pub id: String,
    pub dev_path: String,
    pub kind: Option<DiskKind>,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub size_bytes: u64,
    /// `Some(1)` = SSD (non-rotating), `Some(rpm)` otherwise.
    pub rotation_rate: Option<u16>,
    pub interface: String,
    pub transfer_mode: Option<String>,
    pub standard: Option<String>,
    pub features: Vec<String>,
    pub mount_points: Vec<String>,
    pub removable: bool,

    pub smart_available: bool,
    pub smart_error: Option<String>,
    pub attributes: Vec<Attribute>,
    pub nvme: Option<NvmeHealth>,

    pub temperature_c: Option<i32>,
    pub power_on_hours: Option<u64>,
    pub power_on_count: Option<u64>,
    pub host_reads_gb: Option<u64>,
    pub host_writes_gb: Option<u64>,
    pub nand_writes_gb: Option<u64>,

    pub health: Health,
    pub health_percent: Option<u8>,
    pub health_reasons: Vec<String>,
}

impl Disk {
    /// Folder/section key used by CrystalDiskInfo history: model immediately followed by serial.
    pub fn history_key(&self) -> String {
        let raw = format!("{}{}", self.model.trim(), self.serial.trim());
        raw.chars()
            .map(|c| if matches!(c, '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
            .collect()
    }

    pub fn is_ssd(&self) -> bool {
        self.rotation_rate == Some(1) || self.kind == Some(DiskKind::Nvme)
    }
}
