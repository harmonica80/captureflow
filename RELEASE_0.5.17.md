# CaptureFlow 0.5.17

- 在框選畫面雙擊已放置的編號或文字，或選取後按 F2，即可重新編輯。編號可直接輸入新數字。文字可按 Ctrl+A 後重新輸入。Enter 套用，Esc 放棄這次修改。
- 長截圖模式只顯示確認、長截圖、取消和釘選。標註在長截圖期間暫停，切回一般框選可繼續編輯。
- 一般框選在圖釘與矩形工具之間加上分隔線。

## 驗證

- 前端建置、12 項瀏覽器回歸測試、Rust 格式檢查與差異檢查通過。
- 編輯提示使用不透明深底 #252525 與前景 #FAFAFA，靜態對比約 14.69:1。工具列沿用原色。
- Windows EXE 實測通過。雙擊可把編號改為 27，F2 重新輸入後按 Esc 保留 27。雙擊可把文字改成「第二段」，並可復原與重做。兩個物件均保持可編輯，底圖不含標註。
- 實機確認長截圖只顯示四個按鈕，點原矩形工具的位置不會啟動繪圖。切回一般框選後，工具列、分隔線及原本的標註恢復。測試還原並核對 250 份原有資料檔案的 SHA-256。

## 測試版下載

- [Windows 免安裝測試版 ZIP](https://github.com/harmonica80/captureflow/actions/runs/45635505472/artifacts/15032952960) · [GitHub Actions 建置紀錄](https://github.com/harmonica80/captureflow/actions/runs/45635505472)
- GitHub 下載需要登入，成品保存至 2027 年 2 月 23 日 22:34（臺灣時間）。
- ZIP 內為 `CaptureFlow-portable.exe`，版本 0.5.17，來源提交 `a6caa3e`，大小 6,730,240 bytes。
- EXE 的 SHA-256 為 `D23D59E97D8C9F247FCBEF184B4437E45FF0E8A15A276FDF4618D08FB9A0E33F`，已與 Actions 紀錄核對。
