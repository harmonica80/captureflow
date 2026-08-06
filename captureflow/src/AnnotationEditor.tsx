import { Image } from "@tauri-apps/api/image";
import { invoke } from "@tauri-apps/api/core";
import { downloadDir, join } from "@tauri-apps/api/path";
import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";

type Point = { x: number; y: number };
type Tool =
  | "rectangle"
  | "ellipse"
  | "line"
  | "pen"
  | "arrow"
  | "text"
  | "number"
  | "mosaic"
  | "eraser";
type LineStyle = "solid" | "dashed" | "dotted" | "dashDot";
type Base = {
  id: string;
  color: string;
  strokeWidth: number;
  lineStyle?: LineStyle;
};
type BoxObject = Base & {
  type: "rectangle" | "ellipse" | "mosaic";
  x: number;
  y: number;
  width: number;
  height: number;
  blockSize?: number;
};
type ArrowObject = Base & {
  type: "arrow";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  cx: number;
  cy: number;
};
type LineObject = Base & {
  type: "line";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};
type PenObject = Base & { type: "pen"; points: Point[] };
type TextObject = Base & {
  type: "text";
  x: number;
  y: number;
  text: string;
  fontSize: number;
  fontFamily: string;
  bold: boolean;
  italic: boolean;
  outlineColor: string;
  outlineWidth: number;
};
type NumberObject = Base & {
  type: "number";
  x: number;
  y: number;
  number: number;
  size: number;
};
type Annotation =
  | BoxObject
  | ArrowObject
  | LineObject
  | PenObject
  | TextObject
  | NumberObject;
export type AnnotationObject = Annotation;
type Props = {
  imagePath: string;
  width: number;
  height: number;
  initialObjects?: Annotation[];
  onCopy: (imagePath?: string) => Promise<void>;
  onSave: (imagePath?: string) => Promise<void>;
  onSticker: (imagePath?: string) => Promise<void>;
  onClose: () => void;
  onStatus: (message: string, isError?: boolean) => void;
  language?: "zh-TW" | "en";
};

const colors = [
  "#ff3b30",
  "#111111",
  "#ffffff",
  "#ffb000",
  "#ffe033",
  "#28c76f",
  "#1687ff",
  "#8a5cf6",
  "#9b9b9b",
  "#9bdcff",
];
const fonts = [
  "Microsoft JhengHei UI",
  "Microsoft JhengHei",
  "Segoe UI",
  "Arial",
  "Calibri",
  "Times New Roman",
  "Consolas",
  "標楷體",
  "新細明體",
];
const labelsZh: Record<Tool, string> = {
  rectangle: "矩形",
  ellipse: "圓形",
  line: "線條",
  pen: "畫筆",
  arrow: "箭頭",
  text: "文字",
  number: "序號",
  mosaic: "馬賽克",
  eraser: "橡皮擦",
};
const labelsEn: Record<Tool, string> = {
  rectangle: "Rectangle",
  ellipse: "Ellipse",
  line: "Line",
  pen: "Pen",
  arrow: "Arrow",
  text: "Text",
  number: "Number",
  mosaic: "Mosaic",
  eraser: "Eraser",
};

function Icon({ type }: { type: Tool }) {
  const p = {
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {type === "rectangle" && (
        <rect x="4" y="5" width="16" height="14" rx="1" {...p} />
      )}
      {type === "ellipse" && <ellipse cx="12" cy="12" rx="8" ry="7" {...p} />}
      {type === "line" && <path d="M4 18L20 6" {...p} />}
      {type === "pen" && (
        <>
          <path d="M4 20l4-1 11-11-3-3L5 16z" {...p} />
          <path d="M14.5 6.5l3 3" {...p} />
        </>
      )}
      {type === "arrow" && (
        <>
          <path d="M5 19L19 5" {...p} />
          <path d="M10 5h9v9" {...p} />
        </>
      )}{" "}
      {type === "text" && (
        <>
          <path d="M5 5h14M12 5v14" {...p} />
        </>
      )}
      {type === "number" && (
        <>
          <circle cx="12" cy="12" r="8" {...p} />
          <path d="M10 9l2-1v8" {...p} />
        </>
      )}
      {type === "mosaic" && (
        <>
          <path d="M4 4h7v7H4zM13 13h7v7h-7z" fill="currentColor" />
          <path
            d="M13 4h7v7h-7zM4 13h7v7H4z"
            fill="currentColor"
            opacity=".42"
          />
        </>
      )}
      {type === "eraser" && (
        <>
          <path d="M5 16l8-10 6 5-7 9H8z" {...p} />
          <path d="M10 20h9" {...p} />
        </>
      )}
    </svg>
  );
}

function ActionIcon({
  type,
}: {
  type: "open" | "project" | "copy" | "download" | "pin" | "close";
}) {
  const p = {
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {type === "open" && <path d="M3 7h7l2 2h9l-2 10H5z" {...p} />}{" "}
      {type === "project" && (
        <>
          <path d="M5 4h12l2 2v14H5z" {...p} />
          <path d="M8 4v6h8V4M8 20v-6h8v6" {...p} />
        </>
      )}{" "}
      {type === "copy" && (
        <>
          <rect x="8" y="8" width="11" height="11" rx="1" {...p} />
          <path d="M16 8V5H5v11h3" {...p} />
        </>
      )}{" "}
      {type === "download" && (
        <>
          <path d="M12 3v12M7 10l5 5 5-5M4 20h16" {...p} />
        </>
      )}{" "}
      {type === "pin" && (
        <>
          <path d="M9 4h6l-1 5 3 3v2H7v-2l3-3zM12 14v7" {...p} />
        </>
      )}{" "}
      {type === "close" && <path d="M5 5l14 14M19 5L5 19" {...p} />}
    </svg>
  );
}

function TextStyleIcon({ italic = false }: { italic?: boolean }) {
  const p = {
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 2.2,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      {italic ? (
        <>
          <path d="M10 4h8M6 20h8M15 4L9 20" {...p} />
        </>
      ) : (
        <path d="M7 4h6.5a4 4 0 010 8H7m0 0h7a4 4 0 010 8H7V4" {...p} />
      )}
    </svg>
  );
}

