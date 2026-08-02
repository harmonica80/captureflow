use crate::capture::{capture_frame, MonitorInfo, RectInfo};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, time::SystemTime};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshot {
    pub image_path: String,
    pub metadata_path: String,
    pub selection: RectInfo,
    pub width: i32,
    pub height: i32,
    pub corner_radius: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectedArea {
    selection: RectInfo,
    corner_radius: i32,
    #[serde(default)]
    annotations: Vec<NativeAnnotation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum NativeAnnotation {
    Rectangle {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        stroke_width: i32,
        color: u32,
    },
    Arrow {
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
        stroke_width: i32,
        control_x: i32,
        control_y: i32,
        color: u32,
    },
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

pub async fn select_area(app: AppHandle) -> Result<Option<SelectionSnapshot>, String> {
    let frame = capture_frame()?;
    let virtual_desktop = frame.virtual_desktop;
    let rgba_for_window = frame.rgba.clone();

    let selected = tauri::async_runtime::spawn_blocking(move || {
        platform::run_selector(virtual_desktop, rgba_for_window)
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
    let snapshot = save_selection(
        &app,
        &frame,
        local_selection,
        global_selection,
        selected.corner_radius,
        &selected.annotations,
    )?;
    save_last_selection(&app, global_selection, selected.corner_radius)?;
    Ok(Some(snapshot))
}

pub fn repeat_last_area(app: AppHandle) -> Result<SelectionSnapshot, String> {
    let previous = load_last_selection(&app)?;
    let global_selection = previous.selection;
    let frame = capture_frame()?;
    let local_selection = RectInfo {
        x: global_selection.x - frame.virtual_desktop.x,
        y: global_selection.y - frame.virtual_desktop.y,
        width: global_selection.width,
        height: global_selection.height,
    };
    save_selection(
        &app,
        &frame,
        local_selection,
        global_selection,
        previous.corner_radius,
        &[],
    )
    .map_err(|error| format!("無法重複上次範圍；螢幕配置可能已變更：{error}"))
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
    let snapshot = save_selection(&app, &frame, local_selection, global_selection, 0, &[])?;
    save_last_selection(&app, global_selection, 0)?;
    Ok(snapshot)
}

fn save_selection(
    app: &AppHandle,
    frame: &crate::capture::DesktopFrame,
    local_selection: RectInfo,
    global_selection: RectInfo,
    corner_radius: i32,
    annotations: &[NativeAnnotation],
) -> Result<SelectionSnapshot, String> {
    let mut cropped = crop_rgba(
        &frame.rgba,
        frame.virtual_desktop.width,
        frame.virtual_desktop.height,
        local_selection,
    )?;
    apply_native_annotations(&mut cropped, local_selection, annotations);
    apply_rounded_alpha(
        &mut cropped,
        local_selection.width,
        local_selection.height,
        corner_radius,
    );

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
        &cropped,
        local_selection.width as u32,
        local_selection.height as u32,
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
        width: local_selection.width,
        height: local_selection.height,
        corner_radius,
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
        })
        .map_err(|error| format!("上次範圍資料已損壞：{error}"))
}

fn apply_native_annotations(
    pixels: &mut [u8],
    selection: RectInfo,
    annotations: &[NativeAnnotation],
) {
    for annotation in annotations {
        match *annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                color,
            } => {
                let left = start_x.min(end_x) - selection.x;
                let right = start_x.max(end_x) - selection.x;
                let top = start_y.min(end_y) - selection.y;
                let bottom = start_y.max(end_y) - selection.y;
                draw_rgba_line(
                    pixels,
                    selection.width,
                    selection.height,
                    left,
                    top,
                    right,
                    top,
                    stroke_width,
                    color,
                );
                draw_rgba_line(
                    pixels,
                    selection.width,
                    selection.height,
                    right,
                    top,
                    right,
                    bottom,
                    stroke_width,
                    color,
                );
                draw_rgba_line(
                    pixels,
                    selection.width,
                    selection.height,
                    right,
                    bottom,
                    left,
                    bottom,
                    stroke_width,
                    color,
                );
                draw_rgba_line(
                    pixels,
                    selection.width,
                    selection.height,
                    left,
                    bottom,
                    left,
                    top,
                    stroke_width,
                    color,
                );
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
            } => {
                let points = curved_arrow_points(
                    start_x - selection.x,
                    start_y - selection.y,
                    end_x - selection.x,
                    end_y - selection.y,
                    control_x - selection.x,
                    control_y - selection.y,
                    f64::from(stroke_width),
                );
                fill_rgba_polygon(pixels, selection.width, selection.height, &points, color);
            }
        }
    }
}

fn tapered_arrow_points(x1: i32, y1: i32, x2: i32, y2: i32, stroke_width: f64) -> Vec<(i32, i32)> {
    curved_arrow_points(x1, y1, x2, y2, (x1 + x2) / 2, (y1 + y2) / 2, stroke_width)
}

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
    for index in 0..=12 {
        let t = f64::from(index) / 12.0;
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
        || rect.x + rect.width > source_width
        || rect.y + rect.height > source_height
    {
        return Err("選取範圍超出 virtual desktop".into());
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
    use super::{curved_arrow_points, tapered_arrow_points, NativeAnnotation, SelectedArea};
    use crate::capture::RectInfo;
    use std::{mem::size_of, sync::mpsc};
    use windows::{
        core::w,
        Win32::{
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen,
                CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, GetStockObject,
                InvalidateRect, LineTo, MoveToEx, Polygon, Rectangle, RoundRect, SelectObject,
                SetBkMode, SetTextColor, StretchDIBits, TextOutW, BITMAPINFO, BITMAPINFOHEADER,
                BI_RGB, DIB_RGB_COLORS, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
                Input::KeyboardAndMouse::{
                    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_DOWN, VK_ESCAPE, VK_LEFT,
                    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_UP,
                },
                WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
                    GetWindow, GetWindowRect, IsIconic, IsWindowVisible, LoadCursorW, PostMessageW,
                    PostQuitMessage, RegisterClassW, SetCursor, SetForegroundWindow, ShowWindow,
                    TranslateMessage, CS_HREDRAW, CS_VREDRAW, GW_CHILD, GW_HWNDFIRST, GW_HWNDNEXT,
                    IDC_ARROW, IDC_CROSS, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
                    IDC_SIZEWE, MSG, SW_SHOW, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND,
                    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
                    WM_PAINT, WM_SETCURSOR, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
                },
            },
        },
    };

    static STATE: std::sync::Mutex<Option<SelectorState>> = std::sync::Mutex::new(None);

    struct SelectorState {
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
        hovered_annotation: Option<usize>,
        selected_annotation: Option<usize>,
        current_color: u32,
        sender: Option<mpsc::Sender<Option<SelectedArea>>>,
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

    #[derive(Clone, Copy)]
    enum AnnotationTool {
        Rectangle,
        Arrow,
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
    ) -> Result<Option<SelectedArea>, String> {
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
            origin_x: virtual_desktop.x,
            origin_y: virtual_desktop.y,
            width: virtual_desktop.width,
            height: virtual_desktop.height,
            bgra: rgba,
            dimmed_bgra,
            composite_bgra,
            composite_rect: None,
            interaction: None,
            selection: None,
            hover_candidate: None,
            cursor_position: (virtual_desktop.width / 2, virtual_desktop.height / 2),
            corner_radius: 0,
            annotation_tool: None,
            annotations: Vec::new(),
            hovered_annotation: None,
            selected_annotation: None,
            current_color: 0x0030_3BFF,
            sender: Some(sender),
        });

        let result = unsafe { run_window(virtual_desktop) };
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
            style: CS_HREDRAW | CS_VREDRAW,
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
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));

        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, None, 0, 0);
            if status.0 <= 0 {
                break;
            }
            TranslateMessage(&message);
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
            WM_LBUTTONDOWN => {
                let point = mouse_point(lparam);
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        let point = clamp_point(point, state.width, state.height);
                        if let Some(selection) = state.selection {
                            if let Some(button) =
                                hit_annotation_toolbar(point, selection, state.width, state.height)
                            {
                                match button {
                                    0 => state.annotation_tool = Some(AnnotationTool::Rectangle),
                                    1 => state.annotation_tool = Some(AnnotationTool::Arrow),
                                    2 => {
                                        const COLORS: [u32; 6] = [
                                            0x0030_3BFF,
                                            0x00FF_7B18,
                                            0x0060_CF34,
                                            0x0018_B8F5,
                                            0x00D0_5A8E,
                                            0x0025_2525,
                                        ];
                                        let current = COLORS
                                            .iter()
                                            .position(|color| *color == state.current_color)
                                            .unwrap_or(0);
                                        state.current_color = COLORS[(current + 1) % COLORS.len()];
                                        if let Some(index) = state.selected_annotation {
                                            set_native_color(
                                                &mut state.annotations[index],
                                                state.current_color,
                                            );
                                        }
                                    }
                                    3 => {
                                        state.annotations.pop();
                                    }
                                    4 => {
                                        let result = SelectedArea {
                                            selection,
                                            corner_radius: state.corner_radius,
                                            annotations: state.annotations.clone(),
                                        };
                                        if let Some(sender) = state.sender.take() {
                                            let _ = sender.send(Some(result));
                                        }
                                        let _ = PostMessageW(
                                            Some(hwnd),
                                            WM_CLOSE,
                                            WPARAM(0),
                                            LPARAM(0),
                                        );
                                    }
                                    5 => {
                                        if let Some(sender) = state.sender.take() {
                                            let _ = sender.send(None);
                                        }
                                        let _ = PostMessageW(
                                            Some(hwnd),
                                            WM_CLOSE,
                                            WPARAM(0),
                                            LPARAM(0),
                                        );
                                    }
                                    _ => {}
                                }
                                InvalidateRect(Some(hwnd), None, false);
                                return LRESULT(0);
                            }
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
                                state.selected_annotation = Some(index);
                                state.hovered_annotation = Some(index);
                                state.annotation_tool = Some(match state.annotations[index] {
                                    NativeAnnotation::Rectangle { .. } => AnnotationTool::Rectangle,
                                    NativeAnnotation::Arrow { .. } => AnnotationTool::Arrow,
                                });
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
                                    state.interaction =
                                        Some(Interaction::Annotate { start: point, tool });
                                    SetCapture(hwnd);
                                    return LRESULT(0);
                                }
                            }
                        }
                        if let Some((handle, original)) = state
                            .selection
                            .and_then(|rect| hit_handle(point, rect).map(|handle| (handle, rect)))
                        {
                            state.interaction = Some(Interaction::Resize {
                                handle,
                                start: point,
                                original,
                            });
                        } else if let Some(original) = state
                            .selection
                            .filter(|rect| hit_selection_edge(point, *rect))
                        {
                            state.interaction = Some(Interaction::MoveSelection {
                                start: point,
                                original,
                            });
                        } else if state.selection.is_none() {
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
                        let pointer = mouse_point(lparam);
                        state.cursor_position = (
                            pointer.0.clamp(0, state.width.saturating_sub(1)),
                            pointer.1.clamp(0, state.height.saturating_sub(1)),
                        );
                        if let Some(interaction) = state.interaction {
                            let current = if matches!(interaction, Interaction::Annotate { .. }) {
                                state
                                    .selection
                                    .map(|rect| clamp_to_rect(mouse_point(lparam), rect))
                                    .unwrap_or_else(|| {
                                        clamp_point(mouse_point(lparam), state.width, state.height)
                                    })
                            } else {
                                clamp_point(mouse_point(lparam), state.width, state.height)
                            };
                            if matches!(interaction, Interaction::Annotate { .. }) {
                                state.cursor_position = current;
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
                            } else {
                                state.hovered_annotation =
                                    hit_native_annotation(point, &state.annotations);
                            }
                        }
                        InvalidateRect(Some(hwnd), None, false);
                    }
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if let Some(interaction) = state.interaction.take() {
                            let current = if matches!(interaction, Interaction::Annotate { .. }) {
                                state
                                    .selection
                                    .map(|rect| clamp_to_rect(mouse_point(lparam), rect))
                                    .unwrap_or_else(|| {
                                        clamp_point(mouse_point(lparam), state.width, state.height)
                                    })
                            } else {
                                clamp_point(mouse_point(lparam), state.width, state.height)
                            };
                            if let Interaction::Annotate { start, tool } = interaction {
                                if (current.0 - start.0).abs() >= 3
                                    || (current.1 - start.1).abs() >= 3
                                {
                                    state.annotations.push(match tool {
                                        AnnotationTool::Rectangle => NativeAnnotation::Rectangle {
                                            start_x: start.0,
                                            start_y: start.1,
                                            end_x: current.0,
                                            end_y: current.1,
                                            stroke_width: 4,
                                            color: state.current_color,
                                        },
                                        AnnotationTool::Arrow => NativeAnnotation::Arrow {
                                            start_x: start.0,
                                            start_y: start.1,
                                            end_x: current.0,
                                            end_y: current.1,
                                            stroke_width: 4,
                                            control_x: (start.0 + current.0) / 2,
                                            control_y: (start.1 + current.1) / 2,
                                            color: state.current_color,
                                        },
                                    });
                                    state.selected_annotation = Some(state.annotations.len() - 1);
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
                            state.hover_candidate = None;
                        }
                    }
                }
                let _ = ReleaseCapture();
                InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                finish(None);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_RETURN.0 => {
                let selection = STATE.lock().ok().and_then(|guard| {
                    guard.as_ref().and_then(|state| {
                        state
                            .selection
                            .filter(|rect| rect.width >= 2 && rect.height >= 2)
                    })
                });
                if let Some(selection) = selection {
                    let corner_radius = STATE
                        .lock()
                        .ok()
                        .and_then(|guard| guard.as_ref().map(|state| state.corner_radius))
                        .unwrap_or(0);
                    finish(Some(SelectedArea {
                        selection,
                        corner_radius,
                        annotations: STATE
                            .lock()
                            .ok()
                            .and_then(|guard| guard.as_ref().map(|state| state.annotations.clone()))
                            .unwrap_or_default(),
                    }));
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
                if let Ok(mut guard) = STATE.lock() {
                    if let Some(state) = guard.as_mut() {
                        if let Some(index) = state.hovered_annotation {
                            let step = if delta > 0 { 1 } else { -1 };
                            set_native_stroke_width(&mut state.annotations[index], step);
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
                        if state.interaction.is_none() {
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
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_DESTROY => {
                finish(None);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn paint(hwnd: HWND) {
        let mut paint = PAINTSTRUCT::default();
        let screen_dc = BeginPaint(hwnd, &mut paint);
        if let Ok(mut guard) = STATE.lock() {
            if let Some(state) = guard.as_mut() {
                let dc = CreateCompatibleDC(Some(screen_dc));
                let bitmap = CreateCompatibleBitmap(screen_dc, state.width, state.height);
                let old_bitmap = SelectObject(dc, bitmap.into());
                let visible_rect = state
                    .selection
                    .or(state.hover_candidate)
                    .filter(|rect| rect.width > 0 && rect.height > 0);
                update_composite(state, visible_rect);
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: state.width,
                        biHeight: -state.height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                StretchDIBits(
                    dc,
                    0,
                    0,
                    state.width,
                    state.height,
                    0,
                    0,
                    state.width,
                    state.height,
                    Some(state.composite_bgra.as_ptr().cast()),
                    &info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );

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

                        if state.selection.is_some() {
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

                            draw_annotation_toolbar(dc, state, rect);
                        }

                        let dimensions = format!(
                            "{} × {} px · 圓角 {} px",
                            rect.width, rect.height, state.corner_radius
                        );
                        let dimensions: Vec<u16> = dimensions.encode_utf16().collect();
                        let label_y = if rect.y >= 34 {
                            rect.y - 28
                        } else {
                            (rect.y + rect.height + 10).min(state.height - 22)
                        };
                        SetTextColor(dc, COLORREF(0x00F0_FFFF));
                        TextOutW(dc, rect.x.max(8), label_y, &dimensions);
                    }
                }
                draw_native_annotations(dc, state);
                draw_magnifier(dc, state);
                let _ = BitBlt(
                    screen_dc,
                    0,
                    0,
                    state.width,
                    state.height,
                    Some(dc),
                    0,
                    0,
                    SRCCOPY,
                );
                SelectObject(dc, old_bitmap);
                DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
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

    fn clamp_point(point: (i32, i32), width: i32, height: i32) -> (i32, i32) {
        (point.0.clamp(0, width), point.1.clamp(0, height))
    }

    fn clamp_to_rect(point: (i32, i32), rect: RectInfo) -> (i32, i32) {
        (
            point.0.clamp(rect.x, rect.x + rect.width),
            point.1.clamp(rect.y, rect.y + rect.height),
        )
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

    fn annotation_toolbar_rect(rect: RectInfo, width: i32, height: i32) -> RECT {
        const TOOL_WIDTH: i32 = 292;
        const TOOL_HEIGHT: i32 = 44;
        const GAP: i32 = 12;
        let left = rect.x.clamp(0, (width - TOOL_WIDTH).max(0));
        let top = if rect.y + rect.height + GAP + TOOL_HEIGHT <= height {
            rect.y + rect.height + GAP
        } else if rect.y >= GAP + TOOL_HEIGHT {
            rect.y - GAP - TOOL_HEIGHT
        } else {
            (rect.y + rect.height - TOOL_HEIGHT).clamp(0, (height - TOOL_HEIGHT).max(0))
        };
        RECT {
            left,
            top,
            right: left + TOOL_WIDTH,
            bottom: top + TOOL_HEIGHT,
        }
    }

    fn hit_annotation_toolbar(
        point: (i32, i32),
        rect: RectInfo,
        width: i32,
        height: i32,
    ) -> Option<usize> {
        let toolbar = annotation_toolbar_rect(rect, width, height);
        if !(point.0 >= toolbar.left
            && point.0 < toolbar.right
            && point.1 >= toolbar.top
            && point.1 < toolbar.bottom)
        {
            return None;
        }
        if point.0 < toolbar.left + 28 {
            Some(5)
        } else {
            Some(((point.0 - toolbar.left - 28) / 44) as usize)
        }
    }

    unsafe fn draw_annotation_toolbar(
        dc: windows::Win32::Graphics::Gdi::HDC,
        state: &SelectorState,
        rect: RectInfo,
    ) {
        let toolbar = annotation_toolbar_rect(rect, state.width, state.height);
        let background = CreateSolidBrush(COLORREF(0x00FA_FAFA));
        let old_brush = SelectObject(dc, background.into());
        let border = CreatePen(PS_SOLID, 1, COLORREF(0x00D0_D0D0));
        let old_pen = SelectObject(dc, border.into());
        RoundRect(
            dc,
            toolbar.left,
            toolbar.top,
            toolbar.right,
            toolbar.bottom,
            10,
            10,
        );
        let dot_brush = CreateSolidBrush(COLORREF(0x00D0_D0D0));
        for row in 0..3 {
            for column in 0..2 {
                let dot = RECT {
                    left: toolbar.left + 9 + column * 7,
                    top: toolbar.top + 13 + row * 7,
                    right: toolbar.left + 12 + column * 7,
                    bottom: toolbar.top + 16 + row * 7,
                };
                FillRect(dc, &dot, dot_brush);
            }
        }
        DeleteObject(dot_brush.into());
        for index in 0..6 {
            let left = toolbar.left + 28 + index as i32 * 44;
            let selected = matches!(
                (index, state.annotation_tool),
                (0, Some(AnnotationTool::Rectangle)) | (1, Some(AnnotationTool::Arrow))
            );
            if selected {
                let brush = CreateSolidBrush(COLORREF(0x00FF_DC9B));
                let area = RECT {
                    left,
                    top: toolbar.top,
                    right: left + 44,
                    bottom: toolbar.bottom,
                };
                FillRect(dc, &area, brush);
                DeleteObject(brush.into());
            }
            if matches!(index, 2 | 3 | 5) {
                let separator = CreatePen(PS_SOLID, 1, COLORREF(0x00D5_D5D5));
                let previous_pen = SelectObject(dc, separator.into());
                MoveToEx(dc, left, toolbar.top + 9, None);
                let _ = LineTo(dc, left, toolbar.bottom - 9);
                SelectObject(dc, previous_pen);
                DeleteObject(separator.into());
            }
            draw_toolbar_icon(dc, index, left, toolbar.top, state.current_color);
        }
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        DeleteObject(border.into());
        DeleteObject(background.into());
    }

    unsafe fn draw_toolbar_icon(
        dc: windows::Win32::Graphics::Gdi::HDC,
        index: usize,
        left: i32,
        top: i32,
        current_color: u32,
    ) {
        let pen = CreatePen(PS_SOLID, 2, COLORREF(0x0025_2525));
        let old_pen = SelectObject(dc, pen.into());
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        match index {
            0 => {
                Rectangle(dc, left + 13, top + 12, left + 31, top + 31);
            }
            1 => {
                let points: Vec<POINT> =
                    tapered_arrow_points(left + 12, top + 31, left + 32, top + 11, 2.5)
                        .into_iter()
                        .map(|(x, y)| POINT { x, y })
                        .collect();
                let brush = CreateSolidBrush(COLORREF(0x0025_2525));
                let previous_brush = SelectObject(dc, brush.into());
                Polygon(dc, &points);
                SelectObject(dc, previous_brush);
                DeleteObject(brush.into());
            }
            2 => {
                let brush = CreateSolidBrush(COLORREF(current_color));
                let previous_brush = SelectObject(dc, brush.into());
                let swatch_pen = CreatePen(PS_SOLID, 1, COLORREF(0x0040_4040));
                let previous_pen = SelectObject(dc, swatch_pen.into());
                RoundRect(dc, left + 12, top + 12, left + 32, top + 32, 10, 10);
                SelectObject(dc, previous_pen);
                SelectObject(dc, previous_brush);
                DeleteObject(swatch_pen.into());
                DeleteObject(brush.into());
            }
            3 => {
                MoveToEx(dc, left + 30, top + 14, None);
                let _ = LineTo(dc, left + 20, top + 14);
                let _ = LineTo(dc, left + 14, top + 20);
                let _ = LineTo(dc, left + 20, top + 26);
                MoveToEx(dc, left + 15, top + 20, None);
                let _ = LineTo(dc, left + 30, top + 20);
                let _ = LineTo(dc, left + 33, top + 25);
            }
            4 => {
                MoveToEx(dc, left + 12, top + 22, None);
                let _ = LineTo(dc, left + 19, top + 29);
                let _ = LineTo(dc, left + 33, top + 14);
            }
            5 => {
                MoveToEx(dc, left + 14, top + 14, None);
                let _ = LineTo(dc, left + 30, top + 30);
                MoveToEx(dc, left + 30, top + 14, None);
                let _ = LineTo(dc, left + 14, top + 30);
            }
        }
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(pen.into());
    }

    unsafe fn draw_native_annotations(
        dc: windows::Win32::Graphics::Gdi::HDC,
        state: &SelectorState,
    ) {
        for annotation in &state.annotations {
            draw_native_annotation(dc, *annotation);
        }
        if let Some(Interaction::Annotate { start, tool }) = state.interaction {
            let (end_x, end_y) = state.cursor_position;
            draw_native_annotation(
                dc,
                match tool {
                    AnnotationTool::Rectangle => NativeAnnotation::Rectangle {
                        start_x: start.0,
                        start_y: start.1,
                        end_x,
                        end_y,
                        stroke_width: 4,
                        color: state.current_color,
                    },
                    AnnotationTool::Arrow => NativeAnnotation::Arrow {
                        start_x: start.0,
                        start_y: start.1,
                        end_x,
                        end_y,
                        stroke_width: 4,
                        control_x: (start.0 + end_x) / 2,
                        control_y: (start.1 + end_y) / 2,
                        color: state.current_color,
                    },
                },
            );
        }
        if let Some(index) = state.selected_annotation.or(state.hovered_annotation) {
            draw_native_object_handles(dc, state.annotations[index]);
        }
    }

    unsafe fn draw_native_object_handles(
        dc: windows::Win32::Graphics::Gdi::HDC,
        annotation: NativeAnnotation,
    ) {
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
    ) {
        match annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                color,
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
                let old_pen = SelectObject(dc, GetStockObject(NULL_BRUSH));
                Polygon(dc, &points);
                SelectObject(dc, old_pen);
                SelectObject(dc, old_brush);
                DeleteObject(brush.into());
            }
        }
    }

    fn native_annotation_bounds(annotation: NativeAnnotation) -> RectInfo {
        let (start_x, start_y, end_x, end_y) = match annotation {
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
            } => (start_x, start_y, end_x, end_y),
        };
        RectInfo {
            x: start_x.min(end_x),
            y: start_y.min(end_y),
            width: (end_x - start_x).abs(),
            height: (end_y - start_y).abs(),
        }
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
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                control_x,
                control_y,
                ..
            } => vec![(start_x, start_y), (control_x, control_y), (end_x, end_y)],
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
                color,
            } => match handle {
                AnnotationHandle::Start => NativeAnnotation::Rectangle {
                    start_x: point.0,
                    start_y: point.1,
                    end_x,
                    end_y,
                    stroke_width,
                    color,
                },
                AnnotationHandle::End | AnnotationHandle::Curve => NativeAnnotation::Rectangle {
                    start_x,
                    start_y,
                    end_x: point.0,
                    end_y: point.1,
                    stroke_width,
                    color,
                },
                AnnotationHandle::TopRight => NativeAnnotation::Rectangle {
                    start_x,
                    start_y: point.1,
                    end_x: point.0,
                    end_y,
                    stroke_width,
                    color,
                },
                AnnotationHandle::BottomLeft => NativeAnnotation::Rectangle {
                    start_x: point.0,
                    start_y,
                    end_x,
                    end_y: point.1,
                    stroke_width,
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
                        color,
                    }
                }
            },
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
        if let Some(rect) = state.selection {
            if let Some(handle) = hit_handle(point, rect) {
                return match handle {
                    Handle::Top | Handle::Bottom => IDC_SIZENS,
                    Handle::Left | Handle::Right => IDC_SIZEWE,
                    Handle::TopLeft | Handle::BottomRight => IDC_SIZENWSE,
                    Handle::TopRight | Handle::BottomLeft => IDC_SIZENESW,
                };
            }
            if hit_selection_edge(point, rect) {
                return IDC_SIZEALL;
            }
        }
        if state.annotation_tool.is_some() {
            IDC_CROSS
        } else {
            IDC_ARROW
        }
    }

    fn move_native_annotation(annotation: NativeAnnotation, dx: i32, dy: i32) -> NativeAnnotation {
        match annotation {
            NativeAnnotation::Rectangle {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                color,
            } => NativeAnnotation::Rectangle {
                start_x: start_x + dx,
                start_y: start_y + dy,
                end_x: end_x + dx,
                end_y: end_y + dy,
                stroke_width,
                color,
            },
            NativeAnnotation::Arrow {
                start_x,
                start_y,
                end_x,
                end_y,
                stroke_width,
                control_x,
                control_y,
                color,
            } => NativeAnnotation::Arrow {
                start_x: start_x + dx,
                start_y: start_y + dy,
                end_x: end_x + dx,
                end_y: end_y + dy,
                stroke_width,
                control_x: control_x + dx,
                control_y: control_y + dy,
                color,
            },
        }
    }

    fn set_native_stroke_width(annotation: &mut NativeAnnotation, step: i32) {
        let stroke_width = match annotation {
            NativeAnnotation::Rectangle { stroke_width, .. }
            | NativeAnnotation::Arrow { stroke_width, .. } => stroke_width,
        };
        *stroke_width = (*stroke_width + step).clamp(1, 32);
    }

    fn set_native_color(annotation: &mut NativeAnnotation, new_color: u32) {
        let color = match annotation {
            NativeAnnotation::Rectangle { color, .. } | NativeAnnotation::Arrow { color, .. } => {
                color
            }
        };
        *color = new_color;
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
                        control_x,
                        control_y,
                        ..
                    } => (0..=16).any(|index| {
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

    fn update_composite(state: &mut SelectorState, visible_rect: Option<RectInfo>) {
        let next_key = visible_rect.map(|rect| rect_key(rect, state.corner_radius));
        if state.composite_rect == next_key {
            return;
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
    ) -> Result<Option<super::SelectedArea>, String> {
        Err("PoC-B 目前只支援 Windows".into())
    }
}
