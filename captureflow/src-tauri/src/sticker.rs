use crate::sticker_store::{self, StickerRecord};
use std::path::Path;

pub fn open(app: &tauri::AppHandle, image_path: String, x: i32, y: i32) -> Result<(), String> {
    let image = image::open(Path::new(&image_path))
        .map_err(|error| format!("無法讀取貼圖圖片：{error}"))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("貼圖圖片尺寸無效".into());
    }
    let mut bgra = image.into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let corner_radius = infer_corner_radius(&bgra, width as i32, height as i32);
    let persistence_path = sticker_store::path(app)?;
    let record = StickerRecord {
        id: sticker_store::next_id(),
        image_path,
        x,
        y,
        width: width as i32,
        height: height as i32,
        opacity: 255,
        locked: false,
        click_through: false,
    };
    sticker_store::upsert(&persistence_path, &record);

    std::thread::Builder::new()
        .name("captureflow-sticker".into())
        .spawn(move || {
            platform::run(
                record,
                persistence_path,
                width as i32,
                height as i32,
                corner_radius,
                bgra,
            )
        })
        .map_err(|error| format!("無法啟動貼圖視窗：{error}"))?;
    Ok(())
}

pub fn restore(app: &tauri::AppHandle) {
    let Ok(persistence_path) = sticker_store::path(app) else {
        return;
    };
    for record in sticker_store::load(&persistence_path) {
        let Ok(image) = image::open(Path::new(&record.image_path)) else {
            sticker_store::remove(&persistence_path, record.id);
            continue;
        };
        let image = image.into_rgba8();
        let (width, height) = image.dimensions();
        let mut bgra = image.into_raw();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let corner_radius = infer_corner_radius(&bgra, width as i32, height as i32);
        let persistence_path = persistence_path.clone();
        let _ = std::thread::Builder::new()
            .name("captureflow-sticker-restore".into())
            .spawn(move || {
                platform::run(
                    record,
                    persistence_path,
                    width as i32,
                    height as i32,
                    corner_radius,
                    bgra,
                )
            });
    }
}

fn infer_corner_radius(bgra: &[u8], width: i32, height: i32) -> i32 {
    if width < 2 || height < 2 {
        return 0;
    }
    let top = (0..width)
        .take_while(|x| bgra[(*x as usize) * 4 + 3] < 128)
        .count() as i32;
    let left = (0..height)
        .take_while(|y| bgra[(*y as usize * width as usize) * 4 + 3] < 128)
        .count() as i32;
    top.max(left).clamp(0, width.min(height) / 2)
}

