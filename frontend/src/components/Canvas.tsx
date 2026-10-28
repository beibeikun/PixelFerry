import { useEffect, useRef, useState, type PointerEvent } from "react";
import { renderPreview } from "@/lib/preview";
import { active, type Doc, type Layer, type Preview } from "@/lib/model";
import {
  bounds,
  union,
  snap,
  world,
  resized,
  intersects,
  type Box,
} from "@/lib/geometry";
import { useStore } from "@/lib/store";
type Point = { x: number; y: number };
type Drag = {
  type: "move" | "resize" | "rotate" | "marquee";
  start: Point;
  layers: Layer[];
  previous: string[];
  anchor?: [number, number];
  started: boolean;
};
export function PixelCanvas({
  doc,
  output,
  mode,
  zoom,
}: {
  doc: Doc;
  output?: Preview | null;
  mode: "edit" | "output";
  zoom: number;
}) {
  const latest = useRef(doc);
  latest.current = doc;
  const canvas = useRef<HTMLCanvasElement>(null),
    svg = useRef<SVGSVGElement>(null),
    container = useRef<HTMLDivElement>(null);
  const [editor, setEditor] = useState<Preview | null>(null),
    [available, setAvailable] = useState({ width: 800, height: 700 }),
    [guides, setGuides] = useState<{ v: number | null; h: number | null }>({
      v: null,
      h: null,
    }),
    [marquee, setMarquee] = useState<Box | null>(null);
  const drag = useRef<Drag | null>(null);
  const selected = useStore((s) => s.selected);
  const textEditorOpen = useStore((s) => s.textEditorOpen);
  const scene = active(doc);
  const preview = mode === "output" ? output : editor;
  const width = mode === "output" && output ? output.width : scene.width,
    height = mode === "output" && output ? output.height : scene.height;
  const scale = Math.max(
    0.03,
    Math.min(
      (available.width - 96) / width,
      (available.height - 100) / height,
      1.5,
    ) * zoom,
  );
  useEffect(() => {
    if (!container.current) return;
    const o = new ResizeObserver(([e]) =>
      setAvailable({
        width: e.contentRect.width,
        height: e.contentRect.height,
      }),
    );
    o.observe(container.current);
    return () => o.disconnect();
  }, []);
  useEffect(() => {
    if (mode !== "edit" || textEditorOpen) return;
    let alive = true,
      timer: ReturnType<typeof setTimeout>;
    const draw = async () => {
      try {
        const p = await renderPreview(latest.current);
        if (alive) setEditor(p);
      } catch (e) {
        if (alive) useStore.setState({ error: `画面生成失败：${String(e)}` });
      }
      if (alive) timer = setTimeout(() => void draw(), 120);
    };
    void draw();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [mode, textEditorOpen]);
  useEffect(() => {
    const c = canvas.current;
    if (!c || !preview) return;
    const bytes = Uint8ClampedArray.from(atob(preview.rgba), (c) =>
      c.charCodeAt(0),
    );
    if (bytes.length !== preview.width * preview.height * 4) return;
    c.width = preview.width;
    c.height = preview.height;
    c.getContext("2d")?.putImageData(
      new ImageData(bytes, preview.width, preview.height),
      0,
      0,
    );
  }, [preview]);
  const point = (e: PointerEvent): Point => {
    const r = svg.current!.getBoundingClientRect();
    return {
      x: ((e.clientX - r.left) * scene.width) / r.width,
      y: ((e.clientY - r.top) * scene.height) / r.height,
    };
  };
  const start = (
    e: PointerEvent,
    l?: Layer,
    type: Drag["type"] = "move",
    anchor?: [number, number],
  ) => {
    if (mode !== "edit" || e.button !== 0) return;
    e.stopPropagation();
    svg.current?.setPointerCapture(e.pointerId);
    const store = useStore.getState(),
      add = e.shiftKey || e.ctrlKey || e.metaKey;
    let ids = store.selected;
    if (l) {
      if (l.locked) return;
      if (!ids.includes(l.id)) ids = add ? [...ids, l.id] : [l.id];
      else if (add && type === "move") {
        ids = ids.filter((id) => id !== l.id);
        store.select(ids);
        return;
      }
      store.select(ids);
    } else if (!add) {
      ids = [];
      store.select([]);
    }
    drag.current = {
      type: l ? type : "marquee",
      start: point(e),
      layers: scene.layers
        .filter(
          (item) =>
            ids.includes(item.id) &&
            item.visible &&
            !item.locked &&
            (type === "move" || !l || item.id === l.id),
        )
        .map((l) => structuredClone(l)),
      previous: ids,
      anchor,
      started: false,
    };
  };
  const move = (e: PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const p = point(e),
      store = useStore.getState();
    if (d.type === "marquee") {
      const b = {
        x: Math.min(d.start.x, p.x),
        y: Math.min(d.start.y, p.y),
        width: Math.abs(p.x - d.start.x),
        height: Math.abs(p.y - d.start.y),
      };
      setMarquee(b);
      store.select([
        ...new Set([
          ...d.previous,
          ...scene.layers
            .filter((l) => l.visible && !l.locked && intersects(b, bounds(l)))
            .map((l) => l.id),
        ]),
      ]);
      return;
    }
    if (!d.started) {
      if (Math.hypot(p.x - d.start.x, p.y - d.start.y) < 0.5) return;
      store.begin();
      d.started = true;
    }
    const others = scene.layers
      .filter((l) => l.visible && !d.layers.some((x) => x.id === l.id))
      .map(bounds);
    let updates: Layer[] = [];
    if (d.type === "move") {
      let dx = p.x - d.start.x,
        dy = p.y - d.start.y;
      const b = union(d.layers.map(bounds));
      if (!e.altKey) {
        const s = snap(
          { ...b, x: b.x + dx, y: b.y + dy },
          others,
          scene.width,
          scene.height,
        );
        dx += s.dx;
        dy += s.dy;
        setGuides({ v: s.v, h: s.h });
      } else setGuides({ v: null, h: null });
      updates = d.layers.map((l) => ({
        ...l,
        x: Math.round(l.x + dx),
        y: Math.round(l.y + dy),
      }));
    }
    if (d.type === "resize") {
      let q = p;
      if (!e.altKey) {
        const s = snap(
          { x: p.x, y: p.y, width: 0, height: 0 },
          others,
          scene.width,
          scene.height,
        );
        q = { x: p.x + s.dx, y: p.y + s.dy };
        setGuides({ v: s.v, h: s.h });
      } else setGuides({ v: null, h: null });
      updates = [resized(d.layers[0], d.anchor![0], d.anchor![1], q, e.altKey)];
    }
    if (d.type === "rotate") {
      const l = d.layers[0],
        center = world(l, l.width / 2, l.height / 2);
      let angle =
        (Math.atan2(p.y - center.y, p.x - center.x) * 180) / Math.PI + 90;
      if (e.shiftKey) angle = Math.round(angle / 15) * 15;
      const a = (angle * Math.PI) / 180;
      updates = [
        {
          ...l,
          rotation: angle,
          x:
            center.x -
            (l.width / 2) * Math.cos(a) +
            (l.height / 2) * Math.sin(a),
          y:
            center.y -
            (l.width / 2) * Math.sin(a) -
            (l.height / 2) * Math.cos(a),
        },
      ];
    }
    store.edit((doc) => {
      const s = active(doc);
      for (const l of updates) {
        const index = s.layers.findIndex((x) => x.id === l.id);
        if (index >= 0) s.layers[index] = l;
      }
    }, false);
  };
  const end = () => {
    drag.current = null;
    setGuides({ v: null, h: null });
    setMarquee(null);
  };
  return (
    <div className="canvas-surface" ref={container}>
      <div className="canvas-caption">
        <span className="status-dot" />
        {mode === "edit" ? "场景预览" : "设备实时画面"}
        <span>
          {width} × {height} px
        </span>
      </div>
      <div
        className="canvas-frame"
        style={{ width: width * scale + 24, height: height * scale + 24 }}
      >
        <div
          className="stage"
          style={{ width: width * scale, height: height * scale }}
        >
          <canvas
            ref={canvas}
            aria-label="Rust 场景画面"
            style={{ width: "100%", height: "100%" }}
          />
          {mode === "output" && !output && (
            <div className="canvas-wait">正在等待像素靠岸…</div>
          )}
          {mode === "edit" && (
            <svg
              ref={svg}
              viewBox={`0 0 ${scene.width} ${scene.height}`}
              className="canvas-overlay"
              onPointerDown={(e) => start(e)}
              onPointerMove={move}
              onPointerUp={end}
              onPointerCancel={end}
            >
              {scene.layers
                .filter((l) => l.visible)
                .map((l) => (
                  <g
                    key={l.id}
                    transform={`translate(${l.x} ${l.y}) rotate(${l.rotation})`}
                  >
                    <rect
                      data-layer-id={l.id}
                      width={l.width}
                      height={l.height}
                      fill="transparent"
                      style={{
                        pointerEvents: l.locked ? "none" : "auto",
                        cursor: "move",
                      }}
                      onPointerDown={(e) => start(e, l)}
                    />
                    {selected.includes(l.id) && (
                      <>
                        <rect
                          width={l.width}
                          height={l.height}
                          fill="none"
                          stroke="#72d2b4"
                          strokeWidth={1.5 / scale}
                          pointerEvents="none"
                        />
                        {selected.length === 1 && !l.locked && (
                          <>
                            {[
                              [0, 0],
                              [0.5, 0],
                              [1, 0],
                              [0, 0.5],
                              [1, 0.5],
                              [0, 1],
                              [0.5, 1],
                              [1, 1],
                            ].map(([x, y]) => (
                              <rect
                                key={`${x}-${y}`}
                                x={l.width * x - 4 / scale}
                                y={l.height * y - 4 / scale}
                                width={8 / scale}
                                height={8 / scale}
                                rx={1 / scale}
                                fill="#f8fff9"
                                stroke="#359b7a"
                                strokeWidth={1 / scale}
                                style={{
                                  cursor:
                                    x === 0.5
                                      ? "ns-resize"
                                      : y === 0.5
                                        ? "ew-resize"
                                        : x === y
                                          ? "nwse-resize"
                                          : "nesw-resize",
                                }}
                                onPointerDown={(e) =>
                                  start(e, l, "resize", [x, y])
                                }
                              />
                            ))}
                            <line
                              x1={l.width / 2}
                              x2={l.width / 2}
                              y1={0}
                              y2={-24 / scale}
                              stroke="#72d2b4"
                              strokeWidth={1 / scale}
                            />
                            <circle
                              cx={l.width / 2}
                              cy={-24 / scale}
                              r={5 / scale}
                              fill="#f5aa83"
                              stroke="#fff"
                              strokeWidth={1 / scale}
                              style={{ cursor: "grab" }}
                              onPointerDown={(e) => start(e, l, "rotate")}
                            />
                          </>
                        )}
                      </>
                    )}
                  </g>
                ))}
              {guides.v !== null && (
                <line
                  x1={guides.v}
                  x2={guides.v}
                  y1={0}
                  y2={scene.height}
                  stroke="#72d2b4"
                  strokeWidth={1 / scale}
                  strokeDasharray={`${4 / scale} ${4 / scale}`}
                />
              )}
              {guides.h !== null && (
                <line
                  x1={0}
                  x2={scene.width}
                  y1={guides.h}
                  y2={guides.h}
                  stroke="#72d2b4"
                  strokeWidth={1 / scale}
                  strokeDasharray={`${4 / scale} ${4 / scale}`}
                />
              )}
              {marquee && (
                <rect
                  {...marquee}
                  fill="#72d2b422"
                  stroke="#72d2b4"
                  strokeWidth={1 / scale}
                />
              )}
            </svg>
          )}
        </div>
      </div>
      <div className="canvas-note">
        {mode === "edit"
          ? "拖动组件 · Shift 多选 · Alt 暂停吸附"
          : "每一帧，正在被送往你的屏幕"}
      </div>
    </div>
  );
}
