import "./App.css";
import AnnotationEditor, { type AnnotationObject } from "./AnnotationEditor";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Image } from "@tauri-apps/api/image";
import { join } from "@tauri-apps/api/path";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

type RectInfo = { x: number; y: number; width: number; height: number };
type MonitorInfo = {
  deviceName: string;
  bounds: RectInfo;
  scaleFactor: number;
  isPrimary: boolean;
};
type SelectionSnapshot = {
  imagePath: string;
  metadataPath: string;
  selection: RectInfo;
  width: number;
  height: number;
  cornerRadius?: number;
};
type SettingsView = {
  captureShortcut: string;
  defaultShortcut: string;
  logPath: string;
  historyLimit: number;
  defaultSaveDirectory: string;
  launchAtStartup: boolean;
  language: "zh-TW" | "en";
};
type Project = {
  sourceImage: string;
  canvasWidth: number;
  canvasHeight: number;
  objects: AnnotationObject[];
};
type HistoryEntry = {
  projectPath: string;
  imagePath: string;
  createdAtUnixMs: number;
  canvasWidth: number;
  canvasHeight: number;
  selection?: RectInfo;
  thumbnailDataUrl?: string;
};

const RELEASES_URL = "https://github.com/harmonica80/captureflow/releases";
type Language = "zh-TW" | "en";
const copy = {
  "zh-TW": {
    title: "擷圖與標註工作區", theme: "色系主題", light: "淺色", dark: "深色",
    create: "建立擷圖", createHelp: "桌面模式只負責選取範圍；完成後會自動回到右側編輯器。",
    select: "框選螢幕範圍", selecting: "框選中…", openProject: "開啟 JSON 專案",
    history: "歷史擷圖記錄", captureItem: "擷圖", monitors: "擷取整個螢幕", screen: "螢幕", current: "目前圖片",
    preferences: "偏好設定", shortcut: "快捷鍵", globalShortcut: "全域框選快捷鍵", apply: "套用", reset: "預設",
    historyCount: "保留記錄數量", clearHistory: "清除歷史擷圖記錄", saveImage: "圖片儲存",
    saveFolder: "預設儲存圖檔資料夾", chooseFolder: "選擇資料夾…", startup: "系統啟動時自動執行",
    language: "介面語言", traditionalChinese: "繁體中文", english: "英文", savePreferences: "儲存偏好設定",
    emptyTitle: "尚未載入擷圖", emptyHelp: "可從左側框選螢幕、選擇螢幕，或直接開啟以前的 JSON 專案。",
    start: "開始框選", closeTitle: "關閉 CaptureFlow", closeQuestion: "您要最小化視窗還是結束應用程式？",
    minimize: "最小化視窗", quit: "結束應用程式", cancel: "取消", deleteHistory: "刪除這筆記錄", deleteHistoryConfirm: "確定要刪除這筆擷圖歷史及其自動儲存檔案嗎？",
  },
  en: {
    title: "Screenshot & Annotation Workspace", theme: "Theme", light: "Light", dark: "Dark",
    create: "New Capture", createHelp: "Select on the desktop, then continue editing in this window.",
    select: "Select Screen Area", selecting: "Selecting…", openProject: "Open JSON Project",
    history: "Capture History", captureItem: "Capture", monitors: "Capture Full Display", screen: "Display", current: "Current Image",
    preferences: "Preferences", shortcut: "Shortcut", globalShortcut: "Global capture shortcut", apply: "Apply", reset: "Default",
    historyCount: "History limit", clearHistory: "Clear capture history", saveImage: "Image Storage",
    saveFolder: "Default image folder", chooseFolder: "Choose folder…", startup: "Run when Windows starts",
    language: "Language", traditionalChinese: "Traditional Chinese", english: "English", savePreferences: "Save Preferences",
    emptyTitle: "No screenshot loaded", emptyHelp: "Select an area, capture a display, or open an existing JSON project.",
    start: "Start Capture", closeTitle: "Close CaptureFlow", closeQuestion: "Would you like to minimize the window or quit the application?",
    minimize: "Minimize Window", quit: "Quit Application", cancel: "Cancel", deleteHistory: "Delete this entry", deleteHistoryConfirm: "Delete this capture history entry and its auto-saved files?",
  },
} as const;

