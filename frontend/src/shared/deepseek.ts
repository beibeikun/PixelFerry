import { z } from "zod";

export const DEEPSEEK_BALANCE_URL = "https://api.deepseek.com/user/balance";
export const DEEPSEEK_PRICING_URL =
  "https://api-docs.deepseek.com/zh-cn/quick_start/pricing/";
export const PRICING_VERIFIED = "2026-10-25";
const ranges2026 = [
  ["01-01", "01-03"],
  ["02-15", "02-23"],
  ["04-04", "04-06"],
  ["05-01", "05-05"],
  ["06-19", "06-21"],
  ["09-25", "09-27"],
  ["10-01", "10-07"],
];
export const holidays2026 = ranges2026.flatMap(([start, end]) => {
  const dates: string[] = [];
  for (
    let t = Date.parse(`2026-${start}T00:00:00Z`);
    t <= Date.parse(`2026-${end}T00:00:00Z`);
    t += 86400000
  )
    dates.push(new Date(t).toISOString().slice(0, 10));
  return dates;
});
export const holidaySchema = z
  .string()
  .regex(/^\d{4}-\d{2}-\d{2}$/)
  .refine((s) => {
    const t = Date.parse(s + "T00:00:00Z");
    return Number.isFinite(t) && new Date(t).toISOString().slice(0, 10) === s;
  }, "节假日日期无效");
export const deepSeekPreferencesSchema = z.object({
  currency: z.enum(["CNY", "USD"]).default("CNY"),
  holidays: z
    .array(holidaySchema)
    .max(1500)
    .default(() => holidays2026.slice()),
  calendarYears: z
    .array(z.number().int().min(2026).max(2100))
    .max(75)
    .default(() => [2026]),
});
export type DeepSeekPreferences = z.infer<typeof deepSeekPreferencesSchema>;
export interface DeepSeekBalance {
  currency: "CNY" | "USD";
  total: string;
  granted: string;
  toppedUp: string;
  todaySpent: string | null;
  totalSpent: string | null;
  baselineAt: number;
}
export interface DeepSeekState {
  configured: boolean;
  status: "unconfigured" | "loading" | "ok" | "error";
  timestamp: number;
  available: boolean;
  balances: DeepSeekBalance[];
  error?: string;
  statisticsError?: string;
  note?: string;
}
export const initialDeepSeek: DeepSeekState = {
  configured: false,
  status: "unconfigured",
  timestamp: 0,
  available: false,
  balances: [],
};
export type DeepSeekMetric =
  "summary" | "balance" | "today" | "total" | "period";
export const moneySchema = z
  .string()
  .max(30)
  .regex(/^-?\d+(?:\.\d{1,12})?$/);
export const balanceResponseSchema = z
  .object({
    is_available: z.boolean(),
    balance_infos: z
      .array(
        z.object({
          currency: z.enum(["CNY", "USD"]),
          total_balance: moneySchema,
          granted_balance: moneySchema,
          topped_up_balance: moneySchema,
        }),
      )
      .max(2),
  })
  .superRefine((v, ctx) => {
    if (
      new Set(v.balance_infos.map((b) => b.currency)).size !==
      v.balance_infos.length
    )
      ctx.addIssue({ code: "custom", message: "重复币种" });
  });
export type BalanceResponse = z.infer<typeof balanceResponseSchema>;
const OFFSET = 8 * 3600000;
export const beijingDay = (now: number) =>
  new Date(now + OFFSET).toISOString().slice(0, 10);
