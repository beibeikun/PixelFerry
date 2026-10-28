import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Check, Loader2, Plus, RefreshCw, type LucideIcon } from "lucide-react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./ui/tabs";
import { Button } from "./ui/button";
import { labels, type Doc, type Kind, type Scene } from "@/lib/model";
import { useStore } from "@/lib/store";
import { renderThumbnail } from "@/lib/preview";
import {
  dynamicScene,
  pruneThumbnails,
  thumbnailCache,
  thumbnailKey,
  thumbnailURL,
} from "@/lib/thumbnails";
import { cn } from "@/lib/utils";

const descriptions: Record<Kind, string> = {
  text: "固定文字与动态参数混排",
  clock: "显示当前时间",
  sensor: "显示硬件监测数据",
  deepseek: "余额、消费与峰谷时段",
  bar: "传感器数值或时段进度",
  rect: "背景、色块与圆角面板",
  circle: "圆形装饰与色块",
  image: "图片与静态画面",
  video: "视频与动态媒体",
};
const kinds: Kind[] = [
  "text",
  "clock",
  "sensor",
  "deepseek",
  "bar",
  "rect",
  "circle",
  "image",
  "video",
];

export function SceneLibrary({
  doc,
  busy,
  icons,
  onNew,
  onSelect,
  onAdd,
  footer,
}: {
  doc: Doc;
  busy: boolean;
  icons: Record<Kind, LucideIcon>;
  onNew: () => void;
  onSelect: (id: string) => void;
  onAdd: (kind: Kind) => void;
  footer: ReactNode;
}) {
  const paused = useStore((s) => s.textEditorOpen);
  const keys = useMemo(
    () => doc.scenes.map((scene) => thumbnailKey(doc, scene)),
    [doc],
  );
  useEffect(() => pruneThumbnails(doc), [doc]);
  return (
    <Tabs defaultValue="scenes" className="library-tabs">
      <TabsList aria-label="场景与组件库" className="library-tab-list">
        <TabsTrigger value="scenes">场景库</TabsTrigger>
        <TabsTrigger value="components">组件库</TabsTrigger>
      </TabsList>
      <TabsContent value="scenes" className="scene-library-content">
        <div className="library-section-heading">
          <span>{doc.scenes.length} 个场景</span>
          <Button
            variant="ghost"
            size="sm"
            aria-label="新建场景"
            disabled={busy || doc.scenes.length >= 32}
            onClick={onNew}
          >
            <Plus size={14} />
            新建
          </Button>
        </div>
        <div className="scene-scroll">
          <div className="scene-list">
            {doc.scenes.map((scene, index) => (
              <SceneCard
                key={scene.id}
                doc={doc}
                scene={scene}
                fingerprint={keys[index]}
                index={index}
                busy={busy}
                paused={paused}
                onSelect={onSelect}
              />
            ))}
          </div>
        </div>
        {footer}
      </TabsContent>
      <TabsContent value="components" className="component-library-content">
        <p className="component-library-hint">点击组件，添加到当前场景</p>
        <div className="component-grid">
          {kinds.map((kind) => {
            const Icon = icons[kind];
            return (
              <button
                key={kind}
                className="component-card"
                aria-label={`添加${labels[kind]}`}
                disabled={busy}
                onClick={() => onAdd(kind)}
              >
                <span className="component-card-icon">
                  <Icon size={23} />
                </span>
                <strong>{labels[kind]}</strong>
                <small>{descriptions[kind]}</small>
              </button>
            );
          })}
        </div>
        <p className="component-library-hint">
          图片和动态媒体添加后，可在右侧属性中选择素材。
        </p>
      </TabsContent>
    </Tabs>
  );
}

function SceneCard({
  doc,
  scene,
  fingerprint,
  index,
  busy,
  paused,
  onSelect,
}: {
  doc: Doc;
  scene: Scene;
  fingerprint: string;
  index: number;
  busy: boolean;
  paused: boolean;
  onSelect: (id: string) => void;
}) {
  const element = useRef<HTMLDivElement>(null);
  const latest = useRef({ doc, fingerprint });
  latest.current = { doc, fingerprint };
  const [visible, setVisible] = useState(false);
  const [image, setImage] = useState(() => thumbnailCache.get(scene.id)?.url);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);
  const dynamic = dynamicScene(scene);
  useEffect(() => {
    const observer = new IntersectionObserver(
      ([entry]) => setVisible(entry.isIntersecting),
      { threshold: 0.01 },
    );
    if (element.current) observer.observe(element.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!visible || paused) {
      setLoading(false);
      return;
    }
    const controller = new AbortController();
    const signal = controller.signal;
    let timer: ReturnType<typeof setTimeout>;
    setError("");
    const load = async () => {
      setLoading(true);
      try {
        const frame = await renderThumbnail(
          latest.current.doc,
          scene.id,
          signal,
        );
        if (signal.aborted || latest.current.fingerprint !== fingerprint)
          return;
        const url = thumbnailURL(frame);
        thumbnailCache.set(scene.id, {
          key: fingerprint,
          url,
          time: Date.now(),
        });
        setImage(url);
        setError("");
        if (dynamic) timer = setTimeout(() => void load(), 10_000);
      } catch (e) {
        if (!signal.aborted) setError(String(e));
      } finally {
        if (!signal.aborted) setLoading(false);
      }
    };
    const cached = thumbnailCache.get(scene.id);
    if (cached?.key === fingerprint && retry === 0) {
      setImage(cached.url);
      setLoading(false);
      if (dynamic)
        timer = setTimeout(
          () => void load(),
          Math.max(300, 10_000 - (Date.now() - cached.time)),
        );
    } else {
      timer = setTimeout(() => void load(), 300);
    }
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [fingerprint, scene.id, visible, paused, dynamic, retry]);
  return (
    <div
      ref={element}
      className={cn("scene-card", scene.id === doc.activeSceneId && "selected")}
    >
      <button
        className="scene-card-select"
        aria-label={`切换到场景：${scene.name}`}
        aria-current={scene.id === doc.activeSceneId ? "true" : undefined}
        disabled={busy}
        onClick={() => onSelect(scene.id)}
      >
        <div className="scene-thumbnail">
          {image ? (
            <img src={image} alt={`${scene.name}的场景预览`} />
          ) : (
            <span className="thumbnail-placeholder">
              {error ? "预览暂不可用" : "正在生成预览…"}
            </span>
          )}
          {loading && !paused && visible && (
            <Loader2
              className="thumbnail-spinner animate-spin"
              size={14}
              aria-label="正在更新缩略图"
            />
          )}
        </div>
        <div className="scene-card-title">
          <span>{scene.name}</span>
          {scene.id === doc.activeSceneId ? (
            <Check size={14} />
          ) : (
            <span className="scene-index">
              {String(index + 1).padStart(2, "0")}
            </span>
          )}
        </div>
        <span className="muted text-xs">
          {scene.width} × {scene.height} · {scene.layers.length} 个组件
        </span>
      </button>
      {error && (
        <button
          className="thumbnail-retry"
          title={error}
          disabled={busy}
          onClick={() => setRetry((value) => value + 1)}
        >
          <RefreshCw size={12} />
          重试预览
        </button>
      )}
    </div>
  );
}
