import { Image } from "@tauri-apps/api/image";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

type Point = { x: number; y: number };
type Tool = "select" | "rectangle" | "arrow";
type Annotation =
  | { id: string; type: "rectangle"; x: number; y: number; width: number; height: number; radius?: number; color: string; strokeWidth: number }
  | { id: string; type: "arrow"; x1: number; y1: number; x2: number; y2: number; cx?: number; cy?: number; color: string; strokeWidth: number };
type Interaction = { id: string; mode: "move" | "nw" | "ne" | "sw" | "se" | "start" | "end" | "curve" | "radius"; origin: Point; object: Annotation };
type Project = { schemaVersion: number; sourceImage: string; canvasWidth: number; canvasHeight: number; objects: Annotation[] };
type Props = { imagePath: string; width: number; height: number; onClose: () => void };

function arrowPolygon(x1: number, y1: number, x2: number, y2: number, strokeWidth: number, cx = (x1 + x2) / 2, cy = (y1 + y2) / 2) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const length = Math.max(1, Math.hypot(dx, dy));
  const headLength = Math.min(length * 0.45, strokeWidth * 6);
  const tailHalf = Math.max(0.7, strokeWidth * 0.22);
  const neckHalf = strokeWidth * 1.15;
  const headHalf = strokeWidth * 3.5;
  const endTangentX = x2 - cx;
  const endTangentY = y2 - cy;
  const endTangentLength = Math.max(1, Math.hypot(endTangentX, endTangentY));
  const ux = endTangentX / endTangentLength;
  const uy = endTangentY / endTangentLength;
  const px = -uy;
  const py = ux;
  const neckX = x2 - ux * headLength;
  const neckY = y2 - uy * headLength;
  const left: number[][] = [];
  const right: number[][] = [];
  for (let index = 0; index <= 12; index += 1) {
    const t = index / 12;
    const inverse = 1 - t;
    const x = inverse * inverse * x1 + 2 * inverse * t * cx + t * t * neckX;
    const y = inverse * inverse * y1 + 2 * inverse * t * cy + t * t * neckY;
    const tx = 2 * inverse * (cx - x1) + 2 * t * (neckX - cx);
    const ty = 2 * inverse * (cy - y1) + 2 * t * (neckY - cy);
    const tangentLength = Math.max(1, Math.hypot(tx, ty));
    const half = tailHalf + (neckHalf - tailHalf) * t;
    left.push([x - ty / tangentLength * half, y + tx / tangentLength * half]);
    right.push([x + ty / tangentLength * half, y - tx / tangentLength * half]);
  }
  return [...left, [neckX + px * headHalf, neckY + py * headHalf], [x2, y2], [neckX - px * headHalf, neckY - py * headHalf], ...right.reverse()]
    .map(([x, y]) => `${x},${y}`).join(" ");
}

function movedObject(object: Annotation, mode: Interaction["mode"], dx: number, dy: number): Annotation {
  if (object.type === "arrow") {
    if (mode === "start") return { ...object, x1: object.x1 + dx, y1: object.y1 + dy };
    if (mode === "end") return { ...object, x2: object.x2 + dx, y2: object.y2 + dy };
    if (mode === "curve") return { ...object, cx: (object.cx ?? (object.x1 + object.x2) / 2) + dx, cy: (object.cy ?? (object.y1 + object.y2) / 2) + dy };
    return { ...object, x1: object.x1 + dx, y1: object.y1 + dy, x2: object.x2 + dx, y2: object.y2 + dy, cx: (object.cx ?? (object.x1 + object.x2) / 2) + dx, cy: (object.cy ?? (object.y1 + object.y2) / 2) + dy };
  }
  if (mode === "radius") return { ...object, radius: Math.max(0, Math.min(Math.min(object.width, object.height) / 2, (object.radius ?? 0) + Math.max(dx, dy))) };
  if (mode === "move") return { ...object, x: object.x + dx, y: object.y + dy };
  const left = mode === "nw" || mode === "sw" ? object.x + dx : object.x;
  const right = mode === "ne" || mode === "se" ? object.x + object.width + dx : object.x + object.width;
  const top = mode === "nw" || mode === "ne" ? object.y + dy : object.y;
  const bottom = mode === "sw" || mode === "se" ? object.y + object.height + dy : object.y + object.height;
  return { ...object, x: Math.min(left, right), y: Math.min(top, bottom), width: Math.max(3, Math.abs(right - left)), height: Math.max(3, Math.abs(bottom - top)) };
}

