import { Image } from "@tauri-apps/api/image";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

type Point = { x: number; y: number };
type Tool = "rectangle" | "ellipse" | "pen" | "arrow" | "text" | "number" | "mosaic" | "eraser";
type Base = { id: string; color: string; strokeWidth: number };
type BoxObject = Base & { type: "rectangle" | "ellipse" | "mosaic"; x: number; y: number; width: number; height: number; radius?: number };
type ArrowObject = Base & { type: "arrow"; x1: number; y1: number; x2: number; y2: number; cx: number; cy: number };
type PenObject = Base & { type: "pen"; points: Point[] };
type TextObject = Base & { type: "text"; x: number; y: number; text: string; fontSize: number; outlineColor: string; outlineWidth: number };
type NumberObject = Base & { type: "number"; x: number; y: number; number: number; size: number };
type Annotation = BoxObject | ArrowObject | PenObject | TextObject | NumberObject;
type Project = { canvasWidth: number; canvasHeight: number; objects: Annotation[] };
type Props = { imagePath: string; width: number; height: number };

const colors = ["#ff3b30", "#111111", "#ffffff", "#ffb000", "#ffe033", "#28c76f", "#1687ff", "#8a5cf6", "#9b9b9b", "#9bdcff"];
const labels: Record<Tool, string> = { rectangle: "矩形", ellipse: "圓形", pen: "畫筆", arrow: "箭頭", text: "文字", number: "序號", mosaic: "馬賽克", eraser: "橡皮擦" };