export default function App() {
  const [theme, setTheme] = useState<"light" | "dark">(() =>
    localStorage.getItem("captureflow-theme") === "dark" ? "dark" : "light",
  );
  const [selection, setSelection] = useState<SelectionSnapshot | null>(null),
    [objects, setObjects] = useState<AnnotationObject[]>([]),
    [editorKey, setEditorKey] = useState(0);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]),
    [history, setHistory] = useState<HistoryEntry[]>([]),
    [historyOpen, setHistoryOpen] = useState(false);
  const [settings, setSettings] = useState<SettingsView | null>(null),
    [shortcutDraft, setShortcutDraft] = useState(""),
    [historyLimitDraft, setHistoryLimitDraft] = useState(20),
    [saveDirectoryDraft, setSaveDirectoryDraft] = useState(""),
    [startupDraft, setStartupDraft] = useState(false),
    [languageDraft, setLanguageDraft] = useState<Language>("zh-TW"),
    [version, setVersion] = useState("0.1.0"),
    [updateVersion, setUpdateVersion] = useState("");
  const [busy, setBusy] = useState(""),
    [message, setMessage] = useState(""),
    [error, setError] = useState("");
  const [closePrompt, setClosePrompt] = useState(false);
  const language: Language = settings?.language ?? languageDraft;
  const t = copy[language];

  async function refreshHistory() {
    try {
      setHistory(await invoke<HistoryEntry[]>("list_capture_history"));
    } catch (reason) {
      setError(String(reason));
    }
  }
  function receiveSelection(captured: SelectionSnapshot) {
    setSelection(captured);
    setObjects([]);
    setEditorKey((k) => k + 1);
    setMessage(`擷圖已載入編輯器，並加入最近 ${settings?.historyLimit ?? 20} 筆歷史`);
    setError("");
    setTimeout(() => void refreshHistory(), 900);
  }

  useEffect(() => {
    void getVersion()
      .then((value) => {
        setVersion(value);
        document.title = `CaptureFlow ${value}`;
        const last = Number(
          localStorage.getItem("captureflow-update-check") || 0,
        );
        if (Date.now() - last > 86_400_000) {
          localStorage.setItem("captureflow-update-check", String(Date.now()));
          fetch(
            "https://api.github.com/repos/harmonica80/captureflow/releases/latest",
            { headers: { Accept: "application/vnd.github+json" } },
          )
            .then((r) => (r.ok ? r.json() : null))
            .then((data) => {
              const latest = String(data?.tag_name || "").replace(/^v/, "");
              if (latest && latest !== value) setUpdateVersion(latest);
            })
            .catch(() => {});
        }
      })
      .catch(() => {});
    void invoke<SettingsView>("get_settings")
      .then((value) => {
        setSettings(value);
        setShortcutDraft(value.captureShortcut);
        setHistoryLimitDraft(value.historyLimit);
        setSaveDirectoryDraft(value.defaultSaveDirectory);
        setStartupDraft(value.launchAtStartup);
        setLanguageDraft(value.language);
        document.documentElement.lang = value.language;
      })
      .catch((reason) => setError(String(reason)));
    void invoke<MonitorInfo[]>("list_monitors")
      .then(setMonitors)
      .catch((reason) => setError(String(reason)));
    void refreshHistory();
    const completed = listen<SelectionSnapshot>(
      "captureflow://selection-complete",
      (event) => receiveSelection(event.payload),
    );
    const failed = listen<string>("captureflow://selection-error", (event) =>
      setError(event.payload),
    );
    return () => {
      void completed.then((fn) => fn());
      void failed.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    const listener = appWindow.onCloseRequested((event) => {
      event.preventDefault();
      setClosePrompt(true);
    });
    return () => { void listener.then((unlisten) => unlisten()); };
  }, []);

  async function loadProjectPath(projectPath: string) {
    setBusy("project");
    setError("");
    try {
      const p = await invoke<Project>("load_annotation_project_from", {
        projectPath,
      });
      setSelection({
        imagePath: p.sourceImage,
        metadataPath: projectPath,
        selection: { x: 0, y: 0, width: p.canvasWidth, height: p.canvasHeight },
        width: p.canvasWidth,
        height: p.canvasHeight,
      });
      setObjects(p.objects);
      setEditorKey((k) => k + 1);
      setMessage("已開啟包含來源圖片的可編輯專案");
      setHistoryOpen(false);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy("");
    }
  }
  async function openProject() {
    try {
      const path = await open({
        title: "開啟 CaptureFlow JSON 專案",
        multiple: false,
        directory: false,
        filters: [{ name: "CaptureFlow JSON 專案", extensions: ["json"] }],
      });
      if (path) await loadProjectPath(path);
    } catch (reason) {
      setError(String(reason));
    }
  }
  async function runCapture(kind: "area" | "monitor", deviceName?: string) {
    setBusy(kind === "monitor" ? (deviceName ?? "monitor") : kind);
    setError("");
    try {
      const appWindow = getCurrentWindow();
      if (kind === "area") {
        await appWindow.minimize();
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      }
      const result =
        kind === "area"
          ? await invoke<SelectionSnapshot | null>("select_screen_area")
          : await invoke<SelectionSnapshot>("capture_monitor", { deviceName });
      if (result) receiveSelection(result);
    } catch (reason) {
      setError(String(reason));
    } finally {
      if (kind === "area") {
        const appWindow = getCurrentWindow();
        await appWindow.unminimize();
        await appWindow.setFocus();
      }
      setBusy("");
    }
  }
  async function copySelection(path?: string) {
    if (!selection) return;
    try {
      const image = await Image.fromPath(path ?? selection.imagePath);
      try {
        await writeImage(image);
      } finally {
        await image.close();
      }
      setMessage("圖片已複製到剪貼簿");
    } catch (reason) {
      setError(String(reason));
    }
  }
  async function saveSelection(path?: string) {
    if (!selection) return;
    try {
      const destination = await save({
        title: "儲存 CaptureFlow 圖片",
        defaultPath: settings?.defaultSaveDirectory
          ? await join(settings.defaultSaveDirectory, "CaptureFlow.png")
          : "CaptureFlow.png",
        filters: [
          { name: "PNG 圖片", extensions: ["png"] },
          { name: "JPEG 圖片", extensions: ["jpg", "jpeg"] },
          { name: "WebP 圖片", extensions: ["webp"] },
        ],
      });
      if (destination) {
        await invoke("export_selection", {
          imagePath: path ?? selection.imagePath,
          destination,
        });
        setMessage(`已儲存：${destination}`);
      }
    } catch (reason) {
      setError(String(reason));
    }
  }
  async function openSticker(path?: string) {
    if (!selection) return;
    try {
      await invoke("open_sticker", {
        imagePath: path ?? selection.imagePath,
        x: selection.selection.x,
        y: selection.selection.y,
      });
      setMessage("已建立置頂貼圖");
    } catch (reason) {
      setError(String(reason));
    }
  }
  async function applyShortcut(shortcut: string) {
    try {
      const updated = await invoke<SettingsView>("update_capture_shortcut", {
        shortcut,
      });
      setSettings(updated);
      setShortcutDraft(updated.captureShortcut);
      setMessage(`快捷鍵已更新為 ${updated.captureShortcut}`);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function chooseSaveDirectory() {
    const directory = await open({ title: "選擇預設圖檔資料夾", directory: true, multiple: false });
    if (directory) setSaveDirectoryDraft(directory);
  }
  async function applyPreferences() {
    try {
      const updated = await invoke<SettingsView>("update_preferences", {
        historyLimit: historyLimitDraft,
        defaultSaveDirectory: saveDirectoryDraft,
        launchAtStartup: startupDraft,
        language: languageDraft,
      });
      setSettings(updated);
      setLanguageDraft(updated.language);
      setStartupDraft(updated.launchAtStartup);
      document.documentElement.lang = updated.language;
      await refreshHistory();
      setMessage("偏好設定已儲存");
      setError("");
    } catch (reason) { setError(String(reason)); }
  }
  async function clearHistory() {
    try {
      await invoke("clear_capture_history");
      setHistory([]);
      setMessage("歷史擷圖記錄已清除；手動另存的專案不受影響");
      setError("");
    } catch (reason) { setError(String(reason)); }
  }
  async function deleteHistoryEntry(item: HistoryEntry) {
    if (!window.confirm(t.deleteHistoryConfirm)) return;
    try {
      await invoke("delete_capture_history_entry", { projectPath: item.projectPath });
      setHistory((entries) => entries.filter((entry) => entry.projectPath !== item.projectPath));
      if (selection?.metadataPath === item.projectPath) {
        setSelection(null);
        setObjects([]);
        setEditorKey((key) => key + 1);
      }
      setMessage(t.deleteHistory);
      setError("");
    } catch (reason) {
      setError(String(reason));
    }
  }
  async function revealCurrentImage() {
    if (!selection) return;
    try { await invoke("reveal_file", { imagePath: selection.imagePath }); }
    catch (reason) { setError(String(reason)); }
  }

  return (
    <main className={`app-shell workspace-shell theme-${theme}`}>
      <header className="app-header">
        <div>
          <span className="eyebrow">CAPTUREFLOW</span>
          <h1>{t.title}</h1>
        </div>
        <div className="header-controls">
          <label className="theme-picker">{t.theme}<select value={theme} onChange={(event) => { const value = event.target.value as "light" | "dark"; setTheme(value); localStorage.setItem("captureflow-theme", value); }}><option value="light">{t.light}</option><option value="dark">{t.dark}</option></select></label>
          <div className="header-shortcut"><span>CaptureFlow {version}</span><strong>{settings?.captureShortcut ?? "Alt+Shift+A"}</strong></div>
        </div>
      </header>
      {updateVersion && (
        <aside className="update-notice" role="status">
          <span>發現新版 CaptureFlow {updateVersion}</span>
          <a href={RELEASES_URL} target="_blank" rel="noreferrer">
            查看版本
          </a>
          <button onClick={() => setUpdateVersion("")}>稍後提醒</button>
        </aside>
      )}
      <div className="workspace-layout">
        <aside className="capture-sidebar" aria-label="擷取控制">
          <section className="sidebar-card">
            <h2>{t.create}</h2>
            <p>{t.createHelp}</p>
            <button
              className="capture-button primary"
              onClick={() => runCapture("area")}
              disabled={!!busy}
            >
              {busy === "area" ? t.selecting : t.select}
            </button>
            <button
              className="capture-button"
              onClick={openProject}
              disabled={!!busy}
            >
              {t.openProject}
            </button>
            <button
              className="capture-button history-toggle"
              onClick={() => setHistoryOpen((v) => !v)}
              aria-expanded={historyOpen}
            >
              <span aria-hidden="true">{historyOpen ? "▼" : "▶"}</span>
              {t.history} ({history.length})
            </button>
            {historyOpen && <div className="history-card">
              <h3>最近 {settings?.historyLimit ?? 20} 筆</h3>
              {history.length === 0 ? (
                <p>尚無擷圖歷史。</p>
              ) : (
                history.map((item, index) => {
                  const capturedAt = new Date(item.createdAtUnixMs);
                  return <div className="history-entry-row" key={item.projectPath}>
                    <button
                      className="history-entry"
                      onClick={() => loadProjectPath(item.projectPath)}
                      title={capturedAt.toLocaleString(language)}
                    >
                      {item.thumbnailDataUrl ? (
                        <img src={item.thumbnailDataUrl} alt="" loading="lazy" />
                      ) : (
                        <span className="history-thumbnail-placeholder" aria-hidden="true">▧</span>
                      )}
                      <span className="history-entry-details">
                        <strong>{t.captureItem} {index + 1}</strong>
                        <small>{item.canvasWidth} × {item.canvasHeight}</small>
                        <time dateTime={capturedAt.toISOString()}>
                          {capturedAt.toLocaleDateString(language)}<br />
                          {capturedAt.toLocaleTimeString(language, { hour: "2-digit", minute: "2-digit", second: "2-digit" })}
                        </time>
                      </span>
                    </button>
                    <button
                      type="button"
                      className="history-delete"
                      onClick={() => void deleteHistoryEntry(item)}
                      title={t.deleteHistory}
                      aria-label={`${t.deleteHistory}：${t.captureItem} ${index + 1}`}
                    >
                      X
                    </button>
                  </div>;
                })
              )}
            </div>}
          </section>
          <section className="sidebar-card monitor-card">
            <h2>{t.monitors}</h2>
            <div className="monitor-buttons">
              {monitors.map((monitor, index) => (
                <button
                  key={monitor.deviceName}
                  onClick={() => runCapture("monitor", monitor.deviceName)}
                  disabled={!!busy}
                  title={monitor.deviceName}
                >
                  {t.screen} {index + 1}
                </button>
              ))}
            </div>
          </section>
          {selection && (
            <button className="sidebar-card current-image-card" onClick={revealCurrentImage} title="在檔案總管顯示目前圖片">
              <span>{t.current}</span>
              <strong>
                {selection.width} × {selection.height}
              </strong>
              <small title={selection.imagePath}>{selection.imagePath}</small>
            </button>
          )}
          {settings && (
            <details className="sidebar-card settings-card">
              <summary>{t.preferences}</summary>
              <h3>{t.shortcut}</h3>
              <label htmlFor="capture-shortcut">{t.globalShortcut}</label>
              <input
                id="capture-shortcut"
                value={shortcutDraft}
                onChange={(e) => setShortcutDraft(e.target.value)}
              />
              <div className="compact-actions">
                <button
                  className="capture-button primary"
                  onClick={() => applyShortcut(shortcutDraft)}
                >
                  {t.apply}
                </button>
                <button
                  className="capture-button"
                  onClick={() => applyShortcut(settings.defaultShortcut)}
                >
                  {t.reset}
                </button>
              </div>
              <h3>{t.history}</h3>
              <label htmlFor="history-limit">{t.historyCount}</label>
              <input id="history-limit" type="number" min="1" max="100" value={historyLimitDraft} onChange={(event) => setHistoryLimitDraft(Number(event.target.value))}/>
              <button className="capture-button danger" onClick={clearHistory}>{t.clearHistory}</button>
              <h3>{t.saveImage}</h3>
              <label>{t.saveFolder}</label>
              <button className="folder-picker" onClick={chooseSaveDirectory} title={saveDirectoryDraft || t.chooseFolder}>{saveDirectoryDraft || t.chooseFolder}</button>
              <label className="check-setting"><input type="checkbox" checked={startupDraft} onChange={(event) => setStartupDraft(event.target.checked)} />{t.startup}</label>
              <label htmlFor="language-setting">{t.language}</label>
              <select id="language-setting" value={languageDraft} onChange={(event) => setLanguageDraft(event.target.value as Language)}>
                <option value="zh-TW">{t.traditionalChinese}</option>
                <option value="en">{t.english}</option>
              </select>
              <button className="capture-button primary" onClick={applyPreferences}>{t.savePreferences}</button>
            </details>
          )}
          {(message || error) && (
            <section
              className={`sidebar-notice ${error ? "error" : ""}`}
              role={error ? "alert" : "status"}
            >
              <strong>{error ? "發生錯誤" : "目前狀態"}</strong>
              <span>{error || message}</span>
            </section>
          )}
        </aside>
        <section className="editor-workspace" aria-label="圖片編輯工作區">
          {selection ? (
            <AnnotationEditor
              key={editorKey}
              imagePath={selection.imagePath}
              width={selection.width}
              height={selection.height}
              initialObjects={objects}
              onCopy={copySelection}
              onSave={saveSelection}
              onSticker={openSticker}
              onClose={() => setSelection(null)}
              onStatus={(status, isError) => {
                isError
                  ? (setError(status), setMessage(""))
                  : (setMessage(status), setError(""));
              }}
              language={language}
            />
          ) : (
            <div className="empty-workspace">
              <div className="empty-icon">＋</div>
              <h2>{t.emptyTitle}</h2>
              <p>{t.emptyHelp}</p>
              <button
                className="capture-button primary"
                onClick={() => runCapture("area")}
              >
                {t.start}
              </button>
            </div>
          )}
        </section>
      </div>
      <footer className="app-footer">
        <span>述文老師開發</span>
        <a
          href="https://harmonica80.blogspot.com/"
          target="_blank"
          rel="noreferrer"
        >
          述文老師部落格
        </a>
        <a
          href="https://github.com/harmonica80/captureflow"
          target="_blank"
          rel="noreferrer"
        >
          CaptureFlow GitHub 專案
        </a>
      </footer>
      {closePrompt && (
        <div className="close-dialog-backdrop" role="presentation">
          <section className="close-dialog" role="dialog" aria-modal="true" aria-labelledby="close-dialog-title">
            <h2 id="close-dialog-title">{t.closeTitle}</h2>
            <p>{t.closeQuestion}</p>
            <div className="close-dialog-actions">
              <button className="capture-button primary" onClick={async () => { setClosePrompt(false); await getCurrentWindow().hide(); }}>{t.minimize}</button>
              <button className="capture-button danger" onClick={() => void getCurrentWindow().destroy()}>{t.quit}</button>
              <button className="capture-button" onClick={() => setClosePrompt(false)}>{t.cancel}</button>
            </div>
          </section>
        </div>
      )}
    </main>
  );
}
