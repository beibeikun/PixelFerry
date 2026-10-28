import { z } from "zod";
import type { Layer, Sensor } from "./model";

const styleSchema = z
  .object({
    fontFamily: z
      .string()
      .min(1)
      .refine((s) => utf8(s) <= 400)
      .optional(),
    fontSize: z.number().min(8).max(240).optional(),
    color: z
      .string()
      .regex(/^#[0-9a-fA-F]{6}$/)
      .optional(),
    bold: z.boolean().optional(),
  })
  .strict();
const decimals = z.number().int().min(0).max(8);
export const deepseekMetrics = {
  balance: "余额",
  granted: "赠送余额",
  toppedUp: "充值余额",
  today: "今日消费估算",
  total: "累计消费估算",
  period: "峰谷时段",
} as const;
const runSchema = z.discriminatedUnion("type", [
  z
    .object({
      type: z.literal("text"),
      text: z.string(),
      style: styleSchema.default({}),
    })
    .strict(),
  z
    .object({
      type: z.literal("sensor"),
      sensorId: z
        .string()
        .min(1)
        .refine((s) => utf8(s) <= 2048),
      label: z.string().refine((s) => utf8(s) <= 600),
      decimals,
      showUnit: z.boolean(),
      affixScale: z.number().min(0.25).max(1).optional(),
      style: styleSchema.default({}),
    })
    .strict(),
  z
    .object({
      type: z.literal("deepseek"),
      metric: z.enum([
        "balance",
        "granted",
        "toppedUp",
        "today",
        "total",
        "period",
      ]),
      decimals,
      showCurrency: z.boolean(),
      affixScale: z.number().min(0.25).max(1).optional(),
      style: styleSchema.default({}),
    })
    .strict(),
]);
export const richTextSchema = z
  .object({ version: z.literal(1), runs: z.array(runSchema).max(256) })
  .strict();
export type RichStyle = z.infer<typeof styleSchema>;
export type RichRun = z.infer<typeof runSchema>;
export type FieldRun = Exclude<RichRun, { type: "text" }>;
export type RichText = z.infer<typeof richTextSchema>;
export const utf8 = (value: string) => new TextEncoder().encode(value).length;
export function fieldLabel(run: FieldRun) {
  return run.type === "sensor"
    ? run.label || run.sensorId
    : `DeepSeek · ${deepseekMetrics[run.metric]}`;
}
export function plainSummary(rich: RichText) {
  return rich.runs
    .map((r) => (r.type === "text" ? r.text : `[${fieldLabel(r)}]`))
    .join("");
}
export function validateRichText(value: unknown): RichText {
  const rich = richTextSchema.parse(value);
  const size = rich.runs.reduce(
    (sum, r) =>
      sum +
      (r.type === "text"
        ? utf8(r.text)
        : r.type === "sensor"
          ? utf8(r.label)
          : 0),
    0,
  );
  if (size > 16000 || utf8(plainSummary(rich)) > 16000)
    throw Error("文字内容不能超过 16000 字节");
  return rich;
}
export function normalized(runs: RichRun[]): RichText {
  const result: RichRun[] = [];
  for (const run of runs) {
    if (run.type === "text" && !run.text) continue;
    const previous = result.at(-1);
    if (
      previous?.type === "text" &&
      run.type === "text" &&
      JSON.stringify(previous.style) === JSON.stringify(run.style)
    )
      previous.text += run.text;
    else result.push(structuredClone(run));
  }
  return { version: 1, runs: result };
}
export function textContent(layer: Layer): RichText {
  return (
    layer.richText ?? {
      version: 1,
      runs: [{ type: "text", text: layer.text, style: {} }],
    }
  );
}
export function sensorField(
  sensor: Pick<Sensor, "id" | "name">,
): Extract<FieldRun, { type: "sensor" }> {
  return {
    type: "sensor",
    sensorId: sensor.id,
    label: sensor.name,
    decimals: 1,
    showUnit: true,
    style: {},
  };
}
export function deepseekField(
  metric: keyof typeof deepseekMetrics,
): Extract<FieldRun, { type: "deepseek" }> {
  return {
    type: "deepseek",
    metric,
    decimals: 4,
    showCurrency: true,
    style: {},
  };
}
export function convertToRichText(
  layer: Layer,
  sensors: Sensor[],
): Partial<Layer> {
  let runs: RichRun[];
  const text = (text: string): RichRun => ({ type: "text", text, style: {} });
  if (layer.type === "sensor") {
    if (!layer.sensorId) throw Error("请先选择传感器");
    const field = sensorField(
      sensors.find((s) => s.id === layer.sensorId) ?? {
        id: layer.sensorId,
        name: layer.name,
      },
    );
    field.decimals = layer.text === "integer" ? 0 : 1;
    runs = [field];
  } else if (layer.type === "deepseek") {
    if (layer.deepseekMetric === "summary")
      runs = [
        text("DeepSeek · "),
        deepseekField("period"),
        text("\n余额 "),
        deepseekField("balance"),
        text("\n今日估算 "),
        deepseekField("today"),
        text("\n累计估算 "),
        deepseekField("total"),
      ];
    else
      runs = [
        ...(layer.deepseekMetric === "today"
          ? [text("今日估算 ")]
          : layer.deepseekMetric === "total"
            ? [text("累计估算 ")]
            : []),
        deepseekField(layer.deepseekMetric),
      ];
  } else runs = textContent(layer).runs;
  const richText = validateRichText(normalized(runs));
  return { type: "text", richText, text: plainSummary(richText) };
}