function Icon({ type }: { type: Tool }) {
  const common = { fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  return <svg viewBox="0 0 24 24" aria-hidden="true">
    {type === "rectangle" && <rect x="4" y="5" width="16" height="14" rx="1" {...common} />}
    {type === "ellipse" && <ellipse cx="12" cy="12" rx="8" ry="7" {...common} />}
    {type === "pen" && <><path d="M4 20l4.2-1 10.9-10.9-3.2-3.2L5 15.8 4 20z" {...common}/><path d="M14.8 6l3.2 3.2" {...common}/></>}
    {type === "arrow" && <><path d="M5 19L19 5" {...common}/><path d="M10 5h9v9" {...common}/></>}
    {type === "text" && <><path d="M5 5h14M12 5v14" {...common}/></>}
    {type === "number" && <><circle cx="12" cy="12" r="8" {...common}/><path d="M10 9l2-1v8" {...common}/></>}
    {type === "mosaic" && <><rect x="4" y="4" width="6" height="6" {...common}/><rect x="14" y="4" width="6" height="6" {...common}/><rect x="4" y="14" width="6" height="6" {...common}/><rect x="14" y="14" width="6" height="6" {...common}/></>}
    {type === "eraser" && <><path d="M5 16l8-10 6 5-7 9H8l-3-4z" {...common}/><path d="M10 20h9" {...common}/></>}
  </svg>;
}

function arrowPolygon(object: ArrowObject) {
  const { x1, y1, x2, y2, cx, cy, strokeWidth } = object;
  const tangent = Math.max(1, Math.hypot(x2 - cx, y2 - cy));
  const ux = (x2 - cx) / tangent, uy = (y2 - cy) / tangent, px = -uy, py = ux;
  const head = Math.min(Math.hypot(x2 - x1, y2 - y1) * .42, strokeWidth * 6);
  const neckX = x2 - ux * head, neckY = y2 - uy * head;
  const left: Point[] = [], right: Point[] = [];
  for (let index = 0; index <= 16; index++) {
    const t = index / 16, inv = 1 - t;
    const x = inv * inv * x1 + 2 * inv * t * cx + t * t * neckX;
    const y = inv * inv * y1 + 2 * inv * t * cy + t * t * neckY;
    const tx = 2 * inv * (cx - x1) + 2 * t * (neckX - cx), ty = 2 * inv * (cy - y1) + 2 * t * (neckY - cy);
    const length = Math.max(1, Math.hypot(tx, ty)), half = .8 + strokeWidth * t;
    left.push({ x: x - ty / length * half, y: y + tx / length * half });
    right.push({ x: x + ty / length * half, y: y - tx / length * half });
  }
  return [...left, { x: neckX + px * strokeWidth * 3.4, y: neckY + py * strokeWidth * 3.4 }, { x: x2, y: y2 }, { x: neckX - px * strokeWidth * 3.4, y: neckY - py * strokeWidth * 3.4 }, ...right.reverse()].map((point) => `${point.x},${point.y}`).join(" ");
}

function boundsOf(object: Annotation) {
  if (object.type === "rectangle" || object.type === "ellipse" || object.type === "mosaic") return { x: object.x, y: object.y, width: object.width, height: object.height };
  if (object.type === "arrow") return { x: Math.min(object.x1, object.x2, object.cx), y: Math.min(object.y1, object.y2, object.cy), width: Math.max(object.x1, object.x2, object.cx) - Math.min(object.x1, object.x2, object.cx), height: Math.max(object.y1, object.y2, object.cy) - Math.min(object.y1, object.y2, object.cy) };
  if (object.type === "pen") {
    const xs = object.points.map((p) => p.x), ys = object.points.map((p) => p.y);
    return { x: Math.min(...xs), y: Math.min(...ys), width: Math.max(...xs) - Math.min(...xs), height: Math.max(...ys) - Math.min(...ys) };
  }
  if (object.type === "text") return { x: object.x, y: object.y - object.fontSize, width: Math.max(object.fontSize, object.text.length * object.fontSize * .82), height: object.fontSize * 1.3 };
  if ("size" in object) return { x: object.x - object.size / 2, y: object.y - object.size / 2, width: object.size, height: object.size };
  return { x: 0, y: 0, width: 0, height: 0 };
}

function moveObject(object: Annotation, dx: number, dy: number): Annotation {
  if (object.type === "rectangle" || object.type === "ellipse" || object.type === "mosaic") return { ...object, x: object.x + dx, y: object.y + dy };
  if (object.type === "arrow") return { ...object, x1: object.x1 + dx, y1: object.y1 + dy, x2: object.x2 + dx, y2: object.y2 + dy, cx: object.cx + dx, cy: object.cy + dy };
  if (object.type === "pen") return { ...object, points: object.points.map((point) => ({ x: point.x + dx, y: point.y + dy })) };
  return { ...object, x: object.x + dx, y: object.y + dy };
}

function pixelate(context: CanvasRenderingContext2D, object: BoxObject) {
  const x = Math.max(0, Math.floor(object.x)), y = Math.max(0, Math.floor(object.y));
  const width = Math.max(1, Math.floor(object.width)), height = Math.max(1, Math.floor(object.height));
  const pixels = context.getImageData(x, y, width, height), block = 12;
  for (let by = 0; by < height; by += block) for (let bx = 0; bx < width; bx += block) {
    const maxY = Math.min(height, by + block), maxX = Math.min(width, bx + block); let r = 0, g = 0, b = 0, a = 0, count = 0;
    for (let py = by; py < maxY; py++) for (let px = bx; px < maxX; px++) { const index = (py * width + px) * 4; r += pixels.data[index]; g += pixels.data[index + 1]; b += pixels.data[index + 2]; a += pixels.data[index + 3]; count++; }
    for (let py = by; py < maxY; py++) for (let px = bx; px < maxX; px++) { const index = (py * width + px) * 4; pixels.data[index] = r / count; pixels.data[index + 1] = g / count; pixels.data[index + 2] = b / count; pixels.data[index + 3] = a / count; }
  }
  context.putImageData(pixels, x, y);
}

export default function AnnotationEditor({ imagePath, width, height }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const [tool, setTool] = useState<Tool>("rectangle");
  const [color, setColor] = useState(colors[0]);
  const [strokeWidth, setStrokeWidth] = useState(4);
  const [objects, setObjects] = useState<Annotation[]>([]);
  const [undoStack, setUndoStack] = useState<Annotation[][]>([]);
  const [redoStack, setRedoStack] = useState<Annotation[][]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [start, setStart] = useState<Point | null>(null);
  const [current, setCurrent] = useState<Point | null>(null);
  const [penPoints, setPenPoints] = useState<Point[]>([]);
  const [moving, setMoving] = useState<{ id: string; origin: Point; object: Annotation } | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const source = await Image.fromPath(imagePath);
      try {
        const rgba = await source.rgba();
        if (!cancelled && canvasRef.current) canvasRef.current.getContext("2d")?.putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
      } catch (reason) { if (!cancelled) setError(String(reason)); }
      finally { await source.close(); if (!cancelled) setLoading(false); }
    })();
    return () => { cancelled = true; };
  }, [imagePath, width, height]);

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "z") { event.preventDefault(); event.shiftKey ? redo() : undo(); }
      else if (event.ctrlKey && event.key.toLowerCase() === "y") { event.preventDefault(); redo(); }
      else if ((event.key === "Delete" || event.key === "Backspace") && selectedId) { event.preventDefault(); removeSelected(); }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  });

  const pointFrom = (event: React.PointerEvent<SVGSVGElement>): Point => {
    const rect = event.currentTarget.getBoundingClientRect();
    return { x: (event.clientX - rect.left) * width / rect.width, y: (event.clientY - rect.top) * height / rect.height };
  };
  const snapshot = () => { setUndoStack((items) => [...items, objects]); setRedoStack([]); };
  const undo = () => { const previous = undoStack[undoStack.length - 1]; if (!previous) return; setRedoStack((items) => [...items, objects]); setObjects(previous); setUndoStack((items) => items.slice(0, -1)); setSelectedId(null); };
  const redo = () => { const next = redoStack[redoStack.length - 1]; if (!next) return; setUndoStack((items) => [...items, objects]); setObjects(next); setRedoStack((items) => items.slice(0, -1)); setSelectedId(null); };
  const removeSelected = () => { if (!selectedId) return; snapshot(); setObjects((items) => items.filter((item) => item.id !== selectedId)); setSelectedId(null); };

  function pointerDown(event: React.PointerEvent<SVGSVGElement>) {
    const point = pointFrom(event); setMessage("");
    if (tool === "text") {
      const text = window.prompt("輸入標註文字");
      if (text) { snapshot(); const next: TextObject = { id: crypto.randomUUID(), type: "text", x: point.x, y: point.y, text, fontSize: 28, color, strokeWidth, outlineColor: "#ffffff", outlineWidth: 1 }; setObjects([...objects, next]); setSelectedId(next.id); }
      return;
    }
    if (tool === "number") {
      snapshot(); const next: NumberObject = { id: crypto.randomUUID(), type: "number", x: point.x, y: point.y, number: objects.filter((item) => item.type === "number").length + 1, size: 36, color, strokeWidth }; setObjects([...objects, next]); setSelectedId(next.id); return;
    }
    setStart(point); setCurrent(point); if (tool === "pen") setPenPoints([point]); event.currentTarget.setPointerCapture(event.pointerId);
  }
  function pointerMove(event: React.PointerEvent<SVGSVGElement>) {
    const point = pointFrom(event);
    if (moving) { setObjects((items) => items.map((item) => item.id === moving.id ? moveObject(moving.object, point.x - moving.origin.x, point.y - moving.origin.y) : item)); return; }
    if (!start) return; setCurrent(point); if (tool === "pen") setPenPoints((items) => [...items, point]);
  }
  function pointerUp(event: React.PointerEvent<SVGSVGElement>) {
    if (moving) { setUndoStack((items) => [...items, objects.map((item) => item.id === moving.id ? moving.object : item)]); setRedoStack([]); setMoving(null); return; }
    if (!start) return; const end = pointFrom(event); const id = crypto.randomUUID(); let next: Annotation | null = null;
    if (tool === "rectangle" || tool === "ellipse" || tool === "mosaic") next = { id, type: tool, x: Math.min(start.x, end.x), y: Math.min(start.y, end.y), width: Math.abs(end.x - start.x), height: Math.abs(end.y - start.y), radius: 0, color, strokeWidth };
    else if (tool === "arrow") next = { id, type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, cx: (start.x + end.x) / 2, cy: (start.y + end.y) / 2, color, strokeWidth };
    else if (tool === "pen" && penPoints.length > 1) next = { id, type: "pen", points: penPoints, color, strokeWidth };
    if (next && (tool === "pen" || Math.hypot(end.x - start.x, end.y - start.y) > 3)) { snapshot(); setObjects([...objects, next]); setSelectedId(next.id); }
    setStart(null); setCurrent(null); setPenPoints([]);
  }
  function objectDown(event: React.PointerEvent<SVGElement>, object: Annotation) {
    event.stopPropagation();
    if (tool === "eraser") { snapshot(); setObjects((items) => items.filter((item) => item.id !== object.id)); setSelectedId(null); return; }
    const svg = event.currentTarget.ownerSVGElement; if (!svg) return; const rect = svg.getBoundingClientRect();
    const origin = { x: (event.clientX - rect.left) * width / rect.width, y: (event.clientY - rect.top) * height / rect.height };
    svg.setPointerCapture(event.pointerId); setSelectedId(object.id); setMoving({ id: object.id, origin, object });
  }
  function wheel(event: React.WheelEvent<SVGSVGElement>) {
    if (!selectedId) return; event.preventDefault(); const step = event.deltaY < 0 ? 1 : -1; snapshot();
    setObjects((items) => items.map((item) => item.id === selectedId ? { ...item, strokeWidth: Math.max(1, Math.min(32, item.strokeWidth + step)) } : item));
  }
  async function saveProject() {
    setSaving(true); setError("");
    try { const path = await invoke<string>("save_annotation_project", { imagePath, canvasWidth: width, canvasHeight: height, objects }); setMessage(`可編輯專案已儲存：${path}`); }
    catch (reason) { setError(String(reason)); } finally { setSaving(false); }
  }
  async function loadProject() {
    try { const project = await invoke<Project>("load_latest_annotation_project", { imagePath }); setObjects(project.objects); setUndoStack([]); setRedoStack([]); setSelectedId(null); setMessage("已載入最近的可編輯專案"); }
    catch (reason) { setError(String(reason)); }
  }
  async function applyAnnotations() {
    if (!canvasRef.current || !svgRef.current) return;
    setSaving(true); setError("");
    const output = document.createElement("canvas"); output.width = width; output.height = height;
    const context = output.getContext("2d");
    if (!context) { setSaving(false); setError("無法建立輸出畫布"); return; }
    context.drawImage(canvasRef.current, 0, 0);
    objects.filter((object): object is BoxObject => object.type === "mosaic").forEach((object) => pixelate(context, object));
    const cleanSvg = svgRef.current.cloneNode(true) as SVGSVGElement;
    cleanSvg.querySelectorAll(".selected-box").forEach((node) => node.remove());
    cleanSvg.querySelectorAll('[data-object-type="mosaic"]').forEach((node) => node.remove());
    const markup = new XMLSerializer().serializeToString(cleanSvg);
    const url = URL.createObjectURL(new Blob([markup], { type: "image/svg+xml;charset=utf-8" }));
    try {
      const overlay = new window.Image();
      await new Promise<void>((resolve, reject) => { overlay.onload = () => resolve(); overlay.onerror = () => reject(new Error("無法合成標註圖層")); overlay.src = url; });
      context.drawImage(overlay, 0, 0, width, height);
      const rgba = context.getImageData(0, 0, width, height).data;
      await invoke("save_edited_image", { imagePath, width, height, rgba: Array.from(rgba) });
      canvasRef.current.getContext("2d")?.drawImage(output, 0, 0);
      setObjects([]); setUndoStack([]); setRedoStack([]); setSelectedId(null);
      setMessage("標註已套用至圖片；複製、另存與置頂貼圖將使用更新版本");
    } catch (reason) { setError(String(reason)); }
    finally { URL.revokeObjectURL(url); setSaving(false); }
  }

  const selected = objects.find((item) => item.id === selectedId);
  const draft = start && current ? { x: Math.min(start.x, current.x), y: Math.min(start.y, current.y), width: Math.abs(current.x - start.x), height: Math.abs(current.y - start.y) } : null;
  return <section className="annotation-editor" aria-labelledby="annotation-title">
    <div className="annotation-heading"><div><span>物件式編輯</span><h2 id="annotation-title">標註圖片</h2></div><div className="editor-project-actions"><button onClick={loadProject}>開啟專案</button><button onClick={saveProject} disabled={saving}>儲存專案</button><button className="save" onClick={applyAnnotations} disabled={saving || !objects.length}>{saving ? "處理中…" : "套用標註"}</button></div></div>
    <div className="annotation-toolbar" role="toolbar" aria-label="繪圖工具列">
      <div className="tool-group">{(["rectangle", "ellipse", "pen", "arrow", "text", "number", "mosaic", "eraser"] as Tool[]).map((item) => <button key={item} className={tool === item ? "selected icon-tool" : "icon-tool"} aria-pressed={tool === item} title={labels[item]} onClick={() => setTool(item)}><Icon type={item}/><span>{labels[item]}</span></button>)}</div>
      <div className="toolbar-divider" />
      <div className="color-picker" aria-label="標註顏色">{colors.map((item) => <button key={item} className={color === item ? "active" : ""} style={{ background: item }} aria-label={`選擇顏色 ${item}`} onClick={() => setColor(item)} />)}</div>
      <label className="stroke-control">粗細<input type="range" min="1" max="32" value={strokeWidth} onChange={(event) => setStrokeWidth(Number(event.target.value))}/><output>{strokeWidth}</output></label>
      <div className="toolbar-divider" />
      <button title="復原" aria-label="復原" onClick={undo} disabled={!undoStack.length}>↶</button><button title="重做" aria-label="重做" onClick={redo} disabled={!redoStack.length}>↷</button><button title="刪除選取" aria-label="刪除選取" onClick={removeSelected} disabled={!selectedId}>×</button>
    </div>
    <p className="annotation-help">{labels[tool]}工具 · {selected ? `已選取物件，滾輪調整粗細 ${selected.strokeWidth}px` : "點選物件可移動與編輯"} · Ctrl+Z 復原 · {objects.length} 個物件</p>
    <div className="annotation-stage" style={{ aspectRatio: `${width}/${height}` }}><canvas ref={canvasRef} width={width} height={height}/>
      {!loading && <svg ref={svgRef} viewBox={`0 0 ${width} ${height}`} className={`drawing tool-${tool}`} onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onWheel={wheel}>
        <defs><pattern id="mosaicPattern" width="16" height="16" patternUnits="userSpaceOnUse"><rect width="8" height="8" fill="#777"/><rect x="8" width="8" height="8" fill="#aaa"/><rect y="8" width="8" height="8" fill="#aaa"/><rect x="8" y="8" width="8" height="8" fill="#777"/></pattern></defs>
        {objects.map((object) => {
          const common = { "data-object-type": object.type, onPointerDown: (event: React.PointerEvent<SVGElement>) => objectDown(event, object) };
          if (object.type === "rectangle") return <rect key={object.id} {...common} x={object.x} y={object.y} width={object.width} height={object.height} rx={object.radius} fill="none" stroke={object.color} strokeWidth={object.strokeWidth}/>;
          if (object.type === "ellipse") return <ellipse key={object.id} {...common} cx={object.x + object.width/2} cy={object.y + object.height/2} rx={object.width/2} ry={object.height/2} fill="none" stroke={object.color} strokeWidth={object.strokeWidth}/>;
          if (object.type === "arrow") return <polygon key={object.id} {...common} points={arrowPolygon(object)} fill={object.color}/>;
          if (object.type === "pen") return <polyline key={object.id} {...common} points={object.points.map((p) => `${p.x},${p.y}`).join(" ")} fill="none" stroke={object.color} strokeWidth={object.strokeWidth} strokeLinecap="round" strokeLinejoin="round"/>;
          if (object.type === "text") return <text key={object.id} {...common} x={object.x} y={object.y} fill={object.color} stroke={object.outlineColor} strokeWidth={object.outlineWidth} paintOrder="stroke" fontSize={object.fontSize} fontFamily="Microsoft JhengHei UI, sans-serif">{object.text}</text>;
          if (object.type === "number") return <g key={object.id} {...common}><circle cx={object.x} cy={object.y} r={object.size/2} fill={object.color}/><text x={object.x} y={object.y} textAnchor="middle" dominantBaseline="central" fill="#fff" fontSize={object.size*.55} fontWeight="700">{object.number}</text></g>;
          return <rect key={object.id} {...common} x={object.x} y={object.y} width={object.width} height={object.height} fill="url(#mosaicPattern)" opacity=".82"/>;
        })}
        {selected && (() => { const box = boundsOf(selected); return <rect className="annotation-hover selected-box" x={box.x-5} y={box.y-5} width={box.width+10} height={box.height+10}/>; })()}
        {draft && tool === "rectangle" && <rect {...draft} fill="none" stroke={color} strokeWidth={strokeWidth} strokeDasharray="8 5"/>}
        {draft && tool === "ellipse" && <ellipse cx={draft.x+draft.width/2} cy={draft.y+draft.height/2} rx={draft.width/2} ry={draft.height/2} fill="none" stroke={color} strokeWidth={strokeWidth} strokeDasharray="8 5"/>}
        {start && current && tool === "arrow" && <polygon points={arrowPolygon({ id:"draft",type:"arrow",x1:start.x,y1:start.y,x2:current.x,y2:current.y,cx:(start.x+current.x)/2,cy:(start.y+current.y)/2,color,strokeWidth })} fill={color} opacity=".8"/>}
        {penPoints.length > 1 && <polyline points={penPoints.map((p) => `${p.x},${p.y}`).join(" ")} fill="none" stroke={color} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round"/>}
        {draft && tool === "mosaic" && <rect {...draft} fill="url(#mosaicPattern)" opacity=".75"/>}
      </svg>}
      {loading && <div className="annotation-loading">正在載入截圖…</div>}
    </div>
    {message && <p className="settings-success" role="status">✓ {message}</p>}{error && <p className="settings-error" role="alert">錯誤：{error}</p>}
  </section>;
}
