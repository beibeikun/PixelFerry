import type { Layer } from "./model";
export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}
export function world(l: Layer, x: number, y: number) {
  const a = (l.rotation * Math.PI) / 180,
    c = Math.cos(a),
    s = Math.sin(a);
  return { x: l.x + x * c - y * s, y: l.y + x * s + y * c };
}
export function bounds(l: Layer): Box {
  const p = [
    world(l, 0, 0),
    world(l, l.width, 0),
    world(l, 0, l.height),
    world(l, l.width, l.height),
  ];
  const x = Math.min(...p.map((p) => p.x)),
    y = Math.min(...p.map((p) => p.y));
  return {
    x,
    y,
    width: Math.max(...p.map((p) => p.x)) - x,
    height: Math.max(...p.map((p) => p.y)) - y,
  };
}
export function union(boxes: Box[]): Box {
  if (!boxes.length) return { x: 0, y: 0, width: 0, height: 0 };
  const x = Math.min(...boxes.map((b) => b.x)),
    y = Math.min(...boxes.map((b) => b.y));
  return {
    x,
    y,
    width: Math.max(...boxes.map((b) => b.x + b.width)) - x,
    height: Math.max(...boxes.map((b) => b.y + b.height)) - y,
  };
}
export function intersects(a: Box, b: Box) {
  return (
    a.x <= b.x + b.width &&
    a.x + a.width >= b.x &&
    a.y <= b.y + b.height &&
    a.y + a.height >= b.y
  );
}
export function snap(b: Box, others: Box[], width: number, height: number) {
  const nearest = (
    start: number,
    size: number,
    max: number,
    boxes: [number, number][],
  ) => {
    const targets = [
      0,
      max / 2,
      max,
      ...boxes.flatMap(([x, w]) => [x, x + w / 2, x + w]),
    ];
    let delta = 0,
      line: number | null = null,
      best = 6;
    for (const a of [start, start + size / 2, start + size])
      for (const t of targets) {
        const d = t - a;
        if (Math.abs(d) <= 5 && Math.abs(d) < best) {
          delta = d;
          line = t;
          best = Math.abs(d);
        }
      }
    return { delta, line };
  };
  const x = nearest(
      b.x,
      b.width,
      width,
      others.map((b) => [b.x, b.width]),
    ),
    y = nearest(
      b.y,
      b.height,
      height,
      others.map((b) => [b.y, b.height]),
    );
  return { dx: x.delta, dy: y.delta, v: x.line, h: y.line };
}
export function resized(
  l: Layer,
  ax: number,
  ay: number,
  p: { x: number; y: number },
  centered = false,
): Layer {
  const a = (l.rotation * Math.PI) / 180,
    c = Math.cos(a),
    s = Math.sin(a),
    dx = p.x - l.x,
    dy = p.y - l.y,
    px = dx * c + dy * s,
    py = -dx * s + dy * c,
    fx = centered ? 0.5 : 1 - ax,
    fy = centered ? 0.5 : 1 - ay;
  let w = ax === 0.5 ? l.width : (px - l.width * fx) / (ax - fx),
    h = ay === 0.5 ? l.height : (py - l.height * fy) / (ay - fy);
  if (ax !== 0.5 && ay !== 0.5) {
    const ratio = Math.max(w / l.width, h / l.height);
    w = l.width * ratio;
    h = l.height * ratio;
  }
  w = Math.max(4, Math.min(8192, w));
  h = Math.max(4, Math.min(8192, h));
  const fixed = world(l, l.width * fx, l.height * fy);
  return {
    ...l,
    width: w,
    height: h,
    x: fixed.x - w * fx * c + h * fy * s,
    y: fixed.y - w * fx * s - h * fy * c,
  };
}
