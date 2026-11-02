# CaptureFlow 0.5.12：頁尾創用 CC 授權資訊

- 在視窗下方新增官方 CC BY-NC-SA 4.0 標章與完整授權文字。
- 標章與文字連至 https://creativecommons.org/licenses/by-nc-sa/4.0/deed.zh-hant 。
- 標章隨程式打包，離線仍可顯示；授權文字提供繁體中文及英文，頁尾可換行。

## 驗證

- TypeScript／Vite 正式建置、Rust 格式及 Git 差異檢查通過。
- 10 項既有 Playwright 回歸測試通過。
- 以瀏覽器實際渲染 900px 寬主視窗，確認標章載入、頁尾無水平溢出，點擊連結開啟指定授權頁。
- 新增文字的一般、hover、active、focus 狀態皆達 4.5:1：淺色 #075f3b／#eef2f0 為 6.86:1；hover／active #064b30 為 9.04:1；深色 #9bdcff／#07110d 為 12.85:1。頁尾位置不在背景漸層的有色區域，已檢視實際渲染結果；標章為不透明黑白圖。
- 此授權連結沒有 disabled、selected 或 error 狀態；連結保留底線，並提供鍵盤焦點框。
- 本次為頁尾介面變更，未更動原生擷取、繪圖或編輯流程；這些功能未重新進行實機完整驗收。

## 免安裝交付

待 GitHub Actions 成品與公開下載驗證完成後補記。
