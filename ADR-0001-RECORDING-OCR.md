# ADR-0001：錄影與 OCR 技術路徑

> 狀態：採用（PoC-D）  
> 日期：2026-08-01

## 決策

1. 錄影影像來源採 `Windows.Graphics.Capture` 與 Direct3D 11 frame pool。
2. MP4 預設以 Windows Media Foundation H.264 編碼，不將 FFmpeg 綁入核心 portable EXE。
3. GIF 為選配匯出器；若採 FFmpeg，使用未啟用 GPL/nonfree 的 LGPL 組建並獨立發佈。
4. OCR 第一層採 Windows `Windows.Media.Ocr`，依電腦已安裝語言套件處理繁中、英文與數字。
5. 後續以固定測試集比較繁中品質；若 Windows OCR 未達門檻，再增加可下載的本機 OCR 引擎，不膨脹基本 EXE。

## 依據

- Microsoft 官方文件說明 `Windows.Graphics.Capture` 可取得螢幕或應用視窗的 frame，並提供 `GraphicsCaptureSession.IsSupported()` 能力檢查：<https://learn.microsoft.com/windows/uwp/audio-video-camera/screen-capture>
- Microsoft 的螢幕錄影路徑是 Graphics Capture frame pool、MediaStreamSource 與 MediaTranscoder：<https://learn.microsoft.com/windows/uwp/audio-video-camera/screen-capture-video>
- Media Foundation H.264 encoder 可輸出 H.264，並在適用時優先使用已認證硬體編碼器：<https://learn.microsoft.com/windows/win32/medfound/h-264-video-encoder>
- `Windows.Media.Ocr.OcrEngine` 提供已安裝辨識語言、最大圖片尺寸及帶座標的行與單字結果：<https://learn.microsoft.com/uwp/api/windows.media.ocr.ocrengine>
- FFmpeg 核心為 LGPL 2.1+，但啟用特定選項或組件後可轉為 GPL；官方建議 Windows LGPL 依循時採 DLL 動態連結並提供對應原始碼：<https://ffmpeg.org/legal.html>

## 可執行驗證

PoC-D 主畫面新增「檢查錄影與 OCR 能力」，即時回報：

- Windows Graphics Capture 是否支援。
- Windows OCR 是否可用。
- 繁體中文與英文 OCR 語言包是否安裝。
- 繁體中文語言標籤需接受 `zh-Hant`、`zh-Hant-*`、`zh-TW*` 與 `zh-HK*` 格式。
- OCR 引擎支援的最大圖片邊長與所有可用語言。

## 尚待後續實測

- 用固定繁中、英文、數字圖片集計算字元正確率與行順序正確率。
- 實際錄製 1080p/30、1440p/30 與跨螢幕區域，記錄 CPU、GPU、記憶體與掉幀。
- 在 Intel、AMD 與 NVIDIA 環境列舉實際 Media Foundation H.264 hardware MFT。
- 比較 MP4 與 GIF 的體積、色彩、幀率及產出時間。
