//! User settings stored in `DiskInfo.ini` (keys compatible with CrystalDiskInfo where they overlap).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use smart_core::HealthConfig;

use crate::ini::Ini;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Minutes between automatic refreshes; 0 disables.
    pub auto_refresh: u32,
    pub fahrenheit: bool,
    pub raw_decimal: bool,
    pub hide_serial: bool,
    pub elevate_on_start: bool,
    pub language: String,
    pub window: Option<WindowGeom>,
    /// Per-disk alarm thresholds keyed by `Disk::history_key()`.
    pub alarms: BTreeMap<String, HealthConfig>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct WindowGeom {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_refresh: 10,
            fahrenheit: false,
            raw_decimal: false,
            hide_serial: false,
            elevate_on_start: true,
            language: "English".into(),
            window: None,
            alarms: BTreeMap::new(),
        }
    }
}

const S: &str = "Setting";
const ALARM: &str = "Alarm";

fn flag(ini: &Ini, key: &str, default: bool) -> bool {
    ini.get(S, key).map(|v| v == "1").unwrap_or(default)
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let ini = Ini::load(path);
        let d = Settings::default();
        let num = |k: &str| ini.get(S, k).and_then(|v| v.parse::<i64>().ok());
        let window = match (num("X"), num("Y"), num("Width"), num("Height")) {
            (Some(x), Some(y), Some(w), Some(h)) if w > 200 && h > 200 => {
                Some(WindowGeom { x: x as i32, y: y as i32, width: w as u32, height: h as u32 })
            }
            _ => None,
        };
        let mut alarms = BTreeMap::new();
        for (key, v) in ini.keys(ALARM) {
            let p: Vec<u64> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if let [r, p_, u, l] = p[..] {
                alarms.insert(key, HealthConfig { reallocated: r, pending: p_, uncorrectable: u, life_caution: l.min(100) as u8 });
            }
        }
        Settings {
            auto_refresh: num("AutoRefresh").map(|v| v.clamp(0, 1440) as u32).unwrap_or(d.auto_refresh),
            fahrenheit: flag(&ini, "Temperature", d.fahrenheit),
            raw_decimal: flag(&ini, "RawValueDecimal", d.raw_decimal),
            hide_serial: flag(&ini, "HideSerialNumber", d.hide_serial),
            elevate_on_start: flag(&ini, "ElevateOnStart", d.elevate_on_start),
            language: ini.get(S, "Language").unwrap_or("English").to_string(),
            window,
            alarms,
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        // Load first so unknown keys written by other tools (or CDI) survive.
        let mut ini = Ini::load(path);
        let b = |v: bool| if v { "1" } else { "0" };
        ini.set(S, "AutoRefresh", self.auto_refresh);
        ini.set(S, "Temperature", b(self.fahrenheit));
        ini.set(S, "RawValueDecimal", b(self.raw_decimal));
        ini.set(S, "HideSerialNumber", b(self.hide_serial));
        ini.set(S, "ElevateOnStart", b(self.elevate_on_start));
        ini.set(S, "Language", &self.language);
        if let Some(w) = self.window {
            ini.set(S, "X", w.x);
            ini.set(S, "Y", w.y);
            ini.set(S, "Width", w.width);
            ini.set(S, "Height", w.height);
        }
        ini.remove_section(ALARM);
        for (k, a) in &self.alarms {
            ini.set(ALARM, k, format!("{},{},{},{}", a.reallocated, a.pending, a.uncorrectable, a.life_caution));
        }
        ini.save(path)
    }

    pub fn alarm_for(&self, key: &str) -> HealthConfig {
        self.alarms.get(key).cloned().unwrap_or_default()
    }
}
