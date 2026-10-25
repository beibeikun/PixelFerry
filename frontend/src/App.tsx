import { useEffect, useState, useRef, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Anchor,
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  Circle,
  Clock,
  Copy,
  Download,
  Eye,
  EyeOff,
  FolderOpen,
  Gauge,
  Image as ImageIcon,
  Layers,
  Loader2,
  Lock,
  Monitor,
  Moon,
  MoreHorizontal,
  Move,
  PanelRight,
  Pause,
  Play,
  Plus,
  Power,
  Redo2,
  RefreshCw,
  Save,
  Settings as SettingsIcon,
  Ship,
  SlidersHorizontal,
  Sparkles,
  Square,
  Sun,
  Trash2,
  Type,
  Undo2,
  Unlock,
  Upload,
  Usb,
  Video,
  Waves,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Slider } from "@/components/ui/slider";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from "@/components/ui/tooltip";
import { Textarea } from "@/components/ui/textarea";
import { PixelCanvas } from "@/components/Canvas";
import { api, native } from "@/lib/api";
import {
  active,
  blankScene,
  layer,
  labels,
  type Doc,
  type Kind,
  type Layer,
  type Device,
  type Screen,
  type Snapshot,
  type Binding,
} from "@/lib/model";
import { useStore, flush, operation } from "@/lib/store";
import { bounds, union } from "@/lib/geometry";
import {
  initialDeepSeek,
  pricingProgress,
  deepSeekText,
} from "./shared/deepseek";
import { cn } from "@/lib/utils";
const icons: Record<Kind, typeof Type> = {
  text: Type,
  clock: Clock,
  sensor: Gauge,
  bar: SlidersHorizontal,
  rect: Square,
  circle: Circle,
  image: ImageIcon,
  video: Video,
  deepseek: Sparkles,
};
const empty: Snapshot = {
  version: 1,
  devices: [],
  statuses: [],
  data: { sensors: [], deepseek: initialDeepSeek },
};
function Ferry({ small = false }: { small?: boolean }) {
  return (
    <svg
      viewBox="0 0 64 64"
      width={small ? 28 : 44}
      height={small ? 28 : 44}
      aria-hidden="true"
    >
      <path d="M10 35h44l-9 15H21z" fill="currentColor" />
      <path d="M23 15h19v20H23z" fill="currentColor" opacity=".75" />
      <path d="M17 25h30v10H17z" fill="currentColor" />
      <rect
        x="28"
        y="19"
        width="8"
        height="7"
        rx="1"
        fill="var(--background)"
      />
      <path
        d="M9 54q6-5 12 0t12 0t12 0t12 0"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
      />
    </svg>
  );
}
function IconButton({
  title,
  children,
  onClick,
  disabled = false,
}: {
  title: string;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            aria-label={title}
            title={title}
            variant="ghost"
            size="icon"
            disabled={disabled}
            onClick={onClick}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{title}</TooltipContent>
    </Tooltip>
  );
}
function NumberField({
  label,
  value,
  onChange,
  min = -16384,
  max = 16384,
  step = 1,
}: {
  label: string;
  value: number;
  onChange: (n: number) => void;
  min?: number;
  max?: number;
  step?: number;
}) {
  const [draft, setDraft] = useState(String(value));
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setDraft(String(Math.round(value * 100) / 100));
  }, [value]);
  return (
    <label className="number-field">
      <span>{label}</span>
      <Input
        type="number"
        aria-label={label}
        value={draft}
        min={min}
        max={max}
        step={step}
        onFocus={() => {
          focused.current = true;
        }}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          focused.current = false;
          const n = Number(draft);
          if (draft !== "" && Number.isFinite(n) && n >= min && n <= max)
            onChange(n);
          else setDraft(String(value));
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
        }}
      />
    </label>
  );
}
function ColorField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (c: string) => void;
}) {
  return (
    <label className="color-field">
      <span>{label}</span>
      <div>
        <input
          type="color"
          aria-label={label}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
        <code>{value.toUpperCase()}</code>
      </div>
    </label>
  );
}
function SelectField({
  label,
  value,
  onChange,
  children,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  children: ReactNode;
}) {
  return (
    <label className="select-field">
      <span>{label}</span>
      <div>
        <select
          aria-label={label}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        >
          {children}
        </select>
        <ChevronDown size={14} />
      </div>
    </label>
  );
}
function ToggleField({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="toggle-field">
      <span>{label}</span>
      <Switch aria-label={label} checked={checked} onCheckedChange={onChange} />
    </label>
  );
}

