# CaptureFlow

CaptureFlow 是一套 Windows 免安裝螢幕截圖與物件式標註工具，目標是讓每一次截圖都能繼續編輯，而不是輸出後就失去原始標註資料。

## 主要特色

- 支援多螢幕、混合 DPI、框選範圍及整個螢幕擷取。
- 支援長截圖：框選可捲動內容後，自動向下捲動、比對重疊區域並拼接。
- 矩形、圓形、畫筆、曲線箭頭、文字、序號與馬賽克標註。
- 標註物件可以重新選取、移動、調整粗細、顏色與大小。
- JSON 專案會內嵌原始截圖，移至其他電腦後仍可重新編輯。
- 自動保存最近的截圖與可編輯專案，保留數量可在偏好設定調整。
- 支援複製圖片、另存圖片及建立 Windows 置頂貼圖。
- 可自訂全域截圖快捷鍵與預設圖片儲存資料夾。
- 提供淺色與深色主題，預設使用淺色介面。
- 啟動時以非阻塞方式檢查 GitHub 是否有新版本。

## 基本使用方式

1. 執行 `CaptureFlow-portable.exe`，不需要安裝。
2. 按下「框選螢幕範圍」，或使用預設快捷鍵 `Alt+Shift+A`。
3. 在編輯器選擇標註工具；滑鼠移到物件上可重新選取及調整。
4. 使用右下角縮放控制調整圖片顯示比例。
5. 使用工具列輸出圖片、複製至剪貼簿、建立置頂貼圖或儲存 JSON 專案。
6. 從左側「歷史截圖記錄」或「開啟 JSON 專案」繼續過去的編輯工作。

## 下載

[下載最新版 CaptureFlow-portable.exe](https://github.com/harmonica80/captureflow/releases/latest/download/CaptureFlow-portable.exe)

Windows 可能會對未簽章的免安裝程式顯示 SmartScreen 提醒。請確認下載來源及發布頁提供的 SHA-256 後再執行。

## 開發與授權

- 開發者：[述文老師](https://harmonica80.blogspot.com/)
- 專案首頁：[harmonica80/captureflow](https://github.com/harmonica80/captureflow)
- Windows release 編譯由 GitHub Actions 執行，本機不需要安裝 Visual Studio Build Tools。

目前仍在持續開發階段，歡迎透過 GitHub Issues 回報操作問題與建議。
