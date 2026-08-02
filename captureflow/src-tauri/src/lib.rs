mod annotation;
mod capture;
mod diagnostics;
mod selector;
mod settings;
mod sticker;
mod sticker_store;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[tauri::command]
fn capture_virtual_desktop(app: tauri::AppHandle) -> Result<capture::DesktopSnapshot, String> {
    capture::capture_virtual_desktop(&app)
}

#[tauri::command]
fn list_monitors() -> Result<Vec<capture::MonitorInfo>, String> {
    capture::list_monitors()
}

#[tauri::command]
async fn select_screen_area(
    app: tauri::AppHandle,
) -> Result<Option<selector::SelectionSnapshot>, String> {
    selector::select_area(app).await
}

#[tauri::command]
async fn repeat_last_selection(
    app: tauri::AppHandle,
) -> Result<selector::SelectionSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || selector::repeat_last_area(app))
        .await
        .map_err(|error| format!("重複擷取執行緒異常結束：{error}"))?
}

#[tauri::command]
async fn capture_monitor(
    app: tauri::AppHandle,
    device_name: String,
) -> Result<selector::SelectionSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || selector::capture_monitor(app, &device_name))
        .await
        .map_err(|error| format!("指定螢幕擷取執行緒異常結束：{error}"))?
}

#[tauri::command]
fn open_sticker(app: tauri::AppHandle, image_path: String, x: i32, y: i32) -> Result<(), String> {
    sticker::open(&app, image_path, x, y)
}

#[tauri::command]
fn export_selection(
    app: tauri::AppHandle,
    image_path: String,
    destination: String,
) -> Result<(), String> {
    use std::path::Path;

    let source =
        std::fs::canonicalize(&image_path).map_err(|error| format!("無法讀取截圖來源：{error}"))?;
    let app_data = std::fs::canonicalize(
        app.path()
            .app_data_dir()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("無法讀取應用程式資料目錄：{error}"))?;
    if !source.starts_with(&app_data) {
        return Err("只能匯出 CaptureFlow 產生的截圖。".into());
    }
    let format = match Path::new(&destination)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => image::ImageFormat::Png,
        Some("jpg" | "jpeg") => image::ImageFormat::Jpeg,
        Some("webp") => image::ImageFormat::WebP,
        _ => return Err("請使用 .png、.jpg、.jpeg 或 .webp 副檔名。".into()),
    };
    image::open(source)
        .map_err(|error| format!("無法解碼截圖：{error}"))?
        .save_with_format(destination, format)
        .map_err(|error| format!("無法儲存圖片：{error}"))
}

#[tauri::command]
async fn run_capability_diagnostics() -> Result<diagnostics::CapabilityReport, String> {
    tauri::async_runtime::spawn_blocking(diagnostics::run)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<settings::SettingsView, String> {
    settings::view(&app)
}

#[tauri::command]
fn update_capture_shortcut(
    app: tauri::AppHandle,
    shortcut: String,
) -> Result<settings::SettingsView, String> {
    settings::update_shortcut(&app, shortcut)
}

#[tauri::command]
fn record_client_error(app: tauri::AppHandle, context: String, message: String) {
    settings::append_error(&app, &context, &message);
}

#[tauri::command]
fn save_annotation_project(
    app: tauri::AppHandle,
    image_path: String,
    canvas_width: u32,
    canvas_height: u32,
    objects: serde_json::Value,
) -> Result<String, String> {
    annotation::save_project(&app, &image_path, canvas_width, canvas_height, objects)
}

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn launch_selection(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        match selector::select_area(app.clone()).await {
            Ok(Some(selection)) => {
                focus_main_window(&app);
                let _ = app.emit("captureflow://selection-complete", selection);
            }
            Ok(None) => {}
            Err(error) => {
                settings::append_error(&app, "selection.global_shortcut", &error);
                focus_main_window(&app);
                let _ = app.emit("captureflow://selection-error", error);
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(settings::SettingsState::default())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        launch_selection(app.clone());
                    }
                })
                .build(),
        )
        .setup(|app| {
            sticker::restore(app.handle());
            settings::initialize(app.handle())?;
            let shortcut = settings::view(app.handle())?.capture_shortcut;
            tauri::tray::TrayIconBuilder::new()
                .tooltip(format!("CaptureFlow · {shortcut} 開始截圖"))
                .icon(
                    app.default_window_icon()
                        .ok_or("missing application icon")?
                        .clone(),
                )
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        focus_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            capture_virtual_desktop,
            list_monitors,
            select_screen_area,
            repeat_last_selection,
            capture_monitor,
            open_sticker,
            export_selection,
            run_capability_diagnostics,
            get_settings,
            update_capture_shortcut,
            record_client_error,
            save_annotation_project
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
