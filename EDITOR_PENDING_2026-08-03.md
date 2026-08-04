# CaptureFlow 圖片編輯器待辦（2026-08-03）

本文件記錄使用者已實機驗證、預計下次處理的四項問題。本次只記錄，不修改程式。

> 2026-08-04：以下四項已完成程式修正，等待 Windows portable EXE 實機驗收。

## 1. 序號欄位文案

- 現況：選取既有序號物件時，欄位仍顯示「下一個序號」。
- 需求：選取既有序號物件時改為「修改序號」；只有尚未選取物件、準備建立新序號時才顯示「下一個序號」。

## 2. 滾輪調整與頁面捲動衝突

- 現況：滑過標註物件使用滾輪調整粗細或大小時，偶爾同時觸發右側頁面捲軸。
- 建議修正：在畫布區使用非 passive 的原生 `wheel` listener，命中可調整物件時同步執行 `preventDefault()` 與 `stopPropagation()`；未命中物件時保留正常捲動。
- 驗收：滑過物件時滾輪只調整物件；滑過圖片空白處時仍可正常捲動編輯頁面。

## 3. 複製圖片動作

- 現況：複製圖片只複製目前來源圖片，尚未保證包含未套用的物件式標註。
- 需求：改成「套用標註＋複製圖片」單一動作。
- 注意：建議共用同一個無損合成函式，避免「另存圖片」與「複製圖片」產生不同結果。

## 4. 開啟 JSON 專案失敗

- 實機錯誤：`Command plugin:dialog|open not allowed by ACL`。
- 已知原因：Tauri capability 尚未授權 dialog open 命令，不是 JSON 檔案損壞。
- 修正方向：在 `src-tauri/capabilities/default.json` 加入對應的 dialog open 權限，並一併核對 save 權限。
- 另需檢查：目前預設路徑畫面曾出現 `DownloadsDownloadsCaptureFlow-project1.json`，應使用路徑 API 正確 join，避免重複 Downloads。
