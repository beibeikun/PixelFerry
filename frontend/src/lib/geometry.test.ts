import { describe, it, expect } from "vitest";
import { layer } from "./model";
import { bounds, world, resized, snap, union } from "./geometry";
describe("editor geometry", () => {
  it("snaps against arbitrary resolution edges and center", () => {
    expect(
      snap({ x: 318, y: 70, width: 100, height: 100 }, [], 640, 480).dx,
    ).toBe(2);
    expect(snap({ x: 610, y: 0, width: 28, height: 20 }, [], 640, 480).dx).toBe(
      2,
    );
  });
  it("resizes a rotated layer around its opposite corner", () => {
    const l = { ...layer("rect"), width: 100, height: 100, rotation: 30 };
    const n = resized(l, 1, 1, world(l, 200, 200));
    expect(n.width).toBeCloseTo(200);
    expect(world(n, 0, 0)).toEqual(world(l, 0, 0));
  });
  it("Alt resize preserves center", () => {
    const l = layer("rect"),
      n = resized(l, 1, 0.5, world(l, 500, 100), true);
    expect(world(n, n.width / 2, n.height / 2).x).toBeCloseTo(
      world(l, l.width / 2, l.height / 2).x,
    );
  });
  it("uses rotated bounds for multi-selection alignment", () => {
    const l = {
      ...layer("rect"),
      x: 0,
      y: 0,
      width: 100,
      height: 20,
      rotation: 90,
    };
    expect(bounds(l).width).toBeCloseTo(20);
    expect(
      union([bounds(l), { x: 100, y: 100, width: 50, height: 50 }]).width,
    ).toBeCloseTo(170);
  });
});