#[cfg(windows)]
mod platform {
    use super::{sticker_store, StickerRecord};
    use std::{cell::RefCell, mem::size_of, path::PathBuf};
    use windows::{
        core::w,
        Win32::{
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::{
                Direct2D::{
                    Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
                    D2D1CreateFactory, ID2D1Factory, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
                    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
                    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
                },
                Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                Gdi::{
                    BeginPaint, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
                    EndPaint, FillRect, GetStockObject, InvalidateRect, LineTo, MoveToEx,
                    Rectangle, RedrawWindow, SelectObject, SetBkMode, SetBrushOrgEx,
                    SetStretchBltMode, SetTextColor, SetWindowRgn, StretchDIBits, TextOutW,
                    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DEFAULT_GUI_FONT, DIB_RGB_COLORS,
                    HALFTONE, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, RDW_INVALIDATE, RDW_NOERASE,
                    RDW_UPDATENOW, SRCCOPY, TRANSPARENT,
                },
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
                Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE},
                WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                    GetClientRect, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW,
                    GetWindowRect, KillTimer, PostQuitMessage, RegisterClassW, SetForegroundWindow,
                    SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos,
                    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWL_EXSTYLE, HTCAPTION,
                    HTCLIENT, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA, MSG, SM_CXVIRTUALSCREEN,
                    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_DEFERERASE,
                    SWP_NOACTIVATE, SWP_NOCOPYBITS, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WM_DESTROY,
                    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEWHEEL, WM_MOVE, WM_NCHITTEST,
                    WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
                    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
                },
            },
        },
    };
    use windows_numerics::Vector2;

    thread_local! {
        static STATE: RefCell<Option<StickerState>> = const { RefCell::new(None) };
    }

    struct StickerState {
        record_id: u64,
        image_path: String,
        persistence_path: PathBuf,
        source_width: i32,
        source_height: i32,
        corner_radius: i32,
        bgra: Vec<u8>,
        opacity: u8,
        locked: bool,
        click_through: bool,
        scale: f64,
        scale_percent: i32,
        pending_wheel_delta: i32,
        pending_wheel_cursor: (i32, i32),
        resize_scheduled: bool,
        sticker_hwnd: HWND,
        toolbar_hwnd: HWND,
        toolbar_visible: bool,
    }

    pub fn run(
        record: StickerRecord,
        persistence_path: PathBuf,
        source_width: i32,
        source_height: i32,
        corner_radius: i32,
        bgra: Vec<u8>,
    ) {
        let scale_percent =
            (record.width as f64 / source_width.max(1) as f64 * 100.0).round() as i32;
        STATE.with(|state| {
            *state.borrow_mut() = Some(StickerState {
                record_id: record.id,
                image_path: record.image_path.clone(),
                persistence_path,
                source_width,
                source_height,
                corner_radius,
                bgra,
                opacity: record.opacity,
                locked: record.locked,
                click_through: record.click_through,
                scale: record.width as f64 / source_width.max(1) as f64,
                scale_percent,
                pending_wheel_delta: 0,
                pending_wheel_cursor: (record.x, record.y),
                resize_scheduled: false,
                sticker_hwnd: HWND::default(),
                toolbar_hwnd: HWND::default(),
                toolbar_visible: false,
            });
        });
        let _ = unsafe { run_window(record.x, record.y, record.width, record.height) };
        STATE.with(|state| *state.borrow_mut() = None);
    }

    unsafe fn run_window(mut x: i32, mut y: i32, width: i32, height: i32) -> Result<(), String> {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let virtual_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let virtual_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let virtual_right = virtual_left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let virtual_bottom = virtual_top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
        x = x.clamp(
            virtual_left,
            (virtual_right - width.min(80)).max(virtual_left),
        );
        y = y.clamp(
            virtual_top,
            (virtual_bottom - height.min(60)).max(virtual_top),
        );
        let module =
            GetModuleHandleW(None).map_err(|error| format!("GetModuleHandleW：{error}"))?;
        let instance = HINSTANCE(module.0);
        let class_name = w!("CaptureFlowStickerWindow");
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&window_class);

        let toolbar_class_name = w!("CaptureFlowStickerToolbarWindow");
        let toolbar_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(toolbar_proc),
            hInstance: instance,
            lpszClassName: toolbar_class_name,
            ..Default::default()
        };
        RegisterClassW(&toolbar_class);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_LAYERED.0),
            class_name,
            w!("CaptureFlow Sticker"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|error| format!("無法建立貼圖視窗：{error}"))?;
        let (initial_opacity, initial_click_through) = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|state| (state.opacity, state.click_through))
                .unwrap_or((255, false))
        });
        SetLayeredWindowAttributes(hwnd, COLORREF(0), initial_opacity, LWA_ALPHA)
            .map_err(|error| format!("無法設定貼圖透明度：{error}"))?;
        let toolbar_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0),
            toolbar_class_name,
            w!("CaptureFlow Sticker Toolbar"),
            WS_POPUP,
            x,
            y,
            380,
            40,
            Some(hwnd),
            None,
            Some(instance),
            None,
        )
        .map_err(|error| format!("無法建立貼圖工具列：{error}"))?;
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.sticker_hwnd = hwnd;
                state.toolbar_hwnd = toolbar_hwnd;
            }
        });
        if initial_click_through {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_TRANSPARENT.0 as isize);
        }
        persist_current(hwnd);
        apply_sticker_region(hwnd);
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(toolbar_hwnd, SW_HIDE);
        position_toolbar(hwnd);
        SetTimer(Some(hwnd), 1, 80, None);
        SetForegroundWindow(hwnd);

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
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_NCHITTEST => {
                let (locked, click_through) = STATE.with(|state| {
                    state
                        .borrow()
                        .as_ref()
                        .map(|state| (state.locked, state.click_through))
                        .unwrap_or((false, false))
                });
                LRESULT(if click_through {
                    HTTRANSPARENT as isize
                } else if locked {
                    HTCLIENT as isize
                } else {
                    HTCAPTION as isize
                })
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
                if GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    adjust_opacity(hwnd, delta.signum() * 16);
                } else {
                    queue_resize(hwnd, delta, screen_point(lparam));
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                close_sticker(hwnd);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == b'L' as u16 => {
                toggle_lock(hwnd);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == b'P' as u16 => {
                toggle_click_through(hwnd);
                LRESULT(0)
            }
            WM_MOVE | WM_SIZE => {
                position_toolbar(hwnd);
                apply_sticker_region(hwnd);
                persist_current(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == 1 => {
                update_toolbar_visibility(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == 2 => {
                apply_queued_resize(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                KillTimer(Some(hwnd), 1);
                KillTimer(Some(hwnd), 2);
                STATE.with(|state| {
                    if let Some(state) = state.borrow().as_ref() {
                        if !state.toolbar_hwnd.is_invalid() {
                            let _ = DestroyWindow(state.toolbar_hwnd);
                        }
                    }
                });
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe extern "system" fn toolbar_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_PAINT => {
                paint_toolbar(hwnd);
                LRESULT(0)
            }
            WM_NCHITTEST => LRESULT(HTCLIENT as isize),
            WM_LBUTTONUP => {
                let sticker = sticker_hwnd();
                if !sticker.is_invalid() {
                    toolbar_click(sticker, mouse_point(lparam));
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let sticker = sticker_hwnd();
                if !sticker.is_invalid() {
                    let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
                    if GetKeyState(VK_CONTROL.0 as i32) < 0 {
                        adjust_opacity(sticker, delta.signum() * 16);
                    } else {
                        queue_resize(sticker, delta, screen_point(lparam));
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                let sticker = sticker_hwnd();
                if !sticker.is_invalid() {
                    close_sticker(sticker);
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == b'L' as u16 => {
                let sticker = sticker_hwnd();
                if !sticker.is_invalid() {
                    toggle_lock(sticker);
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == b'P' as u16 => {
                let sticker = sticker_hwnd();
                if !sticker.is_invalid() {
                    toggle_click_through(sticker);
                }
                LRESULT(0)
            }
            WM_DESTROY => LRESULT(0),
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn paint(hwnd: HWND) {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        STATE.with(|state| {
            if let Some(state) = state.borrow().as_ref() {
                let mut client = RECT::default();
                if GetClientRect(hwnd, &mut client).is_ok() {
                    let info = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: state.source_width,
                            biHeight: -state.source_height,
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    // HALFTONE provides substantially better down-sampling than the
                    // default COLORONCOLOR mode when a pinned image is made smaller.
                    SetStretchBltMode(dc, HALFTONE);
                    SetBrushOrgEx(dc, 0, 0, None);
                    StretchDIBits(
                        dc,
                        0,
                        0,
                        client.right,
                        client.bottom,
                        0,
                        0,
                        state.source_width,
                        state.source_height,
                        Some(state.bgra.as_ptr().cast()),
                        &info,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                    let border_pen = CreatePen(PS_SOLID, 2, COLORREF(0x0074_E7AD));
                    let old_pen = SelectObject(dc, border_pen.into());
                    let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                    Rectangle(dc, 0, 0, client.right, client.bottom);
                    SelectObject(dc, old_brush);
                    SelectObject(dc, old_pen);
                    DeleteObject(border_pen.into());
                }
            }
        });
        EndPaint(hwnd, &paint);
    }

    unsafe fn apply_sticker_region(hwnd: HWND) {
        let mut client = RECT::default();
        if GetClientRect(hwnd, &mut client).is_err() {
            return;
        }
        let radius = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|state| {
                    (state.corner_radius as f64 * client.right as f64
                        / state.source_width.max(1) as f64)
                        .round() as i32
                })
                .unwrap_or(0)
        });
        if radius <= 0 {
            return;
        }
        let region = CreateRoundRectRgn(
            0,
            0,
            client.right + 1,
            client.bottom + 1,
            radius * 2,
            radius * 2,
        );
        if region.is_invalid() {
            return;
        }
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            DeleteObject(region.into());
        }
    }

    unsafe fn paint_toolbar(hwnd: HWND) {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        let mut client = RECT::default();
        if GetClientRect(hwnd, &mut client).is_ok() {
            STATE.with(|state| {
                if let Some(state) = state.borrow().as_ref() {
                    draw_toolbar(dc, client.right, state);
                }
            });
        }
        EndPaint(hwnd, &paint);
    }

    unsafe fn resize(hwnd: HWND, wheel_delta: i32, cursor: (i32, i32)) {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let current_width = rect.right - rect.left;
        let current_height = rect.bottom - rect.top;
        let (new_width, new_height) = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return (current_width, current_height);
            };
            let maximum_scale = (2400.0 / state.source_width as f64)
                .min(1600.0 / state.source_height as f64)
                .max(0.05);
            let minimum_scale = (80.0 / state.source_width as f64)
                .max(60.0 / state.source_height as f64)
                .min(maximum_scale);
            // A standard wheel notch is 120. Use roughly 4% per notch and retain
            // the floating-point scale so high-resolution wheels accumulate smoothly.
            let factor = 2_f64.powf(wheel_delta as f64 / 2160.0);
            state.scale = (state.scale * factor).clamp(minimum_scale, maximum_scale);
            state.scale_percent = (state.scale * 100.0).round() as i32;
            (
                (state.source_width as f64 * state.scale).round() as i32,
                (state.source_height as f64 * state.scale).round() as i32,
            )
        });
        if new_width == current_width && new_height == current_height {
            return;
        }
        let anchor_x = (cursor.0 - rect.left) as f64 / current_width.max(1) as f64;
        let anchor_y = (cursor.1 - rect.top) as f64 / current_height.max(1) as f64;
        let new_x = (cursor.0 as f64 - anchor_x * new_width as f64).round() as i32;
        let new_y = (cursor.1 as f64 - anchor_y * new_height as f64).round() as i32;
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            new_x,
            new_y,
            new_width,
            new_height,
            SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_DEFERERASE,
        );
        RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_NOERASE | RDW_UPDATENOW,
        );
        let toolbar = toolbar_hwnd();
        if !toolbar.is_invalid() {
            InvalidateRect(Some(toolbar), None, false);
        }
    }

    unsafe fn queue_resize(hwnd: HWND, wheel_delta: i32, cursor: (i32, i32)) {
        let schedule = STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.pending_wheel_delta = state
                    .pending_wheel_delta
                    .saturating_add(wheel_delta)
                    .clamp(-960, 960);
                state.pending_wheel_cursor = cursor;
                if !state.resize_scheduled {
                    state.resize_scheduled = true;
                    return true;
                }
            }
            false
        });
        // Coalesce wheel bursts so the native window is resized at most once per
        // compositor frame. The accumulated delta preserves the requested scale.
        if schedule {
            SetTimer(Some(hwnd), 2, 16, None);
        }
    }

    unsafe fn apply_queued_resize(hwnd: HWND) {
        KillTimer(Some(hwnd), 2);
        let (delta, cursor) = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return (0, (0, 0));
            };
            let pending = state.pending_wheel_delta;
            state.pending_wheel_delta = 0;
            state.resize_scheduled = false;
            (pending, state.pending_wheel_cursor)
        });
        if delta != 0 {
            resize(hwnd, delta, cursor);
        }
    }

    unsafe fn adjust_opacity(hwnd: HWND, delta: i32) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.opacity = (state.opacity as i32 + delta).clamp(48, 255) as u8;
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), state.opacity, LWA_ALPHA);
            }
        });
        invalidate_sticker_and_toolbar(hwnd);
        persist_current(hwnd);
    }

    unsafe fn toolbar_click(hwnd: HWND, point: (i32, i32)) {
        if point.1 >= 40 {
            return;
        }
        let mut client = RECT::default();
        let toolbar = toolbar_hwnd();
        if toolbar.is_invalid() || GetClientRect(toolbar, &mut client).is_err() {
            return;
        }
        if point.0 >= client.right - 48 {
            close_sticker(hwnd);
        } else if point.0 < 52 {
            toggle_lock(hwnd);
        } else if point.0 < 104 {
            adjust_opacity(hwnd, -16);
        } else if point.0 < 156 {
            adjust_opacity(hwnd, 16);
        } else if point.0 < 208 {
            toggle_click_through(hwnd);
        }
    }

    unsafe fn toggle_lock(hwnd: HWND) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.locked = !state.locked;
            }
        });
        invalidate_sticker_and_toolbar(hwnd);
        persist_current(hwnd);
    }

    unsafe fn toggle_click_through(hwnd: HWND) {
        let enabled = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return false;
            };
            state.click_through = !state.click_through;
            state.click_through
        });
        let current_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let transparent = WS_EX_TRANSPARENT.0 as isize;
        let next_style = if enabled {
            current_style | transparent
        } else {
            current_style & !transparent
        };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next_style);
        invalidate_sticker_and_toolbar(hwnd);
        persist_current(hwnd);
    }

    unsafe fn persist_current(hwnd: HWND) {
        if hwnd.is_invalid() {
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        STATE.with(|state| {
            let state = state.borrow();
            let Some(state) = state.as_ref() else {
                return;
            };
            sticker_store::upsert(
                &state.persistence_path,
                &StickerRecord {
                    id: state.record_id,
                    image_path: state.image_path.clone(),
                    x: rect.left,
                    y: rect.top,
                    width: rect.right - rect.left,
                    height: rect.bottom - rect.top,
                    opacity: state.opacity,
                    locked: state.locked,
                    click_through: state.click_through,
                },
            );
        });
    }

    unsafe fn close_sticker(hwnd: HWND) {
        STATE.with(|state| {
            if let Some(state) = state.borrow().as_ref() {
                sticker_store::remove(&state.persistence_path, state.record_id);
            }
        });
        let _ = DestroyWindow(hwnd);
    }

    unsafe fn position_toolbar(sticker: HWND) {
        let toolbar = toolbar_hwnd();
        if sticker.is_invalid() || toolbar.is_invalid() {
            return;
        }
        let mut sticker_rect = RECT::default();
        if GetWindowRect(sticker, &mut sticker_rect).is_err() {
            return;
        }
        let virtual_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let virtual_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let virtual_width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let virtual_height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let virtual_right = virtual_left + virtual_width;
        let virtual_bottom = virtual_top + virtual_height;
        let sticker_width = sticker_rect.right - sticker_rect.left;
        let toolbar_width = 380.min(virtual_width);
        let toolbar_x = (sticker_rect.left + (sticker_width - toolbar_width) / 2)
            .clamp(virtual_left, virtual_right - toolbar_width);
        let toolbar_y = if sticker_rect.top - 44 >= virtual_top {
            sticker_rect.top - 44
        } else if sticker_rect.bottom + 44 <= virtual_bottom {
            sticker_rect.bottom + 4
        } else {
            sticker_rect.top
        };
        let _ = SetWindowPos(
            toolbar,
            Some(HWND_TOPMOST),
            toolbar_x,
            toolbar_y,
            toolbar_width,
            40,
            SWP_NOACTIVATE,
        );
    }

    unsafe fn update_toolbar_visibility(sticker: HWND) {
        let toolbar = toolbar_hwnd();
        if sticker.is_invalid() || toolbar.is_invalid() {
            return;
        }
        let mut cursor = Default::default();
        let mut sticker_rect = RECT::default();
        let mut toolbar_rect = RECT::default();
        if GetCursorPos(&mut cursor).is_err()
            || GetWindowRect(sticker, &mut sticker_rect).is_err()
            || GetWindowRect(toolbar, &mut toolbar_rect).is_err()
        {
            return;
        }
        let over_sticker = cursor.x >= sticker_rect.left
            && cursor.x < sticker_rect.right
            && cursor.y >= sticker_rect.top
            && cursor.y < sticker_rect.bottom;
        // Bridge the narrow gap between the image and toolbar so it does not flash
        // while the pointer moves from one window to the other.
        let over_toolbar = cursor.x >= toolbar_rect.left - 6
            && cursor.x < toolbar_rect.right + 6
            && cursor.y >= toolbar_rect.top - 6
            && cursor.y < toolbar_rect.bottom + 6;
        let should_show = over_sticker || over_toolbar;
        let changed = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return false;
            };
            if state.toolbar_visible == should_show {
                false
            } else {
                state.toolbar_visible = should_show;
                true
            }
        });
        if changed {
            ShowWindow(toolbar, if should_show { SW_SHOW } else { SW_HIDE });
        }
    }

    unsafe fn invalidate_sticker_and_toolbar(sticker: HWND) {
        InvalidateRect(Some(sticker), None, false);
        let toolbar = toolbar_hwnd();
        if !toolbar.is_invalid() {
            InvalidateRect(Some(toolbar), None, false);
        }
    }

    fn sticker_hwnd() -> HWND {
        STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|state| state.sticker_hwnd)
                .unwrap_or_default()
        })
    }

    fn toolbar_hwnd() -> HWND {
        STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|state| state.toolbar_hwnd)
                .unwrap_or_default()
        })
    }

    unsafe fn draw_toolbar(
        dc: windows::Win32::Graphics::Gdi::HDC,
        width: i32,
        state: &StickerState,
    ) {
        let toolbar_brush = CreateSolidBrush(COLORREF(0x00F7_F6F5));
        let toolbar_rect = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 40,
        };
        FillRect(dc, &toolbar_rect, toolbar_brush);
        DeleteObject(toolbar_brush.into());

        let border_pen = CreatePen(PS_SOLID, 2, COLORREF(0x00E8_8830));
        let old_pen = SelectObject(dc, border_pen.into());
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Rectangle(dc, 1, 1, width - 1, 39);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(border_pen.into());

        draw_vector_icons(dc, width, state.locked, state.click_through);

        let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x0031_2A24));
        let status = format!(
            "{}%   α{}%",
            state.scale_percent,
            (state.opacity as f64 / 255.0 * 100.0).round() as i32
        );
        if state.click_through {
            draw_text(dc, 218, 11, "穿透中");
        } else {
            draw_text(dc, 218, 11, &status);
        }
        SelectObject(dc, old_font);

        let separator_pen = CreatePen(PS_SOLID, 1, COLORREF(0x00D0_CBC6));
        let old_pen = SelectObject(dc, separator_pen.into());
        for x in [53, 105, 157, 209, width - 49] {
            MoveToEx(dc, x, 8, None);
            LineTo(dc, x, 32);
        }
        SelectObject(dc, old_pen);
        DeleteObject(separator_pen.into());
    }

    unsafe fn draw_vector_icons(
        dc: windows::Win32::Graphics::Gdi::HDC,
        width: i32,
        locked: bool,
        click_through: bool,
    ) {
        let factory: ID2D1Factory = match D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
        {
            Ok(factory) => factory,
            Err(_) => return,
        };
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let target = match factory.CreateDCRenderTarget(&properties) {
            Ok(target) => target,
            Err(_) => return,
        };
        let bounds = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 40,
        };
        if target.BindDC(dc, &bounds).is_err() {
            return;
        }
        let brush = match target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 36.0 / 255.0,
                g: 42.0 / 255.0,
                b: 49.0 / 255.0,
                a: 1.0,
            },
            None,
        ) {
            Ok(brush) => brush,
            Err(_) => return,
        };
        target.BeginDraw();
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        let line = |x1: f32, y1: f32, x2: f32, y2: f32| {
            target.DrawLine(
                Vector2 { X: x1, Y: y1 },
                Vector2 { X: x2, Y: y2 },
                &brush,
                1.6,
                None,
            );
        };

        if locked {
            target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                        left: 18.0,
                        top: 19.0,
                        right: 34.0,
                        bottom: 30.0,
                    },
                    radiusX: 1.5,
                    radiusY: 1.5,
                },
                &brush,
                1.6,
                None,
            );
            target.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2 { X: 26.0, Y: 16.0 },
                    radiusX: 5.5,
                    radiusY: 6.5,
                },
                &brush,
                1.6,
                None,
            );
        } else {
            for (x1, y1, x2, y2) in [
                (17., 20., 35., 20.),
                (17., 20., 21., 16.),
                (17., 20., 21., 24.),
                (35., 20., 31., 16.),
                (35., 20., 31., 24.),
                (26., 11., 26., 29.),
                (26., 11., 22., 15.),
                (26., 11., 30., 15.),
                (26., 29., 22., 25.),
                (26., 29., 30., 25.),
            ] {
                line(x1, y1, x2, y2);
            }
        }

        for (x, plus) in [(78.0, false), (130.0, true)] {
            let drop = [
                (-7., 1.),
                (0., -9.),
                (7., 1.),
                (6., 7.),
                (2., 10.),
                (-2., 10.),
                (-6., 7.),
                (-7., 1.),
            ];
            for pair in drop.windows(2) {
                line(
                    x + pair[0].0,
                    20. + pair[0].1,
                    x + pair[1].0,
                    20. + pair[1].1,
                );
            }
            line(x + 9., 26., x + 17., 26.);
            if plus {
                line(x + 13., 22., x + 13., 30.);
            }
        }

        // Mouse pointer: toggles click-through while the separate toolbar stays usable.
        for (x1, y1, x2, y2) in [
            (169., 11., 169., 29.),
            (169., 11., 183., 22.),
            (183., 22., 176., 23.),
            (176., 23., 180., 29.),
            (176., 23., 169., 29.),
        ] {
            line(x1, y1, x2, y2);
        }
        if click_through {
            line(165., 12., 185., 28.);
            line(185., 12., 165., 28.);
        }
        let close_x = (width - 25) as f32;
        line(close_x - 7., 13., close_x + 7., 27.);
        line(close_x + 7., 13., close_x - 7., 27.);
        let _ = target.EndDraw(None, None);
    }

    unsafe fn draw_text(dc: windows::Win32::Graphics::Gdi::HDC, x: i32, y: i32, text: &str) {
        let text: Vec<u16> = text.encode_utf16().collect();
        TextOutW(dc, x, y, &text);
    }

    fn screen_point(lparam: LPARAM) -> (i32, i32) {
        (
            (lparam.0 as u16 as i16) as i32,
            ((lparam.0 >> 16) as u16 as i16) as i32,
        )
    }

    fn mouse_point(lparam: LPARAM) -> (i32, i32) {
        screen_point(lparam)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::StickerRecord;
    use std::path::PathBuf;
    pub fn run(
        _record: StickerRecord,
        _path: PathBuf,
        _source_width: i32,
        _source_height: i32,
        _corner_radius: i32,
        _bgra: Vec<u8>,
    ) {
    }
}
