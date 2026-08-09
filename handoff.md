# CaptureFlow handoff

日期：2026-08-08
目前分支：`main`
遠端：`https://github.com/harmonica80/captureflow.git`

## 本次修正

使用者回報：長擷取啟用後，剪刀圖示所在的全螢幕原生遮罩會攔截底下應用程式的輸入，導致無法正常捲動、取消或退出，只能重新登入 Windows。

已修改：

- `captureflow/src-tauri/src/selector.rs`
- 長擷取核心函式：`capture_long_segment`，目前約在第 1084 行。

修正行為：

1. 長擷取收到向下滾輪事件時，暫時隱藏全螢幕、置頂的選取遮罩。
2. 將游標移到選取範圍中央，透過 Windows `SendInput` 將滾輪事件送給底下應用程式。
3. 等待畫面更新後擷取桌面畫面並拼接長圖。
4. 無論捲動或擷取成功、失敗，均恢復遮罩視窗、前景與焦點。
5. 已保留並確認取消／退出路徑：
   - `Escape`：`finish(None)`、`DestroyWindow`。
   - 工具列取消按鈕：傳送 `None`，再以 `WM_CLOSE` 關閉視窗。
   - `WM_DESTROY`：停止 timer、再次呼叫 `finish(None)`、`PostQuitMessage`。
6. 已確認 `PostMessageW` import 存在，完成與取消按鈕可正常編譯解析。

## 目前工作樹

本次交接預期只有以下兩個未提交檔案：

- `captureflow/src-tauri/src/selector.rs`
- `handoff.md`

不要使用 `git reset --hard` 或 `git checkout --`，以免刪除尚未提交的修正。

## 已完成驗證

- `cargo fmt --manifest-path captureflow/src-tauri/Cargo.toml -- --check`：通過。
- `git diff --check`：通過。
- Rust `cargo check --locked`：已嘗試；編譯因本機沒有 Visual Studio Build Tools 的 `link.exe` 而停止。這是專案既定限制，不是目前程式碼已確認的 Rust 語法錯誤。
- npm 依賴安裝：目前工作區位於同步磁碟，`npm ci` 遇到 `node_modules` 的 `EBADF`／`EPERM` 寫入錯誤；另一台電腦請優先將專案放在一般本機磁碟後再執行 `npm ci`。

## 另一台電腦接手步驟

在專案根目錄執行：

```powershell
git status --short
git diff --check
npm.cmd ci
cargo fmt --manifest-path captureflow\src-tauri\Cargo.toml -- --check
npm.cmd run build
```

確認差異仍存在後，依要上傳的分支執行：

```powershell
git add captureflow/src-tauri/src/selector.rs handoff.md
git commit -m "Fix long capture input blocking"
git push origin main
```

若不直接推送 `main`，請改用另一個分支並在 GitHub 上合併；目前 `.github/workflows/build-portable.yml` 的自動觸發分支是 `main`。

## GitHub Actions 交付

既有 workflow：`.github/workflows/build-portable.yml`

它會執行：

- Windows `windows-latest`
- Node.js 22
- Rust stable `x86_64-pc-windows-msvc`
- `npm ci`
- `npm run build`
- Rust formatting check
- `npm run build:portable`
- 上傳 `CaptureFlow-portable.exe` artifact

推送到 `main` 後，請到 GitHub Actions 確認 `Build portable EXE` 成功，下載 `CaptureFlow-portable` artifact。SHA-256 會在 `Prepare artifact` 步驟的輸出中列出。

## Windows 實機驗收

使用 Actions 產生的 portable EXE，在 Chrome、Edge、檔案總管或其他可捲動視窗測試：

1. 開始框選並按剪刀圖示啟用長擷取。
2. 連續向下捲動至少 10 次。
3. 確認底下應用程式會捲動，長圖預覽持續更新，遮罩不會永久卡住。
4. 測試 `Escape` 取消，以及工具列取消按鈕。
5. 測試完成按鈕，確認能回到主視窗並產生長擷圖。

注意：目前每次滾輪擷取會短暫隱藏／恢復遮罩，可能有短暫閃爍；這是為了讓底下應用程式確實收到輸入。若後續要消除閃爍，需另行設計跨程序的滑鼠穿透方案，不能直接移除恢復遮罩的清理流程。

## 專案規範提醒

不要在本機安裝 Visual Studio Build Tools。Windows Release 應由 GitHub Actions 編譯，並交付 portable EXE、Actions 執行連結與 SHA-256。