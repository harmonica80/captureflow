use serde::Serialize;
use std::{fs, path::PathBuf, time::SystemTime};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RectInfo {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub device_name: String,
    pub bounds: RectInfo,
    pub work_area: RectInfo,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshot {
    pub image_path: String,
    pub metadata_path: String,
    pub captured_at_unix_ms: u128,
    pub virtual_desktop: RectInfo,
    pub monitors: Vec<MonitorInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotMetadata<'a> {
    schema_version: u32,
    captured_at_unix_ms: u128,
    virtual_desktop: &'a RectInfo,
    monitors: &'a [MonitorInfo],
}

pub fn capture_virtual_desktop(app: &AppHandle) -> Result<DesktopSnapshot, String> {
    let captured_at_unix_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| format!("無法取得系統時間：{error}"))?
        .as_millis();

    let output_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("無法取得應用程式資料目錄：{error}"))?
        .join("poc-a");
    fs::create_dir_all(&output_dir).map_err(|error| format!("無法建立 PoC 輸出目錄：{error}"))?;

    let image_path = output_dir.join(format!("virtual-desktop-{captured_at_unix_ms}.png"));
    let metadata_path = output_dir.join(format!("virtual-desktop-{captured_at_unix_ms}.json"));

    let (virtual_desktop, monitors, rgba) = platform::capture()?;
    image::save_buffer_with_format(
        &image_path,
        &rgba,
        virtual_desktop.width as u32,
        virtual_desktop.height as u32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("無法儲存 PNG：{error}"))?;

    let metadata = SnapshotMetadata {
        schema_version: 1,
        captured_at_unix_ms,
        virtual_desktop: &virtual_desktop,
        monitors: &monitors,
    };
    let json =
        serde_json::to_vec_pretty(&metadata).map_err(|error| format!("無法建立 JSON：{error}"))?;
    fs::write(&metadata_path, json).map_err(|error| format!("無法儲存 JSON：{error}"))?;

    Ok(DesktopSnapshot {
        image_path: path_to_string(image_path),
        metadata_path: path_to_string(metadata_path),
        captured_at_unix_ms,
        virtual_desktop,
        monitors,
    })
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(windows)]
mod platform {
    use super::{MonitorInfo, RectInfo};
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::{BOOL, LPARAM, RECT},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC, HMONITOR, MONITORINFO,
            MONITORINFOEXW, MONITORINFOF_PRIMARY, SRCCOPY,
        },
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::{
                GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
                SM_YVIRTUALSCREEN,
            },
        },
    };

    pub fn capture() -> Result<(RectInfo, Vec<MonitorInfo>, Vec<u8>), String> {
        unsafe {
            let virtual_desktop = RectInfo {
                x: GetSystemMetrics(SM_XVIRTUALSCREEN),
                y: GetSystemMetrics(SM_YVIRTUALSCREEN),
                width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
                height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
            };
            if virtual_desktop.width <= 0 || virtual_desktop.height <= 0 {
                return Err("Windows 回報的 virtual desktop 尺寸無效".into());
            }

            let monitors = enumerate_monitors()?;
            let pixels = capture_pixels(&virtual_desktop)?;
            Ok((virtual_desktop, monitors, pixels))
        }
    }

    unsafe fn enumerate_monitors() -> Result<Vec<MonitorInfo>, String> {
        let mut monitors = Vec::<MonitorInfo>::new();
        let result = EnumDisplayMonitors(
            None,
            None,
            Some(enum_monitor_callback),
            LPARAM((&mut monitors as *mut Vec<MonitorInfo>) as isize),
        );
        if !result.as_bool() {
            return Err("EnumDisplayMonitors 無法列舉顯示器".into());
        }
        if monitors.is_empty() {
            return Err("Windows 未回報任何顯示器".into());
        }
        Ok(monitors)
    }

    unsafe extern "system" fn enum_monitor_callback(
        monitor: HMONITOR,
        _monitor_dc: HDC,
        _monitor_rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(data.0 as *mut Vec<MonitorInfo>);
        if let Some(info) = read_monitor(monitor) {
            monitors.push(info);
        }
        BOOL(1)
    }

    unsafe fn read_monitor(monitor: HMONITOR) -> Option<MonitorInfo> {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO).as_bool() {
            return None;
        }

        let mut dpi_x = 96_u32;
        let mut dpi_y = 96_u32;
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let name_len = info
            .szDevice
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(info.szDevice.len());

        Some(MonitorInfo {
            device_name: String::from_utf16_lossy(&info.szDevice[..name_len]),
            bounds: rect_info(info.monitorInfo.rcMonitor),
            work_area: rect_info(info.monitorInfo.rcWork),
            dpi_x,
            dpi_y,
            scale_factor: dpi_x as f64 / 96.0,
            is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        })
    }

    fn rect_info(rect: RECT) -> RectInfo {
        RectInfo {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        }
    }

    unsafe fn capture_pixels(rect: &RectInfo) -> Result<Vec<u8>, String> {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err("GetDC 無法取得桌面裝置內容".into());
        }
        let memory_dc = CreateCompatibleDC(Some(screen_dc));
        if memory_dc.is_invalid() {
            ReleaseDC(None, screen_dc);
            return Err("CreateCompatibleDC 失敗".into());
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, rect.width, rect.height);
        if bitmap.is_invalid() {
            DeleteDC(memory_dc);
            ReleaseDC(None, screen_dc);
            return Err("CreateCompatibleBitmap 失敗".into());
        }
        let old_object = SelectObject(memory_dc, bitmap.into());

        let copied = BitBlt(
            memory_dc,
            0,
            0,
            rect.width,
            rect.height,
            Some(screen_dc),
            rect.x,
            rect.y,
            SRCCOPY,
        )
        .as_bool();

        let result = if copied {
            read_bitmap_rgba(memory_dc, bitmap, rect.width, rect.height)
        } else {
            Err("BitBlt 無法複製 virtual desktop 像素".into())
        };

        SelectObject(memory_dc, old_object);
        DeleteObject(bitmap.into());
        DeleteDC(memory_dc);
        ReleaseDC(None, screen_dc);
        result
    }

    unsafe fn read_bitmap_rgba(
        dc: HDC,
        bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
        width: i32,
        height: i32,
    ) -> Result<Vec<u8>, String> {
        let byte_len = width as usize * height as usize * 4;
        let mut bgra = vec![0_u8; byte_len];
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        if lines != height {
            return Err(format!("GetDIBits 只取得 {lines}/{height} 行像素"));
        }

        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        Ok(bgra)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{MonitorInfo, RectInfo};

    pub fn capture() -> Result<(RectInfo, Vec<MonitorInfo>, Vec<u8>), String> {
        Err("PoC-A 目前只支援 Windows".into())
    }
}