function arrowPolygon(o: ArrowObject) {
  const tangent = Math.max(1, Math.hypot(o.x2 - o.cx, o.y2 - o.cy));
  const ux = (o.x2 - o.cx) / tangent,
    uy = (o.y2 - o.cy) / tangent,
    px = -uy,
    py = ux;
  const length = Math.hypot(o.x2 - o.x1, o.y2 - o.y1),
    head = Math.min(length * 0.42, Math.max(14, o.strokeWidth * 6));
  const nx = o.x2 - ux * head,
    ny = o.y2 - uy * head;
  const left: Point[] = [],
    right: Point[] = [];
  for (let i = 0; i <= 20; i++) {
    const t = i / 20,
      q = 1 - t,
      x = q * q * o.x1 + 2 * q * t * o.cx + t * t * nx,
      y = q * q * o.y1 + 2 * q * t * o.cy + t * t * ny,
      tx = 2 * q * (o.cx - o.x1) + 2 * t * (nx - o.cx),
      ty = 2 * q * (o.cy - o.y1) + 2 * t * (ny - o.cy),
      l = Math.max(1, Math.hypot(tx, ty)),
      half = Math.max(0.8, o.strokeWidth * (0.22 + 0.78 * t));
    left.push({ x: x - (ty / l) * half, y: y + (tx / l) * half });
    right.push({ x: x + (ty / l) * half, y: y - (tx / l) * half });
  }
  return [
    ...left,
    { x: nx + px * o.strokeWidth * 3.2, y: ny + py * o.strokeWidth * 3.2 },
    { x: o.x2, y: o.y2 },
    { x: nx - px * o.strokeWidth * 3.2, y: ny - py * o.strokeWidth * 3.2 },
    ...right.reverse(),
  ]
    .map((p) => `${p.x},${p.y}`)
    .join(" ");
}
function dashArray(style?: LineStyle, width = 1) {
  if (style === "dashed") return `${width * 4} ${width * 2.5}`;
  if (style === "dotted") return `${width * 0.8} ${width * 2}`;
  if (style === "dashDot")
    return `${width * 4} ${width * 2} ${width * 0.8} ${width * 2}`;
  return undefined;
}
function arrowHeadPolygon(o: ArrowObject) {
  const tangent = Math.max(1, Math.hypot(o.x2 - o.cx, o.y2 - o.cy));
  const ux = (o.x2 - o.cx) / tangent,
    uy = (o.y2 - o.cy) / tangent,
    px = -uy,
    py = ux,
    length = Math.hypot(o.x2 - o.x1, o.y2 - o.y1),
    head = Math.min(length * 0.42, Math.max(14, o.strokeWidth * 6)),
    nx = o.x2 - ux * head,
    ny = o.y2 - uy * head,
    spread = o.strokeWidth * 3.2;
  return `${o.x2},${o.y2} ${nx + px * spread},${ny + py * spread} ${nx - px * spread},${ny - py * spread}`;
}
function bounds(o: Annotation) {
  if (o.type === "rectangle" || o.type === "ellipse" || o.type === "mosaic")
    return { x: o.x, y: o.y, width: o.width, height: o.height };
  if (o.type === "line")
    return {
      x: Math.min(o.x1, o.x2),
      y: Math.min(o.y1, o.y2),
      width: Math.abs(o.x2 - o.x1),
      height: Math.abs(o.y2 - o.y1),
    };
  if (o.type === "arrow")
    return {
      x: Math.min(o.x1, o.x2, o.cx),
      y: Math.min(o.y1, o.y2, o.cy),
      width: Math.max(o.x1, o.x2, o.cx) - Math.min(o.x1, o.x2, o.cx),
      height: Math.max(o.y1, o.y2, o.cy) - Math.min(o.y1, o.y2, o.cy),
    };
  if (o.type === "pen") {
    const xs = o.points.map((p) => p.x),
      ys = o.points.map((p) => p.y);
    return {
      x: Math.min(...xs),
      y: Math.min(...ys),
      width: Math.max(...xs) - Math.min(...xs),
      height: Math.max(...ys) - Math.min(...ys),
    };
  }
  if (o.type === "text")
    return {
      x: o.x,
      y: o.y - o.fontSize,
      width: Math.max(o.fontSize, o.text.length * o.fontSize * 0.9),
      height: o.fontSize * 1.35,
    };
  if (o.type === "number")
    return {
      x: o.x - o.size / 2,
      y: o.y - o.size / 2,
      width: o.size,
      height: o.size,
    };
  return { x: 0, y: 0, width: 0, height: 0 };
}
function move(o: Annotation, dx: number, dy: number): Annotation {
  if (o.type === "rectangle" || o.type === "ellipse" || o.type === "mosaic")
    return { ...o, x: o.x + dx, y: o.y + dy };
  if (o.type === "line")
    return {
      ...o,
      x1: o.x1 + dx,
      y1: o.y1 + dy,
      x2: o.x2 + dx,
      y2: o.y2 + dy,
    };
  if (o.type === "arrow")
    return {
      ...o,
      x1: o.x1 + dx,
      y1: o.y1 + dy,
      x2: o.x2 + dx,
      y2: o.y2 + dy,
      cx: o.cx + dx,
      cy: o.cy + dy,
    };
  if (o.type === "pen")
    return {
      ...o,
      points: o.points.map((p) => ({ x: p.x + dx, y: p.y + dy })),
    };
  return { ...o, x: o.x + dx, y: o.y + dy };
}
function pixelate(ctx: CanvasRenderingContext2D, o: BoxObject, block: number) {
  const x = Math.max(0, Math.floor(o.x)),
    y = Math.max(0, Math.floor(o.y)),
    w = Math.min(ctx.canvas.width - x, Math.max(1, Math.floor(o.width))),
    h = Math.min(ctx.canvas.height - y, Math.max(1, Math.floor(o.height)));
  if (w <= 0 || h <= 0) return;
  const data = ctx.getImageData(x, y, w, h);
  for (let by = 0; by < h; by += block)
    for (let bx = 0; bx < w; bx += block) {
      let r = 0,
        g = 0,
        b = 0,
        a = 0,
        n = 0;
      for (let py = by; py < Math.min(h, by + block); py++)
        for (let px = bx; px < Math.min(w, bx + block); px++) {
          const i = (py * w + px) * 4;
          r += data.data[i];
          g += data.data[i + 1];
          b += data.data[i + 2];
          a += data.data[i + 3];
          n++;
        }
      for (let py = by; py < Math.min(h, by + block); py++)
        for (let px = bx; px < Math.min(w, bx + block); px++) {
          const i = (py * w + px) * 4;
          data.data[i] = r / n;
          data.data[i + 1] = g / n;
          data.data[i + 2] = b / n;
          data.data[i + 3] = a / n;
        }
    }
  ctx.putImageData(data, x, y);
}

