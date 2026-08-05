use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::Mutex,
    time::SystemTime,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

pub const DEFAULT_SHORTCUT: &str = "Alt+Shift+A";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub schema_version: u32,
    pub capture_shortcut: String,
    #[serde(default = "default_history_limit")]
    pub history_limit: u32,
    #[serde(default)]
    pub default_save_directory: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            capture_shortcut: DEFAULT_SHORTCUT.into(),
            history_limit: default_history_limit(),
            default_save_directory: String::new(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub capture_shortcut: String,
    pub default_shortcut: String,
    pub log_path: String,
    pub history_limit: u32,
    pub default_save_directory: String,
}

#[derive(Default)]
pub struct SettingsState(pub Mutex<AppSettings>);

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let mut settings = load(app).unwrap_or_default();
    if let Err(error) = app
        .global_shortcut()
        .register(settings.capture_shortcut.as_str())
    {
        append_error(
            app,
            "settings.initialize",
            &format!("無法註冊 {}：{error}", settings.capture_shortcut),
        );
        settings = AppSettings::default();
        app.global_shortcut()
            .register(DEFAULT_SHORTCUT)
            .map_err(|error| format!("無法註冊預設快捷鍵：{error}"))?;
        persist(app, &settings)?;
    }
    *app.state::<SettingsState>()
        .0
        .lock()
        .map_err(|_| "設定狀態鎖定失敗")? = settings;
    Ok(())
}

pub fn view(app: &AppHandle) -> Result<SettingsView, String> {
    let settings = app
        .state::<SettingsState>()
        .0
        .lock()
        .map_err(|_| "設定狀態鎖定失敗")?
        .clone();
    Ok(SettingsView {
        capture_shortcut: settings.capture_shortcut,
        default_shortcut: DEFAULT_SHORTCUT.into(),
        log_path: path_string(log_path(app)?),
        history_limit: settings.history_limit,
        default_save_directory: settings.default_save_directory,
    })
}

pub fn update_shortcut(app: &AppHandle, shortcut: String) -> Result<SettingsView, String> {
    let shortcut = shortcut.trim();
    if shortcut.is_empty() {
        return Err("快捷鍵不可空白。範例：Alt+Shift+A".into());
    }
    let old = app
        .state::<SettingsState>()
        .0
        .lock()
        .map_err(|_| "設定狀態鎖定失敗")?
        .capture_shortcut
        .clone();
    if shortcut.eq_ignore_ascii_case(&old) {
        return view(app);
    }

    app.global_shortcut()
        .unregister(old.as_str())
        .map_err(|error| format!("無法暫停原快捷鍵：{error}"))?;
    if let Err(error) = app.global_shortcut().register(shortcut) {
        let _ = app.global_shortcut().register(old.as_str());
        let message = format!("快捷鍵格式無效或已被其他程式占用：{error}");
        append_error(app, "settings.update_shortcut", &message);
        return Err(message);
    }

    let next = AppSettings {
        schema_version: 1,
        capture_shortcut: shortcut.into(),
        history_limit: app
            .state::<SettingsState>()
            .0
            .lock()
            .map_err(|_| "設定狀態鎖定失敗")?
            .history_limit,
        default_save_directory: app
            .state::<SettingsState>()
            .0
            .lock()
            .map_err(|_| "設定狀態鎖定失敗")?
            .default_save_directory
            .clone(),
    };
    if let Err(error) = persist(app, &next) {
        let _ = app.global_shortcut().unregister(shortcut);
        let _ = app.global_shortcut().register(old.as_str());
        append_error(app, "settings.persist", &error);
        return Err(error);
    }
    *app.state::<SettingsState>()
        .0
        .lock()
        .map_err(|_| "設定狀態鎖定失敗")? = next;
    view(app)
}

pub fn update_preferences(
    app: &AppHandle,
    history_limit: u32,
    default_save_directory: String,
) -> Result<SettingsView, String> {
    if !(1..=100).contains(&history_limit) {
        return Err("歷史截圖記錄數量必須介於 1 到 100。".into());
    }
    if !default_save_directory.is_empty() && !std::path::Path::new(&default_save_directory).is_dir()
    {
        return Err("預設圖檔資料夾不存在。".into());
    }
    let mut next = app
        .state::<SettingsState>()
        .0
        .lock()
        .map_err(|_| "設定狀態鎖定失敗")?
        .clone();
    next.history_limit = history_limit;
    next.default_save_directory = default_save_directory;
    persist(app, &next)?;
    *app.state::<SettingsState>()
        .0
        .lock()
        .map_err(|_| "設定狀態鎖定失敗")? = next;
    view(app)
}

pub fn history_limit(app: &AppHandle) -> usize {
    app.state::<SettingsState>()
        .0
        .lock()
        .map(|settings| settings.history_limit as usize)
        .unwrap_or(20)
}

fn default_history_limit() -> u32 {
    20
}

pub fn append_error(app: &AppHandle, context: &str, message: &str) {
    let Ok(path) = log_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let sanitized = message.replace(['\r', '\n'], " ");
        let _ = writeln!(file, "{timestamp}\t{context}\t{sanitized}");
    }
}

fn load(app: &AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(app)?;
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| format!("設定檔損壞：{error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(error) => Err(format!("無法讀取設定：{error}")),
    }
}

fn persist(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("無法建立設定目錄：{error}"))?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(settings).map_err(|error| format!("無法建立設定：{error}"))?,
    )
    .map_err(|error| format!("無法寫入設定：{error}"))?;
    fs::rename(temporary, path).map_err(|error| format!("無法更新設定：{error}"))
}

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?
        .join("settings.json"))
}

fn log_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_log_dir()
        .map_err(|error| format!("無法取得日誌目錄：{error}"))?
        .join("captureflow.log"))
}

fn path_string(path: std::path::PathBuf) -> String {
    path.to_string_lossy().into_owned()
}
