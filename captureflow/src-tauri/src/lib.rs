mod capture;
mod selector;
mod sticker;
mod sticker_store;
use tauri::Manager;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            sticker::restore(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            capture_virtual_desktop,
            select_screen_area,
            open_sticker
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
