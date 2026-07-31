# PoC-A：Virtual Desktop 快照測試

## 目的

驗證 CaptureFlow 能正確取得 Windows 多螢幕座標、DPI、virtual desktop 範圍及完整桌面像素，作為後續透明選取覆蓋層的座標基礎。

## 執行方式

```powershell
Set-Location captureflow
npm.cmd run tauri dev
```

在 CaptureFlow 視窗按下「執行桌面快照測試」。成功後介面會顯示：

- 顯示器數量。
- Virtual desktop 寬度與高度。
- 每部顯示器的 Windows 裝置名稱。
- 顯示器的 X／Y、寬度、高度、DPI 與縮放比例。
- PNG 與 JSON 的完整輸出路徑。

輸出預設位於 Windows 使用者應用程式資料目錄下的 `poc-a` 資料夾。

## JSON 格式

```json
{
  "schemaVersion": 1,
  "capturedAtUnixMs": 0,
  "virtualDesktop": {
    "x": -1920,
    "y": 0,
    "width": 3840,
    "height": 1080
  },
  "monitors": [
    {
      "deviceName": "DISPLAY1",
      "bounds": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
      "workArea": { "x": 0, "y": 0, "width": 1920, "height": 1040 },
      "dpiX": 96,
      "dpiY": 96,
      "scaleFactor": 1.0,
      "isPrimary": true
    }
  ]
}
```

實際裝置名稱通常包含 `\\.\DISPLAY1`；上例僅為格式示意。

## 必測矩陣

| 案例 | 配置 | 驗收 |
|---|---|---|
| A1 | 單螢幕、100% | PNG 尺寸等於螢幕實體像素 |
| A2 | 雙螢幕、副螢幕在右 | Virtual desktop 寬度涵蓋兩部螢幕 |
| A3 | 雙螢幕、副螢幕在左 | Virtual desktop X 為負值，PNG 左側內容正確 |
| A4 | 副螢幕在主螢幕上方 | Virtual desktop Y 為負值 |
| A5 | 100% + 125% 混合 DPI | 每部螢幕 DPI 正確，PNG 無重疊或空白偏移 |
| A6 | 100% + 150% 混合 DPI | 座標仍採實體像素且畫面完整 |

## 人工檢查項目

- PNG 包含每部顯示器的完整畫面。
- 顯示器交界處沒有重複、缺口或水平／垂直偏移。
- 顏色正常，紅色與藍色未交換。
- PNG 上下方向正確。
- JSON 的各顯示器聯集能覆蓋 virtual desktop。
- 工作列造成的 work area 差異合理。
- 主要顯示器只有一部。

## 已知限制

- PoC 使用 GDI `BitBlt`，部分硬體覆蓋、受保護影音及特殊 GPU 表面可能無法擷取。
- PoC 只驗證座標模型和基本像素路徑，不代表最終錄影後端選型。
- Windows C++ Build Tools、MSVC linker 與 Windows SDK 尚未安裝時，無法在本機啟動 Tauri 程式。
