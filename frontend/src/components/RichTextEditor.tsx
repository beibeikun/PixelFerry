import { useEffect, useRef, useState, useMemo } from "react";
import {
  $getSelection,
  $setSelection,
  $isRangeSelection,
  $isNodeSelection,
  $createNodeSelection,
  $getNodeByKey,
  $isTextNode,
  $isElementNode,
  $getRoot,
  $createRangeSelection,
  $getNearestNodeFromDOMNode,
  CLICK_COMMAND,
  KEY_DOWN_COMMAND,
  FORMAT_TEXT_COMMAND,
  PASTE_COMMAND,
  DRAGSTART_COMMAND,
  DRAGOVER_COMMAND,
  DROP_COMMAND,
  COMMAND_PRIORITY_HIGH,
  HISTORY_PUSH_TAG,
  SKIP_DOM_SELECTION_TAG,
  UNDO_COMMAND,
  REDO_COMMAND,
  type BaseSelection,
} from "lexical";
import { LexicalComposer } from "@lexical/react/LexicalComposer";
import { useLexicalComposerContext } from "@lexical/react/LexicalComposerContext";
import { RichTextPlugin } from "@lexical/react/LexicalRichTextPlugin";
import { ContentEditable } from "@lexical/react/LexicalContentEditable";
import { HistoryPlugin } from "@lexical/react/LexicalHistoryPlugin";
import { LexicalErrorBoundary } from "@lexical/react/LexicalErrorBoundary";
import { $patchStyleText } from "@lexical/selection";
import { mergeRegister } from "@lexical/utils";
import { Undo2, Redo2, Plus, Check, Type } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { renderPreview } from "@/lib/preview";
import {
  active,
  type Doc,
  type Layer,
  type Snapshot,
  type Preview,
} from "@/lib/model";
import {
  deepseekMetrics,
  deepseekField,
  sensorField,
  textContent,
  validateRichText,
  plainSummary,
  type RichText,
  type RichStyle,
  type FieldRun,
} from "@/lib/rich-text";
import {
  ParameterNode,
  $parameter,
  $loadRichText,
  $readRichText,
  styleCSS,
  cssStyle,
} from "./rich-nodes";

type Props = {
  layer: Layer;
  doc: Doc;
  snapshot: Snapshot;
  onClose: () => void;
  onApply: (rich: RichText) => void;
};
export function RichTextEditor(props: Props) {
  const initial = useMemo(() => textContent(props.layer), []);
  const [error, setError] = useState("");
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
    >
      <DialogContent
        className="rich-dialog"
        onKeyDown={(e) => {
          // Modal controls own their shortcuts; never move or delete canvas layers.
          e.stopPropagation();
        }}
      >
        <DialogHeader>
          <DialogTitle>编辑文字与参数</DialogTitle>
          <DialogDescription>
            直接输入文字，插入动态参数。选中文字或点击参数可独立设置样式。
          </DialogDescription>
        </DialogHeader>
        <LexicalComposer
          initialConfig={{
            namespace: "pixelferry-rich-text",
            nodes: [ParameterNode],
            editorState: () => $loadRichText(initial),
            onError: (e) => setError(e.message),
          }}
        >
          <EditorBody
            {...props}
            initial={initial}
            error={error}
            setError={setError}
          />
        </LexicalComposer>
      </DialogContent>
    </Dialog>
  );
}