export default function AnnotationEditor({
  imagePath,
  width,
  height,
  initialObjects = [],
  onCopy,
  onSave,
  onSticker,
  onClose,
  onStatus,
  language = "zh-TW",
}: Props) {
  const labels = language === "en" ? labelsEn : labelsZh;
  const canvasRef = useRef<HTMLCanvasElement>(null),
    previewRef = useRef<HTMLCanvasElement>(null),
    svgRef = useRef<SVGSVGElement>(null),
    inputRef = useRef<HTMLInputElement>(null),
    stageRef = useRef<HTMLDivElement>(null);
  const [tool, setTool] = useState<Tool>("rectangle"),
    [color, setColor] = useState(colors[0]),
    [strokeWidth, setStrokeWidth] = useState(4),
    [lineStyle, setLineStyle] = useState<LineStyle>("solid"),
    [objects, setObjects] = useState<Annotation[]>(initialObjects),
    [undoStack, setUndo] = useState<Annotation[][]>([]),
    [redoStack, setRedo] = useState<Annotation[][]>([]),
    [selectedId, setSelected] = useState<string | null>(null),
    [hoveredId, setHovered] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [start, setStart] = useState<Point | null>(null),
    [current, setCurrent] = useState<Point | null>(null),
    [penPoints, setPenPoints] = useState<Point[]>([]),
    [moving, setMoving] = useState<{
      id: string;
      origin: Point;
      object: Annotation;
    } | null>(null),
    [editing, setEditing] = useState<{
      id: string;
      x: number;
      y: number;
      value: string;
    } | null>(null);
  const [fontFamily, setFontFamily] = useState(fonts[0]),
    [fontSize, setFontSize] = useState(32),
    [bold, setBold] = useState(false),
    [italic, setItalic] = useState(false),
    [outlineColor, setOutlineColor] = useState("#ffffff"),
    [outlineWidth, setOutlineWidth] = useState(1),
    [nextNumber, setNextNumber] = useState(1),
    [mosaicBlock, setMosaicBlock] = useState(18),
    [loading, setLoading] = useState(true),
    [saving, setSaving] = useState(false),
    [message, setMessage] = useState(""),
    [error, setError] = useState(""),
    [visualBounds, setVisualBounds] = useState<{
      x: number;
      y: number;
      width: number;
      height: number;
    } | null>(null);
  const [stageScale, setStageScale] = useState(1);
  const [zoom, setZoom] = useState(100);
  const [zoomMenuOpen, setZoomMenuOpen] = useState(false);
  const selected = objects.find((o) => o.id === selectedId),
    hover = objects.find((o) => o.id === (selectedId || hoveredId));
  const draft =
    start && current
      ? {
          x: Math.min(start.x, current.x),
          y: Math.min(start.y, current.y),
          width: Math.abs(current.x - start.x),
          height: Math.abs(current.y - start.y),
        }
      : null;
  const editingWidth = useMemo(() => {
    if (!editing) return 2;
    const canvas = document.createElement("canvas"),
      context = canvas.getContext("2d");
    if (!context) return fontSize;
    context.font = `${italic ? "italic " : ""}${bold ? "700 " : "400 "}${fontSize}px "${fontFamily}"`;
    return Math.max(
      fontSize * 0.45,
      context.measureText(editing.value || " ").width + 4,
    );
  }, [editing, fontFamily, fontSize, bold, italic]);
  const snapshot = () => {
    setUndo((s) => [...s, objects]);
    setRedo([]);
  };
  const pointFrom = (e: React.PointerEvent<SVGSVGElement>): Point => {
    const r = e.currentTarget.getBoundingClientRect();
    return {
      x: ((e.clientX - r.left) * width) / r.width,
      y: ((e.clientY - r.top) * height) / r.height,
    };
  };

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const source = await Image.fromPath(imagePath);
      try {
        const rgba = await source.rgba();
        if (!cancelled)
          canvasRef.current
            ?.getContext("2d")
            ?.putImageData(
              new ImageData(new Uint8ClampedArray(rgba), width, height),
              0,
              0,
            );
      } catch (e) {
        setError(String(e));
      } finally {
        await source.close();
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [imagePath, width, height]);
  useEffect(() => {
    const c = previewRef.current,
      base = canvasRef.current;
    if (!c || !base || loading) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, width, height);
    ctx.drawImage(base, 0, 0);
    objects
      .filter((o): o is BoxObject => o.type === "mosaic")
      .forEach((o) => pixelate(ctx, o, o.blockSize ?? 18));
    if (draft && tool === "mosaic")
      pixelate(
        ctx,
        {
          id: "draft",
          type: "mosaic",
          color,
          strokeWidth,
          blockSize: mosaicBlock,
          ...draft,
        },
        mosaicBlock,
      );
  }, [
    objects,
    draft,
    tool,
    mosaicBlock,
    loading,
    width,
    height,
    color,
    strokeWidth,
  ]);
  useEffect(() => {
    const id = selectedId || hoveredId;
    if (!id || !svgRef.current) {
      setVisualBounds(null);
      return;
    }
    const frame = requestAnimationFrame(() => {
      const node = svgRef.current?.querySelector(
        `[data-object-id="${id}"]`,
      ) as SVGGraphicsElement | null;
      if (node) {
        const b = node.getBBox();
        setVisualBounds({ x: b.x, y: b.y, width: b.width, height: b.height });
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [selectedId, hoveredId, objects]);
  useEffect(() => {
    if (!editing) return;
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    const input = inputRef.current;
    const resizeText = (event: WheelEvent) => {
      event.preventDefault();
      event.stopPropagation();
      setFontSize((size) =>
        Math.max(8, Math.min(240, size + (event.deltaY < 0 ? 2 : -2))),
      );
    };
    input?.addEventListener("wheel", resizeText, { passive: false });
    return () => {
      window.clearTimeout(timer);
      input?.removeEventListener("wheel", resizeText);
    };
  }, [editing]);
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const update = () =>
      setStageScale(stage.getBoundingClientRect().width / width);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(stage);
    return () => observer.disconnect();
  }, [width]);
  useEffect(() => {
    if (error) onStatus(error, true);
    else if (message) onStatus(message);
  }, [error, message]);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      void invoke("auto_save_annotation_project", {
        imagePath,
        canvasWidth: width,
        canvasHeight: height,
        objects,
      }).catch((e) => onStatus(`無法自動保存歷史專案：${e}`, true));
    }, 600);
    return () => window.clearTimeout(timer);
  }, [imagePath, width, height, objects]);
  useEffect(() => {
    const fn = (e: KeyboardEvent) => {
      if (editing) return;
      if (e.ctrlKey && e.key.toLowerCase() === "z") {
        e.preventDefault();
        e.shiftKey ? redo() : undo();
      } else if (e.ctrlKey && e.key.toLowerCase() === "y") {
        e.preventDefault();
        redo();
      } else if ((e.key === "Delete" || e.key === "Backspace") && selectedId) {
        e.preventDefault();
        removeSelected();
      }
    };
    window.addEventListener("keydown", fn);
    return () => window.removeEventListener("keydown", fn);
  });
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const handleWheel = (event: WheelEvent) => {
      if (!hoveredId) return;
      event.preventDefault();
      event.stopPropagation();
      const step = event.deltaY < 0 ? 1 : -1;
      setUndo((stack) => [...stack, objects]);
      setRedo([]);
      setObjects((items) =>
        items.map((object) =>
          object.id !== hoveredId
            ? object
            : object.type === "text"
              ? {
                  ...object,
                  fontSize: Math.max(
                    8,
                    Math.min(240, object.fontSize + step * 2),
                  ),
                }
              : object.type === "number"
                ? {
                    ...object,
                    size: Math.max(16, Math.min(240, object.size + step * 2)),
                  }
                : object.type === "mosaic"
                  ? {
                      ...object,
                      blockSize: Math.max(
                        4,
                        Math.min(60, (object.blockSize ?? 18) + step * 2),
                      ),
                    }
                  : {
                      ...object,
                      strokeWidth: Math.max(
                        1,
                        Math.min(32, object.strokeWidth + step),
                      ),
                    },
        ),
      );
    };
    svg.addEventListener("wheel", handleWheel, { passive: false });
    return () => svg.removeEventListener("wheel", handleWheel);
  }, [hoveredId, objects]);
  function undo() {
    const p = undoStack[undoStack.length - 1];
    if (!p) return;
    setRedo((s) => [...s, objects]);
    setObjects(p);
    setUndo((s) => s.slice(0, -1));
    setSelected(null);
  }
  function redo() {
    const n = redoStack[redoStack.length - 1];
    if (!n) return;
    setUndo((s) => [...s, objects]);
    setObjects(n);
    setRedo((s) => s.slice(0, -1));
    setSelected(null);
  }
  function removeSelected() {
    if (!selectedId) return;
    snapshot();
    setObjects((a) => a.filter((o) => o.id !== selectedId));
    setSelected(null);
  }
  function down(e: React.PointerEvent<SVGSVGElement>) {
    if ((e.target as Element).closest("[data-object-id],.control-point"))
      return;
    const p = pointFrom(e);
    setSelected(null);
    if (tool === "text") {
      const id = crypto.randomUUID();
      setEditing({ id, x: p.x, y: p.y, value: "" });
      return;
    }
    if (tool === "number") {
      snapshot();
      const o: NumberObject = {
        id: crypto.randomUUID(),
        type: "number",
        x: p.x,
        y: p.y,
        number: nextNumber,
        size: 40,
        color,
        strokeWidth,
      };
      setObjects((a) => [...a, o]);
      setSelected(o.id);
      setNextNumber((n) => n + 1);
      return;
    }
    setStart(p);
    setCurrent(p);
    if (tool === "pen") setPenPoints([p]);
    e.currentTarget.setPointerCapture(e.pointerId);
  }
  function motion(e: React.PointerEvent<SVGSVGElement>) {
    const p = pointFrom(e);
    if (moving) {
      setObjects((a) =>
        a.map((o) =>
          o.id === moving.id
            ? move(moving.object, p.x - moving.origin.x, p.y - moving.origin.y)
            : o,
        ),
      );
      return;
    }
    if (!start) return;
    setCurrent(p);
    if (tool === "pen") setPenPoints((a) => [...a, p]);
  }
  function up(e: React.PointerEvent<SVGSVGElement>) {
    if (moving) {
      setUndo((s) => [
        ...s,
        objects.map((o) => (o.id === moving.id ? moving.object : o)),
      ]);
      setRedo([]);
      setMoving(null);
      return;
    }
    if (!start) return;
    const end = pointFrom(e),
      id = crypto.randomUUID();
    let o: Annotation | null = null;
    if (tool === "rectangle" || tool === "ellipse" || tool === "mosaic")
      o = {
        id,
        type: tool,
        color,
        strokeWidth,
        lineStyle,
        blockSize: tool === "mosaic" ? mosaicBlock : undefined,
        x: Math.min(start.x, end.x),
        y: Math.min(start.y, end.y),
        width: Math.abs(end.x - start.x),
        height: Math.abs(end.y - start.y),
      };
    else if (tool === "arrow")
      o = {
        id,
        type: "arrow",
        color,
        strokeWidth,
        x1: start.x,
        y1: start.y,
        x2: end.x,
        y2: end.y,
        cx: (start.x + end.x) / 2,
        cy: (start.y + end.y) / 2,
        lineStyle,
      };
    else if (tool === "line")
      o = {
        id,
        type: "line",
        color,
        strokeWidth,
        lineStyle,
        x1: start.x,
        y1: start.y,
        x2: end.x,
        y2: end.y,
      };
    else if (tool === "pen" && penPoints.length > 1)
      o = { id, type: "pen", color, strokeWidth, points: penPoints };
    if (
      o &&
      (tool === "pen" || Math.hypot(end.x - start.x, end.y - start.y) > 3)
    ) {
      snapshot();
      setObjects((a) => [...a, o!]);
      setSelected(id);
    }
    setStart(null);
    setCurrent(null);
    setPenPoints([]);
  }
  function objectDown(e: React.PointerEvent<SVGElement>, o: Annotation) {
    e.preventDefault();
    e.stopPropagation();
    window.getSelection()?.removeAllRanges();
    if (tool === "eraser") {
      snapshot();
      setObjects((a) => a.filter((x) => x.id !== o.id));
      return;
    }
    const svg = e.currentTarget.ownerSVGElement;
    if (!svg) return;
    const r = svg.getBoundingClientRect(),
      origin = {
        x: ((e.clientX - r.left) * width) / r.width,
        y: ((e.clientY - r.top) * height) / r.height,
      };
    svg.setPointerCapture(e.pointerId);
    setSelected(o.id);
    setTool(o.type === "mosaic" ? "mosaic" : o.type);
    if (
      o.type === "rectangle" ||
      o.type === "ellipse" ||
      o.type === "line" ||
      o.type === "arrow"
    )
      setLineStyle(o.lineStyle ?? "solid");
    setMoving({ id: o.id, origin, object: o });
  }
  function curveDown(e: React.PointerEvent<SVGCircleElement>, o: ArrowObject) {
    e.stopPropagation();
    const svg = e.currentTarget.ownerSVGElement;
    if (!svg) return;
    snapshot();
    const moveCurve = (ev: PointerEvent) => {
      const r = svg.getBoundingClientRect(),
        p = {
          x: ((ev.clientX - r.left) * width) / r.width,
          y: ((ev.clientY - r.top) * height) / r.height,
        };
      setObjects((a) =>
        a.map((x) =>
          x.id === o.id && x.type === "arrow" ? { ...x, cx: p.x, cy: p.y } : x,
        ),
      );
    };
    const done = () => {
      window.removeEventListener("pointermove", moveCurve);
      window.removeEventListener("pointerup", done);
    };
    window.addEventListener("pointermove", moveCurve);
    window.addEventListener("pointerup", done);
  }
  function commitText() {
    if (!editing) return;
    if (editing.value.trim()) {
      snapshot();
      const o: TextObject = {
        id: editing.id,
        type: "text",
        x: editing.x,
        y: editing.y + fontSize,
        text: editing.value,
        color,
        strokeWidth,
        fontSize,
        fontFamily,
        bold,
        italic,
        outlineColor,
        outlineWidth,
      };
      setObjects((a) => [...a, o]);
      setSelected(o.id);
    }
    setEditing(null);
  }
  function updateSelected(patch: Record<string, unknown>) {
    if (!selectedId) return;
    snapshot();
    setObjects((a) =>
      a.map((o) =>
        o.id === selectedId ? ({ ...o, ...patch } as Annotation) : o,
      ),
    );
  }
  async function saveProject() {
    setSaving(true);
    try {
      const folder = await downloadDir();
      const destination = await save({
        title: "儲存 CaptureFlow 可編輯專案",
        defaultPath: await join(folder, "CaptureFlow-project.json"),
        filters: [{ name: "CaptureFlow JSON 專案", extensions: ["json"] }],
      });
      if (!destination) return;
      const p = await invoke<string>("save_annotation_project", {
        imagePath,
        canvasWidth: width,
        canvasHeight: height,
        objects,
        destination,
      });
      setMessage(`可編輯專案已儲存：${p}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }
  function loadProject() {
    /* 專案開啟入口已移至左側選單。 */
  }
  async function createComposite() {
    if (!canvasRef.current || !svgRef.current) return null;
    setSaving(true);
    const out = document.createElement("canvas");
    out.width = width;
    out.height = height;
    const ctx = out.getContext("2d");
    if (!ctx) {
      setSaving(false);
      return null;
    }
    ctx.drawImage(canvasRef.current, 0, 0);
    objects
      .filter((o): o is BoxObject => o.type === "mosaic")
      .forEach((o) => pixelate(ctx, o, o.blockSize ?? 18));
    const svg = svgRef.current.cloneNode(true) as SVGSVGElement;
    svg
      .querySelectorAll(".selection-ui,[data-object-type='mosaic']")
      .forEach((n) => n.remove());
    const url = URL.createObjectURL(
      new Blob([new XMLSerializer().serializeToString(svg)], {
        type: "image/svg+xml",
      }),
    );
    try {
      const img = new window.Image();
      await new Promise<void>((ok, no) => {
        img.onload = () => ok();
        img.onerror = () => no(new Error("無法合成標註"));
        img.src = url;
      });
      ctx.drawImage(img, 0, 0, width, height);
      const compositePath = await invoke<string>("save_composited_image", {
        width,
        height,
        rgba: Array.from(ctx.getImageData(0, 0, width, height).data),
      });
      setMessage("已建立輸出圖片；標註物件仍可繼續編輯");
      return compositePath;
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      URL.revokeObjectURL(url);
      setSaving(false);
    }
  }
  async function applyAndSave() {
    const path = await createComposite();
    if (path) await onSave(path);
  }
  async function applyAndCopy() {
    const path = await createComposite();
    if (path) await onCopy(path);
  }
  async function applyAndSticker() {
    const path = await createComposite();
    if (path) await onSticker(path);
  }
  const shownObjects = useMemo(() => objects, [objects]);
  return (
    <section className="annotation-editor">
      <div className="annotation-heading">
        <div>
          <span>物件式編輯</span>
          <h2>標註圖片</h2>
        </div>
        <div
          className="editor-action-toolbar"
          role="toolbar"
          aria-label="圖片與專案動作"
        >
          <button
            className="action-icon"
            onClick={loadProject}
            title="開啟 JSON 專案"
            aria-label="開啟 JSON 專案"
          >
            <ActionIcon type="open" />
          </button>
          <button
            className="action-icon"
            onClick={saveProject}
            title="儲存 JSON 專案"
            aria-label="儲存 JSON 專案"
          >
            <ActionIcon type="project" />
          </button>
          <button
            className="action-icon"
            onClick={applyAndCopy}
            disabled={saving}
            title="套用標註並複製圖片"
            aria-label="套用標註並複製圖片"
          >
            <ActionIcon type="copy" />
          </button>
          <button
            className="action-icon primary-action"
            onClick={applyAndSave}
            disabled={saving}
            title="套用標註並另存圖片"
            aria-label="套用標註並另存圖片"
          >
            <ActionIcon type="download" />
          </button>
          <button
            className="action-icon"
            onClick={applyAndSticker}
            disabled={saving}
            title="套用標註並建立置頂貼圖"
            aria-label="套用標註並建立置頂貼圖"
          >
            <ActionIcon type="pin" />
          </button>
          <button
            className="action-icon"
            onClick={onClose}
            title="關閉圖片"
            aria-label="關閉圖片"
          >
            <ActionIcon type="close" />
          </button>
        </div>
      </div>
      <div className="annotation-toolbar-wrap">
        <div
          className="annotation-toolbar"
          role="toolbar"
          aria-label="圖片標註工具"
        >
          <div className="tool-group">
            {(Object.keys(labels) as Tool[]).map((t) => (
              <button
                key={t}
                className={tool === t ? "selected icon-tool" : "icon-tool"}
                title={labels[t]}
                aria-label={labels[t]}
                onClick={() => setTool(t)}
              >
                <Icon type={t} />
              </button>
            ))}
          </div>
          <div className="toolbar-divider" />
          <button
            className="current-color"
            style={{ background: color }}
            title="選擇顏色"
            aria-label="選擇顏色"
            aria-expanded={paletteOpen}
            onClick={() => setPaletteOpen((open) => !open)}
          />
          <label className="stroke-control">
            粗細
            <input
              type="range"
              min="1"
              max="32"
              value={strokeWidth}
              onChange={(e) => {
                const n = +e.target.value;
                setStrokeWidth(n);
                if (selected) updateSelected({ strokeWidth: n });
              }}
            />
            <output>{strokeWidth}</output>
          </label>
          {(tool === "rectangle" ||
            tool === "ellipse" ||
            tool === "line" ||
            tool === "arrow" ||
            selected?.type === "rectangle" ||
            selected?.type === "ellipse" ||
            selected?.type === "line" ||
            selected?.type === "arrow") && (
            <label className="line-style-control" title="線條樣式">
              樣式
              <select
                value={
                  selected && "lineStyle" in selected
                    ? (selected.lineStyle ?? "solid")
                    : lineStyle
                }
                onChange={(event) => {
                  const next = event.target.value as LineStyle;
                  setLineStyle(next);
                  if (selected) updateSelected({ lineStyle: next });
                }}
              >
                <option value="solid">━━━━ 實線</option>
                <option value="dashed">－－－－ 虛線</option>
                <option value="dotted">•••••• 點線</option>
                <option value="dashDot">—·—·— 點劃線</option>
              </select>
            </label>
          )}
          <button
            aria-label="復原"
            title="復原"
            onClick={undo}
            disabled={!undoStack.length}
          >
            ↶
          </button>
          <button
            aria-label="重做"
            title="重做"
            onClick={redo}
            disabled={!redoStack.length}
          >
            ↷
          </button>
          <button
            aria-label="刪除物件"
            title="刪除物件"
            onClick={removeSelected}
            disabled={!selectedId}
          >
            ×
          </button>
        </div>
        {paletteOpen && (
          <div
            className="floating-color-palette"
            role="group"
            aria-label="標註顏色"
          >
            {colors.map((c) => (
              <button
                key={c}
                className={color === c ? "active" : ""}
                style={{ background: c }}
                aria-label={`顏色 ${c}`}
                onClick={() => {
                  setColor(c);
                  setPaletteOpen(false);
                  if (selected) updateSelected({ color: c });
                }}
              />
            ))}
          </div>
        )}
      </div>
      {(tool === "text" || selected?.type === "text") && (
        <div className="property-bar">
          <label>
            字型
            <select
              value={
                selected?.type === "text" ? selected.fontFamily : fontFamily
              }
              onChange={(e) => {
                setFontFamily(e.target.value);
                if (selected?.type === "text")
                  updateSelected({ fontFamily: e.target.value });
              }}
            >
              {fonts.map((f) => (
                <option key={f}>{f}</option>
              ))}
            </select>
          </label>
          <label>
            大小
            <input
              type="number"
              min="8"
              max="240"
              value={selected?.type === "text" ? selected.fontSize : fontSize}
              onChange={(e) => {
                setFontSize(+e.target.value);
                if (selected?.type === "text")
                  updateSelected({ fontSize: +e.target.value });
              }}
            />
          </label>
          <button
            className={
              (selected?.type === "text" ? selected.bold : bold)
                ? "active text-style"
                : "text-style"
            }
            title="粗體"
            aria-label="粗體"
            onClick={() => {
              const next = !(selected?.type === "text" ? selected.bold : bold);
              setBold(next);
              if (selected?.type === "text") updateSelected({ bold: next });
            }}
          >
            <TextStyleIcon />
          </button>
          <button
            className={
              (selected?.type === "text" ? selected.italic : italic)
                ? "active text-style"
                : "text-style"
            }
            title="斜體"
            aria-label="斜體"
            onClick={() => {
              const next = !(selected?.type === "text"
                ? selected.italic
                : italic);
              setItalic(next);
              if (selected?.type === "text") updateSelected({ italic: next });
            }}
          >
            <TextStyleIcon italic />
          </button>
          <label>
            外框色
            <input
              type="color"
              value={outlineColor}
              onChange={(e) => {
                setOutlineColor(e.target.value);
                if (selected?.type === "text")
                  updateSelected({ outlineColor: e.target.value });
              }}
            />
          </label>
          <label>
            外框
            <input
              type="range"
              min="0"
              max="12"
              value={
                selected?.type === "text" ? selected.outlineWidth : outlineWidth
              }
              onChange={(e) => {
                setOutlineWidth(+e.target.value);
                if (selected?.type === "text")
                  updateSelected({ outlineWidth: +e.target.value });
              }}
            />
          </label>
        </div>
      )}
      {(tool === "number" || selected?.type === "number") && (
        <div className="property-bar">
          <label>
            {selected?.type === "number" ? "修改序號" : "下一個序號"}
            <input
              type="number"
              min="0"
              max="9999"
              value={selected?.type === "number" ? selected.number : nextNumber}
              onChange={(e) => {
                const n = +e.target.value;
                setNextNumber(n);
                if (selected?.type === "number") updateSelected({ number: n });
              }}
            />
          </label>
        </div>
      )}
      {tool === "mosaic" && (
        <div className="property-bar">
          <label>
            像素大小
            <input
              type="range"
              min="4"
              max="60"
              value={
                selected?.type === "mosaic"
                  ? (selected.blockSize ?? 18)
                  : mosaicBlock
              }
              onChange={(e) => {
                const n = +e.target.value;
                setMosaicBlock(n);
                if (selected?.type === "mosaic")
                  updateSelected({ blockSize: n });
              }}
            />
            <output>
              {selected?.type === "mosaic"
                ? (selected.blockSize ?? 18)
                : mosaicBlock}
            </output>
          </label>
          <span>滑過馬賽克物件並滾動滑鼠滾輪也能調整</span>
        </div>
      )}
      <p className="annotation-help">
        {labels[tool]}工具 · 滑過或選取物件後可用滾輪調整粗細或大小 ·
        箭頭中央節點可調整彎曲 · Ctrl+Z 復原
      </p>
      <div className="annotation-scroll">
        <div
          ref={stageRef}
          className="annotation-stage"
          style={{ aspectRatio: `${width}/${height}`, width: `${zoom}%` }}
        >
          <canvas ref={canvasRef} width={width} height={height} />
          <canvas
            ref={previewRef}
            width={width}
            height={height}
            className="mosaic-preview"
          />
          {!loading && (
            <svg
              ref={svgRef}
              viewBox={`0 0 ${width} ${height}`}
              preserveAspectRatio="none"
              className={`drawing tool-${tool}`}
              onPointerDown={down}
              onPointerMove={motion}
              onPointerUp={up}
              onPointerLeave={() => setHovered(null)}
            >
              {shownObjects.map((o) => {
                const common = {
                  "data-object-id": o.id,
                  "data-object-type": o.type,
                  onPointerEnter: () => setHovered(o.id),
                  onPointerLeave: () => setHovered(null),
                  onPointerDown: (e: React.PointerEvent<SVGElement>) =>
                    objectDown(e, o),
                };
                if (o.type === "rectangle")
                  return (
                    <rect
                      key={o.id}
                      {...common}
                      x={o.x}
                      y={o.y}
                      width={o.width}
                      height={o.height}
                      fill="none"
                      stroke={o.color}
                      strokeWidth={o.strokeWidth}
                      strokeDasharray={dashArray(o.lineStyle, o.strokeWidth)}
                      strokeLinecap="round"
                    />
                  );
                if (o.type === "ellipse")
                  return (
                    <ellipse
                      key={o.id}
                      {...common}
                      cx={o.x + o.width / 2}
                      cy={o.y + o.height / 2}
                      rx={o.width / 2}
                      ry={o.height / 2}
                      fill="none"
                      stroke={o.color}
                      strokeWidth={o.strokeWidth}
                      strokeDasharray={dashArray(o.lineStyle, o.strokeWidth)}
                      strokeLinecap="round"
                    />
                  );
                if (o.type === "line")
                  return (
                    <line
                      key={o.id}
                      {...common}
                      x1={o.x1}
                      y1={o.y1}
                      x2={o.x2}
                      y2={o.y2}
                      stroke={o.color}
                      strokeWidth={o.strokeWidth}
                      strokeDasharray={dashArray(o.lineStyle, o.strokeWidth)}
                      strokeLinecap="round"
                    />
                  );
                if (o.type === "arrow")
                  return o.lineStyle && o.lineStyle !== "solid" ? (
                    <g key={o.id} {...common}>
                      <path
                        d={`M ${o.x1} ${o.y1} Q ${o.cx} ${o.cy} ${o.x2} ${o.y2}`}
                        fill="none"
                        stroke={o.color}
                        strokeWidth={o.strokeWidth}
                        strokeDasharray={dashArray(o.lineStyle, o.strokeWidth)}
                        strokeLinecap="round"
                      />
                      <polygon points={arrowHeadPolygon(o)} fill={o.color} />
                    </g>
                  ) : (
                    <polygon
                      key={o.id}
                      {...common}
                      points={arrowPolygon(o)}
                      fill={o.color}
                    />
                  );
                if (o.type === "pen")
                  return (
                    <polyline
                      key={o.id}
                      {...common}
                      points={o.points.map((p) => `${p.x},${p.y}`).join(" ")}
                      fill="none"
                      stroke={o.color}
                      strokeWidth={o.strokeWidth}
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  );
                if (o.type === "text")
                  return (
                    <g key={o.id} {...common}>
                      <text
                        x={o.x}
                        y={o.y}
                        transform={
                          o.italic
                            ? `translate(${o.x} ${o.y}) skewX(-12) translate(${-o.x} ${-o.y})`
                            : undefined
                        }
                        fill={o.color}
                        stroke={o.outlineColor}
                        strokeWidth={o.outlineWidth}
                        paintOrder="stroke"
                        fontSize={o.fontSize}
                        fontFamily={o.fontFamily}
                        fontWeight={o.bold ? 700 : 400}
                      >
                        {o.text}
                      </text>
                    </g>
                  );
                if (o.type === "number")
                  return (
                    <g key={o.id} {...common}>
                      <circle cx={o.x} cy={o.y} r={o.size / 2} fill={o.color} />
                      <text
                        x={o.x}
                        y={o.y}
                        textAnchor="middle"
                        dominantBaseline="central"
                        fill="#fff"
                        fontSize={o.size * 0.55}
                        fontWeight="700"
                      >
                        {o.number}
                      </text>
                    </g>
                  );
                return (
                  <rect
                    key={o.id}
                    {...common}
                    x={o.x}
                    y={o.y}
                    width={o.width}
                    height={o.height}
                    fill="transparent"
                  />
                );
              })}
              {hover && (
                <g className="selection-ui">
                  {(() => {
                    const b = visualBounds ?? bounds(hover);
                    return (
                      <rect
                        className="annotation-hover"
                        x={b.x - 4}
                        y={b.y - 4}
                        width={b.width + 8}
                        height={b.height + 8}
                      />
                    );
                  })()}
                  {hover.type === "arrow" && (
                    <circle
                      className="control-point curve-point"
                      cx={hover.cx}
                      cy={hover.cy}
                      r="7"
                      onPointerDown={(e) => curveDown(e, hover)}
                    />
                  )}
                </g>
              )}
              {draft && tool === "rectangle" && (
                <rect
                  {...draft}
                  fill="none"
                  stroke={color}
                  strokeWidth={strokeWidth}
                  strokeDasharray={dashArray(lineStyle, strokeWidth)}
                  strokeLinecap="round"
                />
              )}{" "}
              {draft && tool === "ellipse" && (
                <ellipse
                  cx={draft.x + draft.width / 2}
                  cy={draft.y + draft.height / 2}
                  rx={draft.width / 2}
                  ry={draft.height / 2}
                  fill="none"
                  stroke={color}
                  strokeWidth={strokeWidth}
                  strokeDasharray={dashArray(lineStyle, strokeWidth)}
                  strokeLinecap="round"
                />
              )}{" "}
              {start && current && tool === "line" && (
                <line
                  x1={start.x}
                  y1={start.y}
                  x2={current.x}
                  y2={current.y}
                  stroke={color}
                  strokeWidth={strokeWidth}
                  strokeDasharray={dashArray(lineStyle, strokeWidth)}
                  strokeLinecap="round"
                />
              )}{" "}
              {start && current && tool === "arrow" && (
                lineStyle === "solid" ? <polygon
                  points={arrowPolygon({
                    id: "d",
                    type: "arrow",
                    x1: start.x,
                    y1: start.y,
                    x2: current.x,
                    y2: current.y,
                    cx: (start.x + current.x) / 2,
                    cy: (start.y + current.y) / 2,
                    color,
                    strokeWidth,
                  })}
                  fill={color}
                /> : <g>
                  <path d={`M ${start.x} ${start.y} Q ${(start.x + current.x) / 2} ${(start.y + current.y) / 2} ${current.x} ${current.y}`} fill="none" stroke={color} strokeWidth={strokeWidth} strokeDasharray={dashArray(lineStyle, strokeWidth)} strokeLinecap="round" />
                  <polygon points={arrowHeadPolygon({ id: "d", type: "arrow", x1: start.x, y1: start.y, x2: current.x, y2: current.y, cx: (start.x + current.x) / 2, cy: (start.y + current.y) / 2, color, strokeWidth, lineStyle })} fill={color} />
                </g>
              )}{" "}
              {penPoints.length > 1 && (
                <polyline
                  points={penPoints.map((p) => `${p.x},${p.y}`).join(" ")}
                  fill="none"
                  stroke={color}
                  strokeWidth={strokeWidth}
                  strokeLinecap="round"
                />
              )}
            </svg>
          )}
          {editing && (
            <input
              ref={inputRef}
              className="inline-text-input"
              style={{
                left: `${(editing.x / width) * 100}%`,
                top: `${(editing.y / height) * 100}%`,
                width: `${editingWidth * stageScale}px`,
                fontFamily,
                fontSize: `${fontSize * stageScale}px`,
                fontWeight: bold ? 700 : 400,
                fontStyle: italic ? "italic" : "normal",
                color,
                caretColor: color,
              }}
              value={editing.value}
              onChange={(e) =>
                setEditing({ ...editing, value: e.target.value })
              }
              onBlur={commitText}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitText();
                if (e.key === "Escape") setEditing(null);
              }}
              aria-label="輸入標註文字"
            />
          )}
        </div>
        <div className="zoom-dock">
          {zoomMenuOpen && <div className="zoom-menu"><button onClick={() => setZoom((value) => Math.min(300, value + 10))}>放大 <kbd>Ctrl +</kbd></button><button onClick={() => setZoom((value) => Math.max(25, value - 10))}>縮小 <kbd>Ctrl −</kbd></button><button onClick={() => setZoom(100)}>縮放至 100%</button><button onClick={() => setZoom(100)}>縮放至適合大小</button></div>}
          <div className="zoom-control" role="group" aria-label="圖片縮放控制">
            <button onClick={() => setZoom((value) => Math.max(25, value - 10))} title="縮小圖片">−</button>
            <button className="zoom-value" onClick={() => setZoomMenuOpen((open) => !open)} aria-expanded={zoomMenuOpen}><output aria-live="polite">{zoom}%</output></button>
            <button onClick={() => setZoom((value) => Math.min(300, value + 10))} title="放大圖片">＋</button>
            <button onClick={() => setZoomMenuOpen((open) => !open)} title="顯示縮放選單">«</button>
          </div>
        </div>
      </div>
    </section>
  );
}
