use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{fs, io::Cursor, path::Path, time::SystemTime};
use tauri::{AppHandle, Manager};

pub fn save_edited_image(
    app: &AppHandle,
    image_path: &str,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) -> Result<(), String> {
    let source = fs::canonicalize(image_path).map_err(|error| format!("無法讀取擷圖：{error}"))?;
    let app_data = fs::canonicalize(
        app.path()
            .app_data_dir()
            .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?,
    )
    .map_err(|error| format!("無法讀取應用程式資料目錄：{error}"))?;
    if !source.starts_with(&app_data) {
        return Err("只能更新 CaptureFlow 產生的擷圖。".into());
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

pub fn save_composited_image(
    app: &AppHandle,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) -> Result<String, String> {
    if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
        return Err("合成圖片資料尺寸不正確".into());
    }
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("composites");
    fs::create_dir_all(&directory).map_err(|error| format!("無法建立合成圖片資料夾：{error}"))?;
    let output = directory.join(format!("composite-{timestamp}.png"));
    image::save_buffer_with_format(
        &output,
        &rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("無法儲存合成圖片：{error}"))?;
    Ok(output.to_string_lossy().into_owned())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationProject {
    pub schema_version: u32,
    pub created_at_unix_ms: u128,
    pub source_image: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_image_base64: Option<String>,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub objects: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub project_path: String,
    pub image_path: String,
    pub created_at_unix_ms: u128,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub selection: Option<serde_json::Value>,
    pub thumbnail_data_url: Option<String>,
}

fn history_thumbnail(path: &Path) -> Option<String> {
    let image = image::open(path).ok()?.thumbnail(240, 140);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        BASE64.encode(png.into_inner())
    ))
}

pub fn save_project(
    app: &AppHandle,
    image_path: &str,
    canvas_width: u32,
    canvas_height: u32,
    objects: serde_json::Value,
    destination: Option<String>,
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
        return Err("只能為 CaptureFlow 產生的擷圖建立專案。".into());
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
    let output = destination
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| output_dir.join(format!("project-{timestamp}.captureflow.json")));
    if output.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err("專案檔案必須使用 .json 副檔名".into());
    }
    let project =
        AnnotationProject {
            schema_version: 2,
            created_at_unix_ms: timestamp,
            source_image: image_path.to_string(),
            source_image_base64: Some(BASE64.encode(
                fs::read(&source).map_err(|error| format!("無法讀取專案來源圖片：{error}"))?,
            )),
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

pub fn auto_save_project(
    app: &AppHandle,
    image_path: &str,
    canvas_width: u32,
    canvas_height: u32,
    objects: serde_json::Value,
) -> Result<String, String> {
    let source = fs::canonicalize(image_path).map_err(|error| format!("無法讀取底圖：{error}"))?;
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or("擷圖檔名不正確")?;
    let history_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("history");
    fs::create_dir_all(&history_dir).map_err(|error| format!("無法建立歷史資料夾：{error}"))?;
    let output = history_dir.join(format!("{stem}.captureflow.json"));
    let saved = save_project(
        app,
        image_path,
        canvas_width,
        canvas_height,
        objects,
        Some(output.to_string_lossy().into_owned()),
    )?;
    prune_history(app, crate::settings::history_limit(app))?;
    Ok(saved)
}

pub fn list_history(app: &AppHandle) -> Result<Vec<HistoryEntry>, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let history_dir = app_data.join("history");
    let mut entries = Vec::new();
    let Ok(files) = fs::read_dir(history_dir) else {
        return Ok(entries);
    };
    for file in files.flatten() {
        let path = file.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(project) = serde_json::from_slice::<AnnotationProject>(&bytes) else {
            continue;
        };
        let source = std::path::Path::new(&project.source_image);
        if !source.exists() {
            continue;
        }
        let metadata = source.with_extension("json");
        let selection = fs::read(&metadata)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| value.get("selection").cloned());
        let thumbnail_data_url = history_thumbnail(source);
        entries.push(HistoryEntry {
            project_path: path.to_string_lossy().into_owned(),
            image_path: project.source_image,
            created_at_unix_ms: project.created_at_unix_ms,
            canvas_width: project.canvas_width,
            canvas_height: project.canvas_height,
            selection,
            thumbnail_data_url,
        });
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at_unix_ms));
    entries.truncate(crate::settings::history_limit(app));
    Ok(entries)
}

