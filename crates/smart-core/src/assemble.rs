//! Combines raw device pages into a [`Disk`]. Platform independent so demo mode and tests
//! exercise the same code path as real hardware.

use crate::ata::{self, sata_gen_label};
use crate::model::{Disk, DiskKind};
use crate::nvme;

pub fn apply_ata(d: &mut Disk, identify: &[u8], smart: Option<(&[u8], &[u8])>, return_status_failed: Option<bool>) -> Result<(), String> {
    let id = ata::parse_identify(identify).ok_or("invalid IDENTIFY data")?;
    d.kind = Some(DiskKind::Ata);
    d.model = id.model.clone();
    d.serial = id.serial.clone();
    d.firmware = id.firmware.clone();
    if id.sectors > 0 && d.size_bytes == 0 {
        d.size_bytes = id.sectors * 512;
    }
    if id.rotation_rate.is_some() {
        d.rotation_rate = id.rotation_rate;
    }
    d.standard = id.standard.clone();
    d.features = id.features.clone();
    if let Some(max) = id.sata_max_gen {
        let cur = id.sata_cur_gen.map(sata_gen_label).unwrap_or("----");
        d.transfer_mode = Some(format!("{cur} | {}", sata_gen_label(max)));
    }
    if !id.smart_supported {
        d.smart_error = Some("S.M.A.R.T. not supported".into());
        return Ok(());
    }
    if !id.smart_enabled {
        d.smart_error = Some("S.M.A.R.T. disabled".into());
        return Ok(());
    }
    let Some((vals, thr)) = smart else {
        return Ok(());
    };
    if !ata::checksum_ok(vals) {
        d.smart_error = Some("S.M.A.R.T. data checksum mismatch".into());
    }
    let values = ata::parse_smart_values(vals);
    let thresholds = if ata::checksum_ok(thr) { ata::parse_smart_thresholds(thr) } else { Vec::new() };
    if values.is_empty() {
        d.smart_error = Some("no S.M.A.R.T. attributes reported".into());
        return Ok(());
    }
    let (attrs, der) = ata::build_attributes(&d.model, d.is_ssd(), &values, &thresholds);
    d.attributes = attrs;
    d.temperature_c = der.temperature_c.or(d.temperature_c);
    d.power_on_hours = der.power_on_hours;
    d.power_on_count = der.power_on_count;
    d.host_reads_gb = der.host_reads_gb;
    d.host_writes_gb = der.host_writes_gb;
    d.nand_writes_gb = der.nand_writes_gb;
    d.smart_available = true;
    if return_status_failed == Some(true) {
        d.health_reasons.push("SMART RETURN STATUS: threshold exceeded".into());
    }
    Ok(())
}

pub fn apply_nvme(d: &mut Disk, identify: Option<&[u8]>, log: &[u8]) -> Result<(), String> {
    d.kind = Some(DiskKind::Nvme);
    d.rotation_rate = Some(1);
    if let Some(id) = identify.and_then(nvme::parse_identify_ctrl) {
        d.model = id.model;
        d.serial = id.serial;
        d.firmware = id.firmware;
        d.standard = id.version;
        d.features = id.features;
    }
    let h = nvme::parse_health_log(log).ok_or("invalid NVMe SMART log")?;
    d.attributes = nvme::rows(&h);
    d.temperature_c = nvme::temp_c(&h).or(d.temperature_c);
    d.power_on_hours = Some(h.power_on_hours);
    d.power_on_count = Some(h.power_cycles);
    d.host_reads_gb = Some(nvme::data_units_to_gb(h.data_units_read));
    d.host_writes_gb = Some(nvme::data_units_to_gb(h.data_units_written));
    d.nvme = Some(h);
    d.smart_available = true;
    Ok(())
}
