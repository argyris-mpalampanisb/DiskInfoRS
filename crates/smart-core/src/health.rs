//! Health assessment, modelled on CrystalDiskInfo's Good / Caution / Bad rules.

use serde::{Deserialize, Serialize};

use crate::attrs::{detect_vendor, life_attribute};
use crate::model::{Disk, DiskKind, Health};

/// Per-disk alarm thresholds (CrystalDiskInfo "Advanced > Alarm threshold").
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct HealthConfig {
    /// Raw count of 05 (Reallocated Sectors) at or above which the disk is "Caution".
    pub reallocated: u64,
    /// Raw count of C5 (Current Pending Sectors).
    pub pending: u64,
    /// Raw count of C6 (Uncorrectable Sectors).
    pub uncorrectable: u64,
    /// SSD remaining life (%) at or below which the disk is "Caution".
    pub life_caution: u8,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self { reallocated: 1, pending: 1, uncorrectable: 1, life_caution: 10 }
    }
}

fn low32(v: u64) -> u64 {
    v & 0xFFFF_FFFF
}

pub fn evaluate(d: &mut Disk, cfg: &HealthConfig) {
    d.health_reasons.clear();
    d.health_percent = None;
    if !d.smart_available {
        d.health = Health::Unknown;
        return;
    }
    let mut worst = Health::Good;
    match d.kind {
        Some(DiskKind::Nvme) => {
            if let Some(h) = d.nvme.clone() {
                for a in &mut d.attributes {
                    a.status = Health::Good;
                    match a.id {
                        0x01 => {
                            if h.critical_warning & 0b1101 != 0 {
                                a.status = Health::Bad;
                            } else if h.critical_warning & 0b1_0010 != 0 {
                                a.status = Health::Caution;
                            }
                        }
                        0x03 if h.available_spare_threshold > 0 && h.available_spare < h.available_spare_threshold => a.status = Health::Bad,
                        0x05 if h.percentage_used >= 100 - cfg.life_caution.min(100) => a.status = Health::Caution,
                        _ => {}
                    }
                    if a.status > Health::Good {
                        d.health_reasons.push(format!("{} = {}", a.name, a.raw));
                    }
                    worst = worst.max(a.status);
                }
                d.health_percent = Some(100u8.saturating_sub(h.percentage_used.min(100)));
            }
        }
        _ => {
            for a in &mut d.attributes {
                a.status = Health::Good;
                if let (Some(cur), Some(thr)) = (a.current, a.threshold) {
                    if thr != 0 && thr < 0xFE && cur <= thr {
                        a.status = Health::Bad;
                        d.health_reasons.push(format!("{:02X} {}: value {} <= threshold {}", a.id, a.name, cur, thr));
                    }
                }
                if a.status == Health::Good {
                    let limit = match a.id {
                        0x05 => Some(cfg.reallocated),
                        0xC5 => Some(cfg.pending),
                        0xC6 => Some(cfg.uncorrectable),
                        _ => None,
                    };
                    if let Some(limit) = limit {
                        if limit > 0 && low32(a.raw) >= limit {
                            a.status = Health::Caution;
                            d.health_reasons.push(format!("{:02X} {}: raw {}", a.id, a.name, low32(a.raw)));
                        }
                    }
                }
                worst = worst.max(a.status);
            }
            if d.is_ssd() {
                let vendor = detect_vendor(&d.model, true);
                let life = life_attribute(vendor)
                    .iter()
                    .find_map(|id| d.attributes.iter().find(|a| a.id == *id))
                    .and_then(|a| a.current)
                    .filter(|v| *v <= 100);
                if let Some(l) = life {
                    d.health_percent = Some(l);
                    if l <= cfg.life_caution {
                        worst = worst.max(Health::Caution);
                        d.health_reasons.push(format!("Remaining life {l}%"));
                    }
                }
            }
        }
    }
    if d.health_reasons.iter().any(|r| r.starts_with("SMART RETURN STATUS")) {
        worst = Health::Bad;
    }
    d.health = worst;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Attribute;

    fn attr(id: u8, cur: u8, thr: u8, raw: u64) -> Attribute {
        Attribute { id, name: format!("{id:02X}"), current: Some(cur), worst: Some(cur), threshold: Some(thr), raw, raw_hex: String::new(), status: Health::Unknown }
    }

    fn ata_disk(attrs: Vec<Attribute>) -> Disk {
        Disk { kind: Some(DiskKind::Ata), smart_available: true, rotation_rate: Some(5400), model: "HDD".into(), attributes: attrs, ..Default::default() }
    }

    #[test]
    fn good() {
        let mut d = ata_disk(vec![attr(0x05, 100, 50, 0), attr(0xC5, 100, 0, 0)]);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Good);
    }

    #[test]
    fn caution_pending() {
        let mut d = ata_disk(vec![attr(0x05, 100, 50, 0), attr(0xC5, 100, 0, 2)]);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Caution);
        assert_eq!(d.attributes[1].status, Health::Caution);
        // Raising the threshold makes it Good again.
        evaluate(&mut d, &HealthConfig { pending: 5, ..Default::default() });
        assert_eq!(d.health, Health::Good);
    }

    #[test]
    fn bad_threshold() {
        let mut d = ata_disk(vec![attr(0x05, 30, 36, 1200)]);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Bad);
    }

    #[test]
    fn unknown_without_smart() {
        let mut d = ata_disk(vec![]);
        d.smart_available = false;
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Unknown);
    }

    #[test]
    fn ssd_life() {
        let mut d = ata_disk(vec![attr(0xB1, 8, 0, 3000)]);
        d.rotation_rate = Some(1);
        d.model = "Samsung SSD 860 EVO".into();
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health_percent, Some(8));
        assert_eq!(d.health, Health::Caution);
    }

    fn nvme_disk(spare: u8, used: u8, warn: u8) -> Disk {
        let h = crate::model::NvmeHealth { available_spare: spare, available_spare_threshold: 10, percentage_used: used, critical_warning: warn, ..Default::default() };
        Disk { kind: Some(DiskKind::Nvme), smart_available: true, attributes: crate::nvme::rows(&h), nvme: Some(h), ..Default::default() }
    }

    #[test]
    fn nvme_rules() {
        let mut d = nvme_disk(100, 4, 0);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!((d.health, d.health_percent), (Health::Good, Some(96)));
        let mut d = nvme_disk(100, 92, 0);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Caution);
        let mut d = nvme_disk(5, 10, 0);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Bad);
        let mut d = nvme_disk(100, 10, 0b100);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Bad);
        let mut d = nvme_disk(100, 10, 0b10);
        evaluate(&mut d, &HealthConfig::default());
        assert_eq!(d.health, Health::Caution);
    }
}
