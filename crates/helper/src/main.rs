//! diskinfo-helper: the only part of DiskInfoRS that runs as root.
//!
//! It accepts a fixed set of commands and never takes paths or arguments from its
//! peer, so it can't be used to read or write arbitrary devices or files.
//!
//! Usage:
//!   diskinfo-helper --dump [--demo]    print all disks as pretty JSON and exit
//!   diskinfo-helper --serve [--demo]   JSON-lines server on stdin/stdout:
//!        -> "scan"   <- {"ok":true,"privileged":true,"disks":[...]}
//!        -> "quit"

use std::io::{self, BufRead, Write};

use serde_json::json;
use smart_core::Disk;

fn scan(demo: bool) -> Vec<Disk> {
    if demo {
        let mut d = smart_core::demo::disks();
        for x in &mut d {
            smart_core::evaluate(x, &smart_core::HealthConfig::default());
        }
        return d;
    }
    smart_core::scan(true)
}

fn privileged() -> bool {
    smart_core::is_privileged()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let demo = has("--demo");

    if has("--version") {
        println!("diskinfo-helper {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if has("--serve") {
        serve(demo);
        return;
    }
    if has("--dump") {
        let out = json!({ "privileged": privileged(), "disks": scan(demo) });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return;
    }
    eprintln!("usage: diskinfo-helper (--dump | --serve) [--demo]");
    std::process::exit(2);
}

fn serve(demo: bool) {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{}", json!({ "ready": true, "privileged": privileged(), "version": env!("CARGO_PKG_VERSION") }));
    let _ = out.flush();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let reply = match line.trim() {
            "scan" => json!({ "ok": true, "privileged": privileged(), "disks": scan(demo) }),
            "quit" => break,
            "" => continue,
            other => json!({ "ok": false, "error": format!("unknown command: {}", other.chars().take(32).collect::<String>()) }),
        };
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break; // peer went away
        }
    }
}
