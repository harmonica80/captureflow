use std::path::Path;

pub fn open(image_path: String) -> Result<(), String> {
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
        .spawn(move || platform::run(width as i32, height as i32, bgra))
        .map_err(|error| format!("無法啟動貼圖視窗：{error}"))?;
    Ok(())
}

#[cfg(windows)]
mod platform {
    use std::{cell::RefCell, mem::size_of};
    use windows::{
        core::w,
        Win32::{
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, EndPaint, GetClientRect, StretchDIBits, BITMAPINFO, BITMAPINFOHEADER,
                BI_RGB, DIB_RGB_COLORS, PAINTSTRUCT, SRCCOPY,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
                Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE},
                WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
                    GetWindowRect, PostQuitMessage, RegisterClassW, SetForegroundWindow,
                    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TranslateMessage,
                    CS_HREDRAW, CS_VREDRAW, HTCAPTION, HWND_TOPMOST, LWA_ALPHA, MSG,
                    SWP_NOACTIVATE, SWP_NOMOVE, SW_SHOW, WINDOW_EX_STYLE, WM_DESTROY, WM_KEYDOWN,
                    WM_MOUSEWHEEL, WM_NCHITTEST, WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED,
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
    }

    pub fn run(source_width: i32, source_height: i32, bgra: Vec<u8>) {
        let (window_width, window_height) = initial_size(source_width, source_height);
        STATE.with(|state| {
            *state.borrow_mut() = Some(StickerState {
                source_width,
                source_height,
                bgra,
                opacity: 255,
            });
        });
        let _ = unsafe { run_window(window_width, window_height) };
        STATE.with(|state| *state.borrow_mut() = None);
    }

    unsafe fn run_window(width: i32, height: i32) -> Result<(), String> {
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

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_LAYERED.0),
            class_name,
            w!("CaptureFlow Sticker"),
            WS_POPUP,
            80,
            80,
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
        ShowWindow(hwnd, SW_SHOW);
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
            WM_NCHITTEST => LRESULT(HTCAPTION as isize),
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
                if GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    adjust_opacity(hwnd, delta.signum() * 16);
                } else {
                    resize(hwnd, if delta > 0 { 1.1 } else { 0.9 });
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
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
                }
            }
        });
        EndPaint(hwnd, &paint);
    }

    unsafe fn resize(hwnd: HWND, factor: f64) {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let current_width = rect.right - rect.left;
        let current_height = rect.bottom - rect.top;
        let new_width = ((current_width as f64 * factor).round() as i32).clamp(80, 2400);
        let new_height = ((current_height as f64 * factor).round() as i32).clamp(60, 1600);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            new_width,
            new_height,
            SWP_NOMOVE | SWP_NOACTIVATE,
        );
    }

    unsafe fn adjust_opacity(hwnd: HWND, delta: i32) {
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.opacity = (state.opacity as i32 + delta).clamp(48, 255) as u8;
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), state.opacity, LWA_ALPHA);
            }
        });
    }

    fn initial_size(width: i32, height: i32) -> (i32, i32) {
        let scale = (900.0 / width as f64).min(700.0 / height as f64).min(1.0);
        (
            (width as f64 * scale).round().max(80.0) as i32,
            (height as f64 * scale).round().max(60.0) as i32,
        )
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn run(_source_width: i32, _source_height: i32, _bgra: Vec<u8>) {}
}
