# CaptureFlow 0.5.18

- 框選的編號預設由 34 放大至 51 像素。一般滾輪一格調整 6 像素，支援較小的滾輪增量。
- 框選畫筆在有取樣間距時補收同一筆的 Windows 滑鼠歷史取樣，並以二次曲線繪製。拖曳只重繪受影響區域，並重用已解析的標註字型，進入主編輯器後保留平滑筆跡。
- Ctrl+A 會反白並提示已全選。Backspace 可清除選取文字，清空後按 Enter 會刪除原文字物件，Esc 可以放棄尚未套用的修改。
- 點貼圖上方的「編輯」時，專案讀取成功後關閉原先的置頂貼圖。編輯後可以在原位置重新釘選。

## 驗證

- 前端建置、12 項瀏覽器回歸測試、Rust 格式與差異檢查通過。測試確認貼圖取消釘選後，標註仍可修改並在原位置重新釘選。
- 文字反白使用白字 #FFFFFF、深藍 #164B80，皆為不透明，靜態對比約 8.93:1。文字提示沿用白字深底。
- Backspace 修正的前一個 Windows 建置已實測編號 51 像素及滾輪每格 6 像素、Ctrl+A 反白、Backspace 清除、短曲線保留可編輯、貼圖取消釘選及原位重新釘選。含字型快取的最終 EXE 已建置與核對雜湊，完整實機複驗等待 Windows 桌面解鎖。

## 測試版下載

- [Windows 免安裝測試版 ZIP](https://github.com/harmonica80/captureflow/actions/runs/45648935155/artifacts/15039077994) · [GitHub Actions 建置紀錄](https://github.com/harmonica80/captureflow/actions/runs/45648935155)
- GitHub 下載需要登入，成品保存至 2027 年 2 月 23 日 23:48（臺灣時間）。
- ZIP 內為 `CaptureFlow-portable.exe`，版本 0.5.18，來源提交 `a79b2a5`，大小 6,751,744 bytes。
- EXE 的 SHA-256 為 `C9E4C00E8B6B103FAE2E1B4C8A6FF2F07CAE75E4075242BE313615ECF06BD553`，已與 Actions 紀錄核對。
