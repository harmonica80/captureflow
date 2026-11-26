use crate::capture::{capture_frame, MonitorInfo, RectInfo};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use std::{fs, path::PathBuf, time::SystemTime};
use tauri::{AppHandle, Manager};
use tiny_skia::{FillRule, Paint, PathBuilder, PixmapMut, Rect as SkRect, Stroke, Transform};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshot {
    pub image_path: String,
    pub metadata_path: String,
    pub selection: RectInfo,
    pub width: i32,
    pub height: i32,
    pub corner_radius: i32,
    pub objects: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sticker_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectedArea {
    selection: RectInfo,
    corner_radius: i32,
    #[serde(default)]
    annotations: Vec<NativeAnnotation>,
    #[serde(skip)]
    long_capture: Option<LongCaptureResult>,
    #[serde(skip)]
    pin: bool,
}

#[derive(Clone, Debug)]
struct LongCaptureResult {
    bgra: Vec<u8>,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum NumberShape {
    #[default]
    Circle,
    RoundedRectangle,
}

impl NumberShape {
    fn aspect_ratio(self) -> f32 {
        match self {
            Self::Circle => 1.0,
            Self::RoundedRectangle => 1.35,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
    DashDot,
}

fn default_headed() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum NativeAnnotation {
    Rectangle {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        stroke_width: i32,
        #[serde(default)]
        style: LineStyle,
        color: u32,
    },
    Arrow {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        stroke_width: i32,
        #[serde(default)]
        style: LineStyle,
        #[serde(default = "default_headed")]
        headed: bool,
        control_x: i32,
        control_y: i32,
        color: u32,
    },
    Ellipse {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        stroke_width: i32,
        #[serde(default)]
        style: LineStyle,
        color: u32,
    },
    Freehand {
        #[serde(with = "BigArray")]
        points_x: [i32; 128],
        #[serde(with = "BigArray")]
        points_y: [i32; 128],
        length: usize,
        stroke_width: i32,
        #[serde(default)]
        style: LineStyle,
        color: u32,
    },
    Number {
        x: i32,
        y: i32,
        number: u32,
        size: i32,
        color: u32,
        #[serde(default)]
        shape: NumberShape,
        #[serde(default)]
        label_color: Option<u32>,
    },
    Mosaic {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        block_size: i32,
    },
    Text {
        x: i32,
        y: i32,
        text: [u16; 32],
        length: usize,
        font_size: i32,
        color: u32,
        outline_color: u32,
        #[serde(default = "default_text_outline_width")]
        outline_width: i32,
    },
}

fn default_text_outline_width() -> i32 {
    1
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionMetadata<'a> {
    schema_version: u32,
    captured_at_unix_ms: u128,
    virtual_desktop: RectInfo,
    selection: RectInfo,
    monitors: &'a [MonitorInfo],
    corner_radius: i32,
}

pub async fn select_area(
    app: AppHandle,
    initial_selection: Option<RectInfo>,
) -> Result<Option<SelectionSnapshot>, String> {
    let frame = capture_frame()?;
    let virtual_desktop = frame.virtual_desktop;
    let initial_selection = initial_selection
        .map(|rect| local_selection(rect, virtual_desktop))
        .transpose()?;
    let monitor_bounds = frame
        .monitors
        .iter()
        .filter_map(|monitor| local_selection(monitor.bounds, virtual_desktop).ok())
        .collect();
    let english = crate::settings::view(&app).is_ok_and(|settings| settings.language == "en");
    let rgba_for_window = frame.rgba.clone();

    let selected = tauri::async_runtime::spawn_blocking(move || {
        platform::run_selector(
            virtual_desktop,
            rgba_for_window,
            initial_selection,
            monitor_bounds,
            english,
        )
    })
    .await
    .map_err(|error| format!("選取執行緒異常結束：{error}"))??;

    let Some(selected) = selected else {
        return Ok(None);
    };
    let local_selection = selected.selection;
    let global_selection = RectInfo {
        x: virtual_desktop.x + local_selection.x,
        y: virtual_desktop.y + local_selection.y,
        width: local_selection.width,
        height: local_selection.height,
    };
    let (mut rgba, width, height) = if let Some(long) = selected.long_capture {
        let mut rgba = long.bgra;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        (rgba, long.width, long.height)
    } else {
        (
            crop_rgba(
                &frame.rgba,
                virtual_desktop.width,
                virtual_desktop.height,
                local_selection,
            )?,
            local_selection.width,
            local_selection.height,
        )
    };
    apply_rounded_alpha(&mut rgba, width, height, selected.corner_radius);
    let mut snapshot = write_selection(
        &app,
        &frame,
        global_selection,
        selected.corner_radius,
        width,
        height,
        &rgba,
    )?;
    snapshot.objects = editor_objects(&selected.annotations, local_selection);
    save_last_selection(&app, global_selection, selected.corner_radius)?;
    if selected.pin {
        apply_native_annotations(
            &mut rgba,
            RectInfo {
                width,
                height,
                ..local_selection
            },
            &selected.annotations,
        );
        apply_rounded_alpha(&mut rgba, width, height, selected.corner_radius);
        let preview_path =
            crate::annotation::save_composited_image(&app, width as u32, height as u32, rgba)?;
        snapshot.sticker_id = Some(crate::sticker::open(
            &app,
            preview_path,
            global_selection.x,
            global_selection.y,
            Some(crate::sticker::EditableDocument {
                source_image: snapshot.image_path.clone(),
                canvas_width: width as u32,
                canvas_height: height as u32,
                objects: serde_json::Value::Array(snapshot.objects.clone()),
            }),
        )?);
    }
    Ok(Some(snapshot))
}

fn frame_difference(first: &[u8], second: &[u8]) -> f64 {
    let mut total = 0u64;
    let mut count = 0u64;
    for index in (0..first.len().min(second.len())).step_by(64) {
        total += first[index].abs_diff(second[index]) as u64;
        count += 1;
    }
    total as f64 / count.max(1) as f64
}

fn best_vertical_overlap(previous: &[u8], current: &[u8], width: usize, height: usize) -> usize {
    let row = width * 4;
    let mut best = (f64::MAX, 0usize);
    for overlap in (height / 10..height * 9 / 10).step_by(4) {
        let previous_start = height - overlap;
        let mut difference = 0u64;
        let mut samples = 0u64;
        for y in (0..overlap).step_by(8) {
            for x in (width / 10..width * 9 / 10).step_by(12) {
                let a = (previous_start + y) * row + x * 4;
                let b = y * row + x * 4;
                difference += previous[a].abs_diff(current[b]) as u64;
                difference += previous[a + 1].abs_diff(current[b + 1]) as u64;
                difference += previous[a + 2].abs_diff(current[b + 2]) as u64;
                samples += 3;
            }
        }
        let score = difference as f64 / samples.max(1) as f64;
        if score < best.0 {
            best = (score, overlap);
        }
    }
    if best.0 <= 22.0 {
        best.1
    } else {
        0
    }
}

pub fn repeat_last_area(app: AppHandle) -> Result<SelectionSnapshot, String> {
    let previous = load_last_selection(&app)?;
    recapture_area(&app, previous.selection, previous.corner_radius)
}

fn recapture_area(
    app: &AppHandle,
    global_selection: RectInfo,
    corner_radius: i32,
) -> Result<SelectionSnapshot, String> {
    let frame = capture_frame()?;
    let local_selection = local_selection(global_selection, frame.virtual_desktop)?;
    save_selection(
        app,
        &frame,
        local_selection,
        global_selection,
        corner_radius,
    )
}

fn local_selection(global: RectInfo, desktop: RectInfo) -> Result<RectInfo, String> {
    let error = "擷取範圍已超出目前螢幕，請重新框選。";
    let x = i64::from(global.x) - i64::from(desktop.x);
    let y = i64::from(global.y) - i64::from(desktop.y);
    if x < 0
        || y < 0
        || global.width <= 0
        || global.height <= 0
        || x + i64::from(global.width) > i64::from(desktop.width)
        || y + i64::from(global.height) > i64::from(desktop.height)
    {
        return Err(error.into());
    }
    Ok(RectInfo {
        x: x as i32,
        y: y as i32,
        ..global
    })
}

pub fn capture_monitor(app: AppHandle, device_name: &str) -> Result<SelectionSnapshot, String> {
    let frame = capture_frame()?;
    let monitor = frame
        .monitors
        .iter()
        .find(|monitor| monitor.device_name == device_name)
        .ok_or_else(|| format!("找不到顯示器：{device_name}"))?;
    let global_selection = monitor.bounds;
    let local_selection = RectInfo {
        x: global_selection.x - frame.virtual_desktop.x,
        y: global_selection.y - frame.virtual_desktop.y,
        width: global_selection.width,
        height: global_selection.height,
    };
    let snapshot = save_selection(&app, &frame, local_selection, global_selection, 0)?;
    save_last_selection(&app, global_selection, 0)?;
    Ok(snapshot)
}

fn save_selection(
    app: &AppHandle,
    frame: &crate::capture::DesktopFrame,
    local_selection: RectInfo,
    global_selection: RectInfo,
    corner_radius: i32,
) -> Result<SelectionSnapshot, String> {
    let mut cropped = crop_rgba(
        &frame.rgba,
        frame.virtual_desktop.width,
        frame.virtual_desktop.height,
        local_selection,
    )?;
    apply_rounded_alpha(
        &mut cropped,
        local_selection.width,
        local_selection.height,
        corner_radius,
    );

    write_selection(
        app,
        frame,
        global_selection,
        corner_radius,
        local_selection.width,
        local_selection.height,
        &cropped,
    )
}

fn write_selection(
    app: &AppHandle,
    frame: &crate::capture::DesktopFrame,
    global_selection: RectInfo,
    corner_radius: i32,
    width: i32,
    height: i32,
    rgba: &[u8],
) -> Result<SelectionSnapshot, String> {
    let captured_at_unix_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| format!("無法取得系統時間：{error}"))?
        .as_millis();
    let output_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?
        .join("poc-b");
    fs::create_dir_all(&output_dir).map_err(|error| format!("無法建立 PoC-B 輸出目錄：{error}"))?;

    let image_path = output_dir.join(format!("selection-{captured_at_unix_ms}.png"));
    let metadata_path = output_dir.join(format!("selection-{captured_at_unix_ms}.json"));
    image::save_buffer_with_format(
        &image_path,
        rgba,
        width as u32,
        height as u32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("無法儲存選取 PNG：{error}"))?;

    let metadata = SelectionMetadata {
        schema_version: 1,
        captured_at_unix_ms,
        virtual_desktop: frame.virtual_desktop,
        selection: global_selection,
        monitors: &frame.monitors,
        corner_radius,
    };
    fs::write(
        &metadata_path,
        serde_json::to_vec_pretty(&metadata)
            .map_err(|error| format!("無法建立選取 JSON：{error}"))?,
    )
    .map_err(|error| format!("無法儲存選取 JSON：{error}"))?;

    Ok(SelectionSnapshot {
        image_path: path_string(image_path),
        metadata_path: path_string(metadata_path),
        selection: global_selection,
        width,
        height,
        corner_radius,
        objects: Vec::new(),
        sticker_id: None,
    })
}

fn last_selection_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?
        .join("last-selection.json"))
}

fn save_last_selection(
    app: &AppHandle,
    selection: RectInfo,
    corner_radius: i32,
) -> Result<(), String> {
    let path = last_selection_path(app)?;
    fs::write(
        path,
        serde_json::to_vec_pretty(&SelectedArea {
            selection,
            corner_radius,
            annotations: Vec::new(),
            long_capture: None,
            pin: false,
        })
        .map_err(|error| format!("無法建立上次範圍資料：{error}"))?,
    )
    .map_err(|error| format!("無法儲存上次範圍：{error}"))
}

fn load_last_selection(app: &AppHandle) -> Result<SelectedArea, String> {
    let path = last_selection_path(app)?;
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "尚無上次擷取範圍，請先完成一次框選。".to_string()
        } else {
            format!("無法讀取上次範圍：{error}")
        }
    })?;
    if let Ok(area) = serde_json::from_slice::<SelectedArea>(&bytes) {
        return Ok(area);
    }
    serde_json::from_slice::<RectInfo>(&bytes)
        .map(|selection| SelectedArea {
            selection,
            corner_radius: 0,
            annotations: Vec::new(),
            long_capture: None,
            pin: false,
        })
        .map_err(|error| format!("上次範圍資料已損壞：{error}"))
}

