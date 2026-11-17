# CaptureFlow 0.5.13：讓外部工具擷取框選操作

- 移除主視窗頁尾的 CC 標章、授權文字與連結；保留作者、部落格、專案及版本資訊。
- 原生框選視窗不再要求 Windows 排除擷取。錄製操作示範時，請在外部工具選擇「整個螢幕／顯示器」；只錄指定應用程式時，獨立的框選視窗可能不在該來源內。
- 一般框選維持從顯示工具列之前的底圖輸出；長擷圖仍會在捲動取樣期間暫時隱藏工具列，再顯示預覽。

## 驗證

- TypeScript／Vite 正式建置、Rust 格式檢查及 10 項 Playwright 測試通過。此版沒有新增文字或改動文字色彩；既有頁尾連結及淺深色介面對比測試通過（門檻 4.5:1）。
- Windows 免安裝版實測：在同一台電腦顯示合成測試圖，v0.5.12 的工具列未出現在外部桌面截圖；v0.5.13 的工具列可見。另用 FFmpeg 錄取桌面的測試區域，解碼後的 10 個影格皆可辨認原生工具列。
- CaptureFlow 自己輸出的 360 × 240 普通截圖與 360 × 288 捲動長圖均逐像素符合原圖，不含工具列。這次驗證的是桌面擷取來源，未測單一應用程式擷取或各家錄影軟體。

## 下載與建置

- [下載 Windows 免安裝 EXE](https://github.com/harmonica80/captureflow/releases/download/v0.5.13/CaptureFlow-portable.exe) · [GitHub Actions 建置紀錄](https://github.com/harmonica80/captureflow/actions/runs/44011358115)
- 檔案版本 0.5.13，大小 6,615,040 bytes；來源提交 `b48a69971baab0f7cae51ebe44693505b341c7cb`。
- SHA-256：`34AB7BC272AF089E14BA593905C59BB07FFB5F4440533FECAAF95A44B4E1FD07`。
- 公開下載回傳 HTTP 200；附件名稱正確，重新下載後的 SHA-256 與實機驗證的 Actions 成品相同。