export function App() {
  const s = useStore(),
    doc = s.doc;
  const [snapshot, setSnapshot] = useState<Snapshot>(empty),
    [route, setRoute] = useState<"studio" | "devices" | "assets">("studio"),
    [mode, setMode] = useState<"edit" | "output">("edit"),
    [target, setTarget] = useState<[string, string] | null>(null),
    [theme, setTheme] = useState(
      () => localStorage.getItem("pixelferry-theme") ?? "light",
    ),
    [account, setAccount] = useState(false),
    [info, setInfo] = useState({ version: "0.2.0", simulate: false }),
    [notice, setNotice] = useState(""),
    [zoom, setZoom] = useState(1),
    [rightTab, setRightTab] = useState("properties"),
    [exportAll, setExportAll] = useState(false);
  const selection = useRef(target);
  selection.current = mode === "output" ? target : null;
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
    localStorage.setItem("pixelferry-theme", theme);
  }, [theme]);
  useEffect(() => {
    let alive = true,
      timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const t = selection.current;
        const data = await api.snapshot(t?.[0], t?.[1]);
        if (alive) setSnapshot(data);
      } catch (e) {
        if (alive) useStore.setState({ error: String(e) });
      }
      if (alive) timer = setTimeout(() => void poll(), 450);
    };
    if (!native()) {
      useStore.setState({
        error: "请使用 PixelFerry 桌面版。浏览器预览不会连接 USB 设备。",
      });
      return;
    }
    void api
      .load()
      .then((d) => {
        if (alive) useStore.getState().accept(d);
      })
      .catch((e) => useStore.setState({ error: String(e) }));
    void api.info().then((i) => {
      if (alive) setInfo(i);
    });
    void poll();
    const handlers = [
      listen("pf-hide", () => {
        void flush().catch(() => {});
      }),
      listen("pf-quit", () => {
        void flush()
          .then(() => api.quit())
          .catch((e) => useStore.setState({ error: String(e) }));
      }),
    ];
    return () => {
      alive = false;
      clearTimeout(timer);
      for (const p of handlers) void p.then((off) => off());
    };
  }, []);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (
        (e.target as HTMLElement)?.closest(
          'input,textarea,select,[contenteditable="true"]',
        ) ||
        useStore.getState().busy
      )
        return;
      const state = useStore.getState();
      if (!state.doc) return;
      const command = e.ctrlKey || e.metaKey;
      if (command && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void flush().catch(() => {});
      }
      if (command && e.key.toLowerCase() === "z") {
        e.preventDefault();
        e.shiftKey ? state.redo() : state.undo();
      }
      if (command && e.key.toLowerCase() === "a") {
        e.preventDefault();
        state.select(
          active(state.doc)
            .layers.filter((l) => l.visible && !l.locked)
            .map((l) => l.id),
        );
      }
      if (e.key === "Escape") state.select([]);
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        state.remove();
      }
      const n = e.shiftKey ? 10 : 1;
      const delta: Record<string, [number, number]> = {
        ArrowLeft: [-n, 0],
        ArrowRight: [n, 0],
        ArrowUp: [0, -n],
        ArrowDown: [0, n],
      };
      if (delta[e.key]) {
        e.preventDefault();
        const [dx, dy] = delta[e.key];
        state.edit((doc) => {
          for (const l of active(doc).layers)
            if (state.selected.includes(l.id) && l.visible && !l.locked) {
              l.x += dx;
              l.y += dy;
            }
        });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
  async function run(job: () => Promise<void>) {
    try {
      await job();
    } catch (e) {
      useStore.setState({ error: String(e) });
    }
  }
  async function addMedia() {
    await run(async () => {
      await operation(async () => {
        const a = await api.media();
        if (a) {
          useStore.setState({ busy: false });
          s.edit((d) => d.assets.push(a));
          setRoute("assets");
          setNotice("新素材已登船，点击卡片即可添加到场景。");
        }
      });
    });
  }
  async function exchange(kind: "import" | "export" | "migrate") {
    await run(() =>
      operation(async () => {
        const d = useStore.getState().doc!;
        if (kind === "export") {
          if (await api.export(d, exportAll))
            setNotice("场景已打包，下一站也能继续播放。");
        }
        if (kind === "import") {
          const next = await api.import(d);
          if (next) {
            s.accept(next);
            setNotice("场景已上船，素材和布局都在。");
          }
        }
        if (kind === "migrate") {
          const next = await api.migrate(d);
          if (next) {
            s.accept(next.doc);
            setNotice(
              next.notices.join("；") ||
                "旧版场景和设置已迁移，原目录保持不变。",
            );
          }
        }
      }),
    );
  }
  function addAsset(id: string) {
    const a = doc?.assets.find((a) => a.id === id);
    if (!a) return;
    const l = layer(a.kind);
    l.assetId = a.id;
    l.name = a.name;
    l.x = 0;
    l.y = 0;
    l.width = active(doc!).width;
    l.height = active(doc!).height;
    s.edit((d) => active(d).layers.push(l));
    s.select([l.id]);
    setRoute("studio");
    setMode("edit");
  }
  const count = snapshot.devices.reduce((n, d) => n + d.screens.length, 0),
    connected = snapshot.statuses.filter((d) => d.state === "connected").length;
  return (
    <div className="app-shell">
      <aside className="nav-rail">
        <div className="brand-mark" title="PixelFerry · 像素渡轮">
          <Ferry />
        </div>
        <nav>
          {(
            [
              { id: "studio", label: "工作台", icon: Layers },
              { id: "devices", label: "设备", icon: Monitor },
              { id: "assets", label: "素材库", icon: FolderOpen },
            ] as const
          ).map((n) => (
            <button
              key={n.id}
              className={cn("rail-item", route === n.id && "active")}
              onClick={() => setRoute(n.id)}
              aria-label={n.label}
            >
              <n.icon size={21} />
              <span>{n.label}</span>
            </button>
          ))}
        </nav>
        <div className="rail-bottom">
          <IconButton
            title={theme === "light" ? "切换深海主题" : "切换奶油主题"}
            onClick={() => setTheme(theme === "light" ? "dark" : "light")}
          >
            {theme === "light" ? <Moon /> : <Sun />}
          </IconButton>
          <IconButton title="设置与账户" onClick={() => setAccount(true)}>
            <SettingsIcon />
          </IconButton>
          <span className="rail-version">v{info.version}</span>
        </div>
      </aside>
      <div className="app-body">
        <header className="app-header">
          <div className="wordmark">
            PixelFerry<span>像素渡轮</span>
            <Badge variant="secondary">
              {info.simulate ? "模拟航行" : "像素已就位"}
            </Badge>
          </div>
          <div className="header-actions">
            <span className="connection-pill">
              <span className={cn("status-dot", connected ? "" : "idle")} />
              {connected} 台设备在线
            </span>
            <Button
              variant="ghost"
              onClick={() => void exchange("migrate")}
              disabled={s.busy}
            >
              <Upload size={15} />
              迁移旧版
            </Button>
            <IconButton
              title="完全退出"
              disabled={s.busy}
              onClick={() =>
                void run(async () => {
                  await flush();
                  await api.quit();
                })
              }
            >
              <Power size={17} />
            </IconButton>
          </div>
        </header>
        {!doc ? (
          <div className="loading-page">
            <Ferry />
            <h2>正在把像素搬上船…</h2>
            <p>{s.error || "连接本地 Rust 引擎"}</p>
          </div>
        ) : (
          <div className="app-content">
            <aside className="library-panel">
              <div className="library-heading">
                <div>
                  <span className="eyebrow">YOUR LITTLE WORLD</span>
                  <h2>
                    {route === "studio"
                      ? "场景航线"
                      : route === "devices"
                        ? "设备港口"
                        : "素材行李箱"}
                  </h2>
                </div>
                {route === "studio" ? (
                  <IconButton
                    title="新建场景"
                    disabled={s.busy || doc.scenes.length >= 32}
                    onClick={() => {
                      s.edit((d) => {
                        const current = active(d),
                          scene = blankScene(current.width, current.height);
                        d.scenes.push(scene);
                        d.activeSceneId = scene.id;
                      });
                      s.select([]);
                      setMode("edit");
                    }}
                  >
                    <Plus />
                  </IconButton>
                ) : (
                  <Ship className="soft-icon" size={25} />
                )}
              </div>
              {route === "studio" ? (
                <>
                  <div className="scene-list">
                    {doc.scenes.map((scene, index) => (
                      <button
                        key={scene.id}
                        className={cn(
                          "scene-card",
                          scene.id === doc.activeSceneId && "selected",
                        )}
                        disabled={s.busy}
                        onClick={() => {
                          s.edit((d) => {
                            d.activeSceneId = scene.id;
                          }, false);
                          s.select([]);
                          setMode("edit");
                        }}
                      >
                        <div
                          className="scene-thumbnail"
                          style={{ background: scene.background }}
                        >
                          <span>
                            {scene.layers
                              .find((l) => l.type === "text")
                              ?.text.slice(0, 25) || "新世界"}
                          </span>
                          <small>
                            {scene.layers.some((l) => l.type === "video") ? (
                              <Video size={13} />
                            ) : (
                              <Waves size={18} />
                            )}
                          </small>
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
                          {scene.width} × {scene.height} · {scene.layers.length}{" "}
                          个组件
                        </span>
                      </button>
                    ))}
                  </div>
                  <div className="library-footer">
                    <Button
                      variant="outline"
                      onClick={() => void exchange("import")}
                      disabled={s.busy}
                    >
                      <Download size={15} />
                      导入场景包
                    </Button>
                    <label className="export-scope">
                      <input
                        type="checkbox"
                        checked={exportAll}
                        onChange={(e) => setExportAll(e.target.checked)}
                      />
                      导出全部场景
                    </label>
                    <Button
                      variant="ghost"
                      onClick={() => void exchange("export")}
                      disabled={s.busy}
                    >
                      <Upload size={15} />
                      导出{exportAll ? "全部" : "当前"}场景
                    </Button>
                    <div className="ferry-tip">
                      <Anchor size={18} />
                      <p>
                        每一块小屏幕，
                        <br />
                        都值得一个自己的世界。
                      </p>
                    </div>
                  </div>
                </>
              ) : (
                <>
                  <div className="library-info">
                    <span className="big-count">
                      {route === "devices" ? count : doc.assets.length}
                      <span>{route === "devices" ? "块屏幕" : "件素材"}</span>
                    </span>
                    <p>
                      {route === "devices"
                        ? "同一套场景，可以前往不同设备。每块屏幕都能独立设置。"
                        : "图片、GIF 或视频，把喜欢的画面带上船。"}
                    </p>
                  </div>
                  <div className="ferry-illustration">
                    <Ferry />
                    <Waves />
                    <span>small screen, big stories.</span>
                  </div>
                  {route === "assets" && (
                    <Button onClick={() => void addMedia()} disabled={s.busy}>
                      <Plus size={16} />
                      导入素材
                    </Button>
                  )}
                </>
              )}
            </aside>
            <main className="workspace">
              {route === "studio" ? (
                <>
                  <div className="workspace-toolbar">
                    <div className="workspace-title">
                      <span className="toolbar-icon">
                        <Layers size={17} />
                      </span>
                      <span>{active(doc).name}</span>
                      <ChevronRight size={14} />
                      <span className="muted">
                        {mode === "edit" ? "场景编辑" : "设备画面"}
                      </span>
                    </div>
                    <div className="toolbar-controls">
                      <span className="save-state">
                        <span
                          className={cn(
                            "status-dot",
                            s.serial === s.saved ? "" : "pending",
                          )}
                        />
                        {s.serial === s.saved ? "已保存" : "保存中"}
                      </span>
                      <div className="toolbar-divider" />
                      <IconButton
                        title="撤销 Ctrl+Z"
                        onClick={s.undo}
                        disabled={s.busy || !s.past.length}
                      >
                        <Undo2 />
                      </IconButton>
                      <IconButton
                        title="重做 Ctrl+Shift+Z"
                        onClick={s.redo}
                        disabled={s.busy || !s.future.length}
                      >
                        <Redo2 />
                      </IconButton>
                      <IconButton
                        title="立即保存 Ctrl+S"
                        onClick={() => void run(flush)}
                        disabled={s.busy}
                      >
                        <Save />
                      </IconButton>
                    </div>
                  </div>
                  <div className="editor-tools">
                    <div className="tool-palette">
                      {(Object.keys(labels) as Kind[]).map((kind) => {
                        const Icon = icons[kind];
                        return (
                          <Tooltip key={kind}>
                            <TooltipTrigger
                              render={
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  aria-label={`添加${labels[kind]}`}
                                  disabled={s.busy}
                                  onClick={() => {
                                    s.add(kind);
                                    setMode("edit");
                                    setRightTab("properties");
                                  }}
                                />
                              }
                            >
                              <Icon size={18} />
                            </TooltipTrigger>
                            <TooltipContent>添加{labels[kind]}</TooltipContent>
                          </Tooltip>
                        );
                      })}
                    </div>
                    <div className="editor-mode">
                      <button
                        className={mode === "edit" ? "selected" : ""}
                        onClick={() => setMode("edit")}
                      >
                        编辑
                      </button>
                      <button
                        className={mode === "output" ? "selected" : ""}
                        disabled={!target}
                        onClick={() => setMode("output")}
                      >
                        设备预览
                      </button>
                    </div>
                  </div>
                  <PixelCanvas
                    doc={doc}
                    mode={mode}
                    output={snapshot.preview}
                    zoom={zoom}
                  />
                  <div className="workspace-status">
                    <span>
                      <Move size={12} />
                      {s.selected.length
                        ? `已选 ${s.selected.length} 个组件`
                        : "选择组件开始编辑"}
                    </span>
                    <div>
                      <IconButton
                        title="缩小画布"
                        onClick={() => setZoom(Math.max(0.25, zoom - 0.1))}
                      >
                        <ZoomOut size={14} />
                      </IconButton>
                      <button onClick={() => setZoom(1)}>
                        {Math.round(zoom * 100)}%
                      </button>
                      <IconButton
                        title="放大画布"
                        onClick={() => setZoom(Math.min(2, zoom + 0.1))}
                      >
                        <ZoomIn size={14} />
                      </IconButton>
                    </div>
                    <span className="engine-label">
                      像素渡轮
                      <span className="status-dot" />
                    </span>
                  </div>
                </>
              ) : route === "devices" ? (
                <section className="page-content">
                  <div className="page-title">
                    <div>
                      <span className="eyebrow">ALL ABOARD</span>
                      <h1>给每块屏幕，安排一场旅行。</h1>
                      <p>插入设备后自动发现，选择场景即可开始播放。</p>
                    </div>
                    <Badge variant="secondary">
                      <Usb size={13} />
                      {info.simulate ? "模拟设备" : "USB 本地连接"}
                    </Badge>
                  </div>
                  <div className="device-grid">
                    {snapshot.devices.map((d) => (
                      <article className="device-card" key={d.id}>
                        <div className="device-card-top">
                          <div className="device-art">
                            <Monitor size={34} />
                            <span className="status-dot" />
                          </div>
                          <Badge variant="outline">
                            {d.adapter === "simulator" ? "SIMULATOR" : d.name}
                          </Badge>
                        </div>
                        <h2>{d.name.replace("模拟设备 · ", "")}</h2>
                        <p className="device-subtitle">
                          {d.serial ? `序列号 ${d.serial}` : "USB 屏幕设备"} ·{" "}
                          {d.screens.length} 块屏幕
                        </p>
                        <p className="device-live">
                          {snapshot.statuses.find((s) => s.deviceId === d.id)
                            ?.message || "等待场景登船"}
                        </p>
                        {d.screens.map((screen) => {
                          const b = doc.bindings.find(
                            (b) =>
                              b.deviceId === d.id && b.screenId === screen.id,
                          );
                          return (
                            <div className="screen-row" key={screen.id}>
                              <div>
                                <span>{screen.name}</span>
                                <small>
                                  {screen.width} × {screen.height}
                                </small>
                              </div>
                              <Button
                                variant={b ? "secondary" : "default"}
                                size="sm"
                                disabled={s.busy}
                                onClick={() => {
                                  if (!b)
                                    s.edit((doc) =>
                                      doc.bindings.push({
                                        deviceId: d.id,
                                        screenId: screen.id,
                                        sceneId: doc.activeSceneId,
                                        fit: "contain",
                                        settings: structuredClone(doc.settings),
                                      }),
                                    );
                                  setTarget([d.id, screen.id]);
                                  if (b) {
                                    setRoute("studio");
                                    setMode("output");
                                  }
                                }}
                              >
                                {b ? (
                                  <>
                                    <Eye size={13} />
                                    查看画面
                                  </>
                                ) : d.requiresConfirmation ? (
                                  "确认并绑定"
                                ) : (
                                  "绑定场景"
                                )}
                              </Button>
                              <IconButton
                                title={`${screen.name}设置`}
                                onClick={() => setTarget([d.id, screen.id])}
                              >
                                <SlidersHorizontal size={15} />
                              </IconButton>
                            </div>
                          );
                        })}
                        <div className="device-card-bottom">
                          <span>
                            {snapshot.statuses
                              .find((s) => s.deviceId === d.id)
                              ?.fps.toFixed(1) || "0"}{" "}
                            FPS
                          </span>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => void run(() => api.reconnect(d.id))}
                          >
                            <RefreshCw size={13} />
                            重连
                          </Button>
                        </div>
                      </article>
                    ))}
                  </div>
                  {!snapshot.devices.length && (
                    <div className="empty-page">
                      <Monitor />
                      <h2>港口暂时很安静</h2>
                      <p>连接你的 USB 屏幕，它就会出现在这里。</p>
                    </div>
                  )}
                </section>
              ) : (
                <section className="page-content">
                  <div className="page-title">
                    <div>
                      <span className="eyebrow">PIXELS IN YOUR POCKET</span>
                      <h1>把喜欢的画面，装进行李箱。</h1>
                      <p>支持图片、GIF 与视频，导入后点击卡片加入当前场景。</p>
                    </div>
                    <Button onClick={() => void addMedia()} disabled={s.busy}>
                      <Plus size={16} />
                      导入素材
                    </Button>
                  </div>
                  <div className="asset-grid">
                    {doc.assets.map((a) => (
                      <button
                        className="asset-card"
                        key={a.id}
                        disabled={s.busy}
                        onClick={() => addAsset(a.id)}
                      >
                        <div className={cn("asset-art", a.kind)}>
                          {a.kind === "video" ? (
                            <Play size={32} />
                          ) : (
                            <ImageIcon size={34} />
                          )}
                          <span className="asset-add">
                            <Plus size={15} />
                          </span>
                        </div>
                        <strong>{a.name}</strong>
                        <span>
                          {a.width} × {a.height}
                          {a.duration ? ` · ${Math.round(a.duration)}s` : ""}
                          <Badge variant="secondary">
                            {a.kind === "video" ? "VIDEO" : "IMAGE"}
                          </Badge>
                        </span>
                      </button>
                    ))}
                  </div>
                  {!doc.assets.length && (
                    <div className="empty-page">
                      <ImageIcon />
                      <h2>还没有素材登船</h2>
                      <p>从一张喜欢的图片，或一段小视频开始吧。</p>
                      <Button variant="outline" onClick={() => void addMedia()}>
                        选择素材
                      </Button>
                    </div>
                  )}
                </section>
              )}
            </main>
            <aside className="inspector">
              <Tabs
                value={rightTab}
                onValueChange={(v) => setRightTab(v as string)}
              >
                <TabsList className="inspector-tabs">
                  <TabsTrigger value="properties">
                    <SlidersHorizontal size={14} />
                    属性
                  </TabsTrigger>
                  <TabsTrigger value="layers">
                    <Layers size={14} />
                    图层
                  </TabsTrigger>
                </TabsList>
                <TabsContent value="properties">
                  <Properties
                    doc={doc}
                    snapshot={snapshot}
                    target={target}
                    setAccount={() => setAccount(true)}
                    route={route}
                  />
                </TabsContent>
                <TabsContent value="layers">
                  <LayerList doc={doc} />
                </TabsContent>
              </Tabs>
            </aside>
          </div>
        )}
        <footer className="app-footer">
          <span>
            <Waves size={13} />
            把画面送往不同 USB 设备
          </span>
          <span>
            {info.simulate
              ? "模拟模式，不发送 USB 数据"
              : "本地运行 · 数据留在你的电脑"}
            <span className="footer-dot" />
            PixelFerry {info.version}
          </span>
        </footer>
      </div>
      {s.busy && (
        <div className="busy-cover" role="status">
          <div>
            <Loader2 className="animate-spin" />
            <h3>正在搬运像素…</h3>
            <p>后台屏幕继续播放，请稍候。</p>
          </div>
        </div>
      )}
      {(s.error || notice) && (
        <div className={cn("notice", s.error && "error")} role="status">
          <span>{s.error || notice}</span>
          <button
            aria-label="关闭提示"
            onClick={() => {
              useStore.setState({ error: "" });
              setNotice("");
            }}
          >
            <X size={15} />
          </button>
        </div>
      )}
      {doc && (
        <AccountDialog
          open={account}
          onOpenChange={setAccount}
          doc={doc}
          snapshot={snapshot}
        />
      )}
    </div>
  );
}