fn apply_native_annotations(
    pixels: &mut [u8],
    selection: RectInfo,
    annotations: &[NativeAnnotation],
) {
    let Some(mut pixmap) = PixmapMut::from_bytes(
        pixels,
        selection.width.max(1) as u32,
        selection.height.max(1) as u32,
    ) else {
        return;
    };
    let is_mosaic =
        |annotation: &&NativeAnnotation| matches!(annotation, NativeAnnotation::Mosaic { .. });
    for annotation in annotations.iter().filter(is_mosaic).chain(
        annotations
            .iter()
            .filter(|annotation| !is_mosaic(annotation)),
    ) {
        match *annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                color,
            } => {
                let left = start_x.min(end_x) - selection.x;
                let right = start_x.max(end_x) - selection.x;
                let top = start_y.min(end_y) - selection.y;
                let bottom = start_y.max(end_y) - selection.y;
                if let Some(rect) =
                    SkRect::from_ltrb(left as f32, top as f32, right as f32, bottom as f32)
                {
                    let mut path = PathBuilder::new();
                    path.push_rect(rect);
                    let path = path.finish().unwrap();
                    pixmap.stroke_path(
                        &path,
                        &skia_paint(color),
                        &skia_stroke(stroke_width, style),
                        Transform::identity(),
                        None,
                    );
                }
            }
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                headed,
                control_x,
                control_y,
                color,
            } => {
                let (x1, y1) = (start_x - selection.x, start_y - selection.y);
                let (x2, y2) = (end_x - selection.x, end_y - selection.y);
                let (cx, cy) = (control_x - selection.x, control_y - selection.y);
                let mut path = PathBuilder::new();
                path.move_to(x1 as f32, y1 as f32);
                if headed {
                    let points =
                        curved_arrow_points(x1, y1, x2, y2, cx, cy, f64::from(stroke_width));
                    if style == LineStyle::Solid {
                        fill_skia_polygon(&mut pixmap, &points, color);
                        continue;
                    }
                    let head_start = ARROW_SHAFT_STEPS + 1;
                    let head = &points[head_start..head_start + 3];
                    let neck_x = (head[0].0 + head[2].0) as f32 / 2.0;
                    let neck_y = (head[0].1 + head[2].1) as f32 / 2.0;
                    path.quad_to(cx as f32, cy as f32, neck_x, neck_y);
                    fill_skia_polygon(&mut pixmap, head, color);
                } else {
                    path.line_to(x2 as f32, y2 as f32);
                }
                if let Some(path) = path.finish() {
                    pixmap.stroke_path(
                        &path,
                        &skia_paint(color),
                        &skia_stroke(stroke_width, style),
                        Transform::identity(),
                        None,
                    );
                }
            }
            NativeAnnotation::Ellipse {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                color,
            } => {
                let left = start_x.min(end_x) - selection.x;
                let right = start_x.max(end_x) - selection.x;
                let top = start_y.min(end_y) - selection.y;
                let bottom = start_y.max(end_y) - selection.y;
                if let Some(rect) =
                    SkRect::from_ltrb(left as f32, top as f32, right as f32, bottom as f32)
                {
                    let mut path = PathBuilder::new();
                    path.push_oval(rect);
                    let path = path.finish().unwrap();
                    pixmap.stroke_path(
                        &path,
                        &skia_paint(color),
                        &skia_stroke(stroke_width, style),
                        Transform::identity(),
                        None,
                    );
                }
            }
            NativeAnnotation::Freehand {
                points_x,
                points_y,
                length,
                stroke_width,
                style,
                color,
            } => {
                let length = length.min(points_x.len());
                if length >= 2 {
                    let mut path = PathBuilder::new();
                    path.move_to(
                        (points_x[0] - selection.x) as f32,
                        (points_y[0] - selection.y) as f32,
                    );
                    for index in 1..length {
                        let x = (points_x[index] - selection.x) as f32;
                        let y = (points_y[index] - selection.y) as f32;
                        if index + 1 < length {
                            let next_x = (points_x[index + 1] - selection.x) as f32;
                            let next_y = (points_y[index + 1] - selection.y) as f32;
                            path.quad_to(x, y, (x + next_x) * 0.5, (y + next_y) * 0.5);
                        } else {
                            path.line_to(x, y);
                        }
                    }
                    if let Some(path) = path.finish() {
                        pixmap.stroke_path(
                            &path,
                            &skia_paint(color),
                            &skia_stroke(stroke_width, style),
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            NativeAnnotation::Number {
                x,
                y,
                number,
                size,
                color,
                shape,
                label_color,
            } => {
                let center = (x - selection.x, y - selection.y);
                let height = size.clamp(18, 96) as f32;
                let width = height * shape.aspect_ratio();
                let mut path = PathBuilder::new();
                if let Some(rect) = SkRect::from_xywh(
                    center.0 as f32 - width / 2.0,
                    center.1 as f32 - height / 2.0,
                    width,
                    height,
                ) {
                    match shape {
                        NumberShape::Circle => path.push_oval(rect),
                        NumberShape::RoundedRectangle => {
                            push_rounded_rectangle(&mut path, rect, height * 0.22);
                        }
                    }
                }
                if let Some(path) = path.finish() {
                    pixmap.fill_path(
                        &path,
                        &skia_paint(color),
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
                draw_number_label(
                    &mut pixmap,
                    center,
                    number,
                    height * 0.55,
                    width * 0.75,
                    number_ink(label_color, color),
                );
            }
            NativeAnnotation::Mosaic {
                start_x,
                start_y,
                end_x,
                end_y,
                block_size,
            } => pixelate_rgba_region(
                pixmap.data_mut(),
                selection.width,
                selection.height,
                start_x - selection.x,
                start_y - selection.y,
                end_x - selection.x,
                end_y - selection.y,
                block_size,
            ),
            NativeAnnotation::Text {
                x,
                y,
                text,
                length,
                font_size,
                color,
                outline_color,
                outline_width,
            } => {
                draw_rgba_text(
                    pixmap.data_mut(),
                    selection.width,
                    selection.height,
                    x - selection.x,
                    y - selection.y,
                    &text[..length.min(32)],
                    font_size,
                    color,
                    outline_color,
                    outline_width,
                );
            }
        }
    }
}

fn skia_paint(color: u32) -> Paint<'static> {
    let rgba = colorref_to_rgba(color);
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    paint.anti_alias = true;
    paint
}

fn skia_stroke(width: i32, style: LineStyle) -> Stroke {
    let width = width.max(1) as f32;
    let intervals: &[f32] = match style {
        LineStyle::Solid => &[],
        LineStyle::Dashed => &[4.0, 2.5],
        LineStyle::Dotted => &[0.4, 2.6],
        LineStyle::DashDot => &[4.0, 2.0, 0.4, 2.0],
    };
    Stroke {
        width,
        dash: tiny_skia::StrokeDash::new(
            intervals.iter().map(|interval| interval * width).collect(),
            0.0,
        ),
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Default::default()
    }
}

fn fill_skia_polygon(pixmap: &mut PixmapMut<'_>, points: &[(i32, i32)], color: u32) {
    let Some(first) = points.first() else {
        return;
    };
    let mut builder = PathBuilder::new();
    builder.move_to(first.0 as f32, first.1 as f32);
    for point in &points[1..] {
        builder.line_to(point.0 as f32, point.1 as f32);
    }
    builder.close();
    if let Some(path) = builder.finish() {
        pixmap.fill_path(
            &path,
            &skia_paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn push_rounded_rectangle(path: &mut PathBuilder, rect: SkRect, radius: f32) {
    let (left, top, right, bottom) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    path.move_to(left + radius, top);
    path.line_to(right - radius, top);
    path.quad_to(right, top, right, top + radius);
    path.line_to(right, bottom - radius);
    path.quad_to(right, bottom, right - radius, bottom);
    path.line_to(left + radius, bottom);
    path.quad_to(left, bottom, left, bottom - radius);
    path.line_to(left, top + radius);
    path.quad_to(left, top, left + radius, top);
    path.close();
}

fn draw_number_label(
    pixmap: &mut PixmapMut<'_>,
    center: (i32, i32),
    number: u32,
    mut font_size: f32,
    max_width: f32,
    ink: u32,
) {
    let Some(font) = annotation_font() else {
        return;
    };
    let label = number.to_string();
    let layout = |size| {
        let mut cursor: f32 = 0.0;
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let glyphs: Vec<_> = label
            .chars()
            .map(|digit| {
                let metrics = font.metrics(digit, size);
                let x = cursor.round() as i32 + metrics.xmin;
                let y = -metrics.ymin - metrics.height as i32;
                cursor += metrics.advance_width;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + metrics.width as i32);
                bottom = bottom.max(y + metrics.height as i32);
                (digit, x, y)
            })
            .collect();
        (glyphs, left, top, right, bottom)
    };
    let (mut glyphs, mut left, mut top, mut right, mut bottom) = layout(font_size);
    let ink_width = (right - left) as f32;
    if ink_width > max_width {
        font_size *= max_width / ink_width;
        (glyphs, left, top, right, bottom) = layout(font_size);
    }
    let x_offset = center.0 - (left + right) / 2;
    let y_offset = center.1 - (top + bottom) / 2;
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    let pixels = pixmap.data_mut();
    for (digit, x, y) in glyphs {
        let (metrics, bitmap) = font.rasterize(digit, font_size);
        for (index, alpha) in bitmap
            .into_iter()
            .enumerate()
            .filter(|&(_, alpha)| alpha != 0)
        {
            blend_rgba_pixel(
                pixels,
                width,
                height,
                x_offset + x + (index % metrics.width) as i32,
                y_offset + y + (index / metrics.width) as i32,
                ink,
                alpha,
            );
        }
    }
}

fn color_luminance(color: u32) -> f64 {
    let [red, green, blue, _] = colorref_to_rgba(color);
    let linear = |channel| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
}

fn number_ink(foreground: Option<u32>, background: u32) -> u32 {
    foreground.unwrap_or_else(|| {
        if color_luminance(background) >= 0.179 {
            0
        } else {
            0x00FF_FFFF
        }
    })
}

fn annotation_font() -> Option<&'static fontdue::Font> {
    static FONT: std::sync::OnceLock<Option<fontdue::Font>> = std::sync::OnceLock::new();
    let font = FONT.get_or_init(|| {
        [
            r"C:\Windows\Fonts\msjh.ttc",
            r"C:\Windows\Fonts\segoeui.ttf",
        ]
        .into_iter()
        .find_map(|path| {
            let bytes = fs::read(path).ok()?;
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
        })
    });
    font.as_ref()
}

fn draw_rgba_text(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    utf16: &[u16],
    font_size: i32,
    color: u32,
    outline_color: u32,
    outline_width: i32,
) {
    let Some(font) = annotation_font() else {
        return;
    };
    let content = String::from_utf16_lossy(utf16);
    let mut cursor_x = x;
    for character in content.chars() {
        let (metrics, bitmap) = font.rasterize(character, font_size.max(8) as f32);
        let glyph_x = cursor_x + metrics.xmin;
        let glyph_y = y + font_size - metrics.height as i32 - metrics.ymin;
        for row in 0..metrics.height {
            for column in 0..metrics.width {
                let alpha = bitmap[row * metrics.width + column];
                if alpha == 0 {
                    continue;
                }
                let radius = outline_width.clamp(0, 12);
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx == 0 && dy == 0 || dx * dx + dy * dy > radius * radius {
                            continue;
                        }
                        blend_rgba_pixel(
                            pixels,
                            width,
                            height,
                            glyph_x + column as i32 + dx,
                            glyph_y + row as i32 + dy,
                            outline_color,
                            alpha,
                        );
                    }
                }
                for (dx, dy, draw_color) in [(0, 0, color)] {
                    blend_rgba_pixel(
                        pixels,
                        width,
                        height,
                        glyph_x + column as i32 + dx,
                        glyph_y + row as i32 + dy,
                        draw_color,
                        alpha,
                    );
                }
            }
        }
        cursor_x += metrics.advance_width.round() as i32;
    }
}

fn blend_rgba_pixel(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    color: u32,
    alpha: u8,
) {
    if x < 0 || y < 0 || x >= width || y >= height {
        return;
    }
    let index = ((y * width + x) * 4) as usize;
    let source = colorref_to_rgba(color);
    let a = u16::from(alpha);
    for channel in 0..3 {
        pixels[index + channel] = ((u16::from(source[channel]) * a
            + u16::from(pixels[index + channel]) * (255 - a))
            / 255) as u8;
    }
    pixels[index + 3] = 255;
}

fn pixelate_rgba_region(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    block_size: i32,
) {
    let left = start_x.min(end_x).clamp(0, width);
    let right = start_x.max(end_x).clamp(0, width);
    let top = start_y.min(end_y).clamp(0, height);
    let bottom = start_y.max(end_y).clamp(0, height);
    let block = block_size.clamp(4, 48) as usize;
    for block_y in (top as usize..bottom as usize).step_by(block) {
        for block_x in (left as usize..right as usize).step_by(block) {
            let max_y = (block_y + block).min(bottom as usize);
            let max_x = (block_x + block).min(right as usize);
            let mut sums = [0u64; 4];
            let mut count = 0u64;
            for y in block_y..max_y {
                for x in block_x..max_x {
                    let index = (y * width as usize + x) * 4;
                    for channel in 0..4 {
                        sums[channel] += u64::from(pixels[index + channel]);
                    }
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let average = [
                (sums[0] / count) as u8,
                (sums[1] / count) as u8,
                (sums[2] / count) as u8,
                (sums[3] / count) as u8,
            ];
            for y in block_y..max_y {
                for x in block_x..max_x {
                    let index = (y * width as usize + x) * 4;
                    pixels[index..index + 4].copy_from_slice(&average);
                }
            }
        }
    }
}

fn draw_rgba_ellipse(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    thickness: i32,
    color: u32,
) {
    let cx = f64::from(x1 + x2) / 2.0;
    let cy = f64::from(y1 + y2) / 2.0;
    let rx = f64::from((x2 - x1).abs()).max(2.0) / 2.0;
    let ry = f64::from((y2 - y1).abs()).max(2.0) / 2.0;
    let steps = ((rx + ry) * 3.2).max(48.0) as i32;
    let mut previous = ((cx + rx).round() as i32, cy.round() as i32);
    for index in 1..=steps {
        let angle = std::f64::consts::TAU * f64::from(index) / f64::from(steps);
        let current = (
            (cx + rx * angle.cos()).round() as i32,
            (cy + ry * angle.sin()).round() as i32,
        );
        draw_rgba_line(
            pixels, width, height, previous.0, previous.1, current.0, current.1, thickness, color,
        );
        previous = current;
    }
}

fn tapered_arrow_points(x1: i32, y1: i32, x2: i32, y2: i32, stroke_width: f64) -> Vec<(i32, i32)> {
    curved_arrow_points(x1, y1, x2, y2, (x1 + x2) / 2, (y1 + y2) / 2, stroke_width)
}

const ARROW_SHAFT_STEPS: usize = 12;

fn curved_arrow_points(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    cx: i32,
    cy: i32,
    stroke_width: f64,
) -> Vec<(i32, i32)> {
    let dx = f64::from(x2 - x1);
    let dy = f64::from(y2 - y1);
    let length = dx.hypot(dy).max(1.0);
    let tangent_x = f64::from(x2 - cx);
    let tangent_y = f64::from(y2 - cy);
    let tangent_length = tangent_x.hypot(tangent_y).max(1.0);
    let (ux, uy) = (tangent_x / tangent_length, tangent_y / tangent_length);
    let (px, py) = (-uy, ux);
    let head_length = (length * 0.45).min(stroke_width * 6.0);
    let (neck_x, neck_y) = (
        f64::from(x2) - ux * head_length,
        f64::from(y2) - uy * head_length,
    );
    let tail_half = (stroke_width * 0.22).max(0.7);
    let neck_half = stroke_width * 1.15;
    let head_half = stroke_width * 3.5;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for index in 0..=ARROW_SHAFT_STEPS {
        let t = index as f64 / ARROW_SHAFT_STEPS as f64;
        let inverse = 1.0 - t;
        let x =
            inverse * inverse * f64::from(x1) + 2.0 * inverse * t * f64::from(cx) + t * t * neck_x;
        let y =
            inverse * inverse * f64::from(y1) + 2.0 * inverse * t * f64::from(cy) + t * t * neck_y;
        let tx = 2.0 * inverse * f64::from(cx - x1) + 2.0 * t * (neck_x - f64::from(cx));
        let ty = 2.0 * inverse * f64::from(cy - y1) + 2.0 * t * (neck_y - f64::from(cy));
        let tangent = tx.hypot(ty).max(1.0);
        let half = tail_half + (neck_half - tail_half) * t;
        left.push((
            (x - ty / tangent * half).round() as i32,
            (y + tx / tangent * half).round() as i32,
        ));
        right.push((
            (x + ty / tangent * half).round() as i32,
            (y - tx / tangent * half).round() as i32,
        ));
    }
    let mut points = left;
    points.extend(
        [
            (neck_x + px * head_half, neck_y + py * head_half),
            (f64::from(x2), f64::from(y2)),
            (neck_x - px * head_half, neck_y - py * head_half),
        ]
        .into_iter()
        .map(|(x, y)| (x.round() as i32, y.round() as i32)),
    );
    right.reverse();
    points.extend(right);
    points
}

fn fill_rgba_polygon(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    points: &[(i32, i32)],
    color: u32,
) {
    if points.len() < 3 || width <= 0 || height <= 0 {
        return;
    }
    let min_x = points
        .iter()
        .map(|p| p.0)
        .min()
        .unwrap_or(0)
        .clamp(0, width - 1);
    let max_x = points
        .iter()
        .map(|p| p.0)
        .max()
        .unwrap_or(0)
        .clamp(0, width - 1);
    let min_y = points
        .iter()
        .map(|p| p.1)
        .min()
        .unwrap_or(0)
        .clamp(0, height - 1);
    let max_y = points
        .iter()
        .map(|p| p.1)
        .max()
        .unwrap_or(0)
        .clamp(0, height - 1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let mut inside = false;
            let mut previous = points.len() - 1;
            for current in 0..points.len() {
                let (xi, yi) = points[current];
                let (xj, yj) = points[previous];
                if (yi > y) != (yj > y)
                    && f64::from(x)
                        < f64::from(xj - xi) * f64::from(y - yi) / f64::from(yj - yi)
                            + f64::from(xi)
                {
                    inside = !inside;
                }
                previous = current;
            }
            if inside {
                let index = ((y * width + x) * 4) as usize;
                pixels[index..index + 4].copy_from_slice(&colorref_to_rgba(color));
            }
        }
    }
}

fn draw_rgba_line(
    pixels: &mut [u8],
    width: i32,
    height: i32,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: u32,
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        for py in y0 - thickness..=y0 + thickness {
            for px in x0 - thickness..=x0 + thickness {
                if px >= 0
                    && py >= 0
                    && px < width
                    && py < height
                    && (px - x0).pow(2) + (py - y0).pow(2) <= thickness.pow(2)
                {
                    let index = ((py * width + px) * 4) as usize;
                    pixels[index..index + 4].copy_from_slice(&colorref_to_rgba(color));
                }
            }
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x0 += sx;
        }
        if twice <= dx {
            error += dx;
            y0 += sy;
        }
    }
}

fn colorref_to_rgba(color: u32) -> [u8; 4] {
    [
        (color & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        ((color >> 16) & 0xff) as u8,
        255,
    ]
}

fn editor_objects(annotations: &[NativeAnnotation], area: RectInfo) -> Vec<serde_json::Value> {
    use serde_json::json;
    let x = |position: i32| position - area.x;
    let y = |position: i32| position - area.y;
    let hex = |color| {
        let [r, g, b, _] = colorref_to_rgba(color);
        format!("#{r:02x}{g:02x}{b:02x}")
    };
    let bounds = |x1: i32, y1: i32, x2: i32, y2: i32| {
        (
            x(x1.min(x2)),
            y(y1.min(y2)),
            x1.abs_diff(x2),
            y1.abs_diff(y2),
        )
    };
    annotations.iter().enumerate().map(|(index, annotation)| {
        let mut object = match *annotation {
            NativeAnnotation::Rectangle { start_x, start_y, end_x, end_y, stroke_width, color, style }
            | NativeAnnotation::Ellipse { start_x, start_y, end_x, end_y, stroke_width, color, style } => {
                let (x, y, width, height) = bounds(start_x, start_y, end_x, end_y);
                let kind = if matches!(annotation, NativeAnnotation::Rectangle { .. }) { "rectangle" } else { "ellipse" };
                json!({"type": kind, "x": x, "y": y, "width": width, "height": height,
                    "color": hex(color), "strokeWidth": stroke_width, "lineStyle": style})
            }
            NativeAnnotation::Arrow { start_x, start_y, end_x, end_y, stroke_width, color, style, headed, control_x, control_y } => {
                let kind = if headed { "arrow" } else { "line" };
                json!({"type": kind, "x1": x(start_x), "y1": y(start_y), "x2": x(end_x), "y2": y(end_y),
                    "cx": x(control_x), "cy": y(control_y), "color": hex(color), "strokeWidth": stroke_width, "lineStyle": style})
            }
            NativeAnnotation::Freehand { points_x, points_y, length, stroke_width, color, style } => {
                let points: Vec<_> = points_x.iter().zip(points_y.iter()).take(length)
                    .map(|(&px,&py)| json!({"x": x(px), "y": y(py)})).collect();
                json!({"type": "pen", "points": points, "color": hex(color),
                    "strokeWidth": stroke_width, "lineStyle": style})
            }
            NativeAnnotation::Text { x: px, y: py, text, length, font_size, color, outline_color, outline_width } => {
                json!({"type": "text", "x": x(px), "y": y(py), "text": String::from_utf16_lossy(&text[..length.min(text.len())]),
                    "fontSize": font_size, "fontFamily": "Microsoft JhengHei", "bold": false, "italic": false, "color": hex(color),
                    "strokeWidth": 1, "outlineColor": hex(outline_color), "outlineWidth": outline_width})
            }
            NativeAnnotation::Number { x: px, y: py, number, size, color, shape, label_color } => {
                json!({"type": "number", "x": x(px), "y": y(py), "number": number, "size": size,
                    "color": hex(color), "strokeWidth": 1, "shape": shape, "labelColor": label_color.map(hex)})
            }
            NativeAnnotation::Mosaic { start_x, start_y, end_x, end_y, block_size } => {
                let (x, y, width, height) = bounds(start_x, start_y, end_x, end_y);
                json!({"type": "mosaic", "x": x, "y": y, "width": width, "height": height,
                    "blockSize": block_size, "color": "#000000", "strokeWidth": 1})
            }
        };
        object["id"] = json!(format!("capture-{index}"));
        object
    }).collect()
}

fn apply_rounded_alpha(pixels: &mut [u8], width: i32, height: i32, radius: i32) {
    let radius = radius.clamp(0, width.min(height) / 2);
    if radius == 0 {
        return;
    }
    let radius_squared = i64::from(radius) * i64::from(radius);
    for y in 0..height {
        for x in 0..width {
            let dx = if x < radius {
                radius - x
            } else if x >= width - radius {
                x - (width - radius - 1)
            } else {
                0
            };
            let dy = if y < radius {
                radius - y
            } else if y >= height - radius {
                y - (height - radius - 1)
            } else {
                0
            };
            if dx > 0 && dy > 0 && i64::from(dx * dx + dy * dy) > radius_squared {
                pixels[((y * width + x) * 4 + 3) as usize] = 0;
            }
        }
    }
}

fn crop_rgba(
    source: &[u8],
    source_width: i32,
    source_height: i32,
    rect: RectInfo,
) -> Result<Vec<u8>, String> {
    if rect.x < 0
        || rect.y < 0
        || rect.width <= 0
        || rect.height <= 0
        || i64::from(rect.x) + i64::from(rect.width) > i64::from(source_width)
        || i64::from(rect.y) + i64::from(rect.height) > i64::from(source_height)
    {
        return Err("擷取範圍已超出目前螢幕，請重新框選。".into());
    }
    let expected = source_width as usize * source_height as usize * 4;
    if source.len() != expected {
        return Err("來源桌面像素長度不正確".into());
    }

    let row_bytes = rect.width as usize * 4;
    let mut output = Vec::with_capacity(row_bytes * rect.height as usize);
    for y in rect.y..rect.y + rect.height {
        let start = (y as usize * source_width as usize + rect.x as usize) * 4;
        output.extend_from_slice(&source[start..start + row_bytes]);
    }
    Ok(output)
}

fn path_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(windows)]
mod platform {
    use super::{
        apply_native_annotations, best_vertical_overlap, crop_rgba, curved_arrow_points,
        frame_difference, number_ink, push_rounded_rectangle, tapered_arrow_points, LineStyle,
        LongCaptureResult, NativeAnnotation, NumberShape, SelectedArea,
    };
    use crate::capture::RectInfo;
    use std::{cell::RefCell, collections::HashMap, mem::size_of, sync::mpsc};
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM},
            Graphics::Gdi::{
                Arc, BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
                CreatePen, CreateSolidBrush, DeleteDC, DeleteObject, Ellipse, EndPaint, FillRect,
                GetStockObject, GetTextExtentPoint32W, InvalidateRect, LineTo, MoveToEx, Polygon,
                Rectangle, RoundRect, SelectObject, SetBkMode, SetTextColor, StretchDIBits,
                TextOutW, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
                DIB_RGB_COLORS, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION,
                FONT_QUALITY, HBITMAP, HDC, HGDIOBJ, NULL_BRUSH, NULL_PEN, PAINTSTRUCT, PS_DOT,
                PS_SOLID, SRCCOPY, TRANSPARENT,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
                Input::KeyboardAndMouse::{
                    GetKeyState, GetMouseMovePointsEx, ReleaseCapture, SendInput, SetCapture,
                    SetFocus, GMMP_USE_DISPLAY_POINTS, INPUT, INPUT_0, INPUT_MOUSE,
                    MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSEMOVEPOINT, VK_BACK, VK_CONTROL, VK_DOWN,
                    VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_UP,
                },
                WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                    GetMessageTime, GetMessageW, GetWindow, GetWindowRect, IsIconic,
                    IsWindowVisible, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage,
                    RegisterClassW, SetCursor, SetCursorPos, SetForegroundWindow, SetTimer,
                    ShowWindow, TranslateMessage, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, GW_CHILD,
                    GW_HWNDFIRST, GW_HWNDNEXT, IDC_ARROW, IDC_CROSS, IDC_IBEAM, IDC_SIZEALL,
                    IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, MSG, SW_HIDE, SW_SHOW,
                    WINDOW_EX_STYLE, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
                    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
                    WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                    WS_POPUP,
                },
            },
        },
    };

    fn begin_long_capture(state: &mut SelectorState) -> Result<(), String> {
        let selection = state.selection.ok_or("請先框選長擷圖範圍")?;
        let first = crop_rgba(&state.bgra, state.width, state.height, selection)?;
        state.long_capture = Some(LongCaptureState {
            previous_bgra: first.clone(),
            stitched_bgra: first,
            width: selection.width as usize,
            viewport_height: selection.height as usize,
        });
        Ok(())
    }

    unsafe fn capture_long_segment(
        state: &mut SelectorState,
        hwnd: HWND,
        delta: i32,
    ) -> Result<(), String> {
        let selection = state.selection.ok_or("長擷圖範圍已遺失")?;
        let screen_x = state.origin_x + selection.x + selection.width / 2;
        let screen_y = state.origin_y + selection.y + selection.height / 2;
        let result = (|| {
            // The selector is a topmost full-desktop window. Hide it while the
            // synthetic wheel event is delivered so the underlying app can
            // receive the input normally. The caller restores the window below
            // even when scrolling or capturing fails.
            ShowWindow(hwnd, SW_HIDE);
            std::thread::sleep(std::time::Duration::from_millis(80));
            SetCursorPos(screen_x, screen_y)
                .map_err(|error| format!("無法定位長擷圖游標：{error}"))?;
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: delta as u32,
                        dwFlags: MOUSEEVENTF_WHEEL,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            if SendInput(&[input], size_of::<INPUT>() as i32) != 1 {
                return Err("無法送出長擷圖捲動事件".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            crate::capture::capture_frame()
        })();
        ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        let frame = result?;
        let local = RectInfo {
            x: state.origin_x + selection.x - frame.virtual_desktop.x,
            y: state.origin_y + selection.y - frame.virtual_desktop.y,
            width: selection.width,
            height: selection.height,
        };
        let mut current = crop_rgba(
            &frame.rgba,
            frame.virtual_desktop.width,
            frame.virtual_desktop.height,
            local,
        )?;
        for pixel in current.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        {
            let long = state.long_capture.as_mut().ok_or("長擷圖尚未啟動")?;
            if frame_difference(&long.previous_bgra, &current) < 1.2 {
                return Ok(());
            }
            let overlap = best_vertical_overlap(
                &long.previous_bgra,
                &current,
                long.width,
                long.viewport_height,
            );
            let row = long.width * 4;
            long.stitched_bgra
                .extend_from_slice(&current[overlap * row..]);
            long.previous_bgra.clone_from(&current);
        }

        // Keep the selected viewport synchronized with the page that just
        // scrolled. The stitched preview continues to accumulate separately,
        // while the main selection shows the current underlying content.
        let source_stride = selection.width as usize * 4;
        for row_index in 0..selection.height as usize {
            let source_start = row_index * source_stride;
            let destination_start = ((selection.y as usize + row_index) * state.width as usize
                + selection.x as usize)
                * 4;
            state.bgra[destination_start..destination_start + source_stride]
                .copy_from_slice(&current[source_start..source_start + source_stride]);
        }
        state.composite_rect = None;
        Ok(())
    }

    static STATE: std::sync::Mutex<Option<SelectorState>> = std::sync::Mutex::new(None);
    const ANNOTATION_COLORS: [u32; 10] = [
        0x0030_3BFF, // red
        0x0025_2525, // near black
        0x00FF_FFFF, // white
        0x0018_B8F5, // orange
        0x0000_D7FF, // yellow
        0x0060_CF34, // green
        0x00FF_7B18, // blue
        0x00D0_5A8E, // purple
        0x00A0_A0A0, // gray
        0x00FF_DC9B, // light blue
    ];

    struct SelectorState {
        english: bool,
        monitor_bounds: Vec<RectInfo>,
        origin_x: i32,
        origin_y: i32,
        width: i32,
        height: i32,
        bgra: Vec<u8>,
        dimmed_bgra: Vec<u8>,
        composite_bgra: Vec<u8>,
        composite_rect: Option<(i32, i32, i32, i32, i32)>,
        interaction: Option<Interaction>,
        selection: Option<RectInfo>,
        hover_candidate: Option<RectInfo>,
        cursor_position: (i32, i32),
        corner_radius: i32,
        annotation_tool: Option<AnnotationTool>,
        annotations: Vec<NativeAnnotation>,
        undo_annotations: Vec<Vec<NativeAnnotation>>,
        redo_annotations: Vec<Vec<NativeAnnotation>>,
        stroke_width: i32,
        line_style: LineStyle,
        hovered_annotation: Option<usize>,
        selected_annotation: Option<usize>,
        current_color: u32,
        color_target: Option<ColorTarget>,
        number_background: u32,
        number_label_color: Option<u32>,
        text_outline_color: u32,
        text_outline_width: i32,
        text_input: Option<TextInput>,
        number_input: Option<NumberInput>,
        freehand_points: Vec<(i32, i32)>,
        next_number: u32,
        caret_visible: bool,
        long_capture: Option<LongCaptureState>,
        sender: Option<mpsc::Sender<Option<SelectedArea>>>,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum ColorTarget {
        Main,
        Outline,
        NumberBackground,
        NumberLabel,
    }

    #[derive(Clone, Copy)]
    enum AnnotationProperty {
        OutlineColor,
        OutlineWidth(i32),
    }

    struct LongCaptureState {
        previous_bgra: Vec<u8>,
        stitched_bgra: Vec<u8>,
        width: usize,
        viewport_height: usize,
    }

    #[derive(Clone, Copy)]
    enum Interaction {
        Create {
            start: (i32, i32),
        },
        PendingWindow {
            start: (i32, i32),
            candidate: RectInfo,
        },
        Resize {
            handle: Handle,
            start: (i32, i32),
            original: RectInfo,
        },
        Annotate {
            start: (i32, i32),
            tool: AnnotationTool,
        },
        MoveAnnotation {
            index: usize,
            start: (i32, i32),
            original: NativeAnnotation,
        },
        AdjustAnnotation {
            index: usize,
            handle: AnnotationHandle,
            original: NativeAnnotation,
        },
        MoveSelection {
            start: (i32, i32),
            original: RectInfo,
        },
    }

    #[derive(Clone, Copy, PartialEq)]
    enum AnnotationTool {
        Rectangle,
        Ellipse,
        Freehand,
        Arrow,
        Text,
        Number,
        RoundedNumber,
        Mosaic,
        Eraser,
        Line,
    }

    impl AnnotationTool {
        fn for_number(shape: NumberShape) -> Self {
            match shape {
                NumberShape::Circle => Self::Number,
                NumberShape::RoundedRectangle => Self::RoundedNumber,
            }
        }
    }

    #[derive(Clone, Copy)]
    enum ToolbarButton {
        Capture,
        LongCapture,
        Cancel,
        Pin,
        Tool(AnnotationTool),
        Color,
        NumberLabelColor,
        Thinner,
        Thickness,
        Thicker,
        Style,
        Undo,
        Redo,
        Delete,
    }

    const TOOLBAR_BUTTONS: &[ToolbarButton] = &[
        ToolbarButton::Capture,
        ToolbarButton::LongCapture,
        ToolbarButton::Cancel,
        ToolbarButton::Pin,
        ToolbarButton::Tool(AnnotationTool::Rectangle),
        ToolbarButton::Tool(AnnotationTool::Ellipse),
        ToolbarButton::Tool(AnnotationTool::Line),
        ToolbarButton::Tool(AnnotationTool::Freehand),
        ToolbarButton::Tool(AnnotationTool::Arrow),
        ToolbarButton::Tool(AnnotationTool::Text),
        ToolbarButton::Tool(AnnotationTool::Number),
        ToolbarButton::Tool(AnnotationTool::RoundedNumber),
        ToolbarButton::Tool(AnnotationTool::Mosaic),
        ToolbarButton::Tool(AnnotationTool::Eraser),
        ToolbarButton::Color,
        ToolbarButton::Thickness,
        ToolbarButton::Style,
        ToolbarButton::Undo,
        ToolbarButton::Redo,
        ToolbarButton::Delete,
    ];
    const TOOLBAR_COLOR_INDEX: usize = 14;
    const CAPTURE_BUTTON_COUNT: usize = 4;
    const TOOL_SIZE: i32 = 44;

    impl SelectorState {
        fn number_tool_active(&self) -> bool {
            matches!(
                self.annotation_tool,
                Some(AnnotationTool::Number | AnnotationTool::RoundedNumber)
            )
        }

        fn selected_object(&self) -> Option<NativeAnnotation> {
            self.selected_annotation
                .map(|index| self.annotations[index])
        }

        fn number_colors(&self) -> (u32, Option<u32>) {
            match self.selected_object() {
                Some(NativeAnnotation::Number {
                    color, label_color, ..
                }) => (color, label_color),
                _ => (self.number_background, self.number_label_color),
            }
        }

        fn set_number_colors(&mut self, background: u32, foreground: Option<u32>) {
            self.number_background = background;
            self.number_label_color = foreground;
            if let Some(index) = self.selected_annotation {
                if matches!(self.annotations[index], NativeAnnotation::Number { .. }) {
                    self.save_undo();
                    if let NativeAnnotation::Number {
                        color, label_color, ..
                    } = &mut self.annotations[index]
                    {
                        *color = background;
                        *label_color = foreground;
                    }
                }
            }
        }

        fn text_outline(&self) -> (u32, i32) {
            if let Some(input) = &self.text_input {
                (input.outline_color, input.outline_width)
            } else if let Some(NativeAnnotation::Text {
                outline_color,
                outline_width,
                ..
            }) = self.selected_object()
            {
                (outline_color, outline_width)
            } else {
                (self.text_outline_color, self.text_outline_width)
            }
        }

        fn adjust_text_outline(&mut self, step: i32) {
            let width = (self.text_outline().1 + step).clamp(0, 12);
            self.text_outline_width = width;
            if let Some(input) = &mut self.text_input {
                input.outline_width = width;
            } else if let Some(index) = self.selected_annotation {
                if matches!(self.annotations[index], NativeAnnotation::Text { .. }) {
                    self.save_undo();
                    set_text_outline_width(&mut self.annotations[index], step);
                }
            }
        }

        fn apply_palette_color(&mut self, target: ColorTarget, chosen: Option<u32>) {
            match target {
                ColorTarget::Main => {
                    let Some(color) = chosen else { return };
                    self.current_color = color;
                    if let Some(input) = self.text_input.as_mut() {
                        input.color = color;
                    } else if let Some(index) = self.selected_annotation {
                        self.save_undo();
                        set_native_color(&mut self.annotations[index], color);
                    }
                }
                ColorTarget::Outline => {
                    let Some(color) = chosen else { return };
                    self.text_outline_color = color;
                    if let Some(input) = self.text_input.as_mut() {
                        input.outline_color = color;
                    } else if let Some(index) = self.selected_annotation {
                        self.save_undo();
                        set_text_outline_color(&mut self.annotations[index], color);
                    }
                }
                ColorTarget::NumberBackground => {
                    if let Some(color) = chosen {
                        self.set_number_colors(color, self.number_colors().1);
                    }
                }
                ColorTarget::NumberLabel => self.set_number_colors(self.number_colors().0, chosen),
            }
        }

        fn toolbar_buttons(&self) -> &'static [ToolbarButton] {
            if self.long_capture.is_some() {
                &TOOLBAR_BUTTONS[..CAPTURE_BUTTON_COUNT]
            } else {
                TOOLBAR_BUTTONS
            }
        }
        fn append_freehand_point(&mut self, point: (i32, i32)) {
            if self.freehand_points.last() == Some(&point) {
                return;
            }
            if self.freehand_points.len() == 128 {
                // Keep the newest samples and simplify the least curved part of the older stroke.
                let index = self
                    .freehand_points
                    .windows(3)
                    .take(110)
                    .enumerate()
                    .min_by_key(|(_, points)| {
                        let ab = (
                            i64::from(points[1].0 - points[0].0),
                            i64::from(points[1].1 - points[0].1),
                        );
                        let bc = (
                            i64::from(points[2].0 - points[1].0),
                            i64::from(points[2].1 - points[1].1),
                        );
                        let turn = (ab.0 * bc.1 - ab.1 * bc.0).abs();
                        let reverses_direction = ab.0 * bc.0 + ab.1 * bc.1 < 0;
                        (
                            reverses_direction,
                            turn,
                            ab.0.abs() + ab.1.abs() + bc.0.abs() + bc.1.abs(),
                        )
                    })
                    .map_or(1, |(index, _)| index + 1);
                self.freehand_points.remove(index);
            }
            self.freehand_points.push(point);
        }

        fn tool_thickness(&self) -> i32 {
            match self.annotation_tool {
                Some(AnnotationTool::Text) => (self.stroke_width * 2 + 16).clamp(8, 160),
                Some(AnnotationTool::Number | AnnotationTool::RoundedNumber) => {
                    (self.stroke_width * 2 + 43).clamp(18, 96)
                }
                Some(AnnotationTool::Mosaic) => (self.stroke_width + 8).clamp(4, 48),
                _ => self.stroke_width,
            }
        }

        fn save_undo(&mut self) {
            self.save_undo_state(self.annotations.clone());
        }

        fn save_undo_state(&mut self, previous: Vec<NativeAnnotation>) {
            if self.undo_annotations.len() >= 32 {
                self.undo_annotations.remove(0);
            }
            self.undo_annotations.push(previous);
            self.redo_annotations.clear();
        }

        fn undo_or_redo(&mut self, redo: bool) {
            self.commit_value();
            let (from, to) = if redo {
                (&mut self.redo_annotations, &mut self.undo_annotations)
            } else {
                (&mut self.undo_annotations, &mut self.redo_annotations)
            };
            if let Some(annotations) = from.pop() {
                to.push(std::mem::replace(&mut self.annotations, annotations));
                self.selected_annotation = None;
                self.hovered_annotation = None;
            }
        }

        fn editing_value(&self) -> bool {
            self.text_input.is_some() || self.number_input.is_some()
        }

        fn select_input_contents(&mut self) {
            if !self.editing_value() && self.interaction.is_none() {
                if let Some(index) = self.selected_annotation {
                    self.edit_value(index);
                }
            }
            if let Some(input) = self.text_input.as_mut() {
                input.replace_on_type = true;
            }
            if let Some(input) = self.number_input.as_mut() {
                input.replace_on_type = true;
            }
        }

        fn edit_backspace(&mut self) {
            if let Some(input) = self.number_input.as_mut() {
                if input.replace_on_type {
                    input.digits.clear();
                } else {
                    input.digits.pop();
                }
                input.replace_on_type = false;
            } else if let Some(input) = self.text_input.as_mut() {
                if input.replace_on_type {
                    input.buffer.clear();
                } else if input
                    .buffer
                    .pop()
                    .is_some_and(|last| (0xDC00..=0xDFFF).contains(&last))
                    && input
                        .buffer
                        .last()
                        .is_some_and(|last| (0xD800..=0xDBFF).contains(last))
                {
                    input.buffer.pop();
                }
                input.replace_on_type = false;
                self.caret_visible = true;
            }
        }

        fn commit_value(&mut self) {
            commit_text_input(self);
            if let Some(input) = self.number_input.take() {
                if let Ok(number) = input.digits.parse::<u32>() {
                    if let Some(NativeAnnotation::Number {
                        number: previous, ..
                    }) = self.annotations.get(input.index)
                    {
                        if *previous != number {
                            self.save_undo();
                            if let NativeAnnotation::Number { number: value, .. } =
                                &mut self.annotations[input.index]
                            {
                                *value = number;
                            }
                        }
                    }
                }
            }
        }

        fn edit_value(&mut self, index: usize) -> bool {
            if self.long_capture.is_some() || self.editing_value() {
                return false;
            }
            if !matches!(
                self.annotations.get(index),
                Some(NativeAnnotation::Text { .. } | NativeAnnotation::Number { .. })
            ) {
                return false;
            }
            match self.annotations[index] {
                NativeAnnotation::Text {
                    x,
                    y,
                    text,
                    length,
                    font_size,
                    color,
                    outline_color,
                    outline_width,
                } => {
                    self.annotation_tool = Some(AnnotationTool::Text);
                    self.text_input = Some(TextInput {
                        x,
                        y,
                        buffer: text[..length.min(text.len())].to_vec(),
                        font_size,
                        color,
                        outline_color,
                        outline_width,
                        editing_index: Some(index),
                        replace_on_type: false,
                    });
                    self.caret_visible = true;
                }
                NativeAnnotation::Number { number, shape, .. } => {
                    self.annotation_tool = Some(AnnotationTool::for_number(shape));
                    self.number_input = Some(NumberInput {
                        index,
                        digits: number.to_string(),
                        replace_on_type: true,
                    });
                }
                _ => return false,
            }
            self.selected_annotation = Some(index);
            self.interaction = None;
            true
        }

        fn result(&mut self, pin: bool) -> Option<SelectedArea> {
            let selection = self
                .selection
                .filter(|rect| rect.width >= 2 && rect.height >= 2)?;
            self.commit_value();
            self.selected_annotation = None;
            self.hovered_annotation = None;
            let annotations = std::mem::take(&mut self.annotations);
            Some(SelectedArea {
                selection,
                corner_radius: self.corner_radius,
                annotations: if self.long_capture.is_some() {
                    Vec::new()
                } else {
                    annotations
                },
                long_capture: self.long_capture.take().map(|long| LongCaptureResult {
                    height: (long.stitched_bgra.len() / (long.width * 4)) as i32,
                    width: long.width as i32,
                    bgra: long.stitched_bgra,
                }),
                pin,
            })
        }

        fn send_result(&mut self, result: Option<SelectedArea>) {
            if let Some(sender) = self.sender.take() {
                let _ = sender.send(result);
            }
        }

        fn delete_annotation(&mut self) {
            if let Some(index) = self.selected_annotation.take() {
                self.save_undo();
                self.annotations.remove(index);
                self.hovered_annotation = None;
            }
        }
    }

    struct TextInput {
        x: i32,
        y: i32,
        buffer: Vec<u16>,
        font_size: i32,
        color: u32,
        outline_color: u32,
        outline_width: i32,
        editing_index: Option<usize>,
        replace_on_type: bool,
    }

    struct NumberInput {
        index: usize,
        digits: String,
        replace_on_type: bool,
    }

    #[derive(Clone, Copy)]
    enum Handle {
        TopLeft,
        Top,
        TopRight,
        Right,
        BottomRight,
        Bottom,
        BottomLeft,
        Left,
    }

    #[derive(Clone, Copy)]
    enum AnnotationHandle {
        Start,
        End,
        TopRight,
        BottomLeft,
        Curve,
    }

    pub fn run_selector(
        virtual_desktop: RectInfo,
        mut rgba: Vec<u8>,
        initial_selection: Option<RectInfo>,
        monitor_bounds: Vec<RectInfo>,
        english: bool,
    ) -> Result<Option<SelectedArea>, String> {
        static ACTIVE_SELECTOR: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _active = ACTIVE_SELECTOR
            .try_lock()
            .map_err(|_| "已有框選視窗開啟。")?;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let mut dimmed_bgra = rgba.clone();
        for pixel in dimmed_bgra.chunks_exact_mut(4) {
            pixel[0] = (pixel[0] as u16 * 48 / 100) as u8;
            pixel[1] = (pixel[1] as u16 * 48 / 100) as u8;
            pixel[2] = (pixel[2] as u16 * 48 / 100) as u8;
        }
        let (sender, receiver) = mpsc::channel();
        let composite_bgra = dimmed_bgra.clone();
        *STATE.lock().map_err(|_| "選取狀態鎖定失敗")? = Some(SelectorState {
            english,
            monitor_bounds,
            origin_x: virtual_desktop.x,
            origin_y: virtual_desktop.y,
            width: virtual_desktop.width,
            height: virtual_desktop.height,
            bgra: rgba,
            dimmed_bgra,
            composite_bgra,
            composite_rect: None,
            interaction: None,
            selection: initial_selection,
            hover_candidate: None,
            cursor_position: (virtual_desktop.width / 2, virtual_desktop.height / 2),
            corner_radius: 0,
            annotation_tool: None,
            annotations: Vec::new(),
            undo_annotations: Vec::new(),
            redo_annotations: Vec::new(),
            stroke_width: 4,
            line_style: LineStyle::Solid,
            hovered_annotation: None,
            selected_annotation: None,
            current_color: 0x0030_3BFF,
            color_target: None,
            number_background: ANNOTATION_COLORS[0],
            number_label_color: None,
            text_outline_color: 0x00FF_FFFF,
            text_outline_width: 1,
            text_input: None,
            number_input: None,
            freehand_points: Vec::new(),
            next_number: 1,
            caret_visible: true,
            long_capture: None,
            sender: Some(sender),
        });

        let result = unsafe { run_window(virtual_desktop) };
        PAINT_BUFFER.with(|buffer| *buffer.borrow_mut() = None);
        TOOL_ICONS.with(|icons| icons.borrow_mut().clear());
        if let Err(error) = result {
            *STATE.lock().map_err(|_| "選取狀態清理失敗")? = None;
            return Err(error);
        }
        let selection = receiver
            .recv()
            .map_err(|error| format!("無法接收選取結果：{error}"))?;
        *STATE.lock().map_err(|_| "選取狀態清理失敗")? = None;
        Ok(selection)
    }

    unsafe fn run_window(rect: RectInfo) -> Result<(), String> {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let module =
            GetModuleHandleW(None).map_err(|error| format!("GetModuleHandleW：{error}"))?;
        let instance = HINSTANCE(module.0);
        let class_name = w!("CaptureFlowSelectorWindow");
        let cursor =
            LoadCursorW(None, IDC_CROSS).map_err(|error| format!("無法載入十字游標：{error}"))?;
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            // The class can already exist after a previous selection in this process.
        }

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0),
            class_name,
            w!("CaptureFlow Area Selector"),
            WS_POPUP,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|error| format!("無法建立選取覆蓋層：{error}"))?;
        // Keep the selector visible to desktop recorders. Our own output uses
        // the pre-selector frame, or for scrolling, a frame taken while hidden.
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        SetTimer(Some(hwnd), 1, 500, None);

        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, None, 0, 0);
            if status.0 <= 0 {
                break;
            }
            // Backspace is handled on key-down. Do not generate a second WM_CHAR.
            if message.message != WM_KEYDOWN || message.wParam.0 as u16 != VK_BACK.0 {
                TranslateMessage(&message);
            }
            DispatchMessageW(&message);
        }
        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let point = mouse_point(lparam);
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        let point = clamp_point(point, state.width, state.height);
                        if message == WM_LBUTTONDBLCLK {
                            if let Some(selection) = state.selection {
                                if hit_annotation_toolbar(point, selection, state).is_none()
                                    && !matches!(
                                        state.annotation_tool,
                                        Some(AnnotationTool::Eraser)
                                    )
                                {
                                    state.commit_value();
                                    if let Some(index) =
                                        hit_native_annotation(point, &state.annotations)
                                    {
                                        if state.edit_value(index) {
                                            let _ = ReleaseCapture();
                                            InvalidateRect(Some(hwnd), None, false);
                                        }
                                    }
                                }
                            }
                            return LRESULT(0);
                        }
                        if let Some(selection) = state.selection {
                            if let Some(target) = state.color_target {
                                if let Some(color) = hit_color_palette(point, selection, state) {
                                    let target = if target == ColorTarget::Main
                                        && state.annotation_tool == Some(AnnotationTool::Text)
                                        && GetKeyState(VK_SHIFT.0 as i32) < 0
                                    {
                                        ColorTarget::Outline
                                    } else {
                                        target
                                    };
                                    state.apply_palette_color(target, color);
                                    state.color_target = None;
                                    InvalidateRect(Some(hwnd), None, false);
                                    return LRESULT(0);
                                }
                                if point_in_rect(point, color_palette_rect(selection, state)) {
                                    return LRESULT(0);
                                }
                            }
                            let layout = annotation_toolbar_layout(selection, state);
                            if layout
                                .properties
                                .is_some_and(|rect| point_in_rect(point, rect))
                            {
                                match hit_annotation_property(point, &layout) {
                                    Some(AnnotationProperty::OutlineColor) => {
                                        state.color_target = (state.color_target
                                            != Some(ColorTarget::Outline))
                                        .then_some(ColorTarget::Outline);
                                    }
                                    Some(AnnotationProperty::OutlineWidth(step)) => {
                                        state.color_target = None;
                                        state.adjust_text_outline(step);
                                    }
                                    None => state.color_target = None,
                                }
                                InvalidateRect(Some(hwnd), None, false);
                                return LRESULT(0);
                            }
                            if let Some(button) = hit_annotation_toolbar(point, selection, state) {
                                if state.number_input.is_some()
                                    && !matches!(button, ToolbarButton::Cancel)
                                {
                                    state.commit_value();
                                }
                                match button {
                                    ToolbarButton::Capture | ToolbarButton::Pin => {
                                        if let Some(result) =
                                            state.result(matches!(button, ToolbarButton::Pin))
                                        {
                                            state.send_result(Some(result));
                                            let _ = PostMessageW(
                                                Some(hwnd),
                                                WM_CLOSE,
                                                WPARAM(0),
                                                LPARAM(0),
                                            );
                                        }
                                    }
                                    ToolbarButton::Cancel => {
                                        state.send_result(None);
                                        let _ = PostMessageW(
                                            Some(hwnd),
                                            WM_CLOSE,
                                            WPARAM(0),
                                            LPARAM(0),
                                        );
                                    }
                                    ToolbarButton::LongCapture => {
                                        state.commit_value();
                                        if state.long_capture.is_none() {
                                            if begin_long_capture(state).is_ok() {
                                                state.hovered_annotation = None;
                                                state.selected_annotation = None;
                                            }
                                        } else {
                                            state.long_capture = None;
                                        }
                                    }
                                    ToolbarButton::Tool(tool) => {
                                        state.commit_value();
                                        state.annotation_tool =
                                            if state.annotation_tool == Some(tool) {
                                                None
                                            } else {
                                                Some(tool)
                                            };
                                        state.selected_annotation = None;
                                    }
                                    ToolbarButton::Color | ToolbarButton::NumberLabelColor => {
                                        let target =
                                            if matches!(button, ToolbarButton::NumberLabelColor) {
                                                ColorTarget::NumberLabel
                                            } else if state.number_tool_active() {
                                                ColorTarget::NumberBackground
                                            } else if state.annotation_tool
                                                == Some(AnnotationTool::Text)
                                                && GetKeyState(VK_SHIFT.0 as i32) < 0
                                            {
                                                ColorTarget::Outline
                                            } else {
                                                ColorTarget::Main
                                            };
                                        state.color_target =
                                            (state.color_target != Some(target)).then_some(target);
                                    }
                                    ToolbarButton::Thinner | ToolbarButton::Thicker => {
                                        let step = if matches!(button, ToolbarButton::Thicker) {
                                            1
                                        } else {
                                            -1
                                        };
                                        if let Some(input) = state.text_input.as_mut() {
                                            input.font_size =
                                                (input.font_size + step * 2).clamp(8, 160);
                                        } else {
                                            state.stroke_width =
                                                (state.stroke_width + step).clamp(1, 32);
                                            if let Some(index) = state.selected_annotation {
                                                state.save_undo();
                                                set_native_stroke_width(
                                                    &mut state.annotations[index],
                                                    step,
                                                );
                                            }
                                        }
                                    }
                                    ToolbarButton::Style => {
                                        state.line_style = match state.line_style {
                                            LineStyle::Solid => LineStyle::Dashed,
                                            LineStyle::Dashed => LineStyle::Dotted,
                                            LineStyle::Dotted => LineStyle::DashDot,
                                            LineStyle::DashDot => LineStyle::Solid,
                                        };
                                        if let Some(index) = state.selected_annotation {
                                            if native_line_style(state.annotations[index]).is_some()
                                            {
                                                state.save_undo();
                                                set_native_line_style(
                                                    &mut state.annotations[index],
                                                    state.line_style,
                                                );
                                            }
                                        }
                                    }
                                    ToolbarButton::Undo => state.undo_or_redo(false),
                                    ToolbarButton::Redo => state.undo_or_redo(true),
                                    ToolbarButton::Delete => {
                                        state.commit_value();
                                        state.delete_annotation();
                                    }
                                    ToolbarButton::Thickness => {}
                                }
                                if !matches!(
                                    button,
                                    ToolbarButton::Color | ToolbarButton::NumberLabelColor
                                ) {
                                    state.color_target = None;
                                }
                                InvalidateRect(Some(hwnd), None, false);
                                return LRESULT(0);
                            }
                            state.color_target = None;
                            if state.long_capture.is_some() {
                                return LRESULT(0);
                            }
                            state.commit_value();
                            let button = radius_button_rect(selection, state.width, state.height);
                            if point.0 >= button.left - 8
                                && point.0 <= button.right + 8
                                && point.1 >= button.top - 8
                                && point.1 <= button.bottom + 8
                            {
                                state.corner_radius = if state.corner_radius == 0 {
                                    32.min(selection.width.min(selection.height) / 2)
                                } else {
                                    0
                                };
                                state.composite_bgra.clone_from(&state.dimmed_bgra);
                                state.composite_rect = None;
                                InvalidateRect(Some(hwnd), None, false);
                                return LRESULT(0);
                            }
                            {
                                let interaction = if let Some(handle) = hit_handle(point, selection)
                                {
                                    Some(Interaction::Resize {
                                        handle,
                                        start: point,
                                        original: selection,
                                    })
                                } else if hit_selection_edge(point, selection)
                                    || (state.annotation_tool.is_none()
                                        && clamp_to_rect(point, selection) == point)
                                {
                                    Some(Interaction::MoveSelection {
                                        start: point,
                                        original: selection,
                                    })
                                } else {
                                    None
                                };
                                if let Some(interaction) = interaction {
                                    state.long_capture = None;
                                    state.interaction = Some(interaction);
                                    SetCapture(hwnd);
                                    return LRESULT(0);
                                }
                            }
                            if matches!(state.annotation_tool, Some(AnnotationTool::Eraser)) {
                                if let Some(index) =
                                    hit_native_annotation(point, &state.annotations)
                                {
                                    state.save_undo();
                                    state.annotations.remove(index);
                                    state.selected_annotation = None;
                                    state.hovered_annotation = None;
                                    InvalidateRect(Some(hwnd), None, false);
                                }
                                return LRESULT(0);
                            }
                            if let Some((index, handle)) =
                                hit_native_annotation_handle(point, &state.annotations)
                            {
                                state.selected_annotation = Some(index);
                                state.interaction = Some(Interaction::AdjustAnnotation {
                                    index,
                                    handle,
                                    original: state.annotations[index],
                                });
                                SetCapture(hwnd);
                                return LRESULT(0);
                            }
                            if let Some(index) = hit_native_annotation(point, &state.annotations) {
                                if state.selected_annotation == Some(index)
                                    && GetKeyState(VK_CONTROL.0 as i32) < 0
                                    && state.edit_value(index)
                                {
                                    InvalidateRect(Some(hwnd), None, false);
                                    return LRESULT(0);
                                }
                                state.selected_annotation = Some(index);
                                state.hovered_annotation = Some(index);
                                state.annotation_tool = Some(match state.annotations[index] {
                                    NativeAnnotation::Rectangle { .. } => AnnotationTool::Rectangle,
                                    NativeAnnotation::Ellipse { .. } => AnnotationTool::Ellipse,
                                    NativeAnnotation::Arrow { headed, .. } => {
                                        if headed {
                                            AnnotationTool::Arrow
                                        } else {
                                            AnnotationTool::Line
                                        }
                                    }
                                    NativeAnnotation::Text { .. } => AnnotationTool::Text,
                                    NativeAnnotation::Freehand { .. } => AnnotationTool::Freehand,
                                    NativeAnnotation::Number { shape, .. } => {
                                        AnnotationTool::for_number(shape)
                                    }
                                    NativeAnnotation::Mosaic { .. } => AnnotationTool::Mosaic,
                                });
                                if let Some(style) = native_line_style(state.annotations[index]) {
                                    state.line_style = style;
                                }
                                state.interaction = Some(Interaction::MoveAnnotation {
                                    index,
                                    start: point,
                                    original: state.annotations[index],
                                });
                                SetCapture(hwnd);
                                InvalidateRect(Some(hwnd), None, false);
                                return LRESULT(0);
                            }
                            if let Some(tool) = state.annotation_tool {
                                if point.0 >= selection.x
                                    && point.0 <= selection.x + selection.width
                                    && point.1 >= selection.y
                                    && point.1 <= selection.y + selection.height
                                {
                                    if matches!(tool, AnnotationTool::Text) {
                                        state.text_input = Some(TextInput {
                                            x: point.0,
                                            y: point.1,
                                            buffer: Vec::new(),
                                            font_size: state.tool_thickness(),
                                            color: state.current_color,
                                            outline_color: state.text_outline_color,
                                            outline_width: state.text_outline_width,
                                            editing_index: None,
                                            replace_on_type: false,
                                        });
                                        state.caret_visible = true;
                                    } else if matches!(
                                        tool,
                                        AnnotationTool::Number | AnnotationTool::RoundedNumber
                                    ) {
                                        state.save_undo();
                                        state.annotations.push(NativeAnnotation::Number {
                                            x: point.0,
                                            y: point.1,
                                            number: state.next_number,
                                            size: state.tool_thickness(),
                                            color: state.number_background,
                                            label_color: state.number_label_color,
                                            shape: if tool == AnnotationTool::RoundedNumber {
                                                NumberShape::RoundedRectangle
                                            } else {
                                                NumberShape::Circle
                                            },
                                        });
                                        state.next_number = state.next_number.saturating_add(1);
                                        state.selected_annotation =
                                            Some(state.annotations.len() - 1);
                                    } else if matches!(tool, AnnotationTool::Eraser) {
                                    } else {
                                        if matches!(tool, AnnotationTool::Freehand) {
                                            state.freehand_points.clear();
                                            state.freehand_points.push(point);
                                        }
                                        state.interaction =
                                            Some(Interaction::Annotate { start: point, tool });
                                        SetCapture(hwnd);
                                    }
                                    InvalidateRect(Some(hwnd), None, false);
                                    return LRESULT(0);
                                }
                            }
                        }
                        if state.selection.is_none() {
                            if let Some(candidate) = state.hover_candidate {
                                state.interaction = Some(Interaction::PendingWindow {
                                    start: point,
                                    candidate,
                                });
                            } else {
                                state.interaction = Some(Interaction::Create { start: point });
                                state.selection = Some(empty_rect(point));
                            }
                        } else {
                            state.long_capture = None;
                            state.interaction = Some(Interaction::Create { start: point });
                            state.selection = Some(empty_rect(point));
                        }
                    }
                }
                SetCapture(hwnd);
                InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        let previous_damage = draft_damage(state);
                        let pointer = mouse_point(lparam);
                        state.cursor_position = (
                            pointer.0.clamp(0, state.width.saturating_sub(1)),
                            pointer.1.clamp(0, state.height.saturating_sub(1)),
                        );
                        if let Some(interaction) = state.interaction {
                            let mut current = if matches!(interaction, Interaction::Annotate { .. })
                            {
                                state
                                    .selection
                                    .map(|rect| clamp_to_rect(mouse_point(lparam), rect))
                                    .unwrap_or_else(|| {
                                        clamp_point(mouse_point(lparam), state.width, state.height)
                                    })
                            } else {
                                clamp_point(mouse_point(lparam), state.width, state.height)
                            };
                            if let Interaction::Annotate {
                                start,
                                tool: AnnotationTool::Ellipse,
                            } = interaction
                            {
                                if GetKeyState(VK_SHIFT.0 as i32) < 0 {
                                    current = constrain_square(start, current);
                                }
                            }
                            if matches!(interaction, Interaction::Annotate { .. }) {
                                state.cursor_position = current;
                            }
                            if let Interaction::Annotate {
                                tool: AnnotationTool::Freehand,
                                ..
                            } = interaction
                            {
                                append_mouse_history(state, pointer);
                                state.append_freehand_point(current);
                            }
                            if let Interaction::MoveAnnotation {
                                index,
                                start,
                                original,
                            } = interaction
                            {
                                state.annotations[index] = move_native_annotation(
                                    original,
                                    current.0 - start.0,
                                    current.1 - start.1,
                                );
                            } else if let Interaction::AdjustAnnotation {
                                index,
                                handle,
                                original,
                            } = interaction
                            {
                                state.annotations[index] =
                                    adjust_native_annotation(original, handle, current);
                            } else if let Interaction::PendingWindow { start, .. } = interaction {
                                if (current.0 - start.0).abs() >= 4
                                    || (current.1 - start.1).abs() >= 4
                                {
                                    state.interaction = Some(Interaction::Create { start });
                                    state.hover_candidate = None;
                                    state.selection = Some(normalize_rect(start, current));
                                }
                            } else if let Interaction::MoveSelection { start, original } =
                                interaction
                            {
                                state.selection = Some(move_rect(
                                    original,
                                    current.0 - start.0,
                                    current.1 - start.1,
                                    state.width,
                                    state.height,
                                ));
                            } else if !matches!(interaction, Interaction::Annotate { .. }) {
                                state.selection = Some(update_selection(
                                    interaction,
                                    current,
                                    state.width,
                                    state.height,
                                ));
                            }
                        } else {
                            let point = clamp_point(mouse_point(lparam), state.width, state.height);
                            if state.selection.is_none() {
                                state.hover_candidate = detect_window_at(hwnd, point, state);
                            } else if state.long_capture.is_none() {
                                state.hovered_annotation =
                                    hit_native_annotation(point, &state.annotations);
                            }
                        }
                        let next_damage = draft_damage(state);
                        let dirty = match (previous_damage, next_damage) {
                            (Some(old), Some(new)) => Some(RECT {
                                left: old.left.min(new.left),
                                top: old.top.min(new.top),
                                right: old.right.max(new.right),
                                bottom: old.bottom.max(new.bottom),
                            }),
                            (damage, None) | (None, damage) => damage,
                        };
                        InvalidateRect(
                            Some(hwnd),
                            dirty.as_ref().map(|rect| rect as *const RECT),
                            false,
                        );
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if let Some(interaction) = state.interaction.take() {
                            let mut current = if matches!(interaction, Interaction::Annotate { .. })
                            {
                                state
                                    .selection
                                    .map(|rect| clamp_to_rect(mouse_point(lparam), rect))
                                    .unwrap_or_else(|| {
                                        clamp_point(mouse_point(lparam), state.width, state.height)
                                    })
                            } else {
                                clamp_point(mouse_point(lparam), state.width, state.height)
                            };
                            if let Interaction::Annotate {
                                start,
                                tool: AnnotationTool::Ellipse,
                            } = interaction
                            {
                                if GetKeyState(VK_SHIFT.0 as i32) < 0 {
                                    current = constrain_square(start, current);
                                }
                            }
                            if let Interaction::Annotate { start, tool } = interaction {
                                if (current.0 - start.0).abs() >= 3
                                    || (current.1 - start.1).abs() >= 3
                                {
                                    if tool == AnnotationTool::Freehand {
                                        state.append_freehand_point(current);
                                    }
                                    if let Some(annotation) =
                                        draft_annotation(state, tool, start, current)
                                    {
                                        state.save_undo();
                                        state.annotations.push(annotation);
                                        state.selected_annotation =
                                            Some(state.annotations.len() - 1);
                                    }
                                }
                            } else if !matches!(
                                interaction,
                                Interaction::MoveAnnotation { .. }
                                    | Interaction::AdjustAnnotation { .. }
                                    | Interaction::MoveSelection { .. }
                            ) {
                                state.selection = Some(update_selection(
                                    interaction,
                                    current,
                                    state.width,
                                    state.height,
                                ));
                            }
                            if let Interaction::MoveAnnotation {
                                index, original, ..
                            }
                            | Interaction::AdjustAnnotation {
                                index, original, ..
                            } = interaction
                            {
                                if state.annotations[index] != original {
                                    let mut previous = state.annotations.clone();
                                    previous[index] = original;
                                    state.save_undo_state(previous);
                                }
                            }
                            state.hover_candidate = None;
                        }
                    }
                }
                let _ = ReleaseCapture();
                InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_CHAR => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        let character = wparam.0 as u16;
                        if character == 1 {
                            state.select_input_contents();
                            InvalidateRect(Some(hwnd), None, false);
                            return LRESULT(0);
                        }
                        if character == VK_BACK.0 {
                            state.edit_backspace();
                            InvalidateRect(Some(hwnd), None, false);
                            return LRESULT(0);
                        }
                        if let Some(input) = state.number_input.as_mut() {
                            if (48..=57).contains(&character) {
                                let mut candidate = if input.replace_on_type {
                                    String::new()
                                } else {
                                    input.digits.clone()
                                };
                                candidate.push(char::from(character as u8));
                                if candidate.len() <= 10 && candidate.parse::<u32>().is_ok() {
                                    input.digits = candidate;
                                    input.replace_on_type = false;
                                }
                            }
                            InvalidateRect(Some(hwnd), None, false);
                            return LRESULT(0);
                        }
                        if let Some(input) = state.text_input.as_mut() {
                            if character >= 0x20 {
                                if input.replace_on_type {
                                    input.buffer.clear();
                                }
                                input.replace_on_type = false;
                                if input.buffer.len() < 32 {
                                    input.buffer.push(character);
                                }
                            }
                            state.caret_visible = true;
                            InvalidateRect(Some(hwnd), None, false);
                            return LRESULT(0);
                        }
                    }
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_BACK.0 => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        state.edit_backspace();
                        InvalidateRect(Some(hwnd), None, false);
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if state.editing_value() {
                            state.text_input = None;
                            state.number_input = None;
                            InvalidateRect(Some(hwnd), None, false);
                            return LRESULT(0);
                        }
                    }
                }
                finish(None);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_RETURN.0 => {
                let result = {
                    let Ok(mut guard) = STATE.lock() else {
                        return LRESULT(0);
                    };
                    let Some(state) = guard.as_mut() else {
                        return LRESULT(0);
                    };
                    if state.editing_value() {
                        state.commit_value();
                        InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    }
                    state.result(false)
                };
                if result.is_some() {
                    finish(result);
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_KEYDOWN
                if wparam.0 == 0x71
                    || (wparam.0 == 0x41 && GetKeyState(VK_CONTROL.0 as i32) < 0) =>
            {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if wparam.0 == 0x71 && state.interaction.is_none() && !state.editing_value()
                        {
                            if let Some(index) = state.selected_annotation {
                                state.edit_value(index);
                            }
                        } else if wparam.0 == 0x41 {
                            state.select_input_contents();
                        }
                        InvalidateRect(Some(hwnd), None, false);
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN if matches!(wparam.0, 0x59 | 0x5A | 0x2E) => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if wparam.0 == 0x2E && state.editing_value() {
                            if let Some(input) = state
                                .text_input
                                .as_mut()
                                .filter(|input| input.replace_on_type)
                            {
                                input.buffer.clear();
                                input.replace_on_type = false;
                            }
                            if let Some(input) = state
                                .number_input
                                .as_mut()
                                .filter(|input| input.replace_on_type)
                            {
                                input.digits.clear();
                                input.replace_on_type = false;
                            }
                            InvalidateRect(Some(hwnd), None, false);
                            return LRESULT(0);
                        }
                        if !state.editing_value()
                            && state.interaction.is_none()
                            && state.long_capture.is_none()
                        {
                            if wparam.0 == 0x2E {
                                state.delete_annotation();
                            } else if GetKeyState(VK_CONTROL.0 as i32) < 0 {
                                state.undo_or_redo(
                                    wparam.0 == 0x59 || GetKeyState(VK_SHIFT.0 as i32) < 0,
                                );
                            }
                            InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
                if delta == 0 {
                    return LRESULT(0);
                }
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if state.long_capture.is_some() {
                            if delta < 0 {
                                let _ = capture_long_segment(state, hwnd, delta);
                            }
                        } else if state.number_input.is_some() {
                            // The number being entered is independent of the marker size.
                        } else if let Some(input) = state.text_input.as_mut() {
                            if GetKeyState(VK_SHIFT.0 as i32) < 0 {
                                input.outline_width = (input.outline_width
                                    + if delta > 0 { 1 } else { -1 })
                                .clamp(0, 12);
                                state.text_outline_width = input.outline_width;
                            } else {
                                input.font_size = (input.font_size
                                    + if delta > 0 { 2 } else { -2 })
                                .clamp(8, 160);
                            }
                        } else if let Some(index) = {
                            let (x, y) = mouse_point(lparam);
                            let point = (x - state.origin_x, y - state.origin_y);
                            hit_native_annotation(point, &state.annotations).or_else(|| {
                                state.selected_annotation.filter(|&index| {
                                    matches!(
                                        state.annotations.get(index),
                                        Some(NativeAnnotation::Number { .. })
                                    ) && state
                                        .selection
                                        .is_some_and(|rect| clamp_to_rect(point, rect) == point)
                                })
                            })
                        } {
                            state.save_undo();
                            let step = if delta > 0 { 1 } else { -1 };
                            if GetKeyState(VK_SHIFT.0 as i32) < 0
                                && matches!(state.annotations[index], NativeAnnotation::Text { .. })
                            {
                                set_text_outline_width(&mut state.annotations[index], step);
                                if let NativeAnnotation::Text { outline_width, .. } =
                                    state.annotations[index]
                                {
                                    state.text_outline_width = outline_width;
                                }
                            } else if let NativeAnnotation::Number { size, .. } =
                                &mut state.annotations[index]
                            {
                                let pixels = ((delta.abs() * 6 + 60) / 120).max(1) * delta.signum();
                                *size = (*size + pixels).clamp(18, 96);
                            } else {
                                set_native_stroke_width(&mut state.annotations[index], step);
                            }
                            state.selected_annotation = Some(index);
                        } else if let Some(rect) = state.selection {
                            let step = if delta > 0 { 4 } else { -4 };
                            state.corner_radius = (state.corner_radius + step)
                                .clamp(0, rect.width.min(rect.height) / 2);
                            state.composite_bgra.clone_from(&state.dimmed_bgra);
                            state.composite_rect = None;
                        }
                    }
                }
                InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_KEYDOWN
                if matches!(
                    wparam.0 as u16,
                    key if key == VK_LEFT.0
                        || key == VK_RIGHT.0
                        || key == VK_UP.0
                        || key == VK_DOWN.0
                ) =>
            {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if state.interaction.is_none()
                            && !state.editing_value()
                            && state.long_capture.is_none()
                        {
                            if let Some(selection) = state.selection {
                                let distance = if GetKeyState(VK_SHIFT.0 as i32) < 0 {
                                    10
                                } else {
                                    1
                                };
                                let (dx, dy) = match wparam.0 as u16 {
                                    key if key == VK_LEFT.0 => (-distance, 0),
                                    key if key == VK_RIGHT.0 => (distance, 0),
                                    key if key == VK_UP.0 => (0, -distance),
                                    _ => (0, distance),
                                };
                                state.long_capture = None;
                                state.selection =
                                    Some(move_rect(selection, dx, dy, state.width, state.height));
                            }
                        }
                    }
                }
                InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_SETCURSOR => {
                if let Ok(guard) = STATE.lock() {
                    if let Some(state) = guard.as_ref() {
                        let cursor_id = cursor_for_point(state.cursor_position, state);
                        if let Ok(cursor) = LoadCursorW(None, cursor_id) {
                            SetCursor(Some(cursor));
                            return LRESULT(1);
                        }
                    }
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_TIMER => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if state.text_input.is_some() {
                            state.caret_visible = !state.caret_visible;
                            InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_DESTROY => {
                let _ = KillTimer(Some(hwnd), 1);
                finish(None);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    struct PaintLayer {
        dc: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
    }

    impl PaintLayer {
        unsafe fn new(screen: HDC, width: i32, height: i32) -> Option<Self> {
            let dc = CreateCompatibleDC(Some(screen));
            if dc.0.is_null() {
                return None;
            }
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            if bitmap.0.is_null() {
                let _ = DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, bitmap.into());
            Some(Self {
                dc,
                bitmap,
                previous,
            })
        }
    }

    impl Drop for PaintLayer {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.dc, self.previous);
                DeleteObject(self.bitmap.into());
                let _ = DeleteDC(self.dc);
            }
        }
    }

    struct PaintBuffer {
        foreground: PaintLayer,
        background: PaintLayer,
        background_valid: bool,
        dimensions: (i32, i32),
        annotation_pixels: Vec<u8>,
    }

    impl PaintBuffer {
        unsafe fn new(screen: HDC, width: i32, height: i32) -> Option<Self> {
            Some(Self {
                foreground: PaintLayer::new(screen, width, height)?,
                background: PaintLayer::new(screen, width, height)?,
                background_valid: false,
                dimensions: (width, height),
                annotation_pixels: Vec::new(),
            })
        }
    }

    thread_local! {
        static PAINT_BUFFER: RefCell<Option<PaintBuffer>> = const { RefCell::new(None) };
    }

    unsafe fn draw_bgra(dc: HDC, rect: RectInfo, pixels: &[u8]) {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rect.width,
                biHeight: -rect.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        StretchDIBits(
            dc,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            0,
            0,
            rect.width,
            rect.height,
            Some(pixels.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }

    unsafe fn paint(hwnd: HWND) {
        let mut paint = PAINTSTRUCT::default();
        let screen_dc = BeginPaint(hwnd, &mut paint);
        if let Ok(mut guard) = STATE.lock() {
            if let Some(state) = guard.as_mut() {
                PAINT_BUFFER.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    if slot.as_ref().map(|buffer| buffer.dimensions)
                        != Some((state.width, state.height))
                    {
                        *slot = PaintBuffer::new(screen_dc, state.width, state.height);
                    }
                    let Some(buffer) = slot.as_mut() else { return };
                    let dc = buffer.foreground.dc;
                    let visible_rect = state
                        .selection
                        .or(state.hover_candidate)
                        .filter(|rect| rect.width > 0 && rect.height > 0);
                    if update_composite(state, visible_rect) || !buffer.background_valid {
                        draw_bgra(
                            buffer.background.dc,
                            RectInfo {
                                x: 0,
                                y: 0,
                                width: state.width,
                                height: state.height,
                            },
                            &state.composite_bgra,
                        );
                        buffer.background_valid = true;
                    }
                    let dirty = paint.rcPaint;
                    let dirty_width = dirty.right - dirty.left;
                    let dirty_height = dirty.bottom - dirty.top;
                    let _ = BitBlt(
                        dc,
                        dirty.left,
                        dirty.top,
                        dirty_width,
                        dirty_height,
                        Some(buffer.background.dc),
                        dirty.left,
                        dirty.top,
                        SRCCOPY,
                    );
                    if let Some(bounds) =
                        render_annotations(state, dirty, &mut buffer.annotation_pixels)
                    {
                        draw_bgra(dc, bounds, &buffer.annotation_pixels);
                    }

                    SetBkMode(dc, TRANSPARENT);
                    SetTextColor(dc, COLORREF(0x00F0_FFFF));
                    if state.selection.is_none() {
                        let instructions: Vec<u16> =
                        "移動游標偵測視窗 · 拖曳自由框選 · 滾輪調整圓角 · 方向鍵微調 · Enter 確認"
                            .encode_utf16()
                            .collect();
                        TextOutW(dc, 24, 24, &instructions);
                    }

                    if let Some(rect) = state.selection.or(state.hover_candidate) {
                        if rect.width > 0 && rect.height > 0 {
                            let pen = CreatePen(PS_SOLID, 3, COLORREF(0x0074_E7AD));
                            let old_pen = SelectObject(dc, pen.into());
                            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                            RoundRect(
                                dc,
                                rect.x,
                                rect.y,
                                rect.x + rect.width,
                                rect.y + rect.height,
                                state.corner_radius * 2,
                                state.corner_radius * 2,
                            );
                            SelectObject(dc, old_brush);
                            SelectObject(dc, old_pen);
                            DeleteObject(pen.into());

                            if state.selection.is_some() && state.long_capture.is_none() {
                                let handle_brush = CreateSolidBrush(COLORREF(0x00FF_FFFF));
                                let old_pen = SelectObject(dc, GetStockObject(NULL_BRUSH));
                                let old_brush = SelectObject(dc, handle_brush.into());
                                for (x, y) in handle_points(rect) {
                                    Rectangle(dc, x - 5, y - 5, x + 6, y + 6);
                                }
                                SelectObject(dc, old_brush);
                                SelectObject(dc, old_pen);
                                DeleteObject(handle_brush.into());

                                let button = radius_button_rect(rect, state.width, state.height);
                                let button_brush = CreateSolidBrush(COLORREF(0x00FF_7B18));
                                let old_brush = SelectObject(dc, button_brush.into());
                                let button_pen = CreatePen(PS_SOLID, 2, COLORREF(0x00FF_FFFF));
                                let old_pen = SelectObject(dc, button_pen.into());
                                RoundRect(
                                    dc,
                                    button.left,
                                    button.top,
                                    button.right,
                                    button.bottom,
                                    12,
                                    12,
                                );
                                let inset = 11;
                                RoundRect(
                                    dc,
                                    button.left + inset,
                                    button.top + inset,
                                    button.right - inset,
                                    button.bottom - inset,
                                    8,
                                    8,
                                );
                                SelectObject(dc, old_pen);
                                SelectObject(dc, old_brush);
                                DeleteObject(button_pen.into());
                                DeleteObject(button_brush.into());
                            }
                            draw_long_preview(dc, state, rect);

                            let dimensions = if let Some(long) = state.long_capture.as_ref() {
                                format!(
                                    "長擷圖：請向下滾動 · {} × {} px",
                                    long.width,
                                    long.stitched_bgra.len() / (long.width * 4)
                                )
                            } else {
                                format!(
                                    "{} × {} px · 圓角 {} px",
                                    rect.width, rect.height, state.corner_radius
                                )
                            };
                            let dimensions: Vec<u16> = dimensions.encode_utf16().collect();
                            let label_y = if rect.y >= 34 {
                                rect.y - 28
                            } else {
                                (rect.y + rect.height + 10).min(state.height - 22)
                            };
                            let label_width = dimensions.len() as i32 * 10 + 12;
                            let label_x = rect.x.clamp(0, (state.width - label_width).max(0));
                            let label = RECT {
                                left: label_x,
                                top: label_y,
                                right: label_x + label_width,
                                bottom: label_y + 22,
                            };
                            let background = CreateSolidBrush(COLORREF(0x0025_2525));
                            FillRect(dc, &label, background);
                            DeleteObject(background.into());
                            SetTextColor(dc, COLORREF(0x00FA_FAFA));
                            TextOutW(dc, label_x + 6, label_y + 2, &dimensions);
                        }
                    }
                    if state.long_capture.is_none() {
                        draw_native_annotations(dc, state);
                    }
                    if state.selection.is_none() && state.interaction.is_none() {
                        draw_magnifier(dc, state);
                    }
                    draw_edit_hint(dc, state);
                    if state.interaction.is_none()
                        || matches!(state.interaction, Some(Interaction::Annotate { .. }))
                    {
                        if let Some(rect) = state
                            .selection
                            .filter(|rect| rect.width >= 2 && rect.height >= 2)
                        {
                            draw_annotation_toolbar(dc, state, rect);
                            draw_annotation_properties(dc, state, rect);
                            if state.color_target.is_some() {
                                draw_color_palette(dc, state, rect);
                            } else if state.interaction.is_none() {
                                draw_toolbar_hint(dc, state, rect);
                            }
                        }
                    }
                    let _ = BitBlt(
                        screen_dc,
                        dirty.left,
                        dirty.top,
                        dirty_width,
                        dirty_height,
                        Some(dc),
                        dirty.left,
                        dirty.top,
                        SRCCOPY,
                    );
                });
            }
        }
        EndPaint(hwnd, &paint);
    }

    unsafe fn draw_magnifier(dc: windows::Win32::Graphics::Gdi::HDC, state: &SelectorState) {
        const PANEL_WIDTH: i32 = 240;
        const PANEL_HEIGHT: i32 = 158;
        const SAMPLE_COLUMNS: i32 = 11;
        const SAMPLE_ROWS: i32 = 5;
        const PIXEL_SIZE: i32 = 20;
        const PADDING: i32 = 10;

        let cursor = state.cursor_position;
        let panel_x = if cursor.0 + 28 + PANEL_WIDTH <= state.width {
            cursor.0 + 28
        } else {
            (cursor.0 - 28 - PANEL_WIDTH).max(0)
        };
        let panel_y = if cursor.1 + 28 + PANEL_HEIGHT <= state.height {
            cursor.1 + 28
        } else {
            (cursor.1 - 28 - PANEL_HEIGHT).max(0)
        };
        let panel_rect = RECT {
            left: panel_x,
            top: panel_y,
            right: panel_x + PANEL_WIDTH,
            bottom: panel_y + PANEL_HEIGHT,
        };
        let panel_brush = CreateSolidBrush(COLORREF(0x001C_2310));
        FillRect(dc, &panel_rect, panel_brush);
        DeleteObject(panel_brush.into());

        let source_x =
            (cursor.0 - SAMPLE_COLUMNS / 2).clamp(0, (state.width - SAMPLE_COLUMNS).max(0));
        let source_y = (cursor.1 - SAMPLE_ROWS / 2).clamp(0, (state.height - SAMPLE_ROWS).max(0));
        let mut sample_bgra = Vec::with_capacity((SAMPLE_COLUMNS * SAMPLE_ROWS * 4) as usize);
        for row in source_y..source_y + SAMPLE_ROWS {
            let start = ((row * state.width + source_x) * 4) as usize;
            let end = start + (SAMPLE_COLUMNS * 4) as usize;
            sample_bgra.extend_from_slice(&state.bgra[start..end]);
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SAMPLE_COLUMNS,
                biHeight: -SAMPLE_ROWS,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        StretchDIBits(
            dc,
            panel_x + PADDING,
            panel_y + PADDING,
            SAMPLE_COLUMNS * PIXEL_SIZE,
            SAMPLE_ROWS * PIXEL_SIZE,
            0,
            0,
            SAMPLE_COLUMNS,
            SAMPLE_ROWS,
            Some(sample_bgra.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );

        let focus_x = panel_x + PADDING + (cursor.0 - source_x) * PIXEL_SIZE;
        let focus_y = panel_y + PADDING + (cursor.1 - source_y) * PIXEL_SIZE;
        let focus_pen = CreatePen(PS_SOLID, 2, COLORREF(0x00AD_E774));
        let old_pen = SelectObject(dc, focus_pen.into());
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Rectangle(
            dc,
            focus_x,
            focus_y,
            focus_x + PIXEL_SIZE + 1,
            focus_y + PIXEL_SIZE + 1,
        );
        MoveToEx(dc, focus_x + PIXEL_SIZE / 2, focus_y - 4, None);
        let _ = LineTo(dc, focus_x + PIXEL_SIZE / 2, focus_y + PIXEL_SIZE + 4);
        MoveToEx(dc, focus_x - 4, focus_y + PIXEL_SIZE / 2, None);
        let _ = LineTo(dc, focus_x + PIXEL_SIZE + 4, focus_y + PIXEL_SIZE / 2);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(focus_pen.into());

        let pixel_index = ((cursor.1 as usize * state.width as usize + cursor.0 as usize) * 4)
            .min(state.bgra.len().saturating_sub(4));
        let blue = state.bgra[pixel_index];
        let green = state.bgra[pixel_index + 1];
        let red = state.bgra[pixel_index + 2];
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00F5_F7F4));
        let coordinates = format!(
            "X {}  Y {}",
            state.origin_x + cursor.0,
            state.origin_y + cursor.1
        );
        let coordinates: Vec<u16> = coordinates.encode_utf16().collect();
        TextOutW(dc, panel_x + PADDING, panel_y + 116, &coordinates);
        let color = format!("#{red:02X}{green:02X}{blue:02X}  RGB {red}, {green}, {blue}");
        let color: Vec<u16> = color.encode_utf16().collect();
        TextOutW(dc, panel_x + PADDING, panel_y + 136, &color);

        let border_pen = CreatePen(PS_SOLID, 1, COLORREF(0x00AD_E774));
        let old_pen = SelectObject(dc, border_pen.into());
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Rectangle(
            dc,
            panel_rect.left,
            panel_rect.top,
            panel_rect.right,
            panel_rect.bottom,
        );
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(border_pen.into());
    }

    fn finish(result: Option<SelectedArea>) {
        if let Ok(mut guard) = STATE.lock() {
            if let Some(state) = guard.as_mut() {
                if let Some(sender) = state.sender.take() {
                    let _ = sender.send(result);
                }
            }
        }
    }

    fn mouse_point(lparam: LPARAM) -> (i32, i32) {
        let x = (lparam.0 as u16 as i16) as i32;
        let y = ((lparam.0 >> 16) as u16 as i16) as i32;
        (x, y)
    }

    unsafe fn append_mouse_history(state: &mut SelectorState, pointer: (i32, i32)) {
        let (Some(selection), Some(&last)) = (state.selection, state.freehand_points.last()) else {
            return;
        };
        let current = clamp_to_rect(pointer, selection);
        let dx = i64::from(current.0 - last.0);
        let dy = i64::from(current.1 - last.1);
        // Nearby input already describes the curve; only query Windows when
        // two delivered points leave a visible gap.
        if dx * dx + dy * dy < 64 {
            return;
        }
        let sample = MOUSEMOVEPOINT {
            x: (pointer.0 + state.origin_x) & 0xffff,
            y: (pointer.1 + state.origin_y) & 0xffff,
            time: GetMessageTime() as u32,
            ..Default::default()
        };
        let mut history = [MOUSEMOVEPOINT::default(); 64];
        let count = GetMouseMovePointsEx(
            size_of::<MOUSEMOVEPOINT>() as u32,
            &sample,
            &mut history,
            GMMP_USE_DISPLAY_POINTS,
        );
        if count <= 1 {
            return;
        }
        let origin = (state.origin_x, state.origin_y);
        let local = |point: &MOUSEMOVEPOINT| {
            (
                i32::from(point.x as i16) - origin.0,
                i32::from(point.y as i16) - origin.1,
            )
        };
        let history = &history[..count as usize];
        // A real earlier point anchors this batch to the current stroke. Synthetic or
        // out-of-history moves still use the WM_MOUSEMOVE point.
        if let Some(anchor) = history
            .iter()
            .skip(1)
            .position(|point| local(point) == last)
        {
            for point in history[1..anchor + 1].iter().rev() {
                state.append_freehand_point(clamp_to_rect(local(point), selection));
            }
        }
    }

    fn draft_damage(state: &SelectorState) -> Option<RECT> {
        let Some(Interaction::Annotate { start, tool }) = state.interaction else {
            return None;
        };
        let draft = draft_annotation(state, tool, start, state.cursor_position)?;
        let bounds = native_annotation_bounds(draft);
        let padding = if tool == AnnotationTool::Arrow {
            state.stroke_width * 4 + 4
        } else {
            state.stroke_width + 4
        };
        Some(RECT {
            left: (bounds.x - padding).max(0),
            top: (bounds.y - padding).max(0),
            right: (bounds.x + bounds.width + padding).min(state.width),
            bottom: (bounds.y + bounds.height + padding).min(state.height),
        })
    }

    fn clamp_point(point: (i32, i32), width: i32, height: i32) -> (i32, i32) {
        (point.0.clamp(0, width), point.1.clamp(0, height))
    }

    fn clamp_to_rect(point: (i32, i32), rect: RectInfo) -> (i32, i32) {
        (
            point.0.clamp(rect.x, rect.x + rect.width),
            point.1.clamp(rect.y, rect.y + rect.height),
        )
    }

    fn constrain_square(start: (i32, i32), current: (i32, i32)) -> (i32, i32) {
        let size = (current.0 - start.0).abs().max((current.1 - start.1).abs());
        (
            start.0 + if current.0 < start.0 { -size } else { size },
            start.1 + if current.1 < start.1 { -size } else { size },
        )
    }

    fn commit_text_input(state: &mut SelectorState) {
        let Some(input) = state.text_input.take() else {
            return;
        };
        if input.buffer.is_empty() {
            if let Some(index) = input
                .editing_index
                .filter(|&index| index < state.annotations.len())
            {
                state.save_undo();
                state.annotations.remove(index);
                state.selected_annotation = None;
                state.hovered_annotation = None;
            }
            return;
        }
        let mut text = [0u16; 32];
        let length = input.buffer.len().min(32);
        text[..length].copy_from_slice(&input.buffer[..length]);
        let annotation = NativeAnnotation::Text {
            x: input.x,
            y: input.y,
            text,
            length,
            font_size: input.font_size,
            color: input.color,
            outline_color: input.outline_color,
            outline_width: input.outline_width,
        };
        if let Some(index) = input
            .editing_index
            .filter(|index| *index < state.annotations.len())
        {
            if state.annotations[index] != annotation {
                state.save_undo();
                state.annotations[index] = annotation;
            }
            state.selected_annotation = Some(index);
        } else {
            state.save_undo();
            state.annotations.push(annotation);
            state.selected_annotation = Some(state.annotations.len() - 1);
        }
    }

    fn normalize_rect(start: (i32, i32), end: (i32, i32)) -> RectInfo {
        let left = start.0.min(end.0);
        let top = start.1.min(end.1);
        RectInfo {
            x: left,
            y: top,
            width: start.0.max(end.0) - left,
            height: start.1.max(end.1) - top,
        }
    }

    fn handle_points(rect: RectInfo) -> [(i32, i32); 8] {
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        let middle_x = rect.x + rect.width / 2;
        let middle_y = rect.y + rect.height / 2;
        [
            (rect.x, rect.y),
            (middle_x, rect.y),
            (right, rect.y),
            (right, middle_y),
            (right, bottom),
            (middle_x, bottom),
            (rect.x, bottom),
            (rect.x, middle_y),
        ]
    }

    fn radius_button_rect(rect: RectInfo, width: i32, height: i32) -> RECT {
        const SIZE: i32 = 44;
        const GAP: i32 = 12;
        let left = if rect.x + rect.width + GAP + SIZE <= width {
            rect.x + rect.width + GAP
        } else if rect.x >= GAP + SIZE {
            rect.x - GAP - SIZE
        } else {
            (rect.x + rect.width - SIZE).clamp(0, width - SIZE)
        };
        let top = if rect.y + SIZE <= height {
            rect.y
        } else {
            (height - SIZE).max(0)
        };
        RECT {
            left,
            top,
            right: left + SIZE,
            bottom: top + SIZE,
        }
    }

    struct ToolbarLayout {
        bounds: RECT,
        properties: Option<RECT>,
        monitor: RectInfo,
        columns: i32,
    }

    impl ToolbarLayout {
        fn button(&self, index: usize) -> RECT {
            let index = index as i32;
            let left = self.bounds.left + index % self.columns * TOOL_SIZE;
            let top = self.bounds.top + index / self.columns * TOOL_SIZE;
            RECT {
                left,
                top,
                right: left + TOOL_SIZE,
                bottom: top + TOOL_SIZE,
            }
        }
    }

    fn annotation_toolbar_layout(rect: RectInfo, state: &SelectorState) -> ToolbarLayout {
        const GAP: i32 = 12;
        let monitor = state
            .monitor_bounds
            .iter()
            .copied()
            .max_by_key(|monitor| {
                let overlap_width =
                    (rect.x + rect.width).min(monitor.x + monitor.width) - rect.x.max(monitor.x);
                let overlap_height =
                    (rect.y + rect.height).min(monitor.y + monitor.height) - rect.y.max(monitor.y);
                i64::from(overlap_width.max(0)) * i64::from(overlap_height.max(0))
            })
            .unwrap_or(RectInfo {
                x: 0,
                y: 0,
                width: state.width,
                height: state.height,
            });
        let count = state.toolbar_buttons().len() as i32;
        let columns = (monitor.width / TOOL_SIZE).clamp(1, count);
        let toolbar_width = columns * TOOL_SIZE;
        let toolbar_height = ((count + columns - 1) / columns) * TOOL_SIZE;
        let show_properties =
            state.long_capture.is_none() && state.annotation_tool == Some(AnnotationTool::Text);
        let total_height = toolbar_height + if show_properties { TOOL_SIZE } else { 0 };
        let right = monitor.x + monitor.width;
        let bottom = monitor.y + monitor.height;
        let left = rect
            .x
            .clamp(monitor.x, (right - toolbar_width).max(monitor.x));
        let below = (rect.y + rect.height + GAP).max(monitor.y);
        let above = rect.y - GAP - total_height;
        let top = if below + total_height <= bottom {
            below
        } else if above >= monitor.y && above + total_height <= bottom {
            above
        } else {
            (rect.y + rect.height - total_height)
                .clamp(monitor.y, (bottom - total_height).max(monitor.y))
        };
        ToolbarLayout {
            bounds: RECT {
                left,
                top,
                right: left + toolbar_width,
                bottom: top + toolbar_height,
            },
            properties: show_properties.then_some(RECT {
                left,
                top: top + toolbar_height,
                right: left + toolbar_width,
                bottom: top + total_height,
            }),
            monitor,
            columns,
        }
    }

    fn property_cells(panel: RECT) -> (RECT, RECT) {
        let top = panel.top + 4;
        let bottom = panel.bottom - 4;
        let middle = (panel.left + 108).min(panel.left + (panel.right - panel.left) / 2);
        (
            RECT {
                left: panel.left + 4,
                top,
                right: middle,
                bottom,
            },
            RECT {
                left: middle + 4,
                top,
                right: (middle + 160).min(panel.right - 4),
                bottom,
            },
        )
    }

    fn outline_spinner(cell: RECT) -> RECT {
        RECT {
            left: (cell.right - 48).max(cell.left),
            ..cell
        }
    }

    fn number_color_cells(cell: RECT) -> (RECT, RECT) {
        let middle = (cell.left + cell.right) / 2;
        (
            RECT {
                right: middle,
                ..cell
            },
            RECT {
                left: middle,
                ..cell
            },
        )
    }

    fn hit_annotation_property(
        point: (i32, i32),
        layout: &ToolbarLayout,
    ) -> Option<AnnotationProperty> {
        let (first, second) = property_cells(layout.properties?);
        if point_in_rect(point, first) {
            return Some(AnnotationProperty::OutlineColor);
        } else {
            let (_, up, down) = thickness_bounds(outline_spinner(second));
            if point_in_rect(point, up) {
                return Some(AnnotationProperty::OutlineWidth(1));
            }
            if point_in_rect(point, down) {
                return Some(AnnotationProperty::OutlineWidth(-1));
            }
        }
        None
    }

    fn hit_annotation_toolbar(
        point: (i32, i32),
        rect: RectInfo,
        state: &SelectorState,
    ) -> Option<ToolbarButton> {
        let layout = annotation_toolbar_layout(rect, state);
        if !point_in_rect(point, layout.bounds) {
            return None;
        }
        let column = (point.0 - layout.bounds.left) / TOOL_SIZE;
        let row = (point.1 - layout.bounds.top) / TOOL_SIZE;
        let index = (row * layout.columns + column) as usize;
        let button = state.toolbar_buttons().get(index).copied()?;
        if matches!(button, ToolbarButton::Color)
            && state.number_tool_active()
            && point_in_rect(point, number_color_cells(layout.button(index)).0)
        {
            return Some(ToolbarButton::NumberLabelColor);
        }
        if matches!(button, ToolbarButton::Thickness) {
            let (_, up, down) = thickness_bounds(layout.button(index));
            if point_in_rect(point, up) {
                return Some(ToolbarButton::Thicker);
            }
            if point_in_rect(point, down) {
                return Some(ToolbarButton::Thinner);
            }
        }
        Some(button)
    }

    unsafe fn draw_edit_hint(dc: HDC, state: &SelectorState) {
        let Some(selection) = state.selection else {
            return;
        };
        let (anchor, message) = if let Some(input) = state.number_input.as_ref() {
            let Some(annotation) = state.annotations.get(input.index).copied() else {
                return;
            };
            let digits = if input.digits.is_empty() {
                "–"
            } else {
                &input.digits
            };
            let message = if state.english {
                format!("Number: {digits}  ·  Enter: Apply  ·  Esc: Cancel")
            } else {
                format!("編輯編號：{digits}　·　Enter 確認　·　Esc 取消")
            };
            (native_annotation_bounds(annotation), message)
        } else if let Some(input) = state.text_input.as_ref() {
            let launch_tip = if input.replace_on_type {
                if state.english {
                    "All text selected  ·  Backspace: Clear"
                } else {
                    "已全選　·　Backspace 清除"
                }
            } else if state.english {
                "Ctrl+A: Select all  ·  Enter: Apply  ·  Esc: Cancel"
            } else {
                "Ctrl+A 全選　·　Enter 確認　·　Esc 取消"
            };
            (
                RectInfo {
                    x: input.x,
                    y: input.y,
                    width: 0,
                    height: input.font_size + 4,
                },
                launch_tip.to_owned(),
            )
        } else {
            return;
        };
        let monitor = annotation_toolbar_layout(selection, state).monitor;
        let width = message
            .chars()
            .map(|c| if c.is_ascii() { 9 } else { 17 })
            .sum::<i32>()
            + 16;
        let left = anchor.x.clamp(
            monitor.x,
            (monitor.x + monitor.width - width).max(monitor.x),
        );
        let below = anchor.y + anchor.height + 7;
        let top = if below + 24 <= (selection.y + selection.height).min(monitor.y + monitor.height)
        {
            below
        } else {
            (anchor.y - 31).max(monitor.y)
        };
        let bounds = RECT {
            left,
            top,
            right: left + width,
            bottom: top + 24,
        };
        let background = CreateSolidBrush(COLORREF(0x0025_2525));
        FillRect(dc, &bounds, background);
        DeleteObject(background.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FA_FAFA));
        TextOutW(
            dc,
            left + 8,
            top + 3,
            &message.encode_utf16().collect::<Vec<_>>(),
        );
    }

    unsafe fn draw_toolbar_hint(
        dc: windows::Win32::Graphics::Gdi::HDC,
        state: &SelectorState,
        selection: RectInfo,
    ) {
        let Some(button) = hit_annotation_toolbar(state.cursor_position, selection, state) else {
            return;
        };
        let (zh, en) = match button {
            ToolbarButton::Capture => ("確認擷取", "Capture"),
            ToolbarButton::LongCapture => ("長擷圖", "Scrolling capture"),
            ToolbarButton::Cancel => ("取消", "Cancel"),
            ToolbarButton::Pin => ("擷取並釘選到螢幕", "Capture and pin to screen"),
            ToolbarButton::Tool(AnnotationTool::Rectangle) => ("矩形", "Rectangle"),
            ToolbarButton::Tool(AnnotationTool::Ellipse) => ("橢圓", "Ellipse"),
            ToolbarButton::Tool(AnnotationTool::Line) => ("直線", "Line"),
            ToolbarButton::Tool(AnnotationTool::Freehand) => ("畫筆", "Freehand"),
            ToolbarButton::Tool(AnnotationTool::Arrow) => ("箭頭", "Arrow"),
            ToolbarButton::Tool(AnnotationTool::Text) => (
                "文字 · 點兩下物件可重編",
                "Text · Double-click an object to edit",
            ),
            ToolbarButton::Tool(AnnotationTool::Number) => (
                "編號 · 點兩下物件可重編",
                "Number · Double-click an object to edit",
            ),
            ToolbarButton::Tool(AnnotationTool::RoundedNumber) => (
                "圓角矩形編號 · 點兩下物件可重編",
                "Rounded rectangle number · Double-click an object to edit",
            ),
            ToolbarButton::Tool(AnnotationTool::Mosaic) => ("馬賽克", "Mosaic"),
            ToolbarButton::Tool(AnnotationTool::Eraser) => ("橡皮擦", "Eraser"),
            ToolbarButton::Color if state.number_tool_active() => {
                ("選擇編號背景色", "Choose number background")
            }
            ToolbarButton::NumberLabelColor if state.number_colors().1.is_none() => {
                ("編號文字色（自動）", "Number text color (automatic)")
            }
            ToolbarButton::NumberLabelColor => ("編號文字色", "Number text color"),
            ToolbarButton::Color => ("選擇顏色", "Choose color"),
            ToolbarButton::Thinner => ("縮小筆寬或字級", "Decrease width or size"),
            ToolbarButton::Thickness => ("粗細／大小", "Stroke width or size"),
            ToolbarButton::Thicker => ("增加筆寬或字級", "Increase width or size"),
            ToolbarButton::Style => match state.line_style {
                LineStyle::Solid => ("實線 · 點按切換", "Solid · Click to cycle"),
                LineStyle::Dashed => ("虛線 · 點按切換", "Dashed · Click to cycle"),
                LineStyle::Dotted => ("點線 · 點按切換", "Dotted · Click to cycle"),
                LineStyle::DashDot => ("點劃線 · 點按切換", "Dash-dot · Click to cycle"),
            },
            ToolbarButton::Undo => ("復原", "Undo"),
            ToolbarButton::Redo => ("重做", "Redo"),
            ToolbarButton::Delete => ("刪除選取的標註", "Delete selected annotation"),
        };
        let text = if state.english { en } else { zh };
        let width = text
            .chars()
            .map(|c| if c.is_ascii() { 10 } else { 20 })
            .sum::<i32>()
            + 16;
        let layout = annotation_toolbar_layout(selection, state);
        let toolbar = layout.properties.unwrap_or(layout.bounds);
        let monitor = layout.monitor;
        let left = (state.cursor_position.0 - width / 2).clamp(
            monitor.x,
            (monitor.x + monitor.width - width).max(monitor.x),
        );
        let top = if toolbar.bottom + 29 <= monitor.y + monitor.height {
            toolbar.bottom + 5
        } else {
            (layout.bounds.top - 29).max(monitor.y)
        };
        let bounds = RECT {
            left,
            top,
            right: left + width,
            bottom: top + 24,
        };
        let background = CreateSolidBrush(COLORREF(0x0025_2525));
        FillRect(dc, &bounds, background);
        DeleteObject(background.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FA_FAFA));
        TextOutW(
            dc,
            left + 8,
            top + 3,
            &text.encode_utf16().collect::<Vec<_>>(),
        );
    }

    fn color_palette_rect(rect: RectInfo, state: &SelectorState) -> RECT {
        let layout = annotation_toolbar_layout(rect, state);
        let palette_width = 122;
        let palette_height = if state.color_target == Some(ColorTarget::NumberLabel) {
            84
        } else {
            58
        };
        let cell = layout.button(TOOLBAR_COLOR_INDEX);
        let anchor = match state.color_target {
            Some(ColorTarget::Outline) => layout
                .properties
                .map_or(cell, |panel| property_cells(panel).0),
            Some(ColorTarget::NumberLabel) => number_color_cells(cell).0,
            Some(ColorTarget::NumberBackground) => number_color_cells(cell).1,
            _ => cell,
        };
        let color_left = (anchor.left + anchor.right - palette_width) / 2;
        let monitor = layout.monitor;
        let left = color_left.clamp(
            monitor.x,
            (monitor.x + monitor.width - palette_width).max(monitor.x),
        );
        let bottom = layout.properties.unwrap_or(layout.bounds).bottom;
        let top = if bottom + 4 + palette_height <= monitor.y + monitor.height {
            bottom + 4
        } else {
            (layout.bounds.top - 4 - palette_height).max(monitor.y)
        };
        RECT {
            left,
            top,
            right: left + palette_width,
            bottom: top + palette_height,
        }
    }

    fn hit_color_palette(
        point: (i32, i32),
        rect: RectInfo,
        state: &SelectorState,
    ) -> Option<Option<u32>> {
        let palette = color_palette_rect(rect, state);
        if point.0 < palette.left + 6
            || point.0 >= palette.right - 6
            || point.1 < palette.top + 6
            || point.1 >= palette.bottom - 6
        {
            return None;
        }
        let column = (point.0 - palette.left - 6) / 22;
        let row = (point.1 - palette.top - 6) / 22;
        if state.color_target == Some(ColorTarget::NumberLabel) && point.1 >= palette.top + 56 {
            return Some(None);
        }
        (column < 5 && row < 2).then(|| Some(ANNOTATION_COLORS[(row * 5 + column) as usize]))
    }

    unsafe fn draw_color_palette(
        dc: windows::Win32::Graphics::Gdi::HDC,
        state: &SelectorState,
        rect: RectInfo,
    ) {
        let palette = color_palette_rect(rect, state);
        let background = CreateSolidBrush(COLORREF(0x00FA_FAFA));
        let border = CreatePen(PS_SOLID, 1, COLORREF(0x00B8_B8B8));
        let old_brush = SelectObject(dc, background.into());
        let old_pen = SelectObject(dc, border.into());
        RoundRect(
            dc,
            palette.left,
            palette.top,
            palette.right,
            palette.bottom,
            8,
            8,
        );
        for (index, color) in ANNOTATION_COLORS.iter().enumerate() {
            let column = index as i32 % 5;
            let row = index as i32 / 5;
            let left = palette.left + 7 + column * 22;
            let top = palette.top + 7 + row * 22;
            let brush = CreateSolidBrush(COLORREF(*color));
            let previous = SelectObject(dc, brush.into());
            Rectangle(dc, left, top, left + 18, top + 18);
            SelectObject(dc, previous);
            DeleteObject(brush.into());
        }
        if state.color_target == Some(ColorTarget::NumberLabel) {
            let automatic = if state.english { "Automatic" } else { "自動" };
            with_smoothed_property_font(dc, || {
                draw_property_text(
                    dc,
                    palette.left + 10,
                    palette.top + 57,
                    automatic,
                    0x0025_2525,
                );
            });
        }
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(border.into());
        DeleteObject(background.into());
    }

    fn thickness_bounds(cell: RECT) -> (RECT, RECT, RECT) {
        let field = RECT {
            left: cell.left + 1,
            top: cell.top + 6,
            right: cell.right - 1,
            bottom: cell.bottom - 6,
        };
        let split_x = field.right - 15;
        let split_y = (field.top + field.bottom) / 2;
        let up = RECT {
            left: split_x,
            top: field.top + 1,
            right: field.right - 1,
            bottom: split_y,
        };
        let down = RECT {
            left: split_x,
            top: split_y,
            right: field.right - 1,
            bottom: field.bottom - 1,
        };
        (field, up, down)
    }

    unsafe fn draw_thickness_spinner(dc: HDC, cell: RECT, cursor: (i32, i32), width: i32) {
        let (field, up, down) = thickness_bounds(cell);
        let border = CreateSolidBrush(COLORREF(0x0082_7570));
        FillRect(dc, &field, border);
        let value = RECT {
            left: field.left + 1,
            top: field.top + 1,
            right: up.left,
            bottom: field.bottom - 1,
        };
        let paper = CreateSolidBrush(COLORREF(0x00FF_FFFF));
        FillRect(dc, &value, paper);
        DeleteObject(paper.into());

        for (arrow, direction) in [(up, -1), (down, 1)] {
            let background = CreateSolidBrush(COLORREF(if point_in_rect(cursor, arrow) {
                0x00E6_ED_E0
            } else {
                0x00F1_F4_F2
            }));
            FillRect(dc, &arrow, background);
            DeleteObject(background.into());
            let x = (arrow.left + arrow.right) / 2;
            let y = (arrow.top + arrow.bottom) / 2;
            let glyph = [
                POINT {
                    x: x - 4,
                    y: y - direction * 2,
                },
                POINT {
                    x: x + 4,
                    y: y - direction * 2,
                },
                POINT {
                    x,
                    y: y + direction * 3,
                },
            ];
            let ink = CreateSolidBrush(COLORREF(0x0025_2525));
            let old_brush = SelectObject(dc, ink.into());
            let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            Polygon(dc, &glyph);
            SelectObject(dc, old_pen);
            SelectObject(dc, old_brush);
            DeleteObject(ink.into());
        }
        FillRect(
            dc,
            &RECT {
                right: up.left + 1,
                ..up
            },
            border,
        );
        FillRect(
            dc,
            &RECT {
                top: down.top,
                bottom: down.top + 1,
                ..down
            },
            border,
        );
        FillRect(
            dc,
            &RECT {
                right: down.left + 1,
                ..down
            },
            border,
        );
        DeleteObject(border.into());

        let text: Vec<u16> = width.to_string().encode_utf16().collect();
        let mut extent = SIZE::default();
        GetTextExtentPoint32W(dc, &text, &mut extent);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x0025_2525));
        TextOutW(
            dc,
            value.left + ((value.right - value.left - extent.cx) / 2).max(0),
            value.top + ((value.bottom - value.top - extent.cy) / 2).max(0),
            &text,
        );
    }

    unsafe fn with_smoothed_property_font(dc: HDC, draw: impl FnOnce()) {
        let font = CreateFontW(
            -16,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            FONT_CHARSET(0),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            ANTIALIASED_QUALITY,
            0,
            w!("Microsoft JhengHei UI"),
        );
        if font.0.is_null() {
            draw();
            return;
        }
        let previous = SelectObject(dc, font.into());
        draw();
        SelectObject(dc, previous);
        DeleteObject(font.into());
    }

    unsafe fn draw_property_text(dc: HDC, x: i32, y: i32, text: &str, color: u32) {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(color));
        TextOutW(dc, x, y, &text.encode_utf16().collect::<Vec<_>>());
    }

    unsafe fn draw_annotation_properties(dc: HDC, state: &SelectorState, selection: RectInfo) {
        let layout = annotation_toolbar_layout(selection, state);
        let Some(panel) = layout.properties else {
            return;
        };
        let brush = CreateSolidBrush(COLORREF(0x00F5_F5_F5));
        FillRect(dc, &panel, brush);
        DeleteObject(brush.into());
        let label = |zh, en| if state.english { en } else { zh };
        let (first, second) = property_cells(panel);
        let (color, width) = state.text_outline();
        with_smoothed_property_font(dc, || {
            draw_property_swatch(
                dc,
                state,
                first,
                ColorTarget::Outline,
                label("外框色", "Outline"),
                color,
            );
            draw_property_text(
                dc,
                second.left + 5,
                second.top + 8,
                label("外框粗細", "Outline width"),
                0x0025_2525,
            );
            draw_thickness_spinner(dc, outline_spinner(second), state.cursor_position, width);
        });
    }

    unsafe fn draw_property_swatch(
        dc: HDC,
        state: &SelectorState,
        cell: RECT,
        target: ColorTarget,
        text: &str,
        color: u32,
    ) {
        let selected = state.color_target == Some(target);
        let background = if selected {
            0x00FF_DC9B
        } else if point_in_rect(state.cursor_position, cell) {
            0x00E6_ED_E0
        } else {
            0x00FF_FFFF
        };
        let border = CreateSolidBrush(COLORREF(0x0076_7676));
        let brush = CreateSolidBrush(COLORREF(background));
        FillRect(dc, &cell, border);
        FillRect(
            dc,
            &RECT {
                left: cell.left + 1,
                top: cell.top + 1,
                right: cell.right - 1,
                bottom: cell.bottom - 1,
            },
            brush,
        );
        let swatch = RECT {
            left: cell.right - 23,
            top: cell.top + 8,
            right: cell.right - 5,
            bottom: cell.bottom - 8,
        };
        FillRect(dc, &swatch, border);
        let color_brush = CreateSolidBrush(COLORREF(color));
        FillRect(
            dc,
            &RECT {
                left: swatch.left + 1,
                top: swatch.top + 1,
                right: swatch.right - 1,
                bottom: swatch.bottom - 1,
            },
            color_brush,
        );
        draw_property_text(dc, cell.left + 5, cell.top + 7, text, 0x0025_2525);
        DeleteObject(color_brush.into());
        DeleteObject(brush.into());
        DeleteObject(border.into());
    }

    unsafe fn draw_number_color_cells(dc: HDC, state: &SelectorState, cell: RECT) {
        let (label_cell, background_cell) = number_color_cells(cell);
        let (background, label) = state.number_colors();
        for (bounds, target, color) in [
            (
                label_cell,
                ColorTarget::NumberLabel,
                number_ink(label, background),
            ),
            (background_cell, ColorTarget::NumberBackground, background),
        ] {
            let selected = state.color_target == Some(target);
            let paper = CreateSolidBrush(COLORREF(if selected {
                0x00FF_DC9B
            } else if point_in_rect(state.cursor_position, bounds) {
                0x00E6_ED_E0
            } else {
                0x00FA_FAFA
            }));
            FillRect(dc, &bounds, paper);
            DeleteObject(paper.into());
            let center_x = (bounds.left + bounds.right) / 2;
            let center_y = (bounds.top + bounds.bottom) / 2;
            let border = CreateSolidBrush(COLORREF(0x0025_2525));
            let fill = CreateSolidBrush(COLORREF(color));
            FillRect(
                dc,
                &RECT {
                    left: center_x - 7,
                    top: center_y - 7,
                    right: center_x + 7,
                    bottom: center_y + 7,
                },
                border,
            );
            FillRect(
                dc,
                &RECT {
                    left: center_x - 6,
                    top: center_y - 6,
                    right: center_x + 6,
                    bottom: center_y + 6,
                },
                fill,
            );
            DeleteObject(fill.into());
            if selected {
                FillRect(
                    dc,
                    &RECT {
                        left: center_x - 5,
                        top: center_y + 12,
                        right: center_x + 5,
                        bottom: center_y + 14,
                    },
                    border,
                );
            }
            DeleteObject(border.into());
            if target == ColorTarget::NumberLabel {
                let pen = CreatePen(PS_SOLID, 1, COLORREF(number_ink(None, color)));
                let previous = SelectObject(dc, pen.into());
                MoveToEx(dc, center_x - 4, center_y + 4, None);
                LineTo(dc, center_x, center_y - 4);
                LineTo(dc, center_x + 4, center_y + 5);
                MoveToEx(dc, center_x - 2, center_y + 1, None);
                LineTo(dc, center_x + 3, center_y + 1);
                SelectObject(dc, previous);
                DeleteObject(pen.into());
            }
        }
    }

    unsafe fn draw_annotation_toolbar(
        dc: windows::Win32::Graphics::Gdi::HDC,
        state: &SelectorState,
        rect: RectInfo,
    ) {
        let layout = annotation_toolbar_layout(rect, state);
        let brush = CreateSolidBrush(COLORREF(0x00FA_FAFA));
        FillRect(dc, &layout.bounds, brush);
        DeleteObject(brush.into());
        for (index, button) in state.toolbar_buttons().iter().copied().enumerate() {
            let bounds = layout.button(index);
            let (icon, selected, enabled) = match button {
                ToolbarButton::Capture => (10, false, true),
                ToolbarButton::LongCapture => (12, state.long_capture.is_some(), true),
                ToolbarButton::Cancel => (11, false, true),
                ToolbarButton::Pin => (13, false, true),
                ToolbarButton::Tool(tool) => (
                    match tool {
                        AnnotationTool::Rectangle => 0,
                        AnnotationTool::Ellipse => 1,
                        AnnotationTool::Freehand => 2,
                        AnnotationTool::Arrow => 3,
                        AnnotationTool::Text => 4,
                        AnnotationTool::Number => 5,
                        AnnotationTool::RoundedNumber => 15,
                        AnnotationTool::Mosaic => 6,
                        AnnotationTool::Eraser => 7,
                        AnnotationTool::Line => 14,
                    },
                    state.annotation_tool == Some(tool),
                    true,
                ),
                ToolbarButton::Color if state.number_tool_active() => {
                    draw_number_color_cells(dc, state, bounds);
                    continue;
                }
                ToolbarButton::Color => (8, state.color_target == Some(ColorTarget::Main), true),
                ToolbarButton::Thickness => {
                    let width = state
                        .text_input
                        .as_ref()
                        .map(|input| input.font_size)
                        .or_else(|| state.selected_object().map(native_thickness))
                        .unwrap_or_else(|| state.tool_thickness());
                    draw_thickness_spinner(dc, bounds, state.cursor_position, width);
                    continue;
                }
                // These actions are the two halves of the spinner, never separate cells.
                ToolbarButton::Thicker
                | ToolbarButton::Thinner
                | ToolbarButton::NumberLabelColor => continue,
                ToolbarButton::Style => (18, false, true),
                ToolbarButton::Undo => (
                    9,
                    false,
                    !state.undo_annotations.is_empty() || state.editing_value(),
                ),
                ToolbarButton::Redo => (19, false, !state.redo_annotations.is_empty()),
                ToolbarButton::Delete => (20, false, state.selected_annotation.is_some()),
            };
            draw_toolbar_icon(dc, icon, bounds.left, bounds.top, state, selected, enabled);
        }
        if state.long_capture.is_none() {
            let first_annotation = layout.button(CAPTURE_BUTTON_COUNT);
            let separator = if CAPTURE_BUTTON_COUNT as i32 % layout.columns == 0 {
                RECT {
                    left: layout.bounds.left + 7,
                    top: first_annotation.top,
                    right: layout.bounds.right - 7,
                    bottom: first_annotation.top + 1,
                }
            } else {
                RECT {
                    left: first_annotation.left,
                    top: first_annotation.top + 8,
                    right: first_annotation.left + 1,
                    bottom: first_annotation.bottom - 8,
                }
            };
            let brush = CreateSolidBrush(COLORREF(0x0082_7570));
            FillRect(dc, &separator, brush);
            DeleteObject(brush.into());
        }
    }

    unsafe fn draw_long_preview(
        dc: windows::Win32::Graphics::Gdi::HDC,
        state: &SelectorState,
        rect: RectInfo,
    ) {
        let Some(long) = state.long_capture.as_ref() else {
            return;
        };
        let total_height = long.stitched_bgra.len() / (long.width * 4);
        if total_height == 0 {
            return;
        }
        let mut target_width = 180.min((state.width - rect.x - rect.width - 20).max(0));
        let mut left = rect.x + rect.width + 12;
        if target_width < 90 {
            target_width = 180.min((rect.x - 20).max(0));
            left = rect.x - target_width - 12;
        }
        if target_width < 60 {
            return;
        }
        let target_height = ((total_height as f64 / long.width as f64 * target_width as f64)
            as i32)
            .min(state.height - 40)
            .max(60);
        let top = ((state.height - target_height) / 2).max(10);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: long.width as i32,
                biHeight: -(total_height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        StretchDIBits(
            dc,
            left,
            top,
            target_width,
            target_height,
            0,
            0,
            long.width as i32,
            total_height as i32,
            Some(long.stitched_bgra.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        let border = CreatePen(PS_SOLID, 3, COLORREF(0x0074_E7AD));
        let old_pen = SelectObject(dc, border.into());
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Rectangle(
            dc,
            left - 2,
            top - 2,
            left + target_width + 2,
            top + target_height + 2,
        );
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(border.into());
    }

    #[derive(Clone, Copy, Eq, Hash, PartialEq)]
    struct ToolIcon {
        icon: usize,
        background: [u8; 3],
        ink: u8,
        color: u32,
        style: LineStyle,
    }

    thread_local! {
        static TOOL_ICONS: RefCell<HashMap<ToolIcon, Vec<u8>>> = RefCell::new(HashMap::new());
    }

    unsafe fn draw_toolbar_icon(
        dc: HDC,
        index: usize,
        left: i32,
        top: i32,
        state: &SelectorState,
        selected: bool,
        enabled: bool,
    ) {
        let bounds = RectInfo {
            x: left,
            y: top,
            width: TOOL_SIZE,
            height: TOOL_SIZE,
        };
        let capture_action = matches!(index, 10..=13);
        let hovered = enabled
            && point_in_rect(
                state.cursor_position,
                RECT {
                    left,
                    top,
                    right: left + TOOL_SIZE,
                    bottom: top + TOOL_SIZE,
                },
            );
        let background = match (capture_action, selected, hovered) {
            (true, true, _) => [198, 198, 198],
            (true, false, true) => [204, 204, 204],
            (true, false, false) => [218, 218, 218],
            (false, true, _) => [155, 220, 255],
            (false, false, true) => [224, 237, 230],
            _ => [250, 250, 250],
        };
        let icon = ToolIcon {
            icon: index,
            background,
            ink: if enabled { 37 } else { 105 },
            color: if index == 8 { state.current_color } else { 0 },
            style: if index == 18 {
                state.line_style
            } else {
                LineStyle::Solid
            },
        };
        TOOL_ICONS.with(|icons| {
            let mut icons = icons.borrow_mut();
            let pixels = icons
                .entry(icon)
                .or_insert_with(|| render_toolbar_icon(icon));
            if !pixels.is_empty() {
                draw_bgra(dc, bounds, pixels);
            }
        });
        if capture_action && selected {
            let active = CreateSolidBrush(COLORREF(0x0025_2525));
            FillRect(
                dc,
                &RECT {
                    left: left + 14,
                    top: top + 36,
                    right: left + 30,
                    bottom: top + 38,
                },
                active,
            );
            DeleteObject(active.into());
        }
    }

    fn render_toolbar_icon(icon: ToolIcon) -> Vec<u8> {
        let Some(mut pixmap) = tiny_skia::Pixmap::new(TOOL_SIZE as u32, TOOL_SIZE as u32) else {
            return Vec::new();
        };
        let [r, g, b] = icon.background;
        pixmap.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
        let index = icon.icon;
        let mut paint = tiny_skia::Paint::default();
        let ink = icon.ink;
        paint.set_color_rgba8(ink, ink, ink, 255);
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width: 2.0,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Default::default()
        };
        let scale = 2.0 / 3.0;
        let inset = TOOL_SIZE as f32 * (1.0 - scale) / 2.0;
        let icon_transform =
            tiny_skia::Transform::from_scale(scale, scale).post_translate(inset, inset);
        let mut path = tiny_skia::PathBuilder::new();
        match index {
            0 => {
                if let Some(rect) = tiny_skia::Rect::from_xywh(13.0, 13.0, 18.0, 18.0) {
                    path.push_rect(rect);
                }
            }
            1 => {
                if let Some(rect) = tiny_skia::Rect::from_xywh(12.0, 12.0, 20.0, 20.0) {
                    path.push_oval(rect);
                }
            }
            2 => {
                path.move_to(13.0, 31.0);
                path.line_to(29.0, 15.0);
                path.line_to(33.0, 19.0);
                path.line_to(17.0, 35.0);
                path.line_to(12.0, 36.0);
                path.close();
            }
            3 => {
                path.move_to(12.0, 31.0);
                path.line_to(32.0, 11.0);
                path.move_to(20.0, 11.0);
                path.line_to(32.0, 11.0);
                path.line_to(32.0, 23.0);
            }
            4 => {
                path.move_to(13.0, 12.0);
                path.line_to(31.0, 12.0);
                path.move_to(22.0, 12.0);
                path.line_to(22.0, 32.0);
            }
            5 | 15 => {
                if index == 15 {
                    if let Some(rect) = tiny_skia::Rect::from_xywh(9.0, 12.0, 26.0, 20.0) {
                        push_rounded_rectangle(&mut path, rect, 4.0);
                    }
                } else if let Some(rect) = tiny_skia::Rect::from_xywh(12.0, 12.0, 20.0, 20.0) {
                    path.push_oval(rect);
                }
                path.move_to(22.0, 17.0);
                path.line_to(22.0, 27.0);
                path.move_to(19.0, 19.0);
                path.line_to(22.0, 17.0);
            }
            6 => {
                for row in 0..2 {
                    for column in 0..2 {
                        if let Some(rect) = tiny_skia::Rect::from_xywh(
                            13.0 + column as f32 * 10.0,
                            13.0 + row as f32 * 10.0,
                            8.0,
                            8.0,
                        ) {
                            path.push_rect(rect);
                        }
                    }
                }
            }
            7 => {
                path.move_to(14.0, 27.0);
                path.line_to(25.0, 14.0);
                path.line_to(32.0, 20.0);
                path.line_to(21.0, 33.0);
                path.close();
                path.move_to(18.0, 29.0);
                path.line_to(27.0, 29.0);
            }
            8 => {
                if let Some(rect) = tiny_skia::Rect::from_xywh(13.0, 13.0, 18.0, 18.0) {
                    let rgba = super::colorref_to_rgba(icon.color);
                    let mut fill = tiny_skia::Paint::default();
                    fill.set_color_rgba8(rgba[0], rgba[1], rgba[2], 255);
                    fill.anti_alias = true;
                    let mut swatch = tiny_skia::PathBuilder::new();
                    swatch.push_rect(rect);
                    if let Some(swatch) = swatch.finish() {
                        pixmap.fill_path(
                            &swatch,
                            &fill,
                            tiny_skia::FillRule::Winding,
                            icon_transform,
                            None,
                        );
                        pixmap.stroke_path(
                            &swatch,
                            &paint,
                            &tiny_skia::Stroke {
                                width: 1.0,
                                ..stroke.clone()
                            },
                            icon_transform,
                            None,
                        );
                    }
                }
            }
            9 => {
                path.move_to(33.0, 31.0);
                path.cubic_to(33.0, 20.0, 25.0, 15.0, 14.0, 18.0);
                path.move_to(14.0, 18.0);
                path.line_to(20.0, 12.0);
                path.move_to(14.0, 18.0);
                path.line_to(21.0, 24.0);
            }
            10 => {
                path.move_to(12.0, 22.0);
                path.line_to(19.0, 29.0);
                path.line_to(33.0, 14.0);
            }
            11 => {
                path.move_to(14.0, 14.0);
                path.line_to(30.0, 30.0);
                path.move_to(30.0, 14.0);
                path.line_to(14.0, 30.0);
            }
            12 => {
                path.move_to(14.0, 13.0);
                path.line_to(30.0, 31.0);
                path.move_to(30.0, 13.0);
                path.line_to(14.0, 31.0);
                if let Some(left) = tiny_skia::Rect::from_xywh(9.0, 8.0, 8.0, 8.0) {
                    path.push_oval(left);
                }
                if let Some(right) = tiny_skia::Rect::from_xywh(9.0, 28.0, 8.0, 8.0) {
                    path.push_oval(right);
                }
            }
            13 => {
                path.move_to(15.0, 12.0);
                path.line_to(29.0, 12.0);
                path.line_to(26.0, 15.0);
                path.line_to(26.0, 21.0);
                path.line_to(30.0, 25.0);
                path.line_to(14.0, 25.0);
                path.line_to(18.0, 21.0);
                path.line_to(18.0, 15.0);
                path.close();
                path.move_to(22.0, 25.0);
                path.line_to(22.0, 34.0);
            }
            14 => {
                path.move_to(13.0, 31.0);
                path.line_to(31.0, 13.0);
            }
            18 => {
                path.move_to(10.0, 22.0);
                path.line_to(34.0, 22.0);
                if let Some(line) = path.finish() {
                    pixmap.stroke_path(
                        &line,
                        &paint,
                        &super::skia_stroke(2, icon.style),
                        icon_transform,
                        None,
                    );
                }
                path = tiny_skia::PathBuilder::new();
            }
            19 => {
                path.move_to(11.0, 31.0);
                path.cubic_to(11.0, 20.0, 19.0, 15.0, 30.0, 18.0);
                path.move_to(30.0, 18.0);
                path.line_to(24.0, 12.0);
                path.move_to(30.0, 18.0);
                path.line_to(23.0, 24.0);
            }
            20 => {
                path.move_to(13.0, 15.0);
                path.line_to(31.0, 15.0);
                path.move_to(18.0, 15.0);
                path.line_to(18.0, 12.0);
                path.line_to(26.0, 12.0);
                path.line_to(26.0, 15.0);
                path.move_to(15.0, 17.0);
                path.line_to(17.0, 32.0);
                path.line_to(27.0, 32.0);
                path.line_to(29.0, 17.0);
                path.move_to(22.0, 20.0);
                path.line_to(22.0, 28.0);
            }
            _ => {}
        }
        if index != 8 {
            if let Some(path) = path.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, icon_transform, None);
            }
        }
        let mut bgra = pixmap.take();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        bgra
    }

    fn render_annotations(
        state: &SelectorState,
        dirty: RECT,
        scratch: &mut Vec<u8>,
    ) -> Option<RectInfo> {
        if state.long_capture.is_some() {
            return None;
        }
        let selection = state.selection?;
        let mut annotations: Vec<NativeAnnotation> = state
            .annotations
            .iter()
            .copied()
            .filter(|annotation| !matches!(annotation, NativeAnnotation::Text { .. }))
            .collect();
        if let Some(Interaction::Annotate { start, tool }) = state.interaction {
            if let Some(draft) = draft_annotation(state, tool, start, state.cursor_position) {
                annotations.push(draft);
            }
        }
        let mut left = selection.x + selection.width;
        let mut top = selection.y + selection.height;
        let mut right = selection.x;
        let mut bottom = selection.y;
        for annotation in &annotations {
            let bounds = if let NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                headed: true,
                control_x,
                control_y,
                ..
            } = *annotation
            {
                let left = start_x.min(end_x).min(control_x);
                let top = start_y.min(end_y).min(control_y);
                RectInfo {
                    x: left,
                    y: top,
                    width: start_x.max(end_x).max(control_x) - left,
                    height: start_y.max(end_y).max(control_y) - top,
                }
            } else {
                native_annotation_bounds(*annotation)
            };
            let padding = match *annotation {
                NativeAnnotation::Rectangle { stroke_width, .. }
                | NativeAnnotation::Ellipse { stroke_width, .. }
                | NativeAnnotation::Freehand { stroke_width, .. } => stroke_width + 2,
                NativeAnnotation::Arrow { stroke_width, .. } => stroke_width * 4 + 2,
                NativeAnnotation::Number { .. } => 3,
                _ => 0,
            };
            left = left.min(bounds.x - padding);
            top = top.min(bounds.y - padding);
            right = right.max(bounds.x + bounds.width + padding);
            bottom = bottom.max(bounds.y + bounds.height + padding);
        }
        left = left.max(selection.x);
        top = top.max(selection.y);
        right = right.min(selection.x + selection.width);
        bottom = bottom.min(selection.y + selection.height);
        // Mosaic averages complete blocks, so its render must retain the surrounding pixels.
        if !annotations
            .iter()
            .any(|annotation| matches!(annotation, NativeAnnotation::Mosaic { .. }))
        {
            left = left.max(dirty.left);
            top = top.max(dirty.top);
            right = right.min(dirty.right);
            bottom = bottom.min(dirty.bottom);
        }
        if right <= left || bottom <= top {
            return None;
        }
        let bounds = RectInfo {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        };
        scratch.clear();
        let row_bytes = bounds.width as usize * 4;
        for y in top..bottom {
            let start = (y as usize * state.width as usize + left as usize) * 4;
            scratch.extend_from_slice(&state.composite_bgra[start..start + row_bytes]);
        }
        for pixel in scratch.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        apply_native_annotations(scratch, bounds, &annotations);
        for pixel in scratch.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Some(bounds)
    }

    fn draft_annotation(
        state: &SelectorState,
        tool: AnnotationTool,
        start: (i32, i32),
        (end_x, end_y): (i32, i32),
    ) -> Option<NativeAnnotation> {
        match tool {
            AnnotationTool::Rectangle => Some(NativeAnnotation::Rectangle {
                start_x: start.0,
                start_y: start.1,
                end_x,
                end_y,
                stroke_width: state.stroke_width,
                style: state.line_style,
                color: state.current_color,
            }),
            AnnotationTool::Ellipse => Some(NativeAnnotation::Ellipse {
                start_x: start.0,
                start_y: start.1,
                end_x,
                end_y,
                stroke_width: state.stroke_width,
                style: state.line_style,
                color: state.current_color,
            }),
            AnnotationTool::Freehand => {
                let mut points_x = [0i32; 128];
                let mut points_y = [0i32; 128];
                let length = state.freehand_points.len().min(128);
                for (index, (x, y)) in state.freehand_points.iter().take(length).enumerate() {
                    points_x[index] = *x;
                    points_y[index] = *y;
                }
                Some(NativeAnnotation::Freehand {
                    points_x,
                    points_y,
                    length,
                    stroke_width: state.stroke_width,
                    style: state.line_style,
                    color: state.current_color,
                })
            }
            AnnotationTool::Arrow | AnnotationTool::Line => Some(NativeAnnotation::Arrow {
                start_x: start.0,
                start_y: start.1,
                end_x,
                end_y,
                stroke_width: state.stroke_width,
                style: state.line_style,
                headed: tool == AnnotationTool::Arrow,
                control_x: (start.0 + end_x) / 2,
                control_y: (start.1 + end_y) / 2,
                color: state.current_color,
            }),
            AnnotationTool::Mosaic => Some(NativeAnnotation::Mosaic {
                start_x: start.0,
                start_y: start.1,
                end_x,
                end_y,
                block_size: state.tool_thickness(),
            }),
            AnnotationTool::Text
            | AnnotationTool::Number
            | AnnotationTool::RoundedNumber
            | AnnotationTool::Eraser => None,
        }
    }

    unsafe fn draw_native_annotations(
        dc: windows::Win32::Graphics::Gdi::HDC,
        state: &SelectorState,
    ) {
        for (index, annotation) in state.annotations.iter().enumerate() {
            if state
                .text_input
                .as_ref()
                .and_then(|input| input.editing_index)
                == Some(index)
            {
                continue;
            }
            if matches!(annotation, NativeAnnotation::Text { .. }) {
                draw_native_annotation(dc, *annotation, false);
            }
        }
        if let Some(input) = state.text_input.as_ref() {
            let mut text = [0u16; 32];
            let length = input.buffer.len().min(32);
            text[..length].copy_from_slice(&input.buffer[..length]);
            let draft = NativeAnnotation::Text {
                x: input.x,
                y: input.y,
                text,
                length,
                font_size: input.font_size,
                color: input.color,
                outline_color: input.outline_color,
                outline_width: input.outline_width,
            };
            draw_native_annotation(dc, draft, input.replace_on_type);
            if length > 0 {
                draw_native_object_handles(dc, draft);
            }
            if state.caret_visible && !input.replace_on_type {
                let caret_x = input.x + text_visual_width(&text[..length], input.font_size) + 2;
                for (width, color) in [(4, 0x00FF_FFFF), (2, 0x0025_2525)] {
                    let caret = CreatePen(PS_SOLID, width, COLORREF(color));
                    let old_pen = SelectObject(dc, caret.into());
                    MoveToEx(dc, caret_x, input.y, None);
                    let _ = LineTo(dc, caret_x, input.y + input.font_size + 4);
                    SelectObject(dc, old_pen);
                    DeleteObject(caret.into());
                }
            }
        }
        if let Some(index) = state.selected_annotation.or(state.hovered_annotation) {
            if state
                .text_input
                .as_ref()
                .and_then(|input| input.editing_index)
                != Some(index)
            {
                draw_native_object_handles(dc, state.annotations[index]);
            }
        }
    }

    unsafe fn draw_native_object_handles(
        dc: windows::Win32::Graphics::Gdi::HDC,
        annotation: NativeAnnotation,
    ) {
        if matches!(annotation, NativeAnnotation::Text { .. }) {
            let bounds = native_annotation_bounds(annotation);
            let outline = CreatePen(PS_DOT, 1, COLORREF(0x00D9_7418));
            let previous_pen = SelectObject(dc, outline.into());
            let previous_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            Rectangle(
                dc,
                bounds.x,
                bounds.y,
                bounds.x + bounds.width,
                bounds.y + bounds.height,
            );
            SelectObject(dc, previous_brush);
            SelectObject(dc, previous_pen);
            DeleteObject(outline.into());
        }
        let brush = CreateSolidBrush(COLORREF(0x00FF_FFFF));
        let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00AA_6917));
        let old_brush = SelectObject(dc, brush.into());
        let old_pen = SelectObject(dc, pen.into());
        for (x, y) in annotation_handle_points(annotation) {
            Rectangle(dc, x - 5, y - 5, x + 6, y + 6);
        }
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(pen.into());
        DeleteObject(brush.into());
    }

    unsafe fn draw_native_annotation(
        dc: windows::Win32::Graphics::Gdi::HDC,
        annotation: NativeAnnotation,
        selected_text: bool,
    ) {
        match annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                color,
                ..
            } => {
                let pen = CreatePen(PS_SOLID, stroke_width, COLORREF(color));
                let old_pen = SelectObject(dc, pen.into());
                let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                Rectangle(
                    dc,
                    start_x.min(end_x),
                    start_y.min(end_y),
                    start_x.max(end_x),
                    start_y.max(end_y),
                );
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                DeleteObject(pen.into());
            }
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                control_x,
                control_y,
                color,
                ..
            } => {
                let points: Vec<POINT> = curved_arrow_points(
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    control_x,
                    control_y,
                    f64::from(stroke_width),
                )
                .into_iter()
                .map(|(x, y)| POINT { x, y })
                .collect();
                let brush = CreateSolidBrush(COLORREF(color));
                let old_brush = SelectObject(dc, brush.into());
                let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
                Polygon(dc, &points);
                SelectObject(dc, old_pen);
                SelectObject(dc, old_brush);
                DeleteObject(brush.into());
            }
            NativeAnnotation::Ellipse {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                color,
                ..
            } => {
                let pen = CreatePen(PS_SOLID, stroke_width, COLORREF(color));
                let old_pen = SelectObject(dc, pen.into());
                let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                Ellipse(
                    dc,
                    start_x.min(end_x),
                    start_y.min(end_y),
                    start_x.max(end_x),
                    start_y.max(end_y),
                );
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                DeleteObject(pen.into());
            }
            NativeAnnotation::Text {
                x,
                y,
                text,
                length,
                color,
                outline_color,
                outline_width,
                font_size,
            } => {
                let family: Vec<u16> = "Microsoft JhengHei\0".encode_utf16().collect();
                let font = CreateFontW(
                    -font_size.clamp(8, 160),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    FONT_CHARSET(0),
                    FONT_OUTPUT_PRECISION(0),
                    FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(5),
                    0,
                    PCWSTR(family.as_ptr()),
                );
                let old_font = SelectObject(dc, font.into());
                SetBkMode(dc, TRANSPARENT);
                let slice = &text[..length.min(text.len())];
                if selected_text && !slice.is_empty() {
                    let mut extent = SIZE::default();
                    if GetTextExtentPoint32W(dc, slice, &mut extent).as_bool() {
                        let bounds = RECT {
                            left: x,
                            top: y,
                            right: x + extent.cx.max(1),
                            bottom: y + extent.cy.max(font_size),
                        };
                        let brush = CreateSolidBrush(COLORREF(0x0080_4B16));
                        FillRect(dc, &bounds, brush);
                        DeleteObject(brush.into());
                    }
                    SetTextColor(dc, COLORREF(0x00FF_FFFF));
                    TextOutW(dc, x, y, slice);
                    SelectObject(dc, old_font);
                    DeleteObject(font.into());
                    return;
                }
                SetTextColor(dc, COLORREF(outline_color));
                let radius = outline_width.clamp(0, 12);
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        if dx == 0 && dy == 0 || dx * dx + dy * dy > radius * radius {
                            continue;
                        }
                        TextOutW(dc, x + dx, y + dy, slice);
                    }
                }
                SetTextColor(dc, COLORREF(color));
                TextOutW(dc, x, y, slice);
                SelectObject(dc, old_font);
                DeleteObject(font.into());
            }
            _ => {}
        }
    }

    fn native_annotation_bounds(annotation: NativeAnnotation) -> RectInfo {
        match annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                ..
            }
            | NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                ..
            }
            | NativeAnnotation::Ellipse {
                start_x,
                start_y,
                end_x,
                end_y,
                ..
            }
            | NativeAnnotation::Mosaic {
                start_x,
                start_y,
                end_x,
                end_y,
                ..
            } => RectInfo {
                x: start_x.min(end_x),
                y: start_y.min(end_y),
                width: (end_x - start_x).abs(),
                height: (end_y - start_y).abs(),
            },
            NativeAnnotation::Freehand {
                points_x,
                points_y,
                length,
                ..
            } => {
                let length = length.min(points_x.len()).max(1);
                let min_x = *points_x[..length].iter().min().unwrap_or(&0);
                let max_x = *points_x[..length].iter().max().unwrap_or(&0);
                let min_y = *points_y[..length].iter().min().unwrap_or(&0);
                let max_y = *points_y[..length].iter().max().unwrap_or(&0);
                RectInfo {
                    x: min_x,
                    y: min_y,
                    width: max_x - min_x,
                    height: max_y - min_y,
                }
            }
            NativeAnnotation::Number {
                x, y, size, shape, ..
            } => {
                let width = (size as f32 * shape.aspect_ratio()).ceil() as i32;
                RectInfo {
                    x: x - width / 2,
                    y: y - size / 2,
                    width,
                    height: size,
                }
            }
            NativeAnnotation::Text {
                x,
                y,
                text,
                length,
                font_size,
                ..
            } => RectInfo {
                x,
                y,
                width: text_visual_width(&text[..length.min(text.len())], font_size),
                height: font_size + 6,
            },
        }
    }

    fn text_visual_width(text: &[u16], font_size: i32) -> i32 {
        text.iter()
            .map(|character| {
                if *character <= 0x7f {
                    font_size * 3 / 5
                } else {
                    font_size
                }
            })
            .sum::<i32>()
            .max(font_size / 2)
    }

    fn annotation_handle_points(annotation: NativeAnnotation) -> Vec<(i32, i32)> {
        match annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                ..
            } => vec![
                (start_x, start_y),
                (end_x, start_y),
                (end_x, end_y),
                (start_x, end_y),
            ],
            NativeAnnotation::Ellipse {
                start_x,
                start_y,
                end_x,
                end_y,
                ..
            } => vec![
                (start_x, start_y),
                (end_x, start_y),
                (end_x, end_y),
                (start_x, end_y),
            ],
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                headed: false,
                ..
            } => vec![(start_x, start_y), (end_x, end_y)],
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                control_x,
                control_y,
                ..
            } => vec![(start_x, start_y), (control_x, control_y), (end_x, end_y)],
            NativeAnnotation::Text { .. }
            | NativeAnnotation::Freehand { .. }
            | NativeAnnotation::Number { .. }
            | NativeAnnotation::Mosaic { .. } => {
                let bounds = native_annotation_bounds(annotation);
                vec![
                    (bounds.x, bounds.y),
                    (bounds.x + bounds.width, bounds.y),
                    (bounds.x + bounds.width, bounds.y + bounds.height),
                    (bounds.x, bounds.y + bounds.height),
                ]
            }
        }
    }

    fn hit_native_annotation_handle(
        point: (i32, i32),
        annotations: &[NativeAnnotation],
    ) -> Option<(usize, AnnotationHandle)> {
        annotations
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, annotation)| {
                let handles = match *annotation {
                    NativeAnnotation::Rectangle {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        ..
                    } => vec![
                        ((start_x, start_y), AnnotationHandle::Start),
                        ((end_x, start_y), AnnotationHandle::TopRight),
                        ((end_x, end_y), AnnotationHandle::End),
                        ((start_x, end_y), AnnotationHandle::BottomLeft),
                    ],
                    NativeAnnotation::Ellipse {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        ..
                    } => vec![
                        ((start_x, start_y), AnnotationHandle::Start),
                        ((end_x, start_y), AnnotationHandle::TopRight),
                        ((end_x, end_y), AnnotationHandle::End),
                        ((start_x, end_y), AnnotationHandle::BottomLeft),
                    ],
                    NativeAnnotation::Arrow {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        headed: false,
                        ..
                    } => vec![
                        ((start_x, start_y), AnnotationHandle::Start),
                        ((end_x, end_y), AnnotationHandle::End),
                    ],
                    NativeAnnotation::Arrow {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        control_x,
                        control_y,
                        ..
                    } => vec![
                        ((start_x, start_y), AnnotationHandle::Start),
                        ((end_x, end_y), AnnotationHandle::End),
                        ((control_x, control_y), AnnotationHandle::Curve),
                    ],
                    NativeAnnotation::Text { .. }
                    | NativeAnnotation::Freehand { .. }
                    | NativeAnnotation::Number { .. }
                    | NativeAnnotation::Mosaic { .. } => {
                        let bounds = native_annotation_bounds(*annotation);
                        vec![(
                            (bounds.x + bounds.width, bounds.y + bounds.height),
                            AnnotationHandle::End,
                        )]
                    }
                };
                handles.into_iter().find_map(|((x, y), handle)| {
                    ((point.0 - x).abs() <= 9 && (point.1 - y).abs() <= 9)
                        .then_some((index, handle))
                })
            })
    }

    fn adjust_native_annotation(
        annotation: NativeAnnotation,
        handle: AnnotationHandle,
        point: (i32, i32),
    ) -> NativeAnnotation {
        match annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                color,
            } => match handle {
                AnnotationHandle::Start => NativeAnnotation::Rectangle {
                    start_x: point.0,
                    start_y: point.1,
                    end_x,
                    end_y,
                    stroke_width,
                    style,
                    color,
                },
                AnnotationHandle::End | AnnotationHandle::Curve => NativeAnnotation::Rectangle {
                    start_x,
                    start_y,
                    end_x: point.0,
                    end_y: point.1,
                    stroke_width,
                    style,
                    color,
                },
                AnnotationHandle::TopRight => NativeAnnotation::Rectangle {
                    start_x,
                    start_y: point.1,
                    end_x: point.0,
                    end_y,
                    stroke_width,
                    style,
                    color,
                },
                AnnotationHandle::BottomLeft => NativeAnnotation::Rectangle {
                    start_x: point.0,
                    start_y,
                    end_x,
                    end_y: point.1,
                    stroke_width,
                    style,
                    color,
                },
            },
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                control_x,
                control_y,
                stroke_width,
                style,
                headed,
                color,
            } => match handle {
                AnnotationHandle::Start => NativeAnnotation::Arrow {
                    start_x: point.0,
                    start_y: point.1,
                    end_x,
                    end_y,
                    control_x,
                    control_y,
                    stroke_width,
                    style,
                    headed,
                    color,
                },
                AnnotationHandle::End => NativeAnnotation::Arrow {
                    start_x,
                    start_y,
                    end_x: point.0,
                    end_y: point.1,
                    control_x,
                    control_y,
                    stroke_width,
                    style,
                    headed,
                    color,
                },
                AnnotationHandle::Curve => NativeAnnotation::Arrow {
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    control_x: point.0,
                    control_y: point.1,
                    stroke_width,
                    style,
                    headed,
                    color,
                },
                AnnotationHandle::TopRight | AnnotationHandle::BottomLeft => {
                    NativeAnnotation::Arrow {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        control_x,
                        control_y,
                        stroke_width,
                        style,
                        headed,
                        color,
                    }
                }
            },
            NativeAnnotation::Ellipse {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                color,
            } => {
                let (new_start_x, new_start_y, new_end_x, new_end_y) = match handle {
                    AnnotationHandle::Start => (point.0, point.1, end_x, end_y),
                    AnnotationHandle::End | AnnotationHandle::Curve => {
                        (start_x, start_y, point.0, point.1)
                    }
                    AnnotationHandle::TopRight => (start_x, point.1, point.0, end_y),
                    AnnotationHandle::BottomLeft => (point.0, start_y, end_x, point.1),
                };
                NativeAnnotation::Ellipse {
                    start_x: new_start_x,
                    start_y: new_start_y,
                    end_x: new_end_x,
                    end_y: new_end_y,
                    stroke_width,
                    style,
                    color,
                }
            }
            NativeAnnotation::Text {
                x,
                y,
                text,
                length,
                font_size,
                color,
                outline_color,
                outline_width,
            } => {
                let new_size = match handle {
                    AnnotationHandle::Start => font_size,
                    _ => (point.1 - y).abs().clamp(8, 160),
                };
                NativeAnnotation::Text {
                    x,
                    y,
                    text,
                    length,
                    font_size: new_size,
                    color,
                    outline_color,
                    outline_width,
                }
            }
            NativeAnnotation::Number {
                x,
                y,
                number,
                size: _,
                color,
                shape,
                label_color,
            } => NativeAnnotation::Number {
                x,
                y,
                number,
                size: (2.0
                    * (((point.0 - x).abs() as f32 / shape.aspect_ratio())
                        .max((point.1 - y).abs() as f32)))
                .round()
                .clamp(18.0, 96.0) as i32,
                color,
                shape,
                label_color,
            },
            NativeAnnotation::Mosaic {
                start_x,
                start_y,
                block_size,
                ..
            } => NativeAnnotation::Mosaic {
                start_x,
                start_y,
                end_x: point.0,
                end_y: point.1,
                block_size,
            },
            NativeAnnotation::Freehand { .. } => annotation,
        }
    }

    fn hit_selection_edge(point: (i32, i32), rect: RectInfo) -> bool {
        let tolerance = 7;
        point.0 >= rect.x - tolerance
            && point.0 <= rect.x + rect.width + tolerance
            && point.1 >= rect.y - tolerance
            && point.1 <= rect.y + rect.height + tolerance
            && ((point.0 - rect.x).abs() <= tolerance
                || (point.0 - rect.x - rect.width).abs() <= tolerance
                || (point.1 - rect.y).abs() <= tolerance
                || (point.1 - rect.y - rect.height).abs() <= tolerance)
    }

    fn cursor_for_point(point: (i32, i32), state: &SelectorState) -> windows::core::PCWSTR {
        if state.long_capture.is_some() {
            return IDC_ARROW;
        }
        if let Some(selection) = state.selection {
            let layout = annotation_toolbar_layout(selection, state);
            let toolbar = layout.bounds;
            let radius_button = radius_button_rect(selection, state.width, state.height);
            let over_palette = state.color_target.is_some()
                && point_in_rect(point, color_palette_rect(selection, state));
            if point_in_rect(point, toolbar)
                || point_in_rect(point, radius_button)
                || over_palette
                || layout
                    .properties
                    .is_some_and(|panel| point_in_rect(point, panel))
            {
                return IDC_ARROW;
            }
            if let Some(handle) = hit_handle(point, selection) {
                return match handle {
                    Handle::Top | Handle::Bottom => IDC_SIZENS,
                    Handle::Left | Handle::Right => IDC_SIZEWE,
                    Handle::TopLeft | Handle::BottomRight => IDC_SIZENWSE,
                    Handle::TopRight | Handle::BottomLeft => IDC_SIZENESW,
                };
            }
            if hit_selection_edge(point, selection)
                || (state.annotation_tool.is_none() && clamp_to_rect(point, selection) == point)
            {
                return IDC_SIZEALL;
            }
        }
        if let Some((_, handle)) = hit_native_annotation_handle(point, &state.annotations) {
            return match handle {
                AnnotationHandle::TopRight | AnnotationHandle::BottomLeft => IDC_SIZENESW,
                AnnotationHandle::Start | AnnotationHandle::End | AnnotationHandle::Curve => {
                    IDC_SIZENWSE
                }
            };
        }
        if hit_native_annotation(point, &state.annotations).is_some() {
            return IDC_SIZEALL;
        }
        if matches!(state.annotation_tool, Some(AnnotationTool::Text)) {
            IDC_IBEAM
        } else if state.selection.is_none() || state.annotation_tool.is_some() {
            IDC_CROSS
        } else {
            IDC_ARROW
        }
    }

    fn point_in_rect(point: (i32, i32), rect: RECT) -> bool {
        point.0 >= rect.left && point.0 < rect.right && point.1 >= rect.top && point.1 < rect.bottom
    }

    fn move_native_annotation(annotation: NativeAnnotation, dx: i32, dy: i32) -> NativeAnnotation {
        match annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                color,
            } => NativeAnnotation::Rectangle {
                start_x: start_x + dx,
                start_y: start_y + dy,
                end_x: end_x + dx,
                end_y: end_y + dy,
                stroke_width,
                style,
                color,
            },
            NativeAnnotation::Ellipse {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                color,
            } => NativeAnnotation::Ellipse {
                start_x: start_x + dx,
                start_y: start_y + dy,
                end_x: end_x + dx,
                end_y: end_y + dy,
                stroke_width,
                style,
                color,
            },
            NativeAnnotation::Text {
                x,
                y,
                text,
                length,
                font_size,
                color,
                outline_color,
                outline_width,
            } => NativeAnnotation::Text {
                x: x + dx,
                y: y + dy,
                text,
                length,
                font_size,
                color,
                outline_color,
                outline_width,
            },
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                style,
                headed,
                control_x,
                control_y,
                color,
            } => NativeAnnotation::Arrow {
                start_x: start_x + dx,
                start_y: start_y + dy,
                end_x: end_x + dx,
                end_y: end_y + dy,
                stroke_width,
                style,
                headed,
                control_x: control_x + dx,
                control_y: control_y + dy,
                color,
            },
            NativeAnnotation::Freehand {
                mut points_x,
                mut points_y,
                length,
                stroke_width,
                style,
                color,
            } => {
                for index in 0..length.min(points_x.len()) {
                    points_x[index] += dx;
                    points_y[index] += dy;
                }
                NativeAnnotation::Freehand {
                    points_x,
                    points_y,
                    length,
                    stroke_width,
                    style,
                    color,
                }
            }
            NativeAnnotation::Number {
                x,
                y,
                number,
                size,
                color,
                shape,
                label_color,
            } => NativeAnnotation::Number {
                x: x + dx,
                y: y + dy,
                number,
                size,
                color,
                shape,
                label_color,
            },
            NativeAnnotation::Mosaic {
                start_x,
                start_y,
                end_x,
                end_y,
                block_size,
            } => NativeAnnotation::Mosaic {
                start_x: start_x + dx,
                start_y: start_y + dy,
                end_x: end_x + dx,
                end_y: end_y + dy,
                block_size,
            },
        }
    }

    fn set_native_stroke_width(annotation: &mut NativeAnnotation, step: i32) {
        match annotation {
            NativeAnnotation::Rectangle { stroke_width, .. }
            | NativeAnnotation::Arrow { stroke_width, .. }
            | NativeAnnotation::Ellipse { stroke_width, .. } => {
                *stroke_width = (*stroke_width + step).clamp(1, 32);
            }
            NativeAnnotation::Text { font_size, .. } => {
                *font_size = (*font_size + step * 2).clamp(8, 160);
            }
            NativeAnnotation::Freehand { stroke_width, .. } => {
                *stroke_width = (*stroke_width + step).clamp(1, 32);
            }
            NativeAnnotation::Number { size, .. } => {
                *size = (*size + step * 2).clamp(18, 96);
            }
            NativeAnnotation::Mosaic { block_size, .. } => {
                *block_size = (*block_size + step).clamp(4, 48);
            }
        }
    }

    fn native_thickness(annotation: NativeAnnotation) -> i32 {
        match annotation {
            NativeAnnotation::Rectangle { stroke_width, .. }
            | NativeAnnotation::Ellipse { stroke_width, .. }
            | NativeAnnotation::Freehand { stroke_width, .. }
            | NativeAnnotation::Arrow { stroke_width, .. } => stroke_width,
            NativeAnnotation::Text { font_size, .. } => font_size,
            NativeAnnotation::Number { size, .. } => size,
            NativeAnnotation::Mosaic { block_size, .. } => block_size,
        }
    }

    fn native_line_style(annotation: NativeAnnotation) -> Option<LineStyle> {
        match annotation {
            NativeAnnotation::Rectangle { style, .. }
            | NativeAnnotation::Ellipse { style, .. }
            | NativeAnnotation::Freehand { style, .. }
            | NativeAnnotation::Arrow { style, .. } => Some(style),
            _ => None,
        }
    }

    fn set_native_line_style(annotation: &mut NativeAnnotation, new_style: LineStyle) {
        match annotation {
            NativeAnnotation::Rectangle { style, .. }
            | NativeAnnotation::Ellipse { style, .. }
            | NativeAnnotation::Freehand { style, .. }
            | NativeAnnotation::Arrow { style, .. } => *style = new_style,
            _ => {}
        }
    }

    fn set_native_color(annotation: &mut NativeAnnotation, new_color: u32) {
        let color = match annotation {
            NativeAnnotation::Rectangle { color, .. }
            | NativeAnnotation::Arrow { color, .. }
            | NativeAnnotation::Ellipse { color, .. }
            | NativeAnnotation::Text { color, .. }
            | NativeAnnotation::Freehand { color, .. }
            | NativeAnnotation::Number { color, .. } => color,
            NativeAnnotation::Mosaic { .. } => return,
        };
        *color = new_color;
    }

    fn set_text_outline_color(annotation: &mut NativeAnnotation, new_color: u32) {
        if let NativeAnnotation::Text { outline_color, .. } = annotation {
            *outline_color = new_color;
        }
    }

    fn set_text_outline_width(annotation: &mut NativeAnnotation, step: i32) {
        if let NativeAnnotation::Text { outline_width, .. } = annotation {
            *outline_width = (*outline_width + step).clamp(0, 12);
        }
    }

    fn hit_native_annotation(point: (i32, i32), annotations: &[NativeAnnotation]) -> Option<usize> {
        annotations
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, annotation)| {
                let hit = match *annotation {
                    NativeAnnotation::Rectangle {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        stroke_width,
                        ..
                    } => {
                        let left = start_x.min(end_x);
                        let right = start_x.max(end_x);
                        let top = start_y.min(end_y);
                        let bottom = start_y.max(end_y);
                        let tolerance = stroke_width + 7;
                        point.0 >= left - tolerance
                            && point.0 <= right + tolerance
                            && point.1 >= top - tolerance
                            && point.1 <= bottom + tolerance
                            && ((point.0 - left).abs() <= tolerance
                                || (point.0 - right).abs() <= tolerance
                                || (point.1 - top).abs() <= tolerance
                                || (point.1 - bottom).abs() <= tolerance)
                    }
                    NativeAnnotation::Arrow {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        stroke_width,
                        headed: false,
                        ..
                    } => {
                        distance_to_segment(point, (start_x, start_y), (end_x, end_y))
                            <= f64::from(stroke_width + 7)
                    }
                    NativeAnnotation::Arrow {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        stroke_width,
                        control_x,
                        control_y,
                        ..
                    } => (0_i32..=16).any(|index| {
                        let t1 = f64::from(index) / 16.0;
                        let t0 = f64::from(index.saturating_sub(1)) / 16.0;
                        let curve = |t: f64| {
                            let inv = 1.0 - t;
                            (
                                (inv * inv * f64::from(start_x)
                                    + 2.0 * inv * t * f64::from(control_x)
                                    + t * t * f64::from(end_x))
                                .round() as i32,
                                (inv * inv * f64::from(start_y)
                                    + 2.0 * inv * t * f64::from(control_y)
                                    + t * t * f64::from(end_y))
                                .round() as i32,
                            )
                        };
                        distance_to_segment(point, curve(t0), curve(t1))
                            <= f64::from(stroke_width * 2 + 8)
                    }),
                    NativeAnnotation::Ellipse {
                        start_x,
                        start_y,
                        end_x,
                        end_y,
                        stroke_width,
                        ..
                    } => {
                        let cx = f64::from(start_x + end_x) / 2.0;
                        let cy = f64::from(start_y + end_y) / 2.0;
                        let rx = f64::from((end_x - start_x).abs()).max(2.0) / 2.0;
                        let ry = f64::from((end_y - start_y).abs()).max(2.0) / 2.0;
                        let value = ((f64::from(point.0) - cx) / rx).powi(2)
                            + ((f64::from(point.1) - cy) / ry).powi(2);
                        (value - 1.0).abs() <= (f64::from(stroke_width + 8) / rx.min(ry)).max(0.08)
                    }
                    NativeAnnotation::Text { .. }
                    | NativeAnnotation::Freehand { .. }
                    | NativeAnnotation::Number { .. }
                    | NativeAnnotation::Mosaic { .. } => {
                        let bounds = native_annotation_bounds(*annotation);
                        point.0 >= bounds.x
                            && point.0 <= bounds.x + bounds.width
                            && point.1 >= bounds.y
                            && point.1 <= bounds.y + bounds.height
                    }
                };
                hit.then_some(index)
            })
    }

    fn distance_to_segment(point: (i32, i32), start: (i32, i32), end: (i32, i32)) -> f64 {
        let dx = f64::from(end.0 - start.0);
        let dy = f64::from(end.1 - start.1);
        let length_squared = dx * dx + dy * dy;
        if length_squared == 0.0 {
            return f64::from((point.0 - start.0).pow(2) + (point.1 - start.1).pow(2)).sqrt();
        }
        let t = ((f64::from(point.0 - start.0) * dx + f64::from(point.1 - start.1) * dy)
            / length_squared)
            .clamp(0.0, 1.0);
        let nearest_x = f64::from(start.0) + t * dx;
        let nearest_y = f64::from(start.1) + t * dy;
        (f64::from(point.0) - nearest_x).hypot(f64::from(point.1) - nearest_y)
    }

    fn hit_handle(point: (i32, i32), rect: RectInfo) -> Option<Handle> {
        const HIT_RADIUS: i32 = 9;
        let handles = [
            Handle::TopLeft,
            Handle::Top,
            Handle::TopRight,
            Handle::Right,
            Handle::BottomRight,
            Handle::Bottom,
            Handle::BottomLeft,
            Handle::Left,
        ];
        handle_points(rect)
            .into_iter()
            .zip(handles)
            .find_map(|((x, y), handle)| {
                ((point.0 - x).abs() <= HIT_RADIUS && (point.1 - y).abs() <= HIT_RADIUS)
                    .then_some(handle)
            })
    }

    fn update_selection(
        interaction: Interaction,
        current: (i32, i32),
        width: i32,
        height: i32,
    ) -> RectInfo {
        match interaction {
            Interaction::Create { start } => normalize_rect(start, current),
            Interaction::PendingWindow { candidate, .. } => candidate,
            Interaction::Resize {
                handle,
                start,
                original,
            } => resize_rect(original, handle, start, current, width, height),
            Interaction::Annotate { start, .. } => normalize_rect(start, current),
            Interaction::MoveAnnotation { original, .. } => native_annotation_bounds(original),
            Interaction::AdjustAnnotation { original, .. } => native_annotation_bounds(original),
            Interaction::MoveSelection { original, .. } => original,
        }
    }

    fn resize_rect(
        original: RectInfo,
        handle: Handle,
        start: (i32, i32),
        current: (i32, i32),
        width: i32,
        height: i32,
    ) -> RectInfo {
        let dx = current.0 - start.0;
        let dy = current.1 - start.1;
        let mut left = original.x;
        let mut top = original.y;
        let mut right = original.x + original.width;
        let mut bottom = original.y + original.height;

        if matches!(handle, Handle::TopLeft | Handle::Left | Handle::BottomLeft) {
            left = (original.x + dx).clamp(0, right - 2);
        }
        if matches!(
            handle,
            Handle::TopRight | Handle::Right | Handle::BottomRight
        ) {
            right = (original.x + original.width + dx).clamp(left + 2, width);
        }
        if matches!(handle, Handle::TopLeft | Handle::Top | Handle::TopRight) {
            top = (original.y + dy).clamp(0, bottom - 2);
        }
        if matches!(
            handle,
            Handle::BottomLeft | Handle::Bottom | Handle::BottomRight
        ) {
            bottom = (original.y + original.height + dy).clamp(top + 2, height);
        }
        RectInfo {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }

    fn move_rect(rect: RectInfo, dx: i32, dy: i32, width: i32, height: i32) -> RectInfo {
        RectInfo {
            x: (rect.x + dx).clamp(0, width - rect.width),
            y: (rect.y + dy).clamp(0, height - rect.height),
            ..rect
        }
    }

    fn empty_rect(point: (i32, i32)) -> RectInfo {
        RectInfo {
            x: point.0,
            y: point.1,
            width: 0,
            height: 0,
        }
    }

    fn update_composite(state: &mut SelectorState, visible_rect: Option<RectInfo>) -> bool {
        let next_key = visible_rect.map(|rect| rect_key(rect, state.corner_radius));
        if state.composite_rect == next_key {
            return false;
        }
        if let Some((x, y, width, height, _)) = state.composite_rect {
            copy_rect_pixels(
                &state.dimmed_bgra,
                &mut state.composite_bgra,
                state.width,
                RectInfo {
                    x,
                    y,
                    width,
                    height,
                },
            );
        }
        if let Some(rect) = visible_rect {
            copy_rounded_pixels(
                &state.bgra,
                &mut state.composite_bgra,
                state.width,
                rect,
                state.corner_radius,
            );
        }
        state.composite_rect = next_key;
        true
    }

    fn copy_rect_pixels(source: &[u8], destination: &mut [u8], stride: i32, rect: RectInfo) {
        let row_bytes = rect.width as usize * 4;
        for y in rect.y..rect.y + rect.height {
            let start = (y as usize * stride as usize + rect.x as usize) * 4;
            destination[start..start + row_bytes]
                .copy_from_slice(&source[start..start + row_bytes]);
        }
    }

    fn copy_rounded_pixels(
        source: &[u8],
        destination: &mut [u8],
        stride: i32,
        rect: RectInfo,
        radius: i32,
    ) {
        let radius = radius.clamp(0, rect.width.min(rect.height) / 2);
        if radius == 0 {
            copy_rect_pixels(source, destination, stride, rect);
            return;
        }
        let radius_squared = i64::from(radius) * i64::from(radius);
        for local_y in 0..rect.height {
            for local_x in 0..rect.width {
                let dx = if local_x < radius {
                    radius - local_x
                } else if local_x >= rect.width - radius {
                    local_x - (rect.width - radius - 1)
                } else {
                    0
                };
                let dy = if local_y < radius {
                    radius - local_y
                } else if local_y >= rect.height - radius {
                    local_y - (rect.height - radius - 1)
                } else {
                    0
                };
                if dx == 0 || dy == 0 || i64::from(dx * dx + dy * dy) <= radius_squared {
                    let index = (((rect.y + local_y) * stride + rect.x + local_x) * 4) as usize;
                    destination[index..index + 4].copy_from_slice(&source[index..index + 4]);
                }
            }
        }
    }

    fn rect_key(rect: RectInfo, radius: i32) -> (i32, i32, i32, i32, i32) {
        (rect.x, rect.y, rect.width, rect.height, radius)
    }

    unsafe fn detect_window_at(
        selector_hwnd: HWND,
        local_point: (i32, i32),
        state: &SelectorState,
    ) -> Option<RectInfo> {
        let screen_x = state.origin_x + local_point.0;
        let screen_y = state.origin_y + local_point.1;
        let mut current = GetWindow(selector_hwnd, GW_HWNDFIRST).ok()?;

        while !current.is_invalid() {
            if current != selector_hwnd
                && IsWindowVisible(current).as_bool()
                && !IsIconic(current).as_bool()
            {
                if window_contains_point(current, screen_x, screen_y) {
                    let target =
                        deepest_child_at(current, screen_x, screen_y, 0).unwrap_or(current);
                    return clipped_window_rect(target, state);
                }
            }
            current = match GetWindow(current, GW_HWNDNEXT) {
                Ok(next) => next,
                Err(_) => break,
            };
        }
        None
    }

    unsafe fn deepest_child_at(
        parent: HWND,
        screen_x: i32,
        screen_y: i32,
        depth: u8,
    ) -> Option<HWND> {
        if depth >= 24 {
            return None;
        }
        let mut child = GetWindow(parent, GW_CHILD).ok()?;
        while !child.is_invalid() {
            if IsWindowVisible(child).as_bool() && window_contains_point(child, screen_x, screen_y)
            {
                return Some(
                    deepest_child_at(child, screen_x, screen_y, depth + 1).unwrap_or(child),
                );
            }
            child = match GetWindow(child, GW_HWNDNEXT) {
                Ok(next) => next,
                Err(_) => break,
            };
        }
        None
    }

    unsafe fn window_contains_point(hwnd: HWND, screen_x: i32, screen_y: i32) -> bool {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).is_ok()
            && rect.right > rect.left
            && rect.bottom > rect.top
            && screen_x >= rect.left
            && screen_x < rect.right
            && screen_y >= rect.top
            && screen_y < rect.bottom
    }

    unsafe fn clipped_window_rect(hwnd: HWND, state: &SelectorState) -> Option<RectInfo> {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        let left = (rect.left - state.origin_x).clamp(0, state.width);
        let top = (rect.top - state.origin_y).clamp(0, state.height);
        let right = (rect.right - state.origin_x).clamp(0, state.width);
        let bottom = (rect.bottom - state.origin_y).clamp(0, state.height);
        (right - left >= 2 && bottom - top >= 2).then_some(RectInfo {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

#[cfg(not(windows))]
mod platform {
    use crate::capture::RectInfo;

    pub fn run_selector(
        _virtual_desktop: RectInfo,
        _rgba: Vec<u8>,
        _initial_selection: Option<RectInfo>,
        _monitor_bounds: Vec<RectInfo>,
        _english: bool,
    ) -> Result<Option<super::SelectedArea>, String> {
        Err("PoC-B 目前只支援 Windows".into())
    }
}
