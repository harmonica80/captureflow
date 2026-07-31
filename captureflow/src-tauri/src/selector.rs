use crate::capture::{capture_frame, MonitorInfo, RectInfo};
use serde::Serialize;
use std::{fs, path::PathBuf, time::SystemTime};
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshot {
    pub image_path: String,
    pub metadata_path: String,
    pub selection: RectInfo,
    pub width: i32,
    pub height: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionMetadata<'a> {
    schema_version: u32,
    captured_at_unix_ms: u128,
    virtual_desktop: RectInfo,
    selection: RectInfo,
    monitors: &'a [MonitorInfo],
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

    let Some(local_selection) = selected else {
        return Ok(None);
    };
    let global_selection = RectInfo {
        x: virtual_desktop.x + local_selection.x,
        y: virtual_desktop.y + local_selection.y,
        width: local_selection.width,
        height: local_selection.height,
    };
    let cropped = crop_rgba(
        &frame.rgba,
        virtual_desktop.width,
        virtual_desktop.height,
        local_selection,
    )?;

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
        virtual_desktop,
        selection: global_selection,
        monitors: &frame.monitors,
    };
    fs::write(
        &metadata_path,
        serde_json::to_vec_pretty(&metadata)
            .map_err(|error| format!("無法建立選取 JSON：{error}"))?,
    )
    .map_err(|error| format!("無法儲存選取 JSON：{error}"))?;

    Ok(Some(SelectionSnapshot {
        image_path: path_string(image_path),
        metadata_path: path_string(metadata_path),
        selection: global_selection,
        width: local_selection.width,
        height: local_selection.height,
    }))
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
    use crate::capture::RectInfo;
    use std::{mem::size_of, sync::mpsc};
    use windows::{
        core::w,
        Win32::{
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, GetStockObject,
                IntersectClipRect, InvalidateRect, Rectangle, RestoreDC, SaveDC, SelectObject,
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
                    GetWindow, GetWindowRect, IsIconic, IsWindowVisible, LoadCursorW,
                    PostQuitMessage, RegisterClassW, SetForegroundWindow, ShowWindow,
                    TranslateMessage, CS_HREDRAW, CS_VREDRAW, GW_HWNDFIRST, GW_HWNDNEXT, IDC_CROSS,
                    MSG, SW_SHOW, WINDOW_EX_STYLE, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN,
                    WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WNDCLASSW, WS_EX_TOOLWINDOW,
                    WS_EX_TOPMOST, WS_POPUP,
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
        interaction: Option<Interaction>,
        selection: Option<RectInfo>,
        hover_candidate: Option<RectInfo>,
        sender: Option<mpsc::Sender<Option<RectInfo>>>,
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

    pub fn run_selector(
        virtual_desktop: RectInfo,
        mut rgba: Vec<u8>,
    ) -> Result<Option<RectInfo>, String> {
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
        *STATE.lock().map_err(|_| "選取狀態鎖定失敗")? = Some(SelectorState {
            origin_x: virtual_desktop.x,
            origin_y: virtual_desktop.y,
            width: virtual_desktop.width,
            height: virtual_desktop.height,
            bgra: rgba,
            dimmed_bgra,
            interaction: None,
            selection: None,
            hover_candidate: None,
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
                        if let Some((handle, original)) = state
                            .selection
                            .and_then(|rect| hit_handle(point, rect).map(|handle| (handle, rect)))
                        {
                            state.interaction = Some(Interaction::Resize {
                                handle,
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
                        if let Some(interaction) = state.interaction {
                            let current =
                                clamp_point(mouse_point(lparam), state.width, state.height);
                            if let Interaction::PendingWindow { start, .. } = interaction {
                                if (current.0 - start.0).abs() >= 4
                                    || (current.1 - start.1).abs() >= 4
                                {
                                    state.interaction = Some(Interaction::Create { start });
                                    state.hover_candidate = None;
                                    state.selection = Some(normalize_rect(start, current));
                                }
                            } else {
                                state.selection = Some(update_selection(
                                    interaction,
                                    current,
                                    state.width,
                                    state.height,
                                ));
                            }
                        } else if state.selection.is_none() {
                            let point = clamp_point(mouse_point(lparam), state.width, state.height);
                            state.hover_candidate = detect_window_at(hwnd, point, state);
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
                            let current =
                                clamp_point(mouse_point(lparam), state.width, state.height);
                            state.selection = Some(update_selection(
                                interaction,
                                current,
                                state.width,
                                state.height,
                            ));
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
                if selection.is_some() {
                    finish(selection);
                    let _ = DestroyWindow(hwnd);
                }
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
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
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
        let dc = BeginPaint(hwnd, &mut paint);
        if let Ok(guard) = STATE.lock() {
            if let Some(state) = guard.as_ref() {
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
                    Some(state.dimmed_bgra.as_ptr().cast()),
                    &info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );

                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(0x00F0_FFFF));
                let instructions: Vec<u16> =
                    "移動游標偵測視窗 · 單擊選取 · 拖曳自由框選 · 方向鍵微調 · Enter 確認"
                        .encode_utf16()
                        .collect();
                TextOutW(dc, 24, 24, &instructions);

                if let Some(rect) = state.selection.or(state.hover_candidate) {
                    if rect.width > 0 && rect.height > 0 {
                        // Keep the frozen desktop at a fixed 1:1 coordinate mapping. Only the
                        // clipping rectangle changes while creating or resizing the selection.
                        // Drawing the selection as a source/destination sub-rectangle makes GDI
                        // reinterpret its source origin and causes the visible content to slide.
                        let saved_dc = SaveDC(dc);
                        IntersectClipRect(
                            dc,
                            rect.x,
                            rect.y,
                            rect.x + rect.width,
                            rect.y + rect.height,
                        );
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
                            Some(state.bgra.as_ptr().cast()),
                            &info,
                            DIB_RGB_COLORS,
                            SRCCOPY,
                        );
                        RestoreDC(dc, saved_dc);
                        let pen = CreatePen(PS_SOLID, 3, COLORREF(0x0074_E7AD));
                        let old_pen = SelectObject(dc, pen.into());
                        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                        Rectangle(
                            dc,
                            rect.x,
                            rect.y,
                            rect.x + rect.width,
                            rect.y + rect.height,
                        );
                        SelectObject(dc, old_brush);
                        SelectObject(dc, old_pen);
                        DeleteObject(pen.into());

                        if state.selection.is_some() {
                            let handle_brush = CreateSolidBrush(COLORREF(0x0074_E7AD));
                            let old_pen = SelectObject(dc, GetStockObject(NULL_BRUSH));
                            let old_brush = SelectObject(dc, handle_brush.into());
                            for (x, y) in handle_points(rect) {
                                Rectangle(dc, x - 5, y - 5, x + 6, y + 6);
                            }
                            SelectObject(dc, old_brush);
                            SelectObject(dc, old_pen);
                            DeleteObject(handle_brush.into());
                        }

                        let dimensions = format!("{} × {}", rect.width, rect.height);
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
            }
        }
        EndPaint(hwnd, &paint);
    }

    fn finish(result: Option<RectInfo>) {
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
                let mut rect = RECT::default();
                if GetWindowRect(current, &mut rect).is_ok()
                    && rect.right > rect.left
                    && rect.bottom > rect.top
                    && screen_x >= rect.left
                    && screen_x < rect.right
                    && screen_y >= rect.top
                    && screen_y < rect.bottom
                {
                    let left = (rect.left - state.origin_x).clamp(0, state.width);
                    let top = (rect.top - state.origin_y).clamp(0, state.height);
                    let right = (rect.right - state.origin_x).clamp(0, state.width);
                    let bottom = (rect.bottom - state.origin_y).clamp(0, state.height);
                    if right - left >= 2 && bottom - top >= 2 {
                        return Some(RectInfo {
                            x: left,
                            y: top,
                            width: right - left,
                            height: bottom - top,
                        });
                    }
                }
            }
            current = match GetWindow(current, GW_HWNDNEXT) {
                Ok(next) => next,
                Err(_) => break,
            };
        }
        None
    }
}

#[cfg(not(windows))]
mod platform {
    use crate::capture::RectInfo;

    pub fn run_selector(
        _virtual_desktop: RectInfo,
        _rgba: Vec<u8>,
    ) -> Result<Option<RectInfo>, String> {
        Err("PoC-B 目前只支援 Windows".into())
    }
}
