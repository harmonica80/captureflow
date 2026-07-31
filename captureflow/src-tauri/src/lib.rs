mod capture;
mod selector;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            capture_virtual_desktop,
            select_screen_area
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
