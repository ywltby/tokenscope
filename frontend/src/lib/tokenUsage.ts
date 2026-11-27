import type { BigTokenCounts, TokenCounts, TokenCountValue } from "../types";

/**
 * 把任意 token 计数值（安全范围内的 `number`、超大十进制 `string` 或已有 `bigint`）
 * 无损归一为 JS 原生 `BigInt`。非法/非有限值回退为 `0n`。
 */
export function toTokenBigInt(v: TokenCountValue | bigint | null | undefined): bigint {
  if (typeof v === "bigint") return v;
  if (typeof v === "number") {
    return Number.isFinite(v) ? BigInt(Math.trunc(v)) : 0n;
  }
  if (typeof v === "string") {
    const trimmed = v.trim();
    if (/^-?\d+$/.test(trimmed)) return BigInt(trimmed);
  }
  return 0n;
}

/** IPC 保留互斥计费桶；所有用量展示的输入统一包含两类缓存（原生 BigInt 精确累计）。 */
export function totalInput(
  tokens: Pick<TokenCounts, "input" | "cache_write" | "cache_read">,
): bigint {
  return (
    toTokenBigInt(tokens.input) +
    toTokenBigInt(tokens.cache_write) +
    toTokenBigInt(tokens.cache_read)
  );
}

export function displayTokens(tokens: TokenCounts): BigTokenCounts {
  return {
    input: totalInput(tokens),
    output: toTokenBigInt(tokens.output),
    cache_write: toTokenBigInt(tokens.cache_write),
    cache_read: toTokenBigInt(tokens.cache_read),
  };
}

export function totalTokens(tokens: TokenCounts): bigint {
  return totalInput(tokens) + toTokenBigInt(tokens.output);
}

/**
 * 任意精度占比百分比（0–100，精确到万分之一百分点），避免超大 `BigInt` 直接转
 * `Number` 相除时出现 `Infinity / Infinity = NaN`。
 */
export function ratioPercentBigInt(numerator: bigint, denominator: bigint): number {
  if (denominator <= 0n) return 0;
  const scaled = (numerator * 1_000_000n + denominator / 2n) / denominator;
  return Number(scaled) / 10_000;
}

/** 任意精度两位小数百分比字符串（如 `"17.39"`），供比例条宽度等使用。 */
export function percentageFixed2BigInt(numerator: bigint, denominator: bigint): string {
  if (denominator <= 0n) return "0.00";
  const scaled = (numerator * 10_000n + denominator / 2n) / denominator;
  const intPart = scaled / 100n;
  const fracPart = (scaled % 100n).toString().padStart(2, "0");
  return `${intPart}.${fracPart}`;
}

/**
 * 将任意长度 `BigInt` 按 `10^scalePow10` 缩放后转为有限 `number`（供 ECharts 绘图用）。
 * `scalePow10 = 0` 且处于 `f64` 安全量级内时直接转 `Number`；超过 20 位时只保留前
 * 17 位有效数字并按剩余指数组装，绝不因中间转换产生 `Infinity`。
 */
export function bigIntToScaledNumber(value: bigint, scalePow10 = 0): number {
  if (value === 0n) return 0;
  const negative = value < 0n;
  const digits = (negative ? -value : value).toString();
  const len = digits.length;
  const exp = len - 1 - scalePow10;
  if (scalePow10 === 0 && len <= 15) {
    return negative ? -Number(digits) : Number(digits);
  }
  const sig = digits.slice(0, Math.min(len, 17));
  const frac = sig.length > 1 ? `.${sig.slice(1)}` : "";
  const num = Number(`${sig[0]}${frac}e${exp}`);
  return negative ? -num : num;
}
