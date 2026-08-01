mod capture;
mod diagnostics;
mod selector;
mod sticker;
mod sticker_store;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[tauri::command]
fn capture_virtual_desktop(app: tauri::AppHandle) -> Result<capture::DesktopSnapshot, String> {
    capture::capture_virtual_desktop(&app)
}

#[tauri::command]
async fn select_screen_area(
    app: tauri::AppHandle,
) -> Result<Option<selector::SelectionSnapshot>, String> {
    selector::select_area(app).await
}

#[tauri::command]
fn open_sticker(app: tauri::AppHandle, image_path: String, x: i32, y: i32) -> Result<(), String> {
    sticker::open(&app, image_path, x, y)
}

#[tauri::command]
async fn run_capability_diagnostics() -> Result<diagnostics::CapabilityReport, String> {
    tauri::async_runtime::spawn_blocking(diagnostics::run)
        .await
        .map_err(|error| error.to_string())?
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
                focus_main_window(&app);
                let _ = app.emit("captureflow://selection-error", error);
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
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
            app.global_shortcut().register("Alt+Shift+A")?;
            tauri::tray::TrayIconBuilder::new()
                .tooltip("CaptureFlow · Alt+Shift+A 開始截圖")
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
            select_screen_area,
            open_sticker,
            run_capability_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
