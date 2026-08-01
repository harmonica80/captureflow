import "./App.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

type RectInfo = { x: number; y: number; width: number; height: number };
type MonitorInfo = {
  deviceName: string;
  bounds: RectInfo;
  dpiX: number;
  dpiY: number;
  scaleFactor: number;
  isPrimary: boolean;
};
type DesktopSnapshot = {
  imagePath: string;
  metadataPath: string;
  virtualDesktop: RectInfo;
  monitors: MonitorInfo[];
};
type SelectionSnapshot = {
  imagePath: string;
  metadataPath: string;
  selection: RectInfo;
  width: number;
  height: number;
};
type CapabilityReport = {
  windowsGraphicsCapture: boolean;
  windowsOcr: boolean;
  traditionalChineseOcr: boolean;
  englishOcr: boolean;
  ocrMaxImageDimension: number;
  ocrLanguages: { languageTag: string; displayName: string }[];
  recordingPath: string;
  mp4Encoder: string;
  gifEncoder: string;
};

function App() {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<DesktopSnapshot | null>(null);
  const [error, setError] = useState("");
  const [selecting, setSelecting] = useState(false);
  const [selection, setSelection] = useState<SelectionSnapshot | null>(null);
  const [openingSticker, setOpeningSticker] = useState(false);
  const [checkingCapabilities, setCheckingCapabilities] = useState(false);
  const [capabilities, setCapabilities] = useState<CapabilityReport | null>(null);

  useEffect(() => {
    const stopSelection = listen<SelectionSnapshot>("captureflow://selection-complete", (event) => {
      setSelection(event.payload);
      setError("");
    });
    const stopError = listen<string>("captureflow://selection-error", (event) => {
      setError(event.payload);
    });
    return () => {
      void stopSelection.then((unlisten) => unlisten());
      void stopError.then((unlisten) => unlisten());
    };
  }, []);

  async function runPoc() {
    setRunning(true);
    setError("");
    try {
      setResult(await invoke<DesktopSnapshot>("capture_virtual_desktop"));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRunning(false);
    }
  }

  async function runSelector() {
    setSelecting(true);
    setError("");
    try {
      const selected = await invoke<SelectionSnapshot | null>("select_screen_area");
      if (selected) setSelection(selected);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSelecting(false);
    }
  }

  async function openSticker() {
    if (!selection) return;
    setOpeningSticker(true);
    setError("");
    try {
      await invoke("open_sticker", {
        imagePath: selection.imagePath,
        x: selection.selection.x,
        y: selection.selection.y,
      });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setOpeningSticker(false);
    }
  }

  async function runDiagnostics() {
    setCheckingCapabilities(true);
    setError("");
    try {
      setCapabilities(await invoke<CapabilityReport>("run_capability_diagnostics"));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setCheckingCapabilities(false);
    }
  }

  return (
    <main className="app-shell">
      <section className="hero">
        <span className="eyebrow">CAPTUREFLOW · POC-C</span>
        <h1>讓每一次截圖，都能繼續工作。</h1>
        <p>
          Windows 本機優先的螢幕截取、物件式標註、置頂貼圖與視覺工作流工具。
        </p>
        <div className="status"><i />PoC-C：原生置頂貼圖</div>
        <p className="shortcut-hint"><kbd>Alt</kbd><span>+</span><kbd>Shift</kbd><span>+</span><kbd>A</kbd> 可在任何程式中開始框選截圖</p>
        <div className="action-row">
          <button className="capture-button primary" onClick={runSelector} disabled={selecting}>
            {selecting ? "選取模式執行中…" : "開始框選螢幕範圍"}
          </button>
          <button className="capture-button" onClick={runPoc} disabled={running}>
            {running ? "正在擷取…" : "執行完整桌面快照"}
          </button>
          <button className="capture-button" onClick={runDiagnostics} disabled={checkingCapabilities}>
            {checkingCapabilities ? "檢查中…" : "檢查錄影與 OCR 能力"}
          </button>
        </div>
      </section>

      {capabilities && (
        <section className="result-panel capability-result" aria-live="polite">
          <div className="result-heading">
            <div><span>POC-D CAPABILITY REPORT</span><h2>錄影與 OCR 環境檢查</h2></div>
            <strong>{capabilities.windowsGraphicsCapture ? "可用" : "不可用"}</strong>
          </div>
          <div className="capability-grid">
            <div><b>Windows Graphics Capture</b><span>{capabilities.windowsGraphicsCapture ? "支援" : "不支援"}</span></div>
            <div><b>繁體中文 OCR</b><span>{capabilities.traditionalChineseOcr ? "已安裝" : "未安裝"}</span></div>
            <div><b>英文 OCR</b><span>{capabilities.englishOcr ? "已安裝" : "未安裝"}</span></div>
            <div><b>OCR 最大圖片邊長</b><span>{capabilities.ocrMaxImageDimension}px</span></div>
          </div>
          <p className="path"><b>擷取</b>{capabilities.recordingPath}</p>
          <p className="path"><b>MP4</b>{capabilities.mp4Encoder}</p>
          <p className="path"><b>GIF</b>{capabilities.gifEncoder}</p>
          <div className="language-list" aria-label="已安裝 OCR 語言">
            {capabilities.ocrLanguages.map((language) => (
              <span key={language.languageTag}>{language.displayName} · {language.languageTag}</span>
            ))}
          </div>
        </section>
      )}

      {selection && (
        <section className="result-panel selection-result" aria-live="polite">
          <div className="result-heading">
            <div><span>SELECTION COMPLETE</span><h2>選取圖片已輸出</h2></div>
            <strong>{selection.width} × {selection.height}</strong>
          </div>
          <p className="selection-coordinates">
            全域座標 {selection.selection.x}, {selection.selection.y} · {selection.width} × {selection.height}
          </p>
          <p className="path"><b>PNG</b>{selection.imagePath}</p>
          <p className="path"><b>JSON</b>{selection.metadataPath}</p>
          <div className="sticker-actions">
            <button className="capture-button primary" onClick={openSticker} disabled={openingSticker}>
              {openingSticker ? "正在建立…" : "建立置頂貼圖"}
            </button>
            <span>自動定位工具列 · 拖曳移動 · 游標中心縮放 · Ctrl + 滾輪透明度</span>
          </div>
        </section>
      )}

      {(result || error) && (
        <section className={`result-panel ${error ? "error" : ""}`} aria-live="polite">
          {error ? (
            <><h2>擷取失敗</h2><p>{error}</p></>
          ) : result && (
            <>
              <div className="result-heading">
                <div><span>CAPTURE COMPLETE</span><h2>已擷取 {result.monitors.length} 部顯示器</h2></div>
                <strong>{result.virtualDesktop.width} × {result.virtualDesktop.height}</strong>
              </div>
              <div className="monitor-list">
                {result.monitors.map((monitor) => (
                  <div key={monitor.deviceName}>
                    <b>{monitor.deviceName}{monitor.isPrimary ? " · 主要" : ""}</b>
                    <span>{monitor.bounds.x}, {monitor.bounds.y} · {monitor.bounds.width} × {monitor.bounds.height}</span>
                    <span>{monitor.dpiX} DPI · {Math.round(monitor.scaleFactor * 100)}%</span>
                  </div>
                ))}
              </div>
              <p className="path"><b>PNG</b>{result.imagePath}</p>
              <p className="path"><b>JSON</b>{result.metadataPath}</p>
            </>
          )}
        </section>
      )}

      <section className="roadmap" aria-label="首版開發路線">
        <article className="active">
          <span>01</span><h2>精準截圖</h2>
          <p>多螢幕、混合 DPI、視窗偵測與可調整擷取範圍。</p>
        </article>
        <article>
          <span>02</span><h2>物件式標註</h2>
          <p>箭頭、文字、編號與不破壞底圖的遮蔽工具。</p>
        </article>
        <article>
          <span>03</span><h2>貼圖與歷史</h2>
          <p>置頂參考、重新裁切、搜尋與擷取配方。</p>
        </article>
      </section>
    </main>
  );
}

export default App;
