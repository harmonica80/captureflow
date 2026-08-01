# CaptureFlow 開發準則

本文件適用於整個 CaptureFlow 專案及所有子目錄。後續開發、審查與驗收都必須遵守。

## 無障礙與文字對比

- 所有一般文字與其實際顯示背景的對比度必須符合 WCAG AA，至少為 **4.5:1**。
- 大型文字仍以 4.5:1 為專案預設標準，不因 WCAG 允許較低門檻而自動放寬。
- 必須檢查一般、hover、active、disabled、selected、error、透明視窗及疊加遮罩等所有狀態。
- 半透明文字或背景須以合成後的實際顏色計算；不可只比較 CSS／GDI 設定中的原始色碼。
- 不可只用顏色傳達狀態；鎖定、錯誤、成功與選取狀態應同時提供文字、形狀或圖示提示。
- 新增或修改介面時，應在交付說明中記錄文字對比檢查結果；若無法自動量測，至少列出前景色、背景色與人工驗證範圍。

## Windows 免安裝交付

- 不在本機安裝 Visual Studio Build Tools；Rust／Tauri Windows release 交由 GitHub Actions 編譯。
- 桌面功能完成後須提供更新後的 `CaptureFlow-portable.exe`、Actions 執行連結與 SHA-256。
- 原生繪圖與互動變更必須由 Windows portable EXE 實機驗證，不能只以格式檢查或前端建置視為完成。
