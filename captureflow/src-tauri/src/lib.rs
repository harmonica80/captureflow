mod capture;

#[tauri::command]
fn capture_virtual_desktop(app: tauri::AppHandle) -> Result<capture::DesktopSnapshot, String> {
    capture::capture_virtual_desktop(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![capture_virtual_desktop])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
