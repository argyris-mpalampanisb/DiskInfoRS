mod history;
mod ini;
mod paths;
mod settings;
mod source;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use smart_core::{Disk, HealthConfig};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WindowEvent};

use settings::{Settings, WindowGeom};
use source::Source;

struct AppState {
    data_dir: PathBuf,
    settings: Mutex<Settings>,
    source: Mutex<Source>,
    disks: Mutex<Vec<Disk>>,
    last_refresh: Mutex<Option<String>>,
    last_error: Mutex<Option<String>>,
    initial_disk: Option<String>,
    initial_graph: bool,
}

type Shared = Arc<AppState>;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    disks: Vec<Disk>,
    mode: String,
    data_dir: String,
    settings: Settings,
    last_refresh: Option<String>,
    error: Option<String>,
    version: String,
    initial_disk: Option<String>,
    initial_graph: bool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AppState {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            disks: lock(&self.disks).clone(),
            mode: lock(&self.source).mode().to_string(),
            data_dir: self.data_dir.display().to_string(),
            settings: lock(&self.settings).clone(),
            last_refresh: lock(&self.last_refresh).clone(),
            error: lock(&self.last_error).clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            initial_disk: self.initial_disk.clone(),
            initial_graph: self.initial_graph,
        }
    }

    fn reevaluate(&self, disks: &mut [Disk]) {
        let s = lock(&self.settings);
        for d in disks {
            smart_core::evaluate(d, &s.alarm_for(&d.history_key()));
        }
    }

    /// Scans all disks, applies per-disk alarm thresholds and records history.
    fn refresh(&self) {
        let (result, mode) = {
            let mut src = lock(&self.source);
            let r = src.scan();
            (r, src.mode())
        };
        let mut err = None;
        let mut disks = match result {
            Ok(d) => d,
            Err(e) => {
                err = Some(e);
                lock(&self.source).scan().unwrap_or_default()
            }
        };
        self.reevaluate(&mut disks);
        if mode != "demo" {
            if let Err(e) = history::record(&paths::smart_dir(&self.data_dir), &disks) {
                err.get_or_insert(format!("Could not write history: {e}"));
            }
        }
        *lock(&self.disks) = disks;
        *lock(&self.last_refresh) = Some(chrono::Local::now().format("%Y/%m/%d %H:%M:%S").to_string());
        *lock(&self.last_error) = err;
    }

    fn save_settings(&self) -> Result<(), String> {
        lock(&self.settings).save(&paths::settings_file(&self.data_dir)).map_err(|e| e.to_string())
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_snapshot(state: State<'_, Shared>) -> Result<Snapshot, String> {
    Ok(state.snapshot())
}

#[tauri::command]
async fn refresh(state: State<'_, Shared>) -> Result<Snapshot, String> {
    let s = state.inner().clone();
    blocking(move || {
        s.refresh();
        s.snapshot()
    })
    .await
}

#[tauri::command]
async fn elevate(state: State<'_, Shared>) -> Result<Snapshot, String> {
    let s = state.inner().clone();
    blocking(move || {
        if lock(&s.source).mode() != "readonly" {
            return Ok(s.snapshot());
        }
        let helper = source::start_helper()?;
        *lock(&s.source) = Source::Helper(helper);
        s.refresh();
        Ok(s.snapshot())
    })
    .await?
}

#[tauri::command]
async fn set_settings(state: State<'_, Shared>, settings: Settings) -> Result<Snapshot, String> {
    let s = state.inner().clone();
    blocking(move || {
        {
            let mut cur = lock(&s.settings);
            let window = cur.window;
            let alarms = std::mem::take(&mut cur.alarms);
            *cur = Settings { window, alarms, ..settings };
        }
        s.save_settings()?;
        Ok(s.snapshot())
    })
    .await?
}

#[tauri::command]
async fn set_alarm(state: State<'_, Shared>, key: String, alarm: Option<HealthConfig>) -> Result<Snapshot, String> {
    let s = state.inner().clone();
    blocking(move || {
        {
            let mut cur = lock(&s.settings);
            match alarm {
                Some(a) if a != HealthConfig::default() => {
                    cur.alarms.insert(key, a);
                }
                _ => {
                    cur.alarms.remove(&key);
                }
            }
        }
        s.save_settings()?;
        let mut disks = lock(&s.disks).clone();
        s.reevaluate(&mut disks);
        *lock(&s.disks) = disks;
        Ok(s.snapshot())
    })
    .await?
}

#[tauri::command]
async fn history_series(state: State<'_, Shared>, key: String) -> Result<Vec<String>, String> {
    Ok(history::list_series(&paths::smart_dir(&state.data_dir), &key))
}

#[tauri::command]
async fn history(state: State<'_, Shared>, key: String, series: String) -> Result<Vec<history::Point>, String> {
    Ok(history::read_series(&paths::smart_dir(&state.data_dir), &key, &series))
}

/// Background auto-refresh. Wakes every second so interval changes apply promptly.
fn spawn_auto_refresh(app: AppHandle, state: Shared) {
    thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            thread::sleep(Duration::from_secs(1));
            let minutes = lock(&state.settings).auto_refresh;
            if minutes == 0 || last.elapsed() < Duration::from_secs(minutes as u64 * 60) {
                continue;
            }
            last = Instant::now();
            state.refresh();
            let _ = app.emit("snapshot", state.snapshot());
        }
    });
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo") || std::env::var_os("DISKINFORS_DEMO").is_some();
    // `--disk sdb` (or /dev/sdb) selects that disk at startup.
    let initial_disk = args
        .iter()
        .position(|a| a == "--disk")
        .and_then(|i| args.get(i + 1))
        .map(|d| d.trim_start_matches("/dev/").to_string());
    let data_dir = paths::data_dir();
    let settings = Settings::load(&paths::settings_file(&data_dir));
    let state: Shared = Arc::new(AppState {
        data_dir,
        settings: Mutex::new(settings),
        source: Mutex::new(Source::initial(demo)),
        disks: Mutex::new(Vec::new()),
        last_refresh: Mutex::new(None),
        last_error: Mutex::new(None),
        initial_disk,
        initial_graph: args.iter().any(|a| a == "--graph"),
    });
    state.refresh();

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(state.clone())
        .setup(move |app| {
            let win = app.get_webview_window("main").ok_or("main window missing")?;
            if let Some(g) = lock(&state.settings).window {
                let _ = win.set_size(PhysicalSize::new(g.width, g.height));
                let _ = win.set_position(PhysicalPosition::new(g.x, g.y));
            }
            let st = state.clone();
            let w2 = win.clone();
            win.on_window_event(move |ev| {
                if let WindowEvent::CloseRequested { .. } = ev {
                    if let (Ok(p), Ok(sz)) = (w2.outer_position(), w2.inner_size()) {
                        lock(&st.settings).window = Some(WindowGeom { x: p.x, y: p.y, width: sz.width, height: sz.height });
                        let _ = st.save_settings();
                    }
                }
            });
            spawn_auto_refresh(app.handle().clone(), state.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_snapshot, refresh, elevate, set_settings, set_alarm, history_series, history])
        .run(tauri::generate_context!())
        .expect("error while running DiskInfoRS");
}
