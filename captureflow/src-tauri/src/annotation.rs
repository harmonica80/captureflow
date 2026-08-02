use serde::Serialize;
use std::{fs, time::SystemTime};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnnotationProject<'a> {
    schema_version: u32,
    created_at_unix_ms: u128,
    source_image: &'a str,
    canvas_width: u32,
    canvas_height: u32,
    objects: serde_json::Value,
}

pub fn save_project(
    app: &AppHandle,
    image_path: &str,
    canvas_width: u32,
    canvas_height: u32,
    objects: serde_json::Value,
) -> Result<String, String> {
    if canvas_width == 0 || canvas_height == 0 {
        return Err("專案畫布尺寸不可為零。".into());
    }
    let source = fs::canonicalize(image_path).map_err(|error| format!("無法讀取底圖：{error}"))?;
    let app_data = fs::canonicalize(
        app.path()
            .app_data_dir()
            .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?,
    )
    .map_err(|error| format!("無法讀取應用程式資料目錄：{error}"))?;
    if !source.starts_with(&app_data) {
        return Err("只能為 CaptureFlow 產生的截圖建立專案。".into());
    }
    let objects = objects.as_array().ok_or("標註物件必須是陣列。")?;
    if objects.len() > 10_000 {
        return Err("單一專案最多保存 10,000 個標註物件。".into());
    }
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| format!("無法取得系統時間：{error}"))?
        .as_millis();
    let output_dir = app_data.join("projects");
    fs::create_dir_all(&output_dir).map_err(|error| format!("無法建立專案目錄：{error}"))?;
    let output = output_dir.join(format!("project-{timestamp}.captureflow.json"));
    let project = AnnotationProject {
        schema_version: 1,
        created_at_unix_ms: timestamp,
        source_image: image_path,
        canvas_width,
        canvas_height,
        objects: serde_json::Value::Array(objects.clone()),
    };
    fs::write(
        &output,
        serde_json::to_vec_pretty(&project)
            .map_err(|error| format!("無法建立專案資料：{error}"))?,
    )
    .map_err(|error| format!("無法儲存專案：{error}"))?;
    Ok(output.to_string_lossy().into_owned())
}