function Properties({
  doc,
  snapshot,
  target,
  setAccount,
  route,
}: {
  doc: Doc;
  snapshot: Snapshot;
  target: [string, string] | null;
  setAccount: () => void;
  route: string;
}) {
  const s = useStore(),
    scene = active(doc),
    selected = scene.layers.filter((l) => s.selected.includes(l.id)),
    l = selected.length === 1 ? selected[0] : null;
  const patch = (p: Partial<Layer>) => {
    if (l) s.patch(l.id, p);
  };
  function align(kind: string) {
    const ls = selected.filter((l) => !l.locked);
    const b = union(ls.map(bounds));
    s.edit((d) => {
      for (const l of active(d).layers.filter((l) =>
        ls.some((x) => x.id === l.id),
      )) {
        const lb = bounds(l);
        if (kind === "canvasX") l.x += (scene.width - b.width) / 2 - b.x;
        else if (kind === "canvasY") l.y += (scene.height - b.height) / 2 - b.y;
        else if (kind === "left") l.x += b.x - lb.x;
        else if (kind === "right") l.x += b.x + b.width - lb.x - lb.width;
        else if (kind === "top") l.y += b.y - lb.y;
        else if (kind === "bottom") l.y += b.y + b.height - lb.y - lb.height;
        else if (kind === "centerX")
          l.x += b.x + b.width / 2 - lb.x - lb.width / 2;
        else l.y += b.y + b.height / 2 - lb.y - lb.height / 2;
      }
    });
  }
  return (
    <div className="inspector-content">
      {route === "devices" ? (
        <OutputSettings doc={doc} snapshot={snapshot} target={target} />
      ) : (
        <>
          {selected.length > 1 ? (
            <>
              <section className="property-section">
                <div className="property-title">
                  <h3>一起移动</h3>
                  <Badge variant="secondary">{selected.length} 个组件</Badge>
                </div>
                <div className="align-grid">
                  {[
                    ["left", "左对齐"],
                    ["centerX", "水平中心"],
                    ["right", "右对齐"],
                    ["top", "顶对齐"],
                    ["centerY", "垂直中心"],
                    ["bottom", "底对齐"],
                  ].map(([key, label]) => (
                    <Button
                      variant="outline"
                      size="sm"
                      key={key}
                      onClick={() => align(key)}
                    >
                      {label}
                    </Button>
                  ))}
                </div>
              </section>
            </>
          ) : l ? (
            <>
              <section className="property-section">
                <div className="property-title">
                  <h3>组件属性</h3>
                  <Badge variant="secondary">{labels[l.type]}</Badge>
                </div>
                <Input
                  aria-label="图层名称"
                  value={l.name}
                  onChange={(e) => patch({ name: e.target.value })}
                />
                <div className="property-grid">
                  <NumberField
                    label="X"
                    value={l.x}
                    onChange={(x) => patch({ x })}
                  />
                  <NumberField
                    label="Y"
                    value={l.y}
                    onChange={(y) => patch({ y })}
                  />
                  <NumberField
                    label="宽度"
                    value={l.width}
                    min={4}
                    max={8192}
                    onChange={(width) => patch({ width })}
                  />
                  <NumberField
                    label="高度"
                    value={l.height}
                    min={4}
                    max={8192}
                    onChange={(height) => patch({ height })}
                  />
                  <NumberField
                    label="旋转"
                    value={l.rotation}
                    min={-360}
                    max={360}
                    onChange={(rotation) => patch({ rotation })}
                  />
                  <NumberField
                    label="不透明度 %"
                    value={Math.round(l.opacity * 100)}
                    min={0}
                    max={100}
                    onChange={(n) => patch({ opacity: n / 100 })}
                  />
                </div>
                <ToggleField
                  label="锁定组件"
                  checked={l.locked}
                  onChange={(locked) => patch({ locked })}
                />
                <ToggleField
                  label="显示组件"
                  checked={l.visible}
                  onChange={(visible) => patch({ visible })}
                />
              </section>
              {["text", "clock", "sensor", "deepseek"].includes(l.type) && (
                <section className="property-section">
                  <h3>文字样式</h3>
                  {l.type === "text" && (
                    <Textarea
                      aria-label="文字内容"
                      value={l.text}
                      onChange={(e) => patch({ text: e.target.value })}
                    />
                  )}
                  <div className="property-grid">
                    <NumberField
                      label="字号"
                      value={l.fontSize}
                      min={8}
                      max={240}
                      onChange={(fontSize) => patch({ fontSize })}
                    />
                    <SelectField
                      label="字重"
                      value={l.bold ? "bold" : "normal"}
                      onChange={(v) => patch({ bold: v === "bold" })}
                    >
                      <option value="normal">常规</option>
                      <option value="bold">粗体</option>
                    </SelectField>
                  </div>
                  <Input
                    aria-label="字体"
                    value={l.fontFamily}
                    onChange={(e) => patch({ fontFamily: e.target.value })}
                  />
                  <SelectField
                    label="文字对齐"
                    value={l.align}
                    onChange={(v) => patch({ align: v as Layer["align"] })}
                  >
                    <option value="left">左对齐</option>
                    <option value="center">居中</option>
                    <option value="right">右对齐</option>
                  </SelectField>
                  {l.type === "sensor" && (
                    <ToggleField
                      label="显示整数"
                      checked={l.text === "integer"}
                      onChange={(v) => patch({ text: v ? "integer" : "" })}
                    />
                  )}
                </section>
              )}
              <section className="property-section">
                <h3>外观</h3>
                <ColorField
                  label="颜色"
                  value={l.color}
                  onChange={(color) => patch({ color })}
                />
                {["rect", "circle"].includes(l.type) && (
                  <>
                    {l.type === "rect" && (
                      <NumberField
                        label="圆角"
                        value={l.cornerRadius}
                        min={0}
                        max={1000}
                        onChange={(cornerRadius) => patch({ cornerRadius })}
                      />
                    )}
                    <ToggleField
                      label="毛玻璃"
                      checked={l.glass}
                      onChange={(glass) => patch({ glass })}
                    />
                    {l.glass && (
                      <>
                        <NumberField
                          label="背景模糊"
                          value={l.glassBlur}
                          min={0}
                          max={40}
                          onChange={(glassBlur) => patch({ glassBlur })}
                        />
                        <NumberField
                          label="着色强度 %"
                          value={Math.round(l.glassTintOpacity * 100)}
                          min={0}
                          max={100}
                          onChange={(n) => patch({ glassTintOpacity: n / 100 })}
                        />
                        <p className="field-help">
                          模糊组件下方的画面，效果会进入设备输出。
                        </p>
                      </>
                    )}
                  </>
                )}
                {l.type === "bar" && (
                  <ColorField
                    label="进度条底色"
                    value={l.barBackground}
                    onChange={(barBackground) => patch({ barBackground })}
                  />
                )}
              </section>
              {["sensor", "bar"].includes(l.type) && (
                <section className="property-section">
                  <h3>实时数据</h3>
                  {l.type === "bar" && (
                    <>
                      <SelectField
                        label="进度来源"
                        value={l.barSource}
                        onChange={(v) =>
                          patch({ barSource: v as Layer["barSource"] })
                        }
                      >
                        <option value="sensor">Afterburner 传感器</option>
                        <option value="deepseek-period">
                          DeepSeek 峰谷进度
                        </option>
                      </SelectField>
                      <div className="property-grid">
                        <NumberField
                          label="最小值"
                          value={l.min}
                          onChange={(min) => patch({ min })}
                        />
                        <NumberField
                          label="最大值"
                          value={l.max}
                          onChange={(max) => patch({ max })}
                        />
                      </div>
                    </>
                  )}
                  {l.type === "sensor" || l.barSource === "sensor" ? (
                    <>
                      <SelectField
                        label="传感器"
                        value={l.sensorId ?? ""}
                        onChange={(sensorId) =>
                          patch({ sensorId: sensorId || undefined })
                        }
                      >
                        <option value="">选择数据源</option>
                        {snapshot.data.sensors.map((sensor) => (
                          <option key={sensor.id} value={sensor.id}>
                            {sensor.name} {sensor.device}
                          </option>
                        ))}
                      </SelectField>
                      {!snapshot.data.sensors.length && (
                        <p className="field-help">
                          请运行 Afterburner 并启用监控项。数据不可用时显示 --。
                        </p>
                      )}
                    </>
                  ) : (
                    <>
                      <SelectField
                        label="进度方向"
                        value={l.barDirection}
                        onChange={(v) =>
                          patch({ barDirection: v as Layer["barDirection"] })
                        }
                      >
                        <option value="elapsed">已过时间</option>
                        <option value="remaining">剩余时间</option>
                      </SelectField>
                      <p className="field-help">
                        按北京时间的连续峰谷区间计算，无需 Key。当前已过{" "}
                        {Math.round(
                          (pricingProgress(Date.now(), doc.deepseek).ratio ??
                            0) * 100,
                        )}
                        %。
                      </p>
                    </>
                  )}
                </section>
              )}
              {l.type === "deepseek" && (
                <section className="property-section">
                  <h3>DeepSeek 组件</h3>
                  <SelectField
                    label="显示内容"
                    value={l.deepseekMetric}
                    onChange={(v) =>
                      patch({ deepseekMetric: v as Layer["deepseekMetric"] })
                    }
                  >
                    {[
                      ["summary", "综合信息"],
                      ["balance", "余额"],
                      ["today", "今日估算"],
                      ["total", "累计估算"],
                      ["period", "峰谷时段"],
                    ].map(([v, n]) => (
                      <option key={v} value={v}>
                        {n}
                      </option>
                    ))}
                  </SelectField>
                  <Button variant="outline" onClick={setAccount}>
                    <SettingsIcon size={14} />
                    账户与日历
                  </Button>
                </section>
              )}
              {["image", "video"].includes(l.type) && (
                <section className="property-section">
                  <h3>媒体</h3>
                  <SelectField
                    label="素材"
                    value={l.assetId ?? ""}
                    onChange={(assetId) =>
                      patch({ assetId: assetId || undefined })
                    }
                  >
                    <option value="">选择素材</option>
                    {doc.assets
                      .filter((a) => a.kind === l.type)
                      .map((a) => (
                        <option key={a.id} value={a.id}>
                          {a.name}
                        </option>
                      ))}
                  </SelectField>
                  <SelectField
                    label="填充方式"
                    value={l.fit}
                    onChange={(v) => patch({ fit: v as Layer["fit"] })}
                  >
                    <option value="contain">等比留边</option>
                    <option value="cover">居中裁切</option>
                  </SelectField>
                </section>
              )}
              <section className="property-section">
                <div className="button-pair">
                  <Button
                    variant="outline"
                    onClick={() => {
                      s.edit((d) => {
                        const ls = active(d).layers,
                          i = ls.findIndex((x) => x.id === l.id);
                        if (i < ls.length - 1)
                          [ls[i], ls[i + 1]] = [ls[i + 1], ls[i]];
                      });
                    }}
                  >
                    <ArrowUp size={14} />
                    上移
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() =>
                      s.edit((d) => {
                        const ls = active(d).layers,
                          i = ls.findIndex((x) => x.id === l.id);
                        if (i > 0) [ls[i], ls[i - 1]] = [ls[i - 1], ls[i]];
                      })
                    }
                  >
                    <ArrowDown size={14} />
                    下移
                  </Button>
                </div>
                <Button
                  variant="destructive"
                  onClick={s.remove}
                  disabled={l.locked}
                >
                  <Trash2 size={14} />
                  删除组件
                </Button>
              </section>
            </>
          ) : (
            <section className="property-section">
              <div className="property-title">
                <h3>画布设置</h3>
                <Square size={15} />
              </div>
              <Input
                aria-label="场景名称"
                value={scene.name}
                onChange={(e) => {
                  if (e.target.value.trim())
                    s.edit((d) => {
                      active(d).name = e.target.value;
                    });
                }}
              />
              <div className="property-grid">
                <NumberField
                  label="画布宽度"
                  value={scene.width}
                  min={16}
                  max={4096}
                  onChange={(width) =>
                    s.edit((d) => {
                      active(d).width = width;
                    })
                  }
                />
                <NumberField
                  label="画布高度"
                  value={scene.height}
                  min={16}
                  max={4096}
                  onChange={(height) =>
                    s.edit((d) => {
                      active(d).height = height;
                    })
                  }
                />
              </div>
              <ColorField
                label="背景颜色"
                value={scene.background}
                onChange={(background) =>
                  s.edit((d) => {
                    active(d).background = background;
                  })
                }
              />
              <p className="field-help">
                画布保持自己的尺寸，在不同屏幕上可等比留边或裁切。
              </p>
              <div className="button-pair">
                <Button
                  variant="outline"
                  disabled={doc.scenes.length >= 32}
                  onClick={() =>
                    s.edit((d) => {
                      const copy = structuredClone(active(d));
                      copy.id = crypto.randomUUID();
                      copy.name += " 副本";
                      copy.layers.forEach((l) => (l.id = crypto.randomUUID()));
                      d.scenes.push(copy);
                      d.activeSceneId = copy.id;
                    })
                  }
                >
                  <Copy size={14} />
                  复制
                </Button>
                <Button
                  variant="destructive"
                  disabled={doc.scenes.length <= 1}
                  onClick={() =>
                    s.edit((d) => {
                      const id = d.activeSceneId,
                        index = d.scenes.findIndex((s) => s.id === id);
                      d.scenes.splice(index, 1);
                      d.activeSceneId =
                        d.scenes[Math.min(index, d.scenes.length - 1)].id;
                      d.bindings.forEach((b) => {
                        if (b.sceneId === id) b.sceneId = d.activeSceneId;
                      });
                    })
                  }
                >
                  <Trash2 size={14} />
                  删除
                </Button>
              </div>
            </section>
          )}
          {!!selected.length && (
            <section className="property-section">
              <h3>画布对齐</h3>
              <div className="button-pair">
                <Button variant="outline" onClick={() => align("canvasX")}>
                  水平居中
                </Button>
                <Button variant="outline" onClick={() => align("canvasY")}>
                  垂直居中
                </Button>
              </div>
            </section>
          )}
          <OutputSettings doc={doc} snapshot={snapshot} target={target} />
        </>
      )}
    </div>
  );
}

