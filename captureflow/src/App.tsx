import "./App.css";
import AnnotationEditor, { type AnnotationObject } from "./AnnotationEditor";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Image } from "@tauri-apps/api/image";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

type RectInfo = { x: number; y: number; width: number; height: number };
type MonitorInfo = { deviceName: string; bounds: RectInfo; scaleFactor: number; isPrimary: boolean };
type SelectionSnapshot = { imagePath: string; metadataPath: string; selection: RectInfo; width: number; height: number; cornerRadius?: number };
type SettingsView = { captureShortcut: string; defaultShortcut: string; logPath: string };
type Project = { sourceImage: string; canvasWidth: number; canvasHeight: number; objects: AnnotationObject[] };
type HistoryEntry = { projectPath: string; imagePath: string; createdAtUnixMs: number; canvasWidth: number; canvasHeight: number; selection?: RectInfo };

const RELEASES_URL = "https://github.com/harmonica80/captureflow/releases";

export default function App() {
  const [selection,setSelection]=useState<SelectionSnapshot|null>(null),[objects,setObjects]=useState<AnnotationObject[]>([]),[editorKey,setEditorKey]=useState(0);
  const [monitors,setMonitors]=useState<MonitorInfo[]>([]),[history,setHistory]=useState<HistoryEntry[]>([]),[historyOpen,setHistoryOpen]=useState(false);
  const [settings,setSettings]=useState<SettingsView|null>(null),[shortcutDraft,setShortcutDraft]=useState(""),[version,setVersion]=useState("0.1.0"),[updateVersion,setUpdateVersion]=useState("");
  const [busy,setBusy]=useState(""),[message,setMessage]=useState(""),[error,setError]=useState("");

  async function refreshHistory(){try{setHistory(await invoke<HistoryEntry[]>("list_capture_history"))}catch(reason){setError(String(reason))}}
  function receiveSelection(captured:SelectionSnapshot){setSelection(captured);setObjects([]);setEditorKey(k=>k+1);setMessage("截圖已載入編輯器，並加入最近 20 筆歷史");setError("");setTimeout(()=>void refreshHistory(),900)}

  useEffect(()=>{
    void getVersion().then(value=>{
      setVersion(value);document.title=`CaptureFlow ${value}`;
      const last=Number(localStorage.getItem("captureflow-update-check")||0);
      if(Date.now()-last>86_400_000){localStorage.setItem("captureflow-update-check",String(Date.now()));fetch("https://api.github.com/repos/harmonica80/captureflow/releases/latest",{headers:{Accept:"application/vnd.github+json"}}).then(r=>r.ok?r.json():null).then(data=>{const latest=String(data?.tag_name||"").replace(/^v/,"");if(latest&&latest!==value)setUpdateVersion(latest)}).catch(()=>{})}
    }).catch(()=>{});
    void invoke<SettingsView>("get_settings").then(value=>{setSettings(value);setShortcutDraft(value.captureShortcut)}).catch(reason=>setError(String(reason)));
    void invoke<MonitorInfo[]>("list_monitors").then(setMonitors).catch(reason=>setError(String(reason)));
    void refreshHistory();
    const completed=listen<SelectionSnapshot>("captureflow://selection-complete",event=>receiveSelection(event.payload));
    const failed=listen<string>("captureflow://selection-error",event=>setError(event.payload));
    return()=>{void completed.then(fn=>fn());void failed.then(fn=>fn())};
  },[]);

  async function loadProjectPath(projectPath:string){setBusy("project");setError("");try{const p=await invoke<Project>("load_annotation_project_from",{projectPath});setSelection({imagePath:p.sourceImage,metadataPath:projectPath,selection:{x:0,y:0,width:p.canvasWidth,height:p.canvasHeight},width:p.canvasWidth,height:p.canvasHeight});setObjects(p.objects);setEditorKey(k=>k+1);setMessage("已開啟包含來源圖片的可編輯專案");setHistoryOpen(false)}catch(reason){setError(String(reason))}finally{setBusy("")}}
  async function openProject(){try{const path=await open({title:"開啟 CaptureFlow JSON 專案",multiple:false,directory:false,filters:[{name:"CaptureFlow JSON 專案",extensions:["json"]}]});if(path)await loadProjectPath(path)}catch(reason){setError(String(reason))}}
  async function runCapture(kind:"area"|"monitor",deviceName?:string){setBusy(kind==="monitor"?deviceName??"monitor":kind);setError("");try{const result=kind==="area"?await invoke<SelectionSnapshot|null>("select_screen_area"):await invoke<SelectionSnapshot>("capture_monitor",{deviceName});if(result)receiveSelection(result)}catch(reason){setError(String(reason))}finally{setBusy("")}}
  async function copySelection(path?:string){if(!selection)return;try{const image=await Image.fromPath(path??selection.imagePath);try{await writeImage(image)}finally{await image.close()}setMessage("圖片已複製到剪貼簿")}catch(reason){setError(String(reason))}}
  async function saveSelection(path?:string){if(!selection)return;try{const destination=await save({title:"儲存 CaptureFlow 圖片",defaultPath:"CaptureFlow.png",filters:[{name:"PNG 圖片",extensions:["png"]},{name:"JPEG 圖片",extensions:["jpg","jpeg"]},{name:"WebP 圖片",extensions:["webp"]}]});if(destination){await invoke("export_selection",{imagePath:path??selection.imagePath,destination});setMessage(`已儲存：${destination}`)}}catch(reason){setError(String(reason))}}
  async function openSticker(path?:string){if(!selection)return;try{await invoke("open_sticker",{imagePath:path??selection.imagePath,x:selection.selection.x,y:selection.selection.y});setMessage("已建立置頂貼圖")}catch(reason){setError(String(reason))}}
  async function applyShortcut(shortcut:string){try{const updated=await invoke<SettingsView>("update_capture_shortcut",{shortcut});setSettings(updated);setShortcutDraft(updated.captureShortcut);setMessage(`快捷鍵已更新為 ${updated.captureShortcut}`)}catch(reason){setError(String(reason))}}

  return <main className="app-shell workspace-shell">
    <header className="app-header"><div><span className="eyebrow">CAPTUREFLOW</span><h1>截圖與標註工作區</h1></div><div className="header-shortcut"><span>CaptureFlow {version}</span><strong>{settings?.captureShortcut??"Alt+Shift+A"}</strong></div></header>
    {updateVersion&&<aside className="update-notice" role="status"><span>發現新版 CaptureFlow {updateVersion}</span><a href={RELEASES_URL} target="_blank" rel="noreferrer">查看版本</a><button onClick={()=>setUpdateVersion("")}>稍後提醒</button></aside>}
    <div className="workspace-layout"><aside className="capture-sidebar" aria-label="擷取控制">
      <section className="sidebar-card"><h2>建立截圖</h2><p>桌面模式只負責選取範圍；完成後會自動回到右側編輯器。</p><button className="capture-button primary" onClick={()=>runCapture("area")} disabled={!!busy}>{busy==="area"?"框選中…":"框選螢幕範圍"}</button><button className="capture-button" onClick={openProject} disabled={!!busy}>開啟 JSON 專案</button><button className="capture-button" onClick={()=>setHistoryOpen(v=>!v)}>歷史截圖範圍 ({history.length})</button></section>
      {historyOpen&&<section className="sidebar-card history-card"><h2>最近 20 筆</h2>{history.length===0?<p>尚無截圖歷史。</p>:history.map((item,index)=><button key={item.projectPath} onClick={()=>loadProjectPath(item.projectPath)} title={new Date(item.createdAtUnixMs).toLocaleString()}><span>截圖 {index+1}</span><small>{item.canvasWidth} × {item.canvasHeight} · {new Date(item.createdAtUnixMs).toLocaleString()}</small></button>)}</section>}
      <section className="sidebar-card monitor-card"><h2>擷取整個螢幕</h2><div className="monitor-buttons">{monitors.map((monitor,index)=><button key={monitor.deviceName} onClick={()=>runCapture("monitor",monitor.deviceName)} disabled={!!busy} title={monitor.deviceName}>螢幕 {index+1}</button>)}</div></section>
      {selection&&<section className="sidebar-card current-image-card"><span>目前圖片</span><strong>{selection.width} × {selection.height}</strong><small title={selection.imagePath}>{selection.imagePath}</small></section>}
      {settings&&<details className="sidebar-card settings-card"><summary>快捷鍵設定</summary><label htmlFor="capture-shortcut">全域框選快捷鍵</label><input id="capture-shortcut" value={shortcutDraft} onChange={e=>setShortcutDraft(e.target.value)}/><div className="compact-actions"><button className="capture-button primary" onClick={()=>applyShortcut(shortcutDraft)}>套用</button><button className="capture-button" onClick={()=>applyShortcut(settings.defaultShortcut)}>預設</button></div></details>}
      {(message||error)&&<section className={`sidebar-notice ${error?"error":""}`} role={error?"alert":"status"}><strong>{error?"發生錯誤":"目前狀態"}</strong><span>{error||message}</span></section>}
    </aside><section className="editor-workspace" aria-label="圖片編輯工作區">{selection?<AnnotationEditor key={editorKey} imagePath={selection.imagePath} width={selection.width} height={selection.height} initialObjects={objects} onCopy={copySelection} onSave={saveSelection} onSticker={openSticker} onClose={()=>setSelection(null)} onStatus={(status,isError)=>{isError?(setError(status),setMessage("")):(setMessage(status),setError(""))}}/>:<div className="empty-workspace"><div className="empty-icon">＋</div><h2>尚未載入截圖</h2><p>可從左側框選螢幕、選擇螢幕，或直接開啟以前的 JSON 專案。</p><button className="capture-button primary" onClick={()=>runCapture("area")}>開始框選</button></div>}</section></div>
    <footer className="app-footer"><span>述文老師開發</span><a href="https://harmonica80.blogspot.com/" target="_blank" rel="noreferrer">述文老師部落格</a><a href="https://github.com/harmonica80/captureflow" target="_blank" rel="noreferrer">CaptureFlow GitHub 專案</a></footer>
  </main>;
}
