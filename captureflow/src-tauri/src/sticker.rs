use std::path::Path;

pub fn open(image_path: String, x: i32, y: i32) -> Result<(), String> {
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

    std::thread::Builder::new()
        .name("captureflow-sticker".into())
        .spawn(move || platform::run(width as i32, height as i32, x, y, bgra))
        .map_err(|error| format!("無法啟動貼圖視窗：{error}"))?;
    Ok(())
}

#[cfg(windows)]
mod platform {
    use std::{cell::RefCell, mem::size_of};
    use windows::{
        core::w,
        Win32::{
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
            Graphics::Gdi::{
                Arc, BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, Ellipse, EndPaint,
                FillRect, GetStockObject, InvalidateRect, LineTo, MoveToEx, Polyline, Rectangle,
                SelectObject, SetBkMode, SetTextColor, StretchDIBits, TextOutW, BITMAPINFO,
                BITMAPINFOHEADER, BI_RGB, DEFAULT_GUI_FONT, DIB_RGB_COLORS, NULL_BRUSH,
                PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
                Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE},
                WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                    GetClientRect, GetMessageW, GetSystemMetrics, GetWindowRect, PostQuitMessage,
                    RegisterClassW, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowPos,
                    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HTCAPTION, HTCLIENT,
                    HWND_TOPMOST, LWA_ALPHA, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
                    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SW_SHOW, WINDOW_EX_STYLE,
                    WM_DESTROY, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEWHEEL, WM_MOVE, WM_NCHITTEST,
                    WM_NCRBUTTONUP, WM_PAINT, WM_RBUTTONUP, WM_SIZE, WNDCLASSW, WS_EX_LAYERED,
                    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
                },
            },
        },
    };

    thread_local! {
        static STATE: RefCell<Option<StickerState>> = const { RefCell::new(None) };
    }

    struct StickerState {
        source_width: i32,
        source_height: i32,
        bgra: Vec<u8>,
        opacity: u8,
        locked: bool,
        scale_percent: i32,
        sticker_hwnd: HWND,
        toolbar_hwnd: HWND,
    }

    pub fn run(source_width: i32, source_height: i32, x: i32, y: i32, bgra: Vec<u8>) {
        STATE.with(|state| {
            *state.borrow_mut() = Some(StickerState {
                source_width,
                source_height,
                bgra,
                opacity: 255,
                locked: false,
                scale_percent: 100,
                sticker_hwnd: HWND::default(),
                toolbar_hwnd: HWND::default(),
            });
        });
        let _ = unsafe { run_window(x, y, source_width, source_height) };
        STATE.with(|state| *state.borrow_mut() = None);
    }

    unsafe fn run_window(x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let module =
            GetModuleHandleW(None).map_err(|error| format!("GetModuleHandleW：{error}"))?;
        let instance = HINSTANCE(module.0);
        let class_name = w!("CaptureFlowStickerWindow");
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
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
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
            .map_err(|error| format!("無法設定貼圖透明度：{error}"))?;
        let toolbar_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0),
            toolbar_class_name,
            w!("CaptureFlow Sticker Toolbar"),
            WS_POPUP,
            x,
            y,
            340,
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
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(toolbar_hwnd, SW_SHOW);
        position_toolbar(hwnd);
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
            WM_NCHITTEST => {
                let locked = STATE.with(|state| {
                    state
                        .borrow()
                        .as_ref()
                        .map(|state| state.locked)
                        .unwrap_or(false)
                });
                LRESULT(if locked {
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
                    resize(
                        hwnd,
                        if delta > 0 { 1.1 } else { 0.9 },
                        screen_point(lparam),
                    );
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == b'L' as u16 => {
                toggle_lock(hwnd);
                LRESULT(0)
            }
            WM_RBUTTONUP | WM_NCRBUTTONUP => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_MOVE | WM_SIZE => {
                position_toolbar(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
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
                        resize(
                            sticker,
                            if delta > 0 { 1.1 } else { 0.9 },
                            screen_point(lparam),
                        );
                    }
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                let sticker = sticker_hwnd();
                if !sticker.is_invalid() {
                    let _ = DestroyWindow(sticker);
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
            WM_RBUTTONUP | WM_NCRBUTTONUP => {
                let sticker = sticker_hwnd();
                if !sticker.is_invalid() {
                    let _ = DestroyWindow(sticker);
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

    unsafe fn resize(hwnd: HWND, factor: f64, cursor: (i32, i32)) {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let current_width = rect.right - rect.left;
        let current_height = rect.bottom - rect.top;
        let (new_width, new_height) = STATE.with(|state| {
            let state = state.borrow();
            let Some(state) = state.as_ref() else {
                return (current_width, current_height);
            };
            let current_scale = current_width as f64 / state.source_width as f64;
            let maximum_scale = (2400.0 / state.source_width as f64)
                .min(1600.0 / state.source_height as f64)
                .max(0.05);
            let minimum_scale = (80.0 / state.source_width as f64)
                .max(60.0 / state.source_height as f64)
                .min(maximum_scale);
            let scale = (current_scale * factor).clamp(minimum_scale, maximum_scale);
            (
                (state.source_width as f64 * scale).round() as i32,
                (state.source_height as f64 * scale).round() as i32,
            )
        });
        let new_x = cursor.0
            - ((cursor.0 - rect.left) as i64 * new_width as i64 / current_width as i64) as i32;
        let new_y = cursor.1
            - ((cursor.1 - rect.top) as i64 * new_height as i64 / current_height as i64) as i32;
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            new_x,
            new_y,
            new_width,
            new_height,
            SWP_NOACTIVATE,
        );
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.scale_percent =
                    (new_width as f64 / state.source_width as f64 * 100.0).round() as i32;
            }
        });
        position_toolbar(hwnd);
        invalidate_sticker_and_toolbar(hwnd);
    }

    unsafe fn adjust_opacity(hwnd: HWND, delta: i32) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.opacity = (state.opacity as i32 + delta).clamp(48, 255) as u8;
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), state.opacity, LWA_ALPHA);
            }
        });
        invalidate_sticker_and_toolbar(hwnd);
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
            let _ = DestroyWindow(hwnd);
        } else if point.0 < 52 {
            toggle_lock(hwnd);
        } else if point.0 < 104 {
            adjust_opacity(hwnd, -16);
        } else if point.0 < 156 {
            adjust_opacity(hwnd, 16);
        }
    }

    unsafe fn toggle_lock(hwnd: HWND) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.locked = !state.locked;
            }
        });
        invalidate_sticker_and_toolbar(hwnd);
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
        let toolbar_width = 340.min(virtual_width);
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

        let icon_pen = CreatePen(PS_SOLID, 2, COLORREF(0x0031_2A24));
        let old_pen = SelectObject(dc, icon_pen.into());
        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
        if state.locked {
            draw_lock_icon(dc, 26, 20);
        } else {
            draw_move_icon(dc, 26, 20);
        }
        draw_opacity_icon(dc, 78, 20, false);
        draw_opacity_icon(dc, 130, 20, true);
        draw_close_icon(dc, width - 25, 20);
        draw_zoom_icon(dc, 174, 20);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(icon_pen.into());

        let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x0031_2A24));
        let status = format!(
            "{}%   α{}%",
            state.scale_percent,
            (state.opacity as f64 / 255.0 * 100.0).round() as i32
        );
        draw_text(dc, 188, 11, &status);
        SelectObject(dc, old_font);

        let separator_pen = CreatePen(PS_SOLID, 1, COLORREF(0x00D0_CBC6));
        let old_pen = SelectObject(dc, separator_pen.into());
        for x in [53, 105, 157, width - 49] {
            MoveToEx(dc, x, 8, None);
            LineTo(dc, x, 32);
        }
        SelectObject(dc, old_pen);
        DeleteObject(separator_pen.into());
    }

    unsafe fn draw_move_icon(dc: windows::Win32::Graphics::Gdi::HDC, x: i32, y: i32) {
        MoveToEx(dc, x - 9, y, None);
        LineTo(dc, x + 9, y);
        MoveToEx(dc, x - 9, y, None);
        LineTo(dc, x - 5, y - 4);
        MoveToEx(dc, x - 9, y, None);
        LineTo(dc, x - 5, y + 4);
        MoveToEx(dc, x + 9, y, None);
        LineTo(dc, x + 5, y - 4);
        MoveToEx(dc, x + 9, y, None);
        LineTo(dc, x + 5, y + 4);
        MoveToEx(dc, x, y - 9, None);
        LineTo(dc, x, y + 9);
        MoveToEx(dc, x, y - 9, None);
        LineTo(dc, x - 4, y - 5);
        MoveToEx(dc, x, y - 9, None);
        LineTo(dc, x + 4, y - 5);
        MoveToEx(dc, x, y + 9, None);
        LineTo(dc, x - 4, y + 5);
        MoveToEx(dc, x, y + 9, None);
        LineTo(dc, x + 4, y + 5);
    }

    unsafe fn draw_lock_icon(dc: windows::Win32::Graphics::Gdi::HDC, x: i32, y: i32) {
        Rectangle(dc, x - 8, y - 1, x + 8, y + 10);
        Arc(dc, x - 6, y - 10, x + 6, y + 4, x + 6, y - 2, x - 6, y - 2);
        Ellipse(dc, x - 1, y + 3, x + 2, y + 6);
    }

    unsafe fn draw_opacity_icon(
        dc: windows::Win32::Graphics::Gdi::HDC,
        x: i32,
        y: i32,
        increase: bool,
    ) {
        let drop = [
            POINT { x: x - 7, y: y + 1 },
            POINT { x, y: y - 9 },
            POINT { x: x + 7, y: y + 1 },
            POINT { x: x + 6, y: y + 7 },
            POINT {
                x: x + 2,
                y: y + 10,
            },
            POINT {
                x: x - 2,
                y: y + 10,
            },
            POINT { x: x - 6, y: y + 7 },
            POINT { x: x - 7, y: y + 1 },
        ];
        Polyline(dc, &drop);
        MoveToEx(dc, x + 9, y + 6, None);
        LineTo(dc, x + 17, y + 6);
        if increase {
            MoveToEx(dc, x + 13, y + 2, None);
            LineTo(dc, x + 13, y + 10);
        }
    }

    unsafe fn draw_zoom_icon(dc: windows::Win32::Graphics::Gdi::HDC, x: i32, y: i32) {
        Ellipse(dc, x - 7, y - 7, x + 5, y + 5);
        MoveToEx(dc, x + 3, y + 3, None);
        LineTo(dc, x + 9, y + 9);
    }

    unsafe fn draw_close_icon(dc: windows::Win32::Graphics::Gdi::HDC, x: i32, y: i32) {
        MoveToEx(dc, x - 7, y - 7, None);
        LineTo(dc, x + 7, y + 7);
        MoveToEx(dc, x + 7, y - 7, None);
        LineTo(dc, x - 7, y + 7);
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
    pub fn run(_source_width: i32, _source_height: i32, _x: i32, _y: i32, _bgra: Vec<u8>) {}
}