pub fn prune_history(app: &AppHandle, keep: usize) -> Result<(), String> {
    let history_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("history");
    let mut entries: Vec<(u128, std::path::PathBuf, String)> = fs::read_dir(history_dir)
        .map_err(|error| error.to_string())?
        .flatten()
        .filter_map(|file| {
            let path = file.path();
            let project =
                serde_json::from_slice::<AnnotationProject>(&fs::read(&path).ok()?).ok()?;
            Some((project.created_at_unix_ms, path, project.source_image))
        })
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, project_path, image_path) in entries.into_iter().skip(keep) {
        let image = std::path::PathBuf::from(image_path);
        let _ = fs::remove_file(project_path);
        let _ = fs::remove_file(image.with_extension("json"));
        let _ = fs::remove_file(image);
    }
    Ok(())
}

pub fn clear_history(app: &AppHandle) -> Result<(), String> {
    let history_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("history");
    let Ok(files) = fs::read_dir(&history_dir) else {
        return Ok(());
    };
    for file in files.flatten() {
        let project_path = file.path();
        let image_path = fs::read(&project_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AnnotationProject>(&bytes).ok())
            .map(|project| project.source_image);
        let Some(image_path) = image_path else {
            continue;
        };
        let image = std::path::PathBuf::from(image_path);
        let _ = fs::remove_file(project_path);
        let _ = fs::remove_file(image.with_extension("json"));
        let _ = fs::remove_file(image);
    }
    Ok(())
}

pub fn load_project_from(app: &AppHandle, project_path: &str) -> Result<AnnotationProject, String> {
    let path = std::path::Path::new(project_path);
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err("請選擇 JSON 專案檔案".into());
    }
    let bytes = fs::read(path).map_err(|error| format!("無法讀取專案：{error}"))?;
    let project: AnnotationProject =
        serde_json::from_slice(&bytes).map_err(|error| format!("專案格式不正確：{error}"))?;
    if !matches!(project.schema_version, 1 | 2) || project.objects.as_array().is_none() {
        return Err("不支援的 CaptureFlow 專案格式".into());
    }
    let app_data = fs::canonicalize(
        app.path()
            .app_data_dir()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if let Ok(source) = fs::canonicalize(&project.source_image) {
        if source.starts_with(&app_data) {
            return Ok(project);
        }
    }
    let encoded = project
        .source_image_base64
        .as_deref()
        .ok_or("專案沒有內嵌來源圖片，且原始圖片已不存在")?;
    let bytes = BASE64
        .decode(encoded)
        .map_err(|error| format!("內嵌來源圖片格式不正確：{error}"))?;
    image::load_from_memory(&bytes).map_err(|error| format!("內嵌來源圖片無法讀取：{error}"))?;
    let directory = app_data.join("imported-projects");
    fs::create_dir_all(&directory).map_err(|error| format!("無法建立專案圖片資料夾：{error}"))?;
    let output = directory.join(format!("project-{}.png", project.created_at_unix_ms));
    fs::write(&output, bytes).map_err(|error| format!("無法還原專案來源圖片：{error}"))?;
    let mut restored = project;
    restored.source_image = output.to_string_lossy().into_owned();
    Ok(restored)
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
        return Err("只能開啟 CaptureFlow 產生的擷圖專案。".into());
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
        if matches!(project.schema_version, 1 | 2)
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
        .ok_or_else(|| "找不到目前擷圖的可編輯專案。請先儲存一次。".to_string())
}
