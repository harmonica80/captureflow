# CaptureFlow 下次開發待辦

記錄日期：2026-08-02

## 原生標註工具待修正項目

1. 向量與抗鋸齒品質
   - 目前矩形、圓形、箭頭及文字外框可看見明顯鋸齒。
   - 下一階段須改用具抗鋸齒能力的向量繪圖管線，避免以傳統 GDI 多邊形或逐像素線段作為最終品質。
   - 螢幕預覽與輸出 PNG 必須使用一致的幾何資料與平滑效果。

2. 文字工具游標
   - 選取文字工具並移入可輸入區域時，游標須切換為 Windows 常見的 I-beam 文字游標。
   - 移到既有文字物件與控制點時，仍須依操作切換成移動或縮放游標。

3. 文字物件編輯
   - 完成文字輸入後須顯示可辨識的物件外框與控制點。
   - 文字物件須能重新選取、移動及編輯內容。
   - 滑鼠滾輪須能調整文字大小，並顯示目前字級。
   - 加入清楚的文字外框設定，包括外框顏色、開關與適當的粗細控制；文字色與外框色須分開設定。

4. PixPin 風格工具列
   - 目前工具列圖示仍未達到需求，列為下一次最高優先修正項目。
   - 圖示須依使用者提供的 PixPin 參考圖重新設計：細線向量、統一視覺尺寸、留白、分組、下拉指示與選取狀態。
   - 不接受臨時字元、粗糙折線或僅功能可用但視覺不一致的圖示。
   - 完成後須逐一比較矩形、圓形、箭頭、文字、顏色、復原、完成及取消圖示。

## 驗收方式

- 使用 Windows portable EXE，在 100%、125% 與 150% DPI 下檢查預覽與輸出 PNG。
- 放大檢查斜線、曲線、圓弧、箭頭頭部及文字外框是否平滑。
- 驗證文字 I-beam、物件外框、控制點、移動、滾輪字級及文字／外框雙色設定。
- 工具列須與使用者提供的 PixPin 參考圖並排比較後再交付。
- 所有文字及狀態仍須符合專案 WCAG AA 4.5:1 準則。

## Keyviz 參考實作（2026-08-03 補充）

使用者確認 CaptureFlow 可直接參考先前共同開發的 Keyviz 多螢幕版本：

- Repository／Release：`https://github.com/harmonica80/keyviz-multi-monitor/releases`
- 本機完整原始碼：`D:\個人資料\ChatGPT測試資料夾\outputs\keyviz-current-work`
- 工具與向量圖示定義：`src/lib/drawing-tools.tsx`
  - 使用 Lucide SVG 圖示，包括 `Pencil`、`ArrowUpRight`、`Square`、`Circle`、`Type`、`Eraser` 與物件選取圖示。
  - CaptureFlow 工具列應沿用相同 24×24 viewBox、`currentColor`、一致線寬與圓角端點的設計語言，不再自行以 GDI 折線臨摹圖示。
- 抗鋸齒繪圖與物件資料：`src/pages/screen-drawing.tsx`
  - Canvas 依 `devicePixelRatio` 建立高解析度 backing store。
  - 使用 `lineCap = "round"`、`lineJoin = "round"`、Canvas path／ellipse／fillText，作為平滑預覽基礎。
  - `drawTaperedArrow` 已有實心漸寬箭頭幾何，可移植並再接上 CaptureFlow 的中央曲率控制點。
  - 文字使用 Microsoft JhengHei，字級由 width 映射，編輯期間支援滾輪調整。
- 工具列狀態：`src/stores/drawing_toolbar.ts`
  - 可重用工具清單、顯示順序、正規化與持久化方式。
- 原生透明繪圖層：`src-tauri/src/app/native_drawing.rs`
  - 可參考多螢幕、DPI、透明疊加視窗及 premultiplied alpha 處理。

導入原則：先移植 Keyviz 已驗證的 SVG／Canvas 繪圖層與工具列元件，再保留 CaptureFlow 的截圖選取框、物件控制點、可編輯專案及輸出流程；不可只複製外觀而繼續使用會產生鋸齒的 GDI 最終繪圖。

## 長截圖畫面閃爍（2026-08-05 補充）

- 現況：CaptureFlow 0.3.1 的手動長截圖已可正常拼接，但使用者每向下捲動一次，原生選取遮罩會隱藏、重新擷取並再次顯示，因此畫面會閃爍一次。
- 預期：長截圖啟用後，桌面上的原始截圖畫面、選取框、工具列及右側累積預覽都應保持穩定；使用者捲動時在背景取得新畫面並更新拼接結果，不應讓整個遮罩視窗消失或重繪閃爍。
- 後續技術方向：避免在每次捲動時對選取視窗執行 `SW_HIDE`／`SW_SHOW`；評估使用 Windows 擷取排除機制，或將固定預覽與實際擷取來源分離，讓擷取程序不包含 CaptureFlow 自身的遮罩視窗。
- 驗收：在 Chrome、Edge、檔案總管及一般可捲動視窗連續滾動至少 10 次，選取框與工具列不得閃爍，右側長圖預覽仍須逐段更新且不得把遮罩工具列截入結果。
