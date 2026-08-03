import "./App.css";
import AnnotationEditor from "./AnnotationEditor";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Image } from "@tauri-apps/api/image";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

type RectInfo = { x: number; y: number; width: number; height: number };
type MonitorInfo = { deviceName: string; bounds: RectInfo; scaleFactor: number; isPrimary: boolean };
type SelectionSnapshot = { imagePath: string; metadataPath: string; selection: RectInfo; width: number; height: number };
type SettingsView = { captureShortcut: string; defaultShortcut: string; logPath: string };

export default function App() {
  const [selection, setSelection] = useState<SelectionSnapshot | null>(null);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [shortcutDraft, setShortcutDraft] = useState("");
  const [busy, setBusy] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  function receiveSelection(captured: SelectionSnapshot) {
    setSelection(captured);
    setMessage("截圖已載入編輯器");
    setError("");
  }

  useEffect(() => {
    void invoke<SettingsView>("get_settings").then((value) => {
      setSettings(value);
      setShortcutDraft(value.captureShortcut);
    }).catch((reason) => setError(String(reason)));
    void invoke<MonitorInfo[]>("list_monitors").then(setMonitors).catch((reason) => setError(String(reason)));
    const completed = listen<SelectionSnapshot>("captureflow://selection-complete", (event) => receiveSelection(event.payload));
    const failed = listen<string>("captureflow://selection-error", (event) => setError(event.payload));
    return () => {
      void completed.then((unlisten) => unlisten());
      void failed.then((unlisten) => unlisten());
    };
  }, []);

  async function runCapture(kind: "area" | "repeat" | "monitor", deviceName?: string) {
    setBusy(kind === "monitor" ? deviceName ?? "monitor" : kind);
    setError(""); setMessage("");
    try {
      const result = kind === "area"
        ? await invoke<SelectionSnapshot | null>("select_screen_area")
        : kind === "repeat"
          ? await invoke<SelectionSnapshot>("repeat_last_selection")
          : await invoke<SelectionSnapshot>("capture_monitor", { deviceName });
      if (result) receiveSelection(result);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(""); }
  }

  async function copySelection() {
    if (!selection) return;
    setError("");
    try {
      const image = await Image.fromPath(selection.imagePath);
      try { await writeImage(image); } finally { await image.close(); }
      setMessage("圖片已複製到剪貼簿");
    } catch (reason) { setError(String(reason)); }
  }

  async function saveSelection() {
    if (!selection) return;
    setBusy("save"); setError("");
    try {
      const destination = await save({
        title: "儲存 CaptureFlow 圖片",
        defaultPath: selection.imagePath.split(/[\\/]/).pop() ?? "CaptureFlow.png",
        filters: [{ name: "PNG 圖片", extensions: ["png"] }, { name: "JPEG 圖片", extensions: ["jpg", "jpeg"] }, { name: "WebP 圖片", extensions: ["webp"] }],
      });
      if (destination) {
        await invoke("export_selection", { imagePath: selection.imagePath, destination });
        setMessage(`已儲存：${destination}`);
      }
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(""); }
  }

  async function openSticker() {
    if (!selection) return;
    setBusy("sticker"); setError("");
    try {
      await invoke("open_sticker", { imagePath: selection.imagePath, x: selection.selection.x, y: selection.selection.y });
      setMessage("已建立置頂貼圖");
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(""); }
  }

  async function applyShortcut(shortcut: string) {
    setBusy("settings"); setError("");
    try {
      const updated = await invoke<SettingsView>("update_capture_shortcut", { shortcut });
      setSettings(updated); setShortcutDraft(updated.captureShortcut);
      setMessage(`快捷鍵已更新為 ${updated.captureShortcut}`);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(""); }
  }

  return <main className="app-shell workspace-shell">
    <header className="app-header">
      <div><span className="eyebrow">CAPTUREFLOW</span><h1>截圖與標註工作區</h1></div>
      <div className="header-shortcut"><span>全域快捷鍵</span><strong>{settings?.captureShortcut ?? "Alt+Shift+A"}</strong></div>
    </header>

    <div className="workspace-layout">
      <aside className="capture-sidebar" aria-label="擷取控制">
        <section className="sidebar-card">
          <h2>建立截圖</h2>
          <p>桌面模式只負責選取範圍；完成後會自動回到右側編輯器。</p>
          <button className="capture-button primary" onClick={() => runCapture("area")} disabled={busy !== ""}>{busy === "area" ? "框選中…" : "框選螢幕範圍"}</button>
          <button className="capture-button" onClick={() => runCapture("repeat")} disabled={busy !== ""}>{busy === "repeat" ? "擷取中…" : "重複上次範圍"}</button>
        </section>

        <section className="sidebar-card monitor-card">
          <h2>整個螢幕</h2>
          {monitors.map((monitor) => <button key={monitor.deviceName} onClick={() => runCapture("monitor", monitor.deviceName)} disabled={busy !== ""}>
            <span>{monitor.deviceName}{monitor.isPrimary ? " · 主要" : ""}</span>
            <small>{monitor.bounds.width} × {monitor.bounds.height} · {Math.round(monitor.scaleFactor * 100)}%</small>
          </button>)}
        </section>

        {settings && <details className="sidebar-card settings-card">
          <summary>快捷鍵設定</summary>
          <label htmlFor="capture-shortcut">全域框選快捷鍵</label>
          <input id="capture-shortcut" value={shortcutDraft} onChange={(event) => setShortcutDraft(event.target.value)} disabled={busy === "settings"} />
          <div className="compact-actions">
            <button className="capture-button primary" onClick={() => applyShortcut(shortcutDraft)}>套用</button>
            <button className="capture-button" onClick={() => applyShortcut(settings.defaultShortcut)}>預設</button>
          </div>
        </details>}
      </aside>

      <section className="editor-workspace" aria-label="圖片編輯工作區">
        {selection ? <>
          <div className="workspace-heading">
            <div><span>目前圖片</span><h2>{selection.width} × {selection.height}</h2><small>{selection.imagePath}</small></div>
            <div className="output-actions">
              <button onClick={copySelection}>複製</button>
              <button onClick={saveSelection} disabled={busy === "save"}>{busy === "save" ? "儲存中…" : "另存圖片"}</button>
              <button className="accent" onClick={openSticker} disabled={busy === "sticker"}>{busy === "sticker" ? "建立中…" : "置頂貼圖"}</button>
              <button onClick={() => setSelection(null)}>關閉</button>
            </div>
          </div>
          <AnnotationEditor imagePath={selection.imagePath} width={selection.width} height={selection.height} />
        </> : <div className="empty-workspace">
          <div className="empty-icon" aria-hidden="true">＋</div>
          <h2>尚未載入截圖</h2>
          <p>從左側框選範圍或選擇一部螢幕，即可開始物件式標註。</p>
          <button className="capture-button primary" onClick={() => runCapture("area")}>開始框選</button>
        </div>}
      </section>
    </div>
    {(message || error) && <div className={`app-toast ${error ? "error" : ""}`} role={error ? "alert" : "status"}>{error || message}</div>}
  </main>;
}
