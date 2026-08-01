use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

static STORE_LOCK: Mutex<()> = Mutex::new(());
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickerRecord {
    pub id: u64,
    pub image_path: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub opacity: u8,
    pub locked: bool,
    pub click_through: bool,
}

pub fn path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    Ok(directory.join("stickers.json"))
}

pub fn next_id() -> u64 {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    epoch.saturating_mul(1000) + NEXT_ID.fetch_add(1, Ordering::Relaxed) % 1000
}

pub fn load(path: &Path) -> Vec<StickerRecord> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn upsert(path: &Path, record: &StickerRecord) {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut records = load(path);
    if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
        *existing = record.clone();
    } else {
        records.push(record.clone());
    }
    write(path, &records);
}

pub fn remove(path: &Path, id: u64) {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut records = load(path);
    records.retain(|record| record.id != id);
    write(path, &records);
}

fn write(path: &Path, records: &[StickerRecord]) {
    let Ok(content) = serde_json::to_string_pretty(records) else {
        return;
    };
    let temporary = path.with_extension("json.tmp");
    if fs::write(&temporary, content).is_ok() {
        if fs::rename(&temporary, path).is_err() {
            let _ = fs::remove_file(path);
            let _ = fs::rename(&temporary, path);
        }
    }
}