export function pricingAt(now: number, preferences: DeepSeekPreferences) {
  const local = new Date(now + OFFSET),
    day = local.getUTCDay(),
    minute = local.getUTCHours() * 60 + local.getUTCMinutes();
  const weekend = day === 0 || day === 6,
    holiday = preferences.holidays.includes(beijingDay(now));
  return {
    peak:
      !weekend &&
      !holiday &&
      ((minute >= 540 && minute < 720) || (minute >= 840 && minute < 1080)),
    calendarKnown: preferences.calendarYears.includes(local.getUTCFullYear()),
    weekend,
    holiday,
  };
}
export function pricingPeriod(now: number, preferences: DeepSeekPreferences) {
  const current = pricingAt(now, preferences),
    day = beijingDay(now),
    midnight = Date.parse(day + "T00:00:00Z") - OFFSET;
  let next: number | null = null,
    start: number | null = null;
  for (let d = 0; d < 15 && next === null; d++)
    for (const minute of [0, 540, 720, 840, 1080]) {
      const candidate = midnight + d * 86400000 + minute * 60000;
      if (
        candidate > now &&
        pricingAt(candidate, preferences).peak !== current.peak
      ) {
        next = candidate;
        break;
      }
    }
  for (let d = 0; d < 15 && start === null; d++)
    for (const minute of [1080, 840, 720, 540, 0]) {
      const candidate = midnight - d * 86400000 + minute * 60000;
      if (
        candidate <= now &&
        pricingAt(candidate, preferences).peak === current.peak &&
        pricingAt(candidate - 1, preferences).peak !== current.peak
      ) {
        start = candidate;
        break;
      }
    }
  return { ...current, start, next };
}
export function pricingProgress(now: number, preferences: DeepSeekPreferences) {
  const period = pricingPeriod(now, preferences);
  const ratio =
    period.start !== null && period.next !== null && period.next > period.start
      ? Math.max(
          0,
          Math.min(1, (now - period.start) / (period.next - period.start)),
        )
      : null;
  return { ...period, ratio };
}
export function moneyLabel(
  value: string | null | undefined,
  currency: "CNY" | "USD",
) {
  if (value == null) return "--";
  const number = Number(value);
  return Number.isFinite(number)
    ? `${currency === "CNY" ? "¥" : "$"}${number.toFixed(4)}`
    : "--";
}
export function deepSeekText(
  metric: DeepSeekMetric,
  state: DeepSeekState,
  preferences: DeepSeekPreferences,
  now: number,
): string {
  const period = pricingPeriod(now, preferences),
    periodText = `${period.peak ? "峰时段" : "谷时段"}${!period.calendarKnown ? " · 节假日待核对" : ""}`;
  if (metric === "period") return periodText;
  const balance = state.balances.find(
    (b) => b.currency === preferences.currency,
  );
  const fresh = state.status === "ok" && now - state.timestamp <= 180000;
  const amount = (field: "total" | "todaySpent" | "totalSpent") =>
    fresh
      ? moneyLabel(
          field === "todaySpent" &&
            balance &&
            beijingDay(state.timestamp) !== beijingDay(now)
            ? "0"
            : balance?.[field],
          preferences.currency,
        )
      : "--";
  if (metric === "balance") return amount("total");
  if (metric === "today") return `今日估算 ${amount("todaySpent")}`;
  if (metric === "total") return `累计估算 ${amount("totalSpent")}`;
  return `DeepSeek · ${periodText}\n余额 ${amount("total")}\n今日估算 ${amount("todaySpent")}\n累计估算 ${amount("totalSpent")}`;
}

const unitsSchema = z
  .string()
  .regex(/^-?\d+$/)
  .max(45);
const currencyLedgerSchema = z.object({
  last: unitsSchema,
  timestamp: z.number().nonnegative(),
  baselineAt: z.number().nonnegative(),
  total: unitsSchema,
  days: z.record(z.string(), unitsSchema),
});
export const ledgerSchema = z.object({
  version: z.literal(1),
  accounts: z.record(
    z.string(),
    z.partialRecord(z.enum(["CNY", "USD"]), currencyLedgerSchema),
  ),
});
export type Ledger = z.infer<typeof ledgerSchema>;
export const emptyLedger = (): Ledger => ({ version: 1, accounts: {} });
const SCALE = 100000000n;
export function toUnits(value: string): bigint {
  moneySchema.parse(value);
  const negative = value.startsWith("-"),
    [whole, fraction = ""] = value.replace(/^-/, "").split(".");
  const n = BigInt(whole) * SCALE + BigInt(fraction.padEnd(8, "0").slice(0, 8));
  return negative ? -n : n;
}
export function fromUnits(value: bigint): string {
  const negative = value < 0n,
    n = negative ? -value : value;
  return `${negative ? "-" : ""}${n / SCALE}.${(n % SCALE).toString().padStart(8, "0")}`;
}
export function recordBalances(
  ledger: Ledger,
  account: string,
  response: BalanceResponse,
  now: number,
): { ledger: Ledger; balances: DeepSeekBalance[]; increased: boolean } {
  const next = structuredClone(ledger),
    today = beijingDay(now);
  let increased = false;
  const records =
    next.accounts[account] ??
    (next.accounts[account] = {} as Ledger["accounts"][string]);
  const balances = response.balance_infos.map((balance) => {
    const current = toUnits(balance.total_balance),
      previous = records[balance.currency];
    let total = previous ? BigInt(previous.total) : 0n;
    const days = { ...previous?.days };
    if (previous) {
      const delta = BigInt(previous.last) - current;
      if (delta > 0n) {
        total += delta;
        // Across midnight/offline gaps, the actual day of spending is unknown.
        // Count the decrease in total only; begin today's baseline at this sample.
        if (beijingDay(previous.timestamp) === today)
          days[today] = (BigInt(days[today] ?? "0") + delta).toString();
      }
      if (delta < 0n) increased = true;
    }
    const recent = Object.fromEntries(
      Object.entries(days)
        .sort(([a], [b]) => a.localeCompare(b))
        .slice(-400),
    );
    records[balance.currency] = {
      last: current.toString(),
      timestamp: now,
      baselineAt: previous?.baselineAt ?? now,
      total: total.toString(),
      days: recent,
    };
    return {
      currency: balance.currency,
      total: balance.total_balance,
      granted: balance.granted_balance,
      toppedUp: balance.topped_up_balance,
      todaySpent: fromUnits(BigInt(recent[today] ?? "0")),
      totalSpent: fromUnits(total),
      baselineAt: previous?.baselineAt ?? now,
    };
  });
  return { ledger: next, balances, increased };
}
