import type { Doc, Preview, Scene } from "./model";

export type Thumbnail = { key: string; url: string; time: number };
export const thumbnailCache = new Map<string, Thumbnail>();

export function thumbnailKey(doc: Doc, scene: Scene) {
  const ids = new Set(scene.layers.map((layer) => layer.assetId));
  return JSON.stringify([
    scene,
    doc.assets.filter((asset) => ids.has(asset.id)),
    doc.deepseek,
  ]);
}
export function dynamicScene(scene: Scene) {
  return scene.layers.some(
    (layer) =>
      layer.visible &&
      (["clock", "sensor", "deepseek", "bar", "video"].includes(layer.type) ||
        layer.richText?.runs.some((run) => run.type !== "text")),
  );
}
export function pruneThumbnails(doc: Doc) {
  const ids = new Set(doc.scenes.map((scene) => scene.id));
  for (const id of thumbnailCache.keys()) {
    if (!ids.has(id)) thumbnailCache.delete(id);
  }
}
export function thumbnailURL(frame: Preview) {
  if (
    !Number.isInteger(frame.width) ||
    !Number.isInteger(frame.height) ||
    frame.width < 1 ||
    frame.height < 1 ||
    frame.width > 320 ||
    frame.height > 320
  )
    throw Error("缩略图尺寸无效");
  const bytes = Uint8ClampedArray.from(atob(frame.rgba), (c) =>
    c.charCodeAt(0),
  );
  if (bytes.length !== frame.width * frame.height * 4)
    throw Error("缩略图数据不完整");
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw Error("无法生成缩略图");
  ctx.putImageData(new ImageData(bytes, frame.width, frame.height), 0, 0);
  return canvas.toDataURL("image/png");
}
