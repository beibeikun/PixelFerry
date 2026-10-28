import { describe, it, expect, vi } from "vitest";
vi.mock("./api", () => ({ api: { save: vi.fn(async () => {}) } }));
import { layer } from "./model";
import {
  normalized,
  plainSummary,
  convertToRichText,
  validateRichText,
  deepseekField,
} from "./rich-text";

describe("mixed text documents", () => {
  it("keeps optional affix scaling compatible with old fields and bounds the scale", () => {
    const old = deepseekField("balance");
    expect(
      validateRichText({ version: 1, runs: [old] }).runs[0],
    ).not.toHaveProperty("affixScale");
    for (const affixScale of [0.25, 0.7, 1])
      expect(
        validateRichText({ version: 1, runs: [{ ...old, affixScale }] })
          .runs[0],
      ).toMatchObject({ affixScale });
    for (const affixScale of [0, 0.24, 1.01, NaN, Infinity])
      expect(() =>
        validateRichText({ version: 1, runs: [{ ...old, affixScale }] }),
      ).toThrow();
    expect(layer("text").verticalAlign).toBe("middle");
  });
  it("preserves repeated fields, Unicode, newlines and independent style overrides", () => {
    const rich = normalized([
      { type: "text", text: "余额：", style: {} },
      deepseekField("balance"),
      { type: "text", text: "\n", style: {} },
      {
        ...deepseekField("balance"),
        decimals: 2,
        showCurrency: false,
        style: { fontSize: 48, color: "#ffcc00", bold: true },
      },
    ]);
    expect(validateRichText(JSON.parse(JSON.stringify(rich)))).toEqual(rich);
    expect(plainSummary(rich)).toBe(
      "余额：[DeepSeek · 余额]\n[DeepSeek · 余额]",
    );
    expect(rich.runs[3].style).toEqual({
      fontSize: 48,
      color: "#ffcc00",
      bold: true,
    });
  });
  it("normalizes only adjacent text with identical styles", () => {
    const rich = normalized([
      { type: "text", text: "a", style: {} },
      { type: "text", text: "b", style: {} },
      { type: "text", text: "c", style: { bold: false } },
      { type: "text", text: "", style: {} },
    ]);
    expect(rich.runs).toHaveLength(2);
    expect(rich.runs[0]).toMatchObject({ text: "ab" });
  });
  it("rejects invalid imported formats, styles, types, limits and multibyte text", () => {
    const field = deepseekField("balance");
    for (const bad of [
      { ...field, decimals: 9 },
      { ...field, metric: "unknown" },
      { ...field, style: { fontSize: 241 } },
      { ...field, style: { color: "red" } },
      { ...field, style: { fontFamily: "" } },
      { ...field, script: "run" },
    ])
      expect(() => validateRichText({ version: 1, runs: [bad] })).toThrow();
    expect(() => validateRichText({ version: 2, runs: [] })).toThrow();
    expect(() =>
      validateRichText({ version: 1, runs: Array(257).fill(field) }),
    ).toThrow();
    expect(() =>
      validateRichText({
        version: 1,
        runs: [{ type: "text", text: "字".repeat(5334), style: {} }],
      }),
    ).toThrow();
  });
  it("converts sensors without losing identity and retains summary labels", () => {
    const sensor = layer("sensor");
    sensor.sensorId = "stable:gpu:0";
    sensor.text = "integer";
    sensor.fontSize = 42;
    expect(convertToRichText(sensor, [])).toMatchObject({
      type: "text",
      richText: {
        runs: [
          {
            type: "sensor",
            sensorId: "stable:gpu:0",
            decimals: 0,
            showUnit: true,
          },
        ],
      },
    });
    expect(sensor.type).toBe("sensor");
    expect(sensor.fontSize).toBe(42);
    expect(() => convertToRichText(layer("sensor"), [])).toThrow();
    const deepseek = layer("deepseek");
    deepseek.deepseekMetric = "summary";
    const patch = convertToRichText(deepseek, []);
    expect(
      patch.richText?.runs.filter((r) => r.type === "deepseek"),
    ).toHaveLength(4);
    expect(patch.text).toContain("今日估算");
    expect(patch.text).toContain("累计估算");
  });
});
