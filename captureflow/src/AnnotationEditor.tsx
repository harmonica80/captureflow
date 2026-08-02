import { Image } from "@tauri-apps/api/image";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

type Point = { x: number; y: number };
type Tool = "rectangle" | "arrow";
type Annotation =
  | { id: string; type: "rectangle"; x: number; y: number; width: number; height: number; color: string; strokeWidth: number }
  | { id: string; type: "arrow"; x1: number; y1: number; x2: number; y2: number; color: string; strokeWidth: number };

type Props = {
  imagePath: string;
  width: number;
  height: number;
  onClose: () => void;
};

function arrowPolygon(x1: number, y1: number, x2: number, y2: number, strokeWidth: number) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const length = Math.max(1, Math.hypot(dx, dy));
  const ux = dx / length;
  const uy = dy / length;
  const px = -uy;
  const py = ux;
  // Keep the arrowhead tied to stroke width instead of total arrow length.
  // Very short arrows compress the head, while long arrows retain a stable shape.
  const headLength = Math.min(length * 0.45, strokeWidth * 6);
  const neckX = x2 - ux * headLength;
  const neckY = y2 - uy * headLength;
  const tailHalf = Math.max(0.7, strokeWidth * 0.22);
  const neckHalf = strokeWidth * 1.15;
  const headHalf = strokeWidth * 3.5;
  return [
    [x1 + px * tailHalf, y1 + py * tailHalf],
    [neckX + px * neckHalf, neckY + py * neckHalf],
    [neckX + px * headHalf, neckY + py * headHalf],
    [x2, y2],
    [neckX - px * headHalf, neckY - py * headHalf],
    [neckX - px * neckHalf, neckY - py * neckHalf],
    [x1 - px * tailHalf, y1 - py * tailHalf],
  ].map(([x, y]) => `${x},${y}`).join(" ");
}

