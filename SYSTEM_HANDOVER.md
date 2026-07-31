# CaptureFlow 系統交接摘要

## 專案狀態

目前位於第 0 階段。PoC-A virtual desktop 快照已完成程式實作，等待 MSVC 工具鏈與 Windows 多螢幕實機驗證。Tauri 2 + React + TypeScript 專案位於 `captureflow/`。

## 重要文件

- `免安裝工具開發環境說明書.md`：Windows/Tauri 免安裝建置基準。
- `螢幕截取創新工具開發建議書.md`：完整產品功能、架構及里程碑。
- `PHASE0_EXECUTION.md`：目前階段的工作順序、驗收及環境盤點。
- `POC_A_TESTING.md`：PoC-A 的輸出格式、多螢幕與 DPI 測試矩陣。

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

- `link.exe` 目前不在 PATH，本機 Rust/Tauri 連結預期會失敗。
- 安裝 Visual Studio Build Tools 2022 的「使用 C++ 的桌面開發」、MSVC v143 x64/x86 與 Windows SDK 後再重試。
- 在問題排除前，正式 portable EXE 可由 GitHub Actions `windows-latest` 建置。

## 架構約束

- 設定、素材庫等一般 UI 使用 React/WebView。
- 即時選取、透明覆蓋、貼圖與錄影敏感視窗需優先驗證原生 Windows 路徑。
- 多螢幕 virtual desktop、負座標及混合 DPI 是核心能力。
- 原始圖片與標註物件分層保存；遮蔽輸出必須真正壓平且不可逆。
- OCR、QR、歷史與截圖預設本機處理，不在未經同意時上傳。

## 下一個實作目標

先安裝或提供 Visual Studio Build Tools 2022 的 C++ 工具鏈，完成 PoC-A Rust 型別編譯與實機驗證；通過後進入 PoC-B 透明選取覆蓋層。
