//! Minimal order-preserving INI reader/writer compatible with CrystalDiskInfo files
//! (`key="value"`, UTF-8 or UTF-16LE with BOM).

use std::fs;
use std::io;
use std::path::Path;

#[derive(Default, Debug, Clone)]
pub struct Ini {
    sections: Vec<(String, Vec<(String, String)>)>,
}

fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        String::from_utf8_lossy(b).into_owned()
    }
}

impl Ini {
    pub fn load(path: &Path) -> Ini {
        fs::read(path).map(|b| Ini::parse(&decode(&b))).unwrap_or_default()
    }

    pub fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        let mut cur: Option<usize> = None;
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with(';') || l.starts_with('#') {
                continue;
            }
            if let Some(name) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                cur = Some(ini.section_index(name));
                continue;
            }
            if let (Some(i), Some((k, v))) = (cur, l.split_once('=')) {
                let v = v.trim();
                let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v);
                ini.sections[i].1.push((k.trim().to_string(), v.to_string()));
            }
        }
        ini
    }

    fn section_index(&mut self, name: &str) -> usize {
        if let Some(i) = self.sections.iter().position(|(n, _)| n == name) {
            return i;
        }
        self.sections.push((name.to_string(), Vec::new()));
        self.sections.len() - 1
    }

    pub fn has_section(&self, name: &str) -> bool {
        self.sections.iter().any(|(n, _)| n == name)
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|(n, _)| n == section)
            .and_then(|(_, kv)| kv.iter().find(|(k, _)| k == key))
            .map(|(_, v)| v.as_str())
    }

    pub fn keys(&self, section: &str) -> Vec<(String, String)> {
        self.sections.iter().find(|(n, _)| n == section).map(|(_, kv)| kv.clone()).unwrap_or_default()
    }

    pub fn set(&mut self, section: &str, key: &str, value: impl ToString) {
        let i = self.section_index(section);
        let value = value.to_string();
        let kv = &mut self.sections[i].1;
        match kv.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value,
            None => kv.push((key.to_string(), value)),
        }
    }

    pub fn remove_section(&mut self, section: &str) {
        self.sections.retain(|(n, _)| n != section);
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut s = String::new();
        for (name, kv) in &self.sections {
            s.push_str(&format!("[{name}]\n"));
            for (k, v) in kv {
                s.push_str(&format!("{k}=\"{v}\"\n"));
            }
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("ini.tmp");
        fs::write(&tmp, s)?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut ini = Ini::parse("[Setting]\nAutoRefresh=\"10\"\n[X]\na=1\n");
        assert_eq!(ini.get("Setting", "AutoRefresh"), Some("10"));
        assert_eq!(ini.get("X", "a"), Some("1"));
        ini.set("Setting", "Temperature", 1);
        assert_eq!(ini.get("Setting", "Temperature"), Some("1"));
    }
}