export default function AnnotationEditor({ imagePath, width, height, onClose }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [tool, setTool] = useState<Tool>("rectangle");
  const [objects, setObjects] = useState<Annotation[]>([]);
  const [redo, setRedo] = useState<Annotation[]>([]);
  const [start, setStart] = useState<Point | null>(null);
  const [current, setCurrent] = useState<Point | null>(null);
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
        if (cancelled || !canvasRef.current) return;
        const context = canvasRef.current.getContext("2d");
        if (!context) throw new Error("無法建立標註畫布");
        context.putImageData(new ImageData(new Uint8ClampedArray(rgba), width, height), 0, 0);
      } catch (reason) {
        if (!cancelled) setError(String(reason));
      } finally {
        await source.close();
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [imagePath, width, height]);

  function pointFromEvent(event: React.PointerEvent<SVGSVGElement>): Point {
    const bounds = event.currentTarget.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(width, (event.clientX - bounds.left) * width / bounds.width)),
      y: Math.max(0, Math.min(height, (event.clientY - bounds.top) * height / bounds.height)),
    };
  }

  function pointerDown(event: React.PointerEvent<SVGSVGElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    const point = pointFromEvent(event);
    setStart(point);
    setCurrent(point);
    setMessage("");
  }

  function pointerUp(event: React.PointerEvent<SVGSVGElement>) {
    if (!start) return;
    const end = pointFromEvent(event);
    const distance = Math.hypot(end.x - start.x, end.y - start.y);
    if (distance >= 3) {
      const id = `${Date.now()}-${objects.length}`;
      const next: Annotation = tool === "rectangle"
        ? { id, type: "rectangle", x: Math.min(start.x, end.x), y: Math.min(start.y, end.y), width: Math.abs(end.x - start.x), height: Math.abs(end.y - start.y), color: "#ff3b30", strokeWidth: 4 }
        : { id, type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, color: "#ff3b30", strokeWidth: 4 };
      setObjects((existing) => [...existing, next]);
      setRedo([]);
    }
    setStart(null);
    setCurrent(null);
  }

  function undo() {
    setObjects((existing) => {
      const removed = existing[existing.length - 1];
      if (!removed) return existing;
      setRedo((items) => [...items, removed]);
      return existing.slice(0, -1);
    });
  }

  function redoLast() {
    setRedo((items) => {
      const restored = items[items.length - 1];
      if (!restored) return items;
      setObjects((existing) => [...existing, restored]);
      return items.slice(0, -1);
    });
  }

  async function saveProject() {
    setSaving(true);
    setError("");
    setMessage("");
    try {
      const path = await invoke<string>("save_annotation_project", {
        imagePath,
        canvasWidth: width,
        canvasHeight: height,
        objects,
      });
      setMessage(`可編輯專案已儲存：${path}`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSaving(false);
    }
  }

  const draft = start && current ? { start, end: current } : null;
  return (
    <section className="annotation-editor" aria-labelledby="annotation-title">
      <div className="annotation-heading">
        <div><span>PHASE 2.1</span><h2 id="annotation-title">非破壞式標註</h2></div>
        <button className="capture-button" onClick={onClose}>關閉編輯器</button>
      </div>
      <div className="annotation-toolbar" aria-label="標註工具列">
        <button className={tool === "rectangle" ? "selected" : ""} aria-pressed={tool === "rectangle"} onClick={() => setTool("rectangle")}>▭ 矩形</button>
        <button className={tool === "arrow" ? "selected" : ""} aria-pressed={tool === "arrow"} onClick={() => setTool("arrow")}>↗ 箭頭</button>
        <button onClick={undo} disabled={objects.length === 0}>↶ 復原</button>
        <button onClick={redoLast} disabled={redo.length === 0}>↷ 重做</button>
        <button onClick={() => { setObjects([]); setRedo([]); }} disabled={objects.length === 0}>清除全部</button>
        <button className="save" onClick={saveProject} disabled={saving}>{saving ? "儲存中…" : "儲存可編輯專案"}</button>
      </div>
      <p className="annotation-help">目前工具：{tool === "rectangle" ? "矩形" : "箭頭"} · 在圖片上拖曳建立物件 · 已有 {objects.length} 個物件</p>
      <div className="annotation-stage" style={{ aspectRatio: `${width} / ${height}` }}>
        <canvas ref={canvasRef} width={width} height={height} aria-label="截圖底圖" />
        {!loading && (
          <svg viewBox={`0 0 ${width} ${height}`} onPointerDown={pointerDown} onPointerMove={(event) => start && setCurrent(pointFromEvent(event))} onPointerUp={pointerUp} aria-label="標註畫布">
            {objects.map((object) => object.type === "rectangle"
              ? <rect key={object.id} x={object.x} y={object.y} width={object.width} height={object.height} fill="none" stroke={object.color} strokeWidth={object.strokeWidth} />
              : <polygon key={object.id} points={arrowPolygon(object.x1, object.y1, object.x2, object.y2, object.strokeWidth)} fill={object.color} />)}
            {draft && tool === "rectangle" && <rect x={Math.min(draft.start.x, draft.end.x)} y={Math.min(draft.start.y, draft.end.y)} width={Math.abs(draft.end.x - draft.start.x)} height={Math.abs(draft.end.y - draft.start.y)} fill="none" stroke="#ff3b30" strokeWidth="4" strokeDasharray="10 6" />}
            {draft && tool === "arrow" && <polygon points={arrowPolygon(draft.start.x, draft.start.y, draft.end.x, draft.end.y, 4)} fill="#ff3b30" opacity="0.82" />}
          </svg>
        )}
        {loading && <div className="annotation-loading">正在載入底圖…</div>}
      </div>
      {message && <p className="settings-success" role="status">✓ {message}</p>}
      {error && <p className="settings-error" role="alert">錯誤：{error}</p>}
    </section>
  );
}
