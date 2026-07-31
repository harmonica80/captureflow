# CaptureFlow 第 0 階段執行基線

> 狀態：PoC-A 已通過 GitHub Windows 編譯，等待本機實際執行與多螢幕驗證  
> 建立日期：2026-07-31  
> 階段目標：用最小可執行原型消除 Windows 擷取核心的高風險假設。

## 1. 產品基線

- 平台：Windows 10／11 x64。
- 交付：單一免安裝 EXE；設定及快取可保存在使用者資料目錄。
- 架構：React + TypeScript + Vite + Tauri 2 + Rust。
- 第一個公開版本優先：精準截圖、物件式標註、貼圖、歷史。
- OCR、QR、長截圖及錄影在核心穩定後分階段加入。
- 核心資料預設只在本機處理。

## 2. 本機環境盤點

盤點日期：2026-07-31。

| 項目 | 結果 | 狀態 |
|---|---|---|
| Windows | Windows 11 Home x64，10.0.26200 | 可用 |
| PowerShell | 5.1.26100.8875 | 可用 |
| Node.js | v25.9.0 | 可開發；正式 CI 使用 Node.js 22 LTS |
| npm | 11.17.0 | 可用，PowerShell 使用 `npm.cmd` |
| Rust | rustc/cargo 1.96.0 | 可用 |
| Rust toolchain | stable-x86_64-pc-windows-msvc | 正確 |
| Git | 2.48.1.windows.1 | 可用 |
| GitHub CLI | 2.92.0 | 可用 |
| MSVC linker | `link.exe` 未找到 | 阻擋本機 Rust/Tauri EXE 建置 |
| WebView2 | Windows 11 一般已內建，盤點登錄值未取得 | 首次 Tauri 執行時驗證 |

## 3. 第 0 階段 PoC

### PoC-A：桌面快照與座標模型

- 列舉所有顯示器的實體座標、工作區、DPI 與主要顯示器。
- 擷取整個 virtual desktop，涵蓋負座標。
- 保存測試截圖及 JSON 中繼資料。

退出標準：兩種以上顯示器排列與兩種 DPI 組合，像素與座標一致。

目前進度：

- 已實作 Win32 顯示器列舉、bounds、work area、主要顯示器及有效 DPI。
- 已實作 GDI `BitBlt` virtual desktop 擷取與 BGRA → RGBA PNG 輸出。
- 已實作 schema v1 JSON 中繼資料與 Tauri command。
- 已建立繁中測試介面及 `POC_A_TESTING.md` 測試矩陣。
- GitHub Actions run `30619404697` 已通過完整 Rust/Tauri release 編譯。
- 已產出 `CaptureFlow-portable.exe`（v0.1.0，4.17 MB）。
- SHA-256：`AF1D155B03299000C4E7A6D8D451B55D9F0686FA02D1B29EBBE464B12C69032F`。
- 待辦：在本機執行 portable EXE，完成 `POC_A_TESTING.md` 多螢幕實機測試。

### PoC-B：透明選取覆蓋層

- 在 virtual desktop 建立透明、置頂、無框視窗。
- 凍結快照後提供拖曳選取、八方向調整、尺寸提示與 Esc 取消。
- 選取完成後正確轉回來源圖片像素。

退出標準：主螢幕及負座標副螢幕可單獨與跨螢幕選取。

### PoC-C：置頂貼圖

- 將裁切結果建立為獨立置頂視窗。
- 驗證移動、縮放、透明度、鎖定與滑鼠穿透。
- 保存及恢復視窗位置；螢幕移除後移回可見區域。

退出標準：重啟程式及切換螢幕配置後貼圖仍可操作。

### PoC-D：錄影與 OCR 選型

- 驗證 Windows Graphics Capture 的區域擷取能力。
- 比較 MP4／GIF 編碼路徑、FFmpeg 打包體積及授權。
- 使用固定繁中、英文、數字樣本比較 OCR 引擎。

退出標準：形成 ADR，選定錄影及 OCR 技術，不要求本階段完成產品功能。

## 4. 第一個 Sprint（建議 2 週）

1. 完成專案骨架、CI 與 portable build。
2. 建立 Rust `display` 模組，輸出顯示器座標和 DPI。
3. 建立 Rust `capture` 模組，產生 virtual desktop 快照。
4. 建立最小選取覆蓋層，只做矩形框選與取消。
5. 建立多螢幕測試紀錄格式與手動測試表。
6. 產出第一個 PoC portable EXE。

## 5. 完成定義

- 原始碼、錯誤處理與必要測試已完成。
- `npm.cmd run build` 通過。
- `cargo fmt --check` 與 `cargo check` 通過，或清楚記錄環境阻礙。
- GitHub Actions Windows 建置通過。
- 產出 portable EXE、檔案大小、版本與 SHA-256。
- 涉及透明視窗、多螢幕或錄影時，完成 Windows 實機驗證。

## 6. 尚待產品決策

以下不阻擋 PoC-A／B，可在第 0 階段結束前確認：

- 正式產品名稱及識別圖示。
- v1.0 是否必須包含 MP4／GIF 錄影。
- 歷史截圖預設保存天數與容量。
- 是否預設啟用開機啟動。
- 公開發布前是否購買程式碼簽章憑證。