export default function AnnotationEditor({ imagePath, width, height, onClose }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tool, setTool] = useState<Tool>("select");
  const [objects, setObjects] = useState<Annotation[]>([]);
  const [undoStack, setUndoStack] = useState<Annotation[][]>([]);
  const [redoStack, setRedoStack] = useState<Annotation[][]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [start, setStart] = useState<Point | null>(null);
  const [current, setCurrent] = useState<Point | null>(null);
  const [interaction, setInteraction] = useState<Interaction | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    function handleShortcut(event: KeyboardEvent) {
      if (event.ctrlKey && event.key.toLowerCase() === "z") {
        event.preventDefault();
        if (event.shiftKey) redoLast(); else undo();
        return;
      }
      if (event.ctrlKey && event.key.toLowerCase() === "y") {
        event.preventDefault();
        redoLast();
        return;
      }
      if ((event.key !== "Delete" && event.key !== "Backspace") || !selectedId) return;
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, textarea, [contenteditable='true']")) return;
      event.preventDefault();
      recordChange(objects);
      setObjects((items) => items.filter((item) => item.id !== selectedId));
      setSelectedId(null);
    }
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, [objects, redoStack, selectedId, undoStack]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const source = await Image.fromPath(imagePath);
      try {
        const rgba = await source.rgba();
        if (cancelled || !canvasRef.current) return;
        const context = canvasRef.current.getContext("2d");
        if (!context) throw new Error("無法建立標註畫布");
        context.putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
      } catch (reason) { if (!cancelled) setError(String(reason)); }
      finally { await source.close(); if (!cancelled) setLoading(false); }
    })();
    return () => { cancelled = true; };
  }, [imagePath, width, height]);

  function pointFromEvent(event: React.PointerEvent<SVGSVGElement>): Point {
    const bounds = event.currentTarget.getBoundingClientRect();
    return { x: Math.max(0, Math.min(width, (event.clientX - bounds.left) * width / bounds.width)), y: Math.max(0, Math.min(height, (event.clientY - bounds.top) * height / bounds.height)) };
  }
  function recordChange(previous: Annotation[]) { setUndoStack((items) => [...items, previous]); setRedoStack([]); }
  function beginInteraction(event: React.PointerEvent<SVGElement>, object: Annotation, mode: Interaction["mode"]) {
    event.stopPropagation();
    const svg = event.currentTarget.ownerSVGElement;
    if (!svg) return;
    svg.setPointerCapture(event.pointerId);
    const bounds = svg.getBoundingClientRect();
    const origin = {
      x: Math.max(0, Math.min(width, (event.clientX - bounds.left) * width / bounds.width)),
      y: Math.max(0, Math.min(height, (event.clientY - bounds.top) * height / bounds.height)),
    };
    setSelectedId(object.id);
    setTool(object.type);
    setInteraction({ id: object.id, mode, origin, object: { ...object } });
    setMessage("");
  }
  function pointerDown(event: React.PointerEvent<SVGSVGElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    if (tool === "select") { setSelectedId(null); return; }
    const point = pointFromEvent(event); setStart(point); setCurrent(point); setMessage("");
  }
  function pointerMove(event: React.PointerEvent<SVGSVGElement>) {
    const point = pointFromEvent(event);
    if (interaction) {
      const dx = point.x - interaction.origin.x; const dy = point.y - interaction.origin.y;
      setObjects((items) => items.map((item) => item.id === interaction.id ? movedObject(interaction.object, interaction.mode, dx, dy) : item));
    } else if (start) setCurrent(point);
  }
  function pointerUp(event: React.PointerEvent<SVGSVGElement>) {
    if (interaction) { recordChange(objects.map((item) => item.id === interaction.id ? interaction.object : item)); setInteraction(null); return; }
    if (!start || tool === "select") return;
    const end = pointFromEvent(event);
    if (Math.hypot(end.x - start.x, end.y - start.y) >= 3) {
      const id = `${Date.now()}-${objects.length}`;
      const next: Annotation = tool === "rectangle"
        ? { id, type: "rectangle", x: Math.min(start.x, end.x), y: Math.min(start.y, end.y), width: Math.abs(end.x - start.x), height: Math.abs(end.y - start.y), radius: 0, color: "#ff3b30", strokeWidth: 4 }
        : { id, type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, cx: (start.x + end.x) / 2, cy: (start.y + end.y) / 2, color: "#ff3b30", strokeWidth: 4 };
      recordChange(objects); setObjects([...objects, next]); setSelectedId(next.id);
    }
    setStart(null); setCurrent(null);
  }
  function undo() { const previous = undoStack[undoStack.length - 1]; if (!previous) return; setRedoStack((items) => [...items, objects]); setObjects(previous); setUndoStack((items) => items.slice(0, -1)); setSelectedId(null); }
  function redoLast() { const next = redoStack[redoStack.length - 1]; if (!next) return; setUndoStack((items) => [...items, objects]); setObjects(next); setRedoStack((items) => items.slice(0, -1)); setSelectedId(null); }
  function removeSelected() { if (!selectedId) return; recordChange(objects); setObjects(objects.filter((item) => item.id !== selectedId)); setSelectedId(null); }
  function adjustStroke(event: React.WheelEvent<SVGSVGElement>) {
    const id = hoveredId ?? selectedId;
    if (!id) return;
    event.preventDefault();
    const previous = objects;
    const step = event.deltaY < 0 ? 1 : -1;
    const next = objects.map((item) => item.id === id ? { ...item, strokeWidth: Math.max(1, Math.min(32, item.strokeWidth + step)) } : item);
    if (next.every((item, index) => item.strokeWidth === previous[index].strokeWidth)) return;
    recordChange(previous); setObjects(next); setSelectedId(id);
  }
  async function loadProject() {
    setError(""); setMessage("");
    try {
      const project = await invoke<Project>("load_latest_annotation_project", { imagePath });
      if (project.canvasWidth !== width || project.canvasHeight !== height) throw new Error("專案畫布尺寸與目前截圖不符。");
      setObjects(project.objects); setUndoStack([]); setRedoStack([]); setSelectedId(null); setTool("select");
      setMessage(`已重新開啟最近專案，共 ${project.objects.length} 個物件。`);
    } catch (reason) { setError(String(reason)); }
  }
  async function saveProject() {
    setSaving(true); setError(""); setMessage("");
    try { const path = await invoke<string>("save_annotation_project", { imagePath, canvasWidth: width, canvasHeight: height, objects }); setMessage(`可編輯專案已儲存：${path}`); }
    catch (reason) { setError(String(reason)); } finally { setSaving(false); }
  }

  const draft = start && current ? { start, end: current } : null;
  const selected = objects.find((object) => object.id === selectedId);
  const handle = (x: number, y: number, mode: Interaction["mode"], object: Annotation) => <rect className="annotation-handle" x={x - 6} y={y - 6} width="12" height="12" rx="2" onPointerDown={(event) => beginInteraction(event, object, mode)} />;
  return (
    <section className="annotation-editor" aria-labelledby="annotation-title">
      <div className="annotation-heading"><div><span>PHASE 2.2</span><h2 id="annotation-title">專案重開與物件操作</h2></div><button className="capture-button" onClick={onClose}>關閉編輯器</button></div>
      <div className="annotation-toolbar" aria-label="標註工具列">
        <button className={tool === "select" ? "selected" : ""} aria-pressed={tool === "select"} onClick={() => setTool("select")}>⌖ 選取</button>
        <button className={tool === "rectangle" ? "selected" : ""} aria-pressed={tool === "rectangle"} onClick={() => setTool("rectangle")}>▭ 矩形</button>
        <button className={tool === "arrow" ? "selected" : ""} aria-pressed={tool === "arrow"} onClick={() => setTool("arrow")}>↗ 箭頭</button>
        <button onClick={undo} disabled={undoStack.length === 0}>↶ 復原</button><button onClick={redoLast} disabled={redoStack.length === 0}>↷ 重做</button><button onClick={removeSelected} disabled={!selectedId}>⌫ 刪除選取</button>
        <button onClick={() => { recordChange(objects); setObjects([]); setSelectedId(null); }} disabled={objects.length === 0}>清除全部</button>
        <button onClick={loadProject}>重新開啟最近專案</button><button className="save" onClick={saveProject} disabled={saving}>{saving ? "儲存中…" : "儲存可編輯專案"}</button>
      </div>
      <p className="annotation-help">目前工具：{tool === "select" ? "選取" : tool === "rectangle" ? "矩形" : "箭頭"} · {selected ? `已選取 ${selected.type === "rectangle" ? "矩形" : "箭頭"}，滾輪調整線寬 ${selected.strokeWidth}px` : "游標移到物件會顯示外框，點擊後可編輯"} · Ctrl+Z 復原 · 共 {objects.length} 個物件</p>
      <div className="annotation-stage" style={{ aspectRatio: `${width} / ${height}` }}><canvas ref={canvasRef} width={width} height={height} aria-label="截圖底圖" />
        {!loading && <svg className={tool === "select" ? "selecting" : "drawing"} viewBox={`0 0 ${width} ${height}`} onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onWheel={adjustStroke} aria-label="標註畫布">
          {objects.map((object) => object.type === "rectangle"
            ? <rect key={object.id} x={object.x} y={object.y} width={object.width} height={object.height} rx={object.radius ?? 0} fill="transparent" stroke={object.color} strokeWidth={object.strokeWidth} onPointerEnter={() => setHoveredId(object.id)} onPointerLeave={() => setHoveredId((id) => id === object.id ? null : id)} onPointerDown={(event) => beginInteraction(event, object, "move")} />
            : <polygon key={object.id} points={arrowPolygon(object.x1, object.y1, object.x2, object.y2, object.strokeWidth, object.cx, object.cy)} fill={object.color} stroke="transparent" strokeWidth="14" onPointerEnter={() => setHoveredId(object.id)} onPointerLeave={() => setHoveredId((id) => id === object.id ? null : id)} onPointerDown={(event) => beginInteraction(event, object, "move")} />)}
          {objects.filter((object) => object.id === hoveredId && object.id !== selectedId).map((object) => object.type === "rectangle"
            ? <rect key={`hover-${object.id}`} className="annotation-hover" x={object.x - 4} y={object.y - 4} width={object.width + 8} height={object.height + 8} rx={(object.radius ?? 0) + 4} />
            : <path key={`hover-${object.id}`} className="annotation-hover" d={`M ${object.x1} ${object.y1} Q ${object.cx ?? (object.x1 + object.x2) / 2} ${object.cy ?? (object.y1 + object.y2) / 2} ${object.x2} ${object.y2}`} />)}
          {selected?.type === "rectangle" && <g aria-label="已選取矩形"><rect className="annotation-selection" x={selected.x - 3} y={selected.y - 3} width={selected.width + 6} height={selected.height + 6} rx={selected.radius ?? 0} />{handle(selected.x, selected.y, "nw", selected)}{handle(selected.x + selected.width, selected.y, "ne", selected)}{handle(selected.x, selected.y + selected.height, "sw", selected)}{handle(selected.x + selected.width, selected.y + selected.height, "se", selected)}<circle className="annotation-radius-handle" cx={selected.x + Math.max(12, selected.radius ?? 0)} cy={selected.y + Math.max(12, selected.radius ?? 0)} r="6" onPointerDown={(event) => beginInteraction(event, selected, "radius")} /></g>}
          {selected?.type === "arrow" && <g aria-label="已選取箭頭"><path className="annotation-selection" d={`M ${selected.x1} ${selected.y1} Q ${selected.cx ?? (selected.x1 + selected.x2) / 2} ${selected.cy ?? (selected.y1 + selected.y2) / 2} ${selected.x2} ${selected.y2}`} />{handle(selected.x1, selected.y1, "start", selected)}{handle(selected.x2, selected.y2, "end", selected)}<circle className="annotation-curve-handle" cx={selected.cx ?? (selected.x1 + selected.x2) / 2} cy={selected.cy ?? (selected.y1 + selected.y2) / 2} r="7" onPointerDown={(event) => beginInteraction(event, selected, "curve")} /></g>}
          {draft && tool === "rectangle" && <rect x={Math.min(draft.start.x, draft.end.x)} y={Math.min(draft.start.y, draft.end.y)} width={Math.abs(draft.end.x - draft.start.x)} height={Math.abs(draft.end.y - draft.start.y)} fill="none" stroke="#ff3b30" strokeWidth="4" strokeDasharray="10 6" />}
          {draft && tool === "arrow" && <polygon points={arrowPolygon(draft.start.x, draft.start.y, draft.end.x, draft.end.y, 4)} fill="#ff3b30" opacity="0.82" />}
        </svg>}{loading && <div className="annotation-loading">正在載入底圖…</div>}</div>
      {message && <p className="settings-success" role="status">✓ {message}</p>}{error && <p className="settings-error" role="alert">錯誤：{error}</p>}
    </section>
  );
}