function OutputSettings({
  doc,
  snapshot,
  target,
}: {
  doc: Doc;
  snapshot: Snapshot;
  target: [string, string] | null;
}) {
  const s = useStore(),
    device = snapshot.devices.find((d) => d.id === target?.[0]),
    screen = device?.screens.find((x) => x.id === target?.[1]),
    binding = doc.bindings.find(
      (b) => b.deviceId === device?.id && b.screenId === screen?.id,
    );
  function update(fn: (b: Binding) => void) {
    s.edit((d) => {
      const b = d.bindings.find(
        (b) => b.deviceId === device?.id && b.screenId === screen?.id,
      );
      if (b) fn(b);
    });
  }
  return (
    <section className="property-section output-settings">
      <div className="property-title">
        <h3>屏幕输出</h3>
        <Monitor size={15} />
      </div>
      {!device || !screen ? (
        <div className="inspector-empty">
          <Monitor size={23} />
          <p>
            前往设备港口，
            <br />
            选择一块屏幕。
          </p>
        </div>
      ) : (
        <>
          <p className="output-name">
            {device.name}
            <span>
              {screen.width} × {screen.height}
            </span>
          </p>
          {!binding ? (
            <Button
              onClick={() =>
                s.edit((d) =>
                  d.bindings.push({
                    deviceId: device.id,
                    screenId: screen.id,
                    sceneId: d.activeSceneId,
                    fit: "contain",
                    settings: structuredClone(d.settings),
                  }),
                )
              }
            >
              绑定当前场景
            </Button>
          ) : (
            <>
              <SelectField
                label="播放场景"
                value={binding.sceneId}
                onChange={(sceneId) => update((b) => (b.sceneId = sceneId))}
              >
                {doc.scenes.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </SelectField>
              <SelectField
                label="画面适配"
                value={binding.fit}
                onChange={(fit) =>
                  update((b) => (b.fit = fit as Binding["fit"]))
                }
              >
                <option value="contain">等比留边</option>
                <option value="cover">居中裁切</option>
              </SelectField>
              {screen.power && (
                <ToggleField
                  label="屏幕开启"
                  checked={binding.settings.power}
                  onChange={(power) =>
                    update((b) => (b.settings.power = power))
                  }
                />
              )}
              {screen.brightness && (
                <label className="slider-field">
                  <span>
                    亮度<strong>{binding.settings.brightness}%</strong>
                  </span>
                  <Slider
                    aria-label="亮度"
                    value={[binding.settings.brightness]}
                    min={0}
                    max={100}
                    step={1}
                    onValueChange={(v) =>
                      update(
                        (b) =>
                          (b.settings.brightness = Array.isArray(v) ? v[0] : v),
                      )
                    }
                  />
                </label>
              )}
              {screen.direction && (
                <SelectField
                  label="安装方向"
                  value={String(binding.settings.direction)}
                  onChange={(v) =>
                    update((b) => (b.settings.direction = Number(v)))
                  }
                >
                  {[0, 1, 2, 3].map((v) => (
                    <option key={v} value={v}>
                      {v * 90}°
                    </option>
                  ))}
                </SelectField>
              )}
              {screen.encoding === "jpeg" && (
                <NumberField
                  label="JPEG 质量"
                  value={binding.settings.quality}
                  min={40}
                  max={90}
                  onChange={(quality) =>
                    update((b) => (b.settings.quality = quality))
                  }
                />
              )}
              <NumberField
                label="渲染帧率"
                value={binding.settings.renderFps}
                min={1}
                max={30}
                onChange={(renderFps) =>
                  update((b) => (b.settings.renderFps = renderFps))
                }
              />
              <Button
                variant="ghost"
                onClick={() =>
                  s.edit((d) => {
                    d.bindings = d.bindings.filter(
                      (b) =>
                        b.deviceId !== device.id || b.screenId !== screen.id,
                    );
                  })
                }
              >
                解除场景绑定
              </Button>
            </>
          )}
        </>
      )}
    </section>
  );
}

function LayerList({ doc }: { doc: Doc }) {
  const s = useStore();
  return (
    <div className="inspector-content">
      <div className="property-section">
        <div className="property-title">
          <h3>图层顺序</h3>
          <span className="muted text-xs">上方图层优先显示</span>
        </div>
        {[...active(doc).layers].reverse().map((l) => {
          const Icon = icons[l.type];
          return (
            <div
              key={l.id}
              className={cn(
                "layer-row",
                s.selected.includes(l.id) && "selected",
              )}
            >
              <button
                className="layer-select"
                onClick={(e) =>
                  s.select(
                    e.shiftKey || e.ctrlKey
                      ? s.selected.includes(l.id)
                        ? s.selected.filter((id) => id !== l.id)
                        : [...s.selected, l.id]
                      : [l.id],
                  )
                }
              >
                <Icon size={15} />
                <span>{l.name}</span>
              </button>
              <button
                title={l.visible ? "隐藏" : "显示"}
                aria-label={`${l.name}${l.visible ? "隐藏" : "显示"}`}
                onClick={() => s.patch(l.id, { visible: !l.visible })}
              >
                {l.visible ? <Eye size={13} /> : <EyeOff size={13} />}
              </button>
              <button
                title={l.locked ? "解锁" : "锁定"}
                onClick={() => s.patch(l.id, { locked: !l.locked })}
              >
                {l.locked ? <Lock size={13} /> : <Unlock size={13} />}
              </button>
            </div>
          );
        })}
      </div>
      <div className="field-help px-4">
        Shift / Ctrl 点击可多选。选中组件后按方向键微调位置。
      </div>
    </div>
  );
}

function AccountDialog({
  open,
  onOpenChange,
  doc,
  snapshot,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  doc: Doc;
  snapshot: Snapshot;
}) {
  const s = useStore(),
    [key, setKey] = useState(""),
    [holidays, setHolidays] = useState(doc.deepseek.holidays.join("\n")),
    [years, setYears] = useState(doc.deepseek.calendarYears.join(",")),
    [pending, setPending] = useState(false);
  useEffect(() => {
    if (open) {
      setHolidays(doc.deepseek.holidays.join("\n"));
      setYears(doc.deepseek.calendarYears.join(","));
    } else setKey("");
  }, [open]);
  async function job(fn: () => Promise<void>) {
    setPending(true);
    try {
      await fn();
    } catch (e) {
      useStore.setState({ error: String(e) });
    } finally {
      setPending(false);
    }
  }
  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!pending) onOpenChange(v);
      }}
    >
      <DialogContent className="account-dialog">
        <DialogHeader>
          <div className="dialog-icon">
            <Ferry small />
          </div>
          <DialogTitle>让像素航行得更自在</DialogTitle>
          <DialogDescription>
            账户、节假日日历与应用偏好，都在这里。
          </DialogDescription>
        </DialogHeader>
        <Tabs defaultValue="account">
          <TabsList className="w-full">
            <TabsTrigger value="account">DeepSeek</TabsTrigger>
            <TabsTrigger value="calendar">峰谷日历</TabsTrigger>
            <TabsTrigger value="app">应用</TabsTrigger>
          </TabsList>
          <TabsContent value="account">
            <div className="account-summary">
              <Badge variant="secondary">
                {snapshot.data.deepseek.status === "ok"
                  ? "已连接"
                  : snapshot.data.deepseek.configured
                    ? "连接待检查"
                    : "尚未配置"}
              </Badge>
              <pre>
                {deepSeekText(
                  "summary",
                  snapshot.data.deepseek,
                  doc.deepseek,
                  Date.now(),
                )}
              </pre>
            </div>
            <label className="form-label">
              API Key
              <Input
                type="password"
                autoComplete="off"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                placeholder="输入后保存在 Windows 加密存储"
                aria-label="DeepSeek API Key"
              />
            </label>
            <div className="button-pair">
              <Button
                disabled={!key.trim() || pending}
                onClick={() =>
                  void job(async () => {
                    const value = key;
                    setKey("");
                    await api.key(value);
                  })
                }
              >
                保存并连接
              </Button>
              <Button
                variant="outline"
                disabled={pending}
                onClick={() => void job(() => api.refresh())}
              >
                <RefreshCw size={14} />
                刷新
              </Button>
              <Button
                variant="ghost"
                disabled={pending}
                onClick={() => void job(() => api.key(null))}
              >
                移除
              </Button>
            </div>
            <SelectField
              label="显示币种"
              value={doc.deepseek.currency}
              onChange={(v) =>
                s.edit((d) => (d.deepseek.currency = v as "CNY" | "USD"))
              }
            >
              <option>CNY</option>
              <option>USD</option>
            </SelectField>
            <p className="field-help">
              消费为本地观察到的余额减少估算，不等同官方账单。Key
              不会进入场景包。
            </p>
            {snapshot.data.deepseek.error && (
              <p className="field-error">{snapshot.data.deepseek.error}</p>
            )}
          </TabsContent>
          <TabsContent value="calendar">
            <label className="form-label">
              节假日，每行 YYYY-MM-DD
              <Textarea
                aria-label="节假日日历"
                value={holidays}
                onChange={(e) => setHolidays(e.target.value)}
                rows={7}
              />
            </label>
            <label className="form-label">
              已核对年份，以逗号分隔
              <Input
                aria-label="已核对年份"
                value={years}
                onChange={(e) => setYears(e.target.value)}
              />
            </label>
            <Button
              onClick={() => {
                const dates = holidays
                    .split("\n")
                    .map((s) => s.trim())
                    .filter(Boolean),
                  ys = years.split(",").map(Number);
                if (
                  dates.length > 1500 ||
                  dates.some(
                    (d) =>
                      !/^\d{4}-\d{2}-\d{2}$/.test(d) ||
                      !Number.isFinite(Date.parse(d)) ||
                      new Date(d).toISOString().slice(0, 10) !== d,
                  ) ||
                  ys.some((y) => !Number.isInteger(y) || y < 2026 || y > 2100)
                ) {
                  useStore.setState({ error: "日期或年份格式无效" });
                  return;
                }
                s.edit((d) => {
                  d.deepseek.holidays = dates;
                  d.deepseek.calendarYears = ys;
                });
              }}
            >
              保存日历
            </Button>
            <p className="field-help">
              按北京时间计算连续峰谷时段。未核对的年份会显示提示。
            </p>
          </TabsContent>
          <TabsContent value="app">
            <div className="settings-about">
              <Ferry />
              <h2>PixelFerry</h2>
              <p>像素渡轮 · 把画面送往不同 USB 设备</p>
            </div>
            <ToggleField
              label="登录 Windows 后启动"
              checked={doc.settings.autoStart}
              onChange={(autoStart) =>
                s.edit((d) => (d.settings.autoStart = autoStart))
              }
            />
            <p className="field-help">
              关闭窗口后收起到托盘，屏幕继续播放。完全退出会停止后台与所有设备工作进程。
            </p>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
