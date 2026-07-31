# CaptureFlow 系統交接摘要

## 專案狀態

目前位於第 0 階段。PoC-A virtual desktop 快照已完成程式實作，並由 GitHub Actions 成功產生 portable EXE；下一步是 Windows 本機多螢幕實測。Tauri 2 + React + TypeScript 專案位於 `captureflow/`。

## 重要文件

- `免安裝工具開發環境說明書.md`：Windows/Tauri 免安裝建置基準。
- `螢幕截取創新工具開發建議書.md`：完整產品功能、架構及里程碑。
- `PHASE0_EXECUTION.md`：目前階段的工作順序、驗收及環境盤點。
- `POC_A_TESTING.md`：PoC-A 的輸出格式、多螢幕與 DPI 測試矩陣。
- `POC_B_TESTING.md`：原生跨螢幕選取層的操作與驗收矩陣。

## 常用指令

```powershell
Set-Location captureflow
npm.cmd ci
npm.cmd run build
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo check --manifest-path src-tauri\Cargo.toml
npm.cmd run build:portable
```

## 本機已知阻礙

- 依使用者決定，不在本機安裝 Visual Studio Build Tools 2022。
- `link.exe` 不在 PATH，因此所有 Rust/Tauri portable EXE 由 GitHub Actions `windows-latest` 建置。
- PoC-A 成功建置 run：`30619404697`。
- PoC-B 成功建置 run：`30621992095`。
- 本機交付檔：`captureflow/release/CaptureFlow-portable.exe`。
- 目前 PoC-B v0.1.0 SHA-256：`C017981FA7AB26B447D62456FA85D9B0D6817D54D13AB36ED431D5660B72F2DB`。

## 架構約束

- 設定、素材庫等一般 UI 使用 React/WebView。
- 即時選取、透明覆蓋、貼圖與錄影敏感視窗需優先驗證原生 Windows 路徑。
- 多螢幕 virtual desktop、負座標及混合 DPI 是核心能力。
- 原始圖片與標註物件分層保存；遮蔽輸出必須真正壓平且不可逆。
- OCR、QR、歷史與截圖預設本機處理，不在未經同意時上傳。

## 下一個實作目標

PoC-A 已在右側雙螢幕、125% + 100% 混合 DPI 配置通過。PoC-B 已通過 GitHub Windows 編譯並下載至 `captureflow/release`；下一步依 `POC_B_TESTING.md` 完成原生跨螢幕選取實測。