function EditorBody({
  layer,
  doc,
  snapshot,
  onClose,
  onApply,
  initial,
  error,
  setError,
}: Props & {
  initial: RichText;
  error: string;
  setError: (v: string) => void;
}) {
  const [editor] = useLexicalComposerContext();
  const [draft, setDraft] = useState(initial),
    [selected, setSelected] = useState<{ key: string; field: FieldRun } | null>(
      null,
    ),
    [style, setStyle] = useState<RichStyle>({});
  const [search, setSearch] = useState(""),
    [source, setSource] = useState<"deepseek" | "sensor">("deepseek"),
    [metric, setMetric] = useState<keyof typeof deepseekMetrics>("balance"),
    [sensorId, setSensorId] = useState("");
  const savedSelection = useRef<BaseSelection | null>(null);
  const content = useRef(JSON.stringify(initial));
  const styleRef = useRef(style);
  styleRef.current = style;
  const restore = () => {
    const selection = savedSelection.current?.clone();
    if (selection) $setSelection(selection);
    else $getRoot().selectEnd();
  };
  function applyStyle(
    patch: Partial<Record<keyof RichStyle, RichStyle[keyof RichStyle] | null>>,
  ) {
    editor.update(
      () => {
        restore();
        const selection = $getSelection();
        if ($isNodeSelection(selection)) {
          for (const node of selection.getNodes())
            if ($isTextNode(node)) {
              const next = { ...cssStyle(node.getStyle()) };
              for (const [key, value] of Object.entries(patch)) {
                if (value === null) delete next[key as keyof RichStyle];
                else Object.assign(next, { [key]: value });
              }
              node.setStyle(styleCSS(next));
            }
        } else if ($isRangeSelection(selection)) {
          const keys = {
            fontFamily: "font-family",
            fontSize: "font-size",
            color: "color",
            bold: "font-weight",
          } as const;
          const css: Record<string, string | null> = {};
          for (const [key, value] of Object.entries(patch))
            css[keys[key as keyof RichStyle]] =
              value === null
                ? null
                : key === "fontSize"
                  ? `${value}px`
                  : key === "bold"
                    ? value
                      ? "700"
                      : "400"
                    : String(value);
          $patchStyleText(selection, css);
        }
      },
      { tag: [HISTORY_PUSH_TAG, SKIP_DOM_SELECTION_TAG] },
    );
  }
  useEffect(
    () =>
      mergeRegister(
        editor.registerCommand(
          KEY_DOWN_COMMAND,
          (event) => {
            if (
              !(event.ctrlKey || event.metaKey) ||
              event.shiftKey ||
              !["Home", "End"].includes(event.key)
            )
              return false;
            const element =
              event.key === "Home"
                ? $getRoot().getFirstChild()
                : $getRoot().getLastChild();
            if (!$isElementNode(element)) return false;
            event.preventDefault();
            const selection = $createRangeSelection();
            const offset = event.key === "Home" ? 0 : element.getChildrenSize();
            selection.anchor.set(element.getKey(), offset, "element");
            selection.focus.set(element.getKey(), offset, "element");
            $setSelection(selection);
            return true;
          },
          COMMAND_PRIORITY_HIGH,
        ),
        editor.registerUpdateListener(
          ({ editorState, dirtyElements, dirtyLeaves }) => {
            editorState.read(() => {
              const selection = $getSelection() ?? savedSelection.current;
              if (selection) savedSelection.current = selection.clone();
              const nodes = selection?.getNodes() ?? [];
              const field =
                nodes.length === 1 && nodes[0] instanceof ParameterNode
                  ? nodes[0]
                  : null;
              setSelected(
                field ? { key: field.getKey(), field: field.field() } : null,
              );
              const first = $isRangeSelection(selection)
                ? selection.anchor.getNode()
                : nodes[0];
              setStyle(
                $isRangeSelection(selection) && selection.isCollapsed()
                  ? cssStyle(selection.style)
                  : $isTextNode(first)
                    ? cssStyle(first.getStyle())
                    : {},
              );
              if (dirtyElements.size || dirtyLeaves.size) {
                const next = $readRichText();
                const json = JSON.stringify(next);
                if (json !== content.current) {
                  content.current = json;
                  setDraft(next);
                }
              }
            });
          },
        ),
        editor.registerCommand(
          CLICK_COMMAND,
          (event) => {
            const element = (
              event.target as HTMLElement
            ).closest?.<HTMLElement>("[data-parameter-key]");
            if (!element) return false;
            const key = element.dataset.parameterKey!;
            const node = $getNodeByKey(key);
            if (!(node instanceof ParameterNode)) return false;
            event.preventDefault();
            // Text tokens need a real DOM-backed range. A node selection can
            // be replaced by the browser's old caret after clicking a token.
            node.select(0, node.getTextContentSize());
            editor.focus();
            return true;
          },
          COMMAND_PRIORITY_HIGH,
        ),
        editor.registerCommand(
          FORMAT_TEXT_COMMAND,
          (format) => {
            if (format === "bold")
              queueMicrotask(() =>
                applyStyle({ bold: !(styleRef.current.bold ?? layer.bold) }),
              );
            return true;
          },
          COMMAND_PRIORITY_HIGH,
        ),
        editor.registerCommand(
          PASTE_COMMAND,
          (event) => {
            if (!event || !("clipboardData" in event) || !event.clipboardData)
              return false;
            // Preserve our own structured nodes; external content is plain text only.
            if (event.clipboardData.getData("application/x-lexical-editor"))
              return false;
            const text = event.clipboardData.getData("text/plain");
            event.preventDefault();
            const selection = $getSelection();
            if ($isRangeSelection(selection)) selection.insertRawText(text);
            else if ($isNodeSelection(selection)) {
              const nodes = selection.getNodes();
              nodes[0]?.selectPrevious();
              const range = $getSelection();
              if ($isRangeSelection(range)) {
                range.insertRawText(text);
                nodes.forEach((node) => node.remove());
              }
            }
            return true;
          },
          COMMAND_PRIORITY_HIGH,
        ),
      ),
    [editor],
  );
  useEffect(() => {
    const root = editor.getRootElement();
    root
      ?.querySelectorAll<HTMLElement>("[data-parameter-key]")
      .forEach((el) =>
        el.classList.toggle(
          "selected",
          el.dataset.parameterKey === selected?.key,
        ),
      );
  }, [selected, editor]);
  useEffect(() => {
    return mergeRegister(
      editor.registerCommand(DRAGSTART_COMMAND, start, COMMAND_PRIORITY_HIGH),
      editor.registerCommand(DRAGOVER_COMMAND, over, COMMAND_PRIORITY_HIGH),
      editor.registerCommand(DROP_COMMAND, drop, COMMAND_PRIORITY_HIGH),
    );
    function start(event: DragEvent) {
      const el = (event.target as HTMLElement).closest<HTMLElement>(
        "[data-parameter-key]",
      );
      if (!el || !event.dataTransfer) return false;
      editor.getEditorState().read(() => {
        const node = $getNodeByKey(el.dataset.parameterKey!);
        if (node instanceof ParameterNode) {
          event.dataTransfer!.setData(
            "application/x-pixelferry-field",
            JSON.stringify({
              key: node.getKey(),
              field: node.field(),
              editor: editor.getKey(),
            }),
          );
          event.dataTransfer!.effectAllowed = "move";
        }
      });
      return true;
    }
    function over(event: DragEvent) {
      if (
        event.dataTransfer?.types.includes("application/x-pixelferry-field")
      ) {
        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
        return true;
      }
      return false;
    }
    function drop(event: DragEvent) {
      const data = event.dataTransfer?.getData(
        "application/x-pixelferry-field",
      );
      if (!data) return false;
      event.preventDefault();
      event.stopPropagation();
      const range = document.caretRangeFromPoint(event.clientX, event.clientY);
      if (!range) return true;
      try {
        const parsed = JSON.parse(data);
        const field = validateRichText({ version: 1, runs: [parsed.field] })
          .runs[0];
        if (field.type === "text") return true;
        editor.update(
          () => {
            const node = $getNearestNodeFromDOMNode(range.startContainer);
            if (!node) return;
            if (
              parsed.editor === editor.getKey() &&
              node.getKey() === parsed.key
            )
              return;
            if (node instanceof ParameterNode) {
              const r = editor
                .getElementByKey(node.getKey())!
                .getBoundingClientRect();
              event.clientX < r.left + r.width / 2
                ? node.selectPrevious()
                : node.selectNext();
            } else if ($isTextNode(node)) {
              const selection = $createRangeSelection();
              const offset = Math.min(
                range.startOffset,
                node.getTextContentSize(),
              );
              selection.anchor.set(node.getKey(), offset, "text");
              selection.focus.set(node.getKey(), offset, "text");
              $setSelection(selection);
            } else node.selectEnd();
            const selection = $getSelection();
            if ($isRangeSelection(selection)) {
              selection.insertNodes([$parameter(field)]);
              if (parsed.editor === editor.getKey())
                $getNodeByKey(parsed.key)?.remove();
            }
          },
          { tag: HISTORY_PUSH_TAG },
        );
      } catch (e) {
        setError(String(e));
      }
      return true;
    }
  }, [editor]);
  const sensors = snapshot.data.sensors.filter((s) =>
    `${s.name} ${s.device} ${s.unit}`
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
  const chosenSensor = snapshot.data.sensors.find((s) => s.id === sensorId);
  function insert() {
    const field =
      source === "deepseek"
        ? deepseekField(metric)
        : chosenSensor
          ? sensorField(chosenSensor)
          : null;
    if (!field) return;
    field.style = { ...style };
    editor.update(
      () => {
        restore();
        const selection = $getSelection();
        const node = $parameter(field);
        if ($isRangeSelection(selection)) selection.insertNodes([node]);
        else if ($isNodeSelection(selection)) {
          const anchor = selection.getNodes().at(-1);
          anchor?.insertAfter(node);
        }
        const next = $createNodeSelection();
        next.add(node.getKey());
        $setSelection(next);
      },
      { tag: HISTORY_PUSH_TAG },
    );
    editor.focus();
  }
  function updateField(patch: Partial<FieldRun>) {
    if (!selected) return;
    const key = selected.key;
    editor.update(
      () => {
        restore();
        const node = $getNodeByKey(key);
        if (node instanceof ParameterNode)
          node.setField({ ...node.field(), ...patch } as FieldRun);
      },
      {
        tag: editor.getRootElement()?.contains(document.activeElement)
          ? HISTORY_PUSH_TAG
          : [HISTORY_PUSH_TAG, SKIP_DOM_SELECTION_TAG],
      },
    );
  }
  function replaceField(field: FieldRun) {
    if (!selected) return;
    const key = selected.key;
    editor.update(
      () => {
        restore();
        const node = $getNodeByKey(key);
        if (node instanceof ParameterNode)
          node.setField({ ...field, style: node.field().style });
      },
      {
        tag: editor.getRootElement()?.contains(document.activeElement)
          ? HISTORY_PUSH_TAG
          : [HISTORY_PUSH_TAG, SKIP_DOM_SELECTION_TAG],
      },
    );
  }
  let validation = "";
  try {
    validateRichText(draft);
  } catch {
    validation =
      "最多 256 个片段，文字累计不超过 16000 字节；请检查字体和格式。";
  }
  const field = selected?.field;
  return (
    <>
      <div className="rich-toolbar" aria-label="片段样式">
        <Button
          variant="ghost"
          size="icon"
          aria-label="撤销文字编辑"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => editor.dispatchCommand(UNDO_COMMAND, undefined)}
        >
          <Undo2 size={16} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label="重做文字编辑"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => editor.dispatchCommand(REDO_COMMAND, undefined)}
        >
          <Redo2 size={16} />
        </Button>
        <label>
          字体
          <Input
            aria-label="片段字体"
            placeholder={layer.fontFamily}
            value={style.fontFamily ?? ""}
            onChange={(e) => applyStyle({ fontFamily: e.target.value || null })}
          />
        </label>
        <FontSizeField
          value={style.fontSize}
          inherited={layer.fontSize}
          onChange={(value) => applyStyle({ fontSize: value })}
        />
        <label>
          颜色
          <input
            aria-label="片段颜色"
            type="color"
            value={style.color ?? layer.color}
            onChange={(e) => applyStyle({ color: e.target.value })}
          />
        </label>
        <Button
          variant={(style.bold ?? layer.bold) ? "secondary" : "outline"}
          aria-label="片段粗体"
          aria-pressed={style.bold ?? layer.bold}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => applyStyle({ bold: !(style.bold ?? layer.bold) })}
        >
          B
        </Button>
        <Button
          variant="ghost"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() =>
            applyStyle({
              fontFamily: null,
              fontSize: null,
              color: null,
              bold: null,
            })
          }
        >
          继承元素样式
        </Button>
      </div>
      <div className="rich-layout">
        <div className="rich-main">
          <div
            className="rich-edit-surface"
            style={{
              backgroundColor: active(doc).background,
              fontFamily: layer.fontFamily,
              fontSize: layer.fontSize,
              color: layer.color,
              fontWeight: layer.bold ? 700 : 400,
              textAlign: layer.align,
            }}
          >
            <RichTextPlugin
              contentEditable={
                <ContentEditable
                  className="rich-editable"
                  aria-label="混排文字内容"
                  spellCheck={false}
                />
              }
              placeholder={
                <div className="rich-placeholder">
                  输入文字，或在右侧插入参数…
                </div>
              }
              ErrorBoundary={LexicalErrorBoundary}
            />
            <HistoryPlugin />
          </div>
          <p className="field-help">
            选中文字设置样式；点击参数修改格式，拖动参数可调整位置。Enter 换行。
          </p>
          <DraftPreview
            doc={doc}
            layer={layer}
            draft={draft}
            valid={!validation}
          />
        </div>
        <aside className="rich-parameters">
          <h3>
            <Plus size={15} />
            插入参数
          </h3>
          <label>
            参数来源
            <select
              aria-label="参数来源"
              value={source}
              onChange={(e) => setSource(e.target.value as typeof source)}
            >
              <option value="deepseek">DeepSeek</option>
              <option value="sensor">传感器</option>
            </select>
          </label>
          {source === "deepseek" ? (
            <label>
              指标
              <select
                aria-label="DeepSeek 插入指标"
                value={metric}
                onChange={(e) => setMetric(e.target.value as typeof metric)}
              >
                {Object.entries(deepseekMetrics).map(([id, name]) => (
                  <option key={id} value={id}>
                    {name}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <>
              <Input
                aria-label="搜索传感器"
                placeholder="搜索名称、设备或单位"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
              <div
                className="rich-sensor-list"
                role="listbox"
                aria-label="传感器列表"
              >
                {sensors.map((s) => (
                  <button
                    type="button"
                    role="option"
                    aria-selected={sensorId === s.id}
                    key={s.id}
                    onClick={() => setSensorId(s.id)}
                  >
                    <strong>{s.name}</strong>
                    <small>
                      {s.device || "Afterburner"} · {s.unit || "无单位"} ·{" "}
                      {s.status === "ok" &&
                      snapshot.data &&
                      Date.now() - s.timestamp <= 5000
                        ? (s.value ?? "--")
                        : "离线"}
                    </small>
                  </button>
                ))}
                {!sensors.length && (
                  <p className="field-help">
                    暂无匹配传感器。请检查 Afterburner 数据源。
                  </p>
                )}
              </div>
            </>
          )}
          <Button
            disabled={source === "sensor" && !chosenSensor}
            onClick={insert}
          >
            <Plus size={14} />
            在光标处插入
          </Button>
          <div className="rich-field-settings">
            <h3>
              <Type size={15} />
              选中参数
            </h3>
            {!field ? (
              <p className="field-help">
                点击编辑区中的参数标签，调整其来源和显示格式。
              </p>
            ) : (
              <>
                <label>
                  来源
                  <select
                    aria-label="选中参数来源"
                    value={field.type}
                    onChange={(e) => {
                      if (e.target.value === "deepseek")
                        replaceField(deepseekField("balance"));
                      else if (snapshot.data.sensors[0])
                        replaceField(sensorField(snapshot.data.sensors[0]));
                    }}
                  >
                    <option value="deepseek">DeepSeek</option>
                    <option
                      value="sensor"
                      disabled={
                        !snapshot.data.sensors.length && field.type !== "sensor"
                      }
                    >
                      传感器
                    </option>
                  </select>
                </label>
                {field.type === "deepseek" ? (
                  <label>
                    指标
                    <select
                      aria-label="选中参数指标"
                      value={field.metric}
                      onChange={(e) =>
                        updateField({
                          metric: e.target.value as typeof field.metric,
                        })
                      }
                    >
                      {Object.entries(deepseekMetrics).map(([id, name]) => (
                        <option key={id} value={id}>
                          {name}
                        </option>
                      ))}
                    </select>
                  </label>
                ) : (
                  <label>
                    传感器
                    <select
                      aria-label="选中参数传感器"
                      value={field.sensorId}
                      onChange={(e) => {
                        const s = snapshot.data.sensors.find(
                          (s) => s.id === e.target.value,
                        );
                        if (s) updateField({ sensorId: s.id, label: s.name });
                      }}
                    >
                      {!snapshot.data.sensors.some(
                        (s) => s.id === field.sensorId,
                      ) && (
                        <option value={field.sensorId}>
                          {field.label}（当前离线）
                        </option>
                      )}
                      {snapshot.data.sensors.map((s) => (
                        <option key={s.id} value={s.id}>
                          {s.name} · {s.device || s.unit}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                {!(field.type === "deepseek" && field.metric === "period") && (
                  <>
                    <label>
                      小数位数
                      <select
                        aria-label="参数小数位数"
                        value={field.decimals}
                        onChange={(e) =>
                          updateField({ decimals: Number(e.target.value) })
                        }
                      >
                        {Array.from({ length: 9 }, (_, n) => (
                          <option key={n} value={n}>
                            {n === 0 ? "整数" : `${n} 位小数`}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="rich-checkbox">
                      <input
                        type="checkbox"
                        checked={
                          field.type === "sensor"
                            ? field.showUnit
                            : field.showCurrency
                        }
                        onChange={(e) =>
                          updateField(
                            field.type === "sensor"
                              ? { showUnit: e.target.checked }
                              : { showCurrency: e.target.checked },
                          )
                        }
                      />
                      {field.type === "sensor" ? "显示单位" : "显示货币符号"}
                    </label>
                    <label>
                      {field.type === "sensor" ? "单位字号" : "货币符号字号"}
                      <select
                        aria-label="单位或货币符号字号"
                        disabled={
                          field.type === "sensor"
                            ? !field.showUnit
                            : !field.showCurrency
                        }
                        value={Math.round((field.affixScale ?? 1) * 100)}
                        onChange={(e) =>
                          updateField({
                            affixScale: Number(e.target.value) / 100,
                          })
                        }
                      >
                        {[100, 90, 80, 70, 60, 50, 40, 30, 25].map(
                          (percent) => (
                            <option key={percent} value={percent}>
                              {percent === 100 ? "与数值相同" : `${percent}%`}
                            </option>
                          ),
                        )}
                      </select>
                      <span className="field-help">
                        相对于该参数的字号，数值大小保持不变。
                      </span>
                    </label>
                  </>
                )}
              </>
            )}
          </div>
        </aside>
      </div>
      {(validation || error) && (
        <p role="alert" className="field-error">
          {validation || error}
        </p>
      )}
      <div className="rich-footer">
        <span className="field-help">
          草稿未保存 · {draft.runs.length} 个片段
        </span>
        <Button variant="outline" onClick={onClose}>
          取消
        </Button>
        <Button
          disabled={!!validation}
          onClick={() => {
            try {
              const rich = validateRichText(
                editor.getEditorState().read($readRichText),
              );
              onApply(rich);
            } catch (e) {
              setError(String(e));
            }
          }}
        >
          <Check size={14} />
          应用
        </Button>
      </div>
    </>
  );
}

function FontSizeField({
  value,
  inherited,
  onChange,
}: {
  value: number | undefined;
  inherited: number;
  onChange: (value: number | null) => void;
}) {
  const [draft, setDraft] = useState(value === undefined ? "" : String(value));
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setDraft(value === undefined ? "" : String(value));
  }, [value]);
  return (
    <label>
      字号
      <Input
        aria-label="片段字号"
        type="number"
        min={8}
        max={240}
        placeholder={String(inherited)}
        value={draft}
        onFocus={() => {
          focused.current = true;
        }}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          focused.current = false;
          const n = Number(draft);
          if (!draft) onChange(null);
          else if (Number.isFinite(n) && n >= 8 && n <= 240) onChange(n);
          else setDraft(value === undefined ? "" : String(value));
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
        }}
      />
    </label>
  );
}

function DraftPreview({
  doc,
  layer,
  draft,
  valid,
}: {
  doc: Doc;
  layer: Layer;
  draft: RichText;
  valid: boolean;
}) {
  const [frame, setFrame] = useState<Preview | null>(null),
    [error, setError] = useState("");
  const canvas = useRef<HTMLCanvasElement>(null),
    latest = useRef({
      doc,
      layer,
      draft,
      valid,
      revision: 0,
      edited: Date.now(),
    });
  useEffect(() => {
    latest.current = {
      doc,
      layer,
      draft,
      valid,
      revision: latest.current.revision + 1,
      edited: Date.now(),
    };
  }, [doc, layer, draft, valid]);
  useEffect(() => {
    let alive = true,
      busy = false,
      lastRevision = -1,
      lastRequest = 0;
    const timer = setInterval(() => {
      const current = latest.current,
        now = Date.now();
      if (
        busy ||
        !current.valid ||
        now - current.edited < 200 ||
        (current.revision === lastRevision && now - lastRequest < 1000)
      )
        return;
      const previewDoc = structuredClone(current.doc);
      const target = active(previewDoc).layers.find(
        (l) => l.id === current.layer.id,
      );
      if (!target) return;
      Object.assign(target, {
        richText: current.draft,
        text: plainSummary(current.draft),
      });
      busy = true;
      lastRequest = now;
      lastRevision = current.revision;
      void renderPreview(previewDoc)
        .then((next) => {
          if (alive && latest.current.revision === current.revision) {
            setFrame(next);
            setError("");
          }
        })
        .catch((e) => {
          if (alive) setError(String(e));
        })
        .finally(() => {
          busy = false;
        });
    }, 100);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);
  useEffect(() => {
    if (!frame || !canvas.current) return;
    const bytes = Uint8ClampedArray.from(atob(frame.rgba), (c) =>
      c.charCodeAt(0),
    );
    if (bytes.length !== frame.width * frame.height * 4) return;
    canvas.current.width = frame.width;
    canvas.current.height = frame.height;
    canvas.current
      .getContext("2d")
      ?.putImageData(new ImageData(bytes, frame.width, frame.height), 0, 0);
  }, [frame]);
  return (
    <div className="rich-preview">
      <h3>
        实际效果 <span>当前场景 · 实时数据</span>
      </h3>
      <canvas ref={canvas} aria-label="混排实际效果" />
      {!frame && !error && <p className="field-help">正在生成预览…</p>}
      {error && <p className="field-error">{error}</p>}
    </div>
  );
}
