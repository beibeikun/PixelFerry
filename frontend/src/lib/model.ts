import type { DeepSeekState, DeepSeekPreferences } from "../shared/deepseek";
import { validateRichText, utf8, type RichText } from "./rich-text";
export type Kind =
  | "image"
  | "video"
  | "text"
  | "clock"
  | "sensor"
  | "bar"
  | "rect"
  | "circle"
  | "deepseek";
export type Fit = "contain" | "cover";
export interface Layer {
  id: string;
  type: Kind;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  rotation: number;
  opacity: number;
  visible: boolean;
  locked: boolean;
  color: string;
  text: string;
  richText?: RichText;
  fontSize: number;
  fontFamily: string;
  align: "left" | "center" | "right";
  verticalAlign?: "top" | "middle" | "bottom";
  bold: boolean;
  assetId?: string;
  sensorId?: string;
  min: number;
  max: number;
  fit: Fit;
  cornerRadius: number;
  glass: boolean;
  glassBlur: number;
  glassTintOpacity: number;
  deepseekMetric: "summary" | "balance" | "today" | "total" | "period";
  barBackground: string;
  barSource: "sensor" | "deepseek-period";
  barDirection: "elapsed" | "remaining";
}
export interface Scene {
  id: string;
  name: string;
  background: string;
  width: number;
  height: number;
  layers: Layer[];
}
export interface Asset {
  id: string;
  name: string;
  file: string;
  kind: "image" | "video";
  width: number;
  height: number;
  duration?: number;
}
export interface Settings {
  brightness: number;
  direction: number;
  power: boolean;
  autoStart: boolean;
  quality: number;
  renderFps: number;
}
export interface Binding {
  deviceId: string;
  screenId: string;
  sceneId: string;
  fit: Fit;
  settings: Settings;
}
export interface Doc {
  version: 2;
  scenes: Scene[];
  activeSceneId: string;
  assets: Asset[];
  settings: Settings;
  deepseek: DeepSeekPreferences;
  bindings: Binding[];
  migratedFrom?: string | null;
}
export interface Screen {
  id: string;
  name: string;
  width: number;
  height: number;
  brightness: boolean;
  direction: boolean;
  power: boolean;
  encoding: "jpeg" | "rgba8" | "rgb565le";
  repeatFps: number;
}
export interface Device {
  id: string;
  adapter: string;
  name: string;
  serial?: string | null;
  container?: string | null;
  screens: Screen[];
  requiresConfirmation: boolean;
}
export interface DeviceStatus {
  deviceId: string;
  state: string;
  message: string;
  fps: number;
  bytesPerSecond: number;
  frames: number;
  errors: number;
  screenFrames: Record<string, number>;
}
export interface Sensor {
  id: string;
  name: string;
  unit: string;
  value: number | null;
  timestamp: number;
  status: string;
  sourceId: number;
  gpuIndex: number;
  device: string;
}
export interface Preview {
  width: number;
  height: number;
  rgba: string;
  sequence: number;
}
export interface Snapshot {
  version: number;
  devices: Device[];
  statuses: DeviceStatus[];
  data: {
    sensors: Sensor[];
    sensor_error?: string | null;
    deepseek: DeepSeekState;
  };
  preview?: Preview | null;
  error?: string | null;
}
export const labels: Record<Kind, string> = {
  image: "图片",
  video: "动态媒体",
  text: "文字",
  clock: "时钟",
  sensor: "传感器",
  bar: "进度条",
  rect: "矩形",
  circle: "圆形",
  deepseek: "DeepSeek",
};
export function layer(kind: Kind): Layer {
  return {
    id: crypto.randomUUID(),
    type: kind,
    name: labels[kind],
    x: 40,
    y: 40,
    width: ["text", "sensor", "clock", "deepseek"].includes(kind) ? 360 : 200,
    height:
      kind === "bar"
        ? 22
        : ["text", "sensor", "clock", "deepseek"].includes(kind)
          ? 64
          : 200,
    rotation: 0,
    opacity: 1,
    visible: true,
    locked: false,
    color: "#e6efff",
    text: kind === "text" ? "你好，像素世界" : "",
    fontSize: 32,
    fontFamily: "Microsoft YaHei",
    align: "left",
    verticalAlign: "middle",
    bold: false,
    min: 0,
    max: 100,
    fit: "cover",
    cornerRadius: 12,
    glass: false,
    glassBlur: 12,
    glassTintOpacity: 0.18,
    deepseekMetric: "summary",
    barBackground: "#29364c",
    barSource: "sensor",
    barDirection: "elapsed",
  };
}
export function blankScene(width = 480, height = 480): Scene {
  return {
    id: crypto.randomUUID(),
    name: "新的航线",
    background: "#101827",
    width,
    height,
    layers: [],
  };
}
export function active(doc: Doc) {
  return doc.scenes.find((s) => s.id === doc.activeSceneId) ?? doc.scenes[0];
}
export function validate(doc: Doc) {
  if (doc.scenes.length < 1 || doc.scenes.length > 32)
    throw Error("需要保留 1–32 个场景");
  if (doc.assets.length > 1000) throw Error("素材库最多 1000 个素材");
  if (doc.bindings.length > 8) throw Error("最多同时绑定 8 块屏幕");
  if (!doc.scenes.some((s) => s.id === doc.activeSceneId))
    throw Error("活动场景不存在");
  for (const s of doc.scenes) {
    if (
      s.width < 1 ||
      s.height < 1 ||
      s.width > 8192 ||
      s.height > 8192 ||
      s.width * s.height > 16777216
    )
      throw Error("画布尺寸超限");
    if (s.layers.length > 128) throw Error("每个场景最多 128 个组件");
    if (s.layers.filter((l) => l.visible && l.type === "video").length > 1)
      throw Error("每个场景最多一个可见动态媒体图层");
    for (const l of s.layers) {
      if (
        l.verticalAlign !== undefined &&
        !["top", "middle", "bottom"].includes(l.verticalAlign)
      )
        throw Error("垂直对齐无效");
      if (utf8(l.text) > 16000) throw Error("文字过长");
      if (l.richText) {
        if (l.type !== "text") throw Error("只有文字组件支持混排");
        validateRichText(l.richText);
      }
      if (
        ![l.x, l.y, l.width, l.height, l.rotation, l.opacity].every(
          Number.isFinite,
        ) ||
        l.width < 4 ||
        l.height < 4 ||
        l.width > 8192 ||
        l.height > 8192 ||
        Math.abs(l.x) > 16384 ||
        Math.abs(l.y) > 16384
      )
        throw Error("组件尺寸或位置无效");
    }
  }
  if (
    doc.bindings.filter(
      (b) =>
        b.settings.power &&
        doc.scenes
          .find((s) => s.id === b.sceneId)
          ?.layers.some((l) => l.visible && l.type === "video"),
    ).length > 4
  )
    throw Error("最多同时播放 4 路动态输出");
}
