use serde::{Deserialize, Serialize};
use std::{fs, time::SystemTime};
use tauri::{AppHandle, Manager};

pub fn save_edited_image(
    app: &AppHandle,
    image_path: &str,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) -> Result<(), String> {
    let source = fs::canonicalize(image_path).map_err(|error| format!("無法讀取截圖：{error}"))?;
    let app_data = fs::canonicalize(
        app.path()
            .app_data_dir()
            .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?,
    )
    .map_err(|error| format!("無法讀取應用程式資料目錄：{error}"))?;
    if !source.starts_with(&app_data) {
        return Err("只能更新 CaptureFlow 產生的截圖。".into());
    }
    if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
        return Err("編輯圖片尺寸或像素資料不正確。".into());
    }
    image::save_buffer_with_format(
        source,
        &rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("無法儲存編輯圖片：{error}"))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationProject {
    schema_version: u32,
    created_at_unix_ms: u128,
    source_image: String,
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
        source_image: image_path.to_string(),
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

pub fn load_latest_project(app: &AppHandle, image_path: &str) -> Result<AnnotationProject, String> {
    let source = fs::canonicalize(image_path).map_err(|error| format!("無法讀取底圖：{error}"))?;
    let app_data = fs::canonicalize(
        app.path()
            .app_data_dir()
            .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?,
    )
    .map_err(|error| format!("無法讀取應用程式資料目錄：{error}"))?;
    if !source.starts_with(&app_data) {
        return Err("只能開啟 CaptureFlow 產生的截圖專案。".into());
    }

    let project_dir = app_data.join("projects");
    let entries = fs::read_dir(&project_dir).map_err(|_| "尚未儲存任何可編輯專案。".to_string())?;
    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(project) = serde_json::from_slice::<AnnotationProject>(&bytes) else {
            continue;
        };
        let Ok(project_source) = fs::canonicalize(&project.source_image) else {
            continue;
        };
        if project.schema_version == 1
            && project_source == source
            && project.objects.as_array().is_some()
        {
            matches.push((project.created_at_unix_ms, project));
        }
    }
    matches
        .into_iter()
        .max_by_key(|(created_at, _)| *created_at)
        .map(|(_, project)| project)
        .ok_or_else(|| "找不到目前截圖的可編輯專案。請先儲存一次。".to_string())
}
