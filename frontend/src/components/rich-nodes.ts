import {
  TextNode,
  $isTextNode,
  $isElementNode,
  $isLineBreakNode,
  $getRoot,
  $getSelection,
  $isRangeSelection,
  $createParagraphNode,
  $createTextNode,
  type NodeKey,
  type SerializedTextNode,
  type LexicalNode,
  type EditorConfig,
} from "lexical";
import {
  fieldLabel,
  normalized,
  richTextSchema,
  type FieldRun,
  type RichStyle,
  type RichText,
  type RichRun,
} from "@/lib/rich-text";

export function styleCSS(style: RichStyle) {
  return Object.entries({
    "font-family": style.fontFamily,
    "font-size":
      style.fontSize === undefined ? undefined : `${style.fontSize}px`,
    color: style.color,
    "font-weight":
      style.bold === undefined ? undefined : style.bold ? "700" : "400",
  })
    .filter(([, v]) => v !== undefined)
    .map(([k, v]) => `${k}:${v}`)
    .join(";");
}
export function cssStyle(css: string): RichStyle {
  const declaration = document.createElement("span").style;
  declaration.cssText = css;
  const result: RichStyle = {};
  if (declaration.fontFamily)
    result.fontFamily = declaration.fontFamily.replace(/^['"]|['"]$/g, "");
  if (declaration.fontSize)
    result.fontSize = Number.parseFloat(declaration.fontSize);
  if (declaration.color) {
    const rgb = declaration.color.match(/^rgba?\((\d+),\s*(\d+),\s*(\d+)/);
    result.color = rgb
      ? `#${rgb
          .slice(1, 4)
          .map((n) => Number(n).toString(16).padStart(2, "0"))
          .join("")}`
      : declaration.color;
  }
  if (declaration.fontWeight)
    result.bold =
      declaration.fontWeight === "bold" ||
      Number(declaration.fontWeight) >= 600;
  return result;
}
type SerializedParameter = SerializedTextNode & { field: FieldRun };
export class ParameterNode extends TextNode {
  __field: FieldRun;
  static getType() {
    return "parameter";
  }
  static clone(node: ParameterNode) {
    return new ParameterNode(node.__field, node.__key);
  }
  constructor(field: FieldRun, key?: NodeKey) {
    super(`[${fieldLabel(field)}]`, key);
    this.__field = structuredClone(field);
  }
  static importJSON(json: SerializedParameter): ParameterNode {
    const rich = richTextSchema.parse({ version: 1, runs: [json.field] });
    const field = rich.runs[0];
    if (field.type === "text") throw Error("无效参数标签");
    return $parameter(field).updateFromJSON({
      ...json,
      text: `[${fieldLabel(field)}]`,
      mode: "token",
    });
  }
  exportJSON(): SerializedParameter {
    return {
      ...super.exportJSON(),
      type: "parameter",
      version: 1,
      field: this.field(),
    };
  }
  field(): FieldRun {
    return { ...this.getLatest().__field, style: cssStyle(this.getStyle()) };
  }
  setField(field: FieldRun) {
    const node = this.getWritable();
    const selection = $getSelection();
    const selected =
      $isRangeSelection(selection) &&
      (selection.anchor.key === node.getKey() ||
        selection.focus.key === node.getKey());
    node.__field = structuredClone(field);
    node.setTextContent(`[${fieldLabel(field)}]`);
    if (selected) node.select(0, node.getTextContentSize());
    return node;
  }
  createDOM(config: EditorConfig): HTMLElement {
    const dom = super.createDOM(config);
    dom.classList.add("parameter-chip");
    dom.dataset.parameterKey = this.getKey();
    dom.draggable = true;
    dom.contentEditable = "false";
    dom.title = "点击编辑参数或拖动调整位置";
    return dom;
  }
  updateDOM(previous: this, dom: HTMLElement, config: EditorConfig): boolean {
    return super.updateDOM(previous, dom, config);
  }
  canInsertTextBefore() {
    return false;
  }
  canInsertTextAfter() {
    return false;
  }
  isTextEntity() {
    return true;
  }
}
export function $parameter(field: FieldRun) {
  return new ParameterNode(field)
    .setMode("token")
    .setStyle(styleCSS(field.style));
}
export function $loadRichText(rich: RichText) {
  const root = $getRoot();
  root.clear();
  let paragraph = $createParagraphNode();
  root.append(paragraph);
  for (const run of rich.runs) {
    if (run.type !== "text") {
      paragraph.append($parameter(run));
      continue;
    }
    const lines = run.text.split("\n");
    lines.forEach((text, i) => {
      if (i) {
        paragraph = $createParagraphNode();
        root.append(paragraph);
      }
      if (text)
        paragraph.append($createTextNode(text).setStyle(styleCSS(run.style)));
    });
  }
}
export function $readRichText(): RichText {
  const runs: RichRun[] = [];
  const walk = (node: LexicalNode) => {
    if (node instanceof ParameterNode) runs.push(node.field());
    else if ($isTextNode(node))
      runs.push({
        type: "text",
        text: node.getTextContent(),
        style: cssStyle(node.getStyle()),
      });
    else if ($isLineBreakNode(node))
      runs.push({ type: "text", text: "\n", style: {} });
    else if ($isElementNode(node)) node.getChildren().forEach(walk);
  };
  $getRoot()
    .getChildren()
    .forEach((node, i) => {
      if (i) runs.push({ type: "text", text: "\n", style: {} });
      walk(node);
    });
  return normalized(runs);
}
