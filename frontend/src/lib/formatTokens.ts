// Token 数量级换算与科学计数法格式化（指标卡副行 + 图表坐标轴共用同一量级表）。
//
// 量级阶梯（与用户约定一致）：
// - 中文（zh-CN）：< 1 万不缩略，1 万（万）→ 1 亿（亿）→ 1 万亿（万亿）→ ≥ 10¹⁶ 转上标科学计数法；
// - 英文（en-US）：< 1K 不缩略，1K → 1M → 1B → 1T（10¹²）→ ≥ 10¹⁵ 转上标科学计数法。
//
// 指标卡路径全程基于原生 BigInt 与十进制字符串做精确四舍五入与跨级进位，支持任意长度
// 聚合总量（不受 JS f64 2⁵³ 或 10³⁰⁸ 限制），且避免出现 "10000.00 万亿" / "1000.00T" / "10.00 × 10¹⁶"。
import type { TokenCountValue } from "../types";
import { toTokenBigInt } from "./tokenUsage";
import { localeRef, t, type AppLocale } from "./translate";

interface TokenScaleStep {
  /** 量级对应的 10 的幂次（如 4 表示 10⁴ = 1 万）。 */
  readonly exp: number;
  readonly unit: string;
  /** 图表坐标轴刻度的小数位数（尾部 .0 自动省略）。 */
  readonly axisDecimals: number;
}

interface LocaleTokenScaleConfig {
  readonly steps: readonly TokenScaleStep[];
  /** 最高命名单位继续进位转为科学计数法的 10 的幂次阈值。 */
  readonly scientificExp: number;
}

/** 按界面语言定义的量级阶梯（指标卡与图表坐标轴同源）。 */
const TOKEN_SCALES: Record<AppLocale, LocaleTokenScaleConfig> = {
  "zh-CN": {
    steps: [
      { exp: 4, unit: "万", axisDecimals: 0 },
      { exp: 8, unit: "亿", axisDecimals: 1 },
      { exp: 12, unit: "万亿", axisDecimals: 1 },
    ],
    scientificExp: 16,
  },
  "en-US": {
    steps: [
      { exp: 3, unit: "K", axisDecimals: 1 },
      { exp: 6, unit: "M", axisDecimals: 1 },
      { exp: 9, unit: "B", axisDecimals: 1 },
      { exp: 12, unit: "T", axisDecimals: 1 },
    ],
    scientificExp: 15,
  },
};

const SUPERSCRIPT_DIGITS = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"] as const;

/** 将整数指数转为 Unicode 上标字符（如 16 → "¹⁶"）。 */
export function toSuperscript(exp: number): string {
  const s = String(Math.trunc(exp));
  let out = "";
  for (const ch of s) {
    if (ch === "-") out += "⁻";
    else if (ch >= "0" && ch <= "9") out += SUPERSCRIPT_DIGITS[ch.charCodeAt(0) - 48];
    else out += ch;
  }
  return out;
}

/**
 * 纯十进制字符串实现 `roundHalfUp(BigInt(digits) / 10^(exp - decimals))`。
 * 对任意位数的大整数均精确且 $O(1)$ 处理截断位，无浮点误差。
 */
function roundScaledDigits(digits: string, exp: number, decimals: number): bigint {
  const shift = exp - decimals;
  if (shift <= 0) {
    return BigInt(digits) * 10n ** BigInt(-shift);
  }
  const len = digits.length;
  const head = len > shift ? digits.slice(0, len - shift) : "0";
  const nextDigit = len >= shift ? digits[len - shift] : "0";
  return BigInt(head) + (nextDigit >= "5" ? 1n : 0n);
}

function formatScaledFixed(scaledInt: bigint, decimals: number, trimTrailingZeros = false): string {
  if (decimals === 0) return scaledInt.toString();
  const s = scaledInt.toString().padStart(decimals + 1, "0");
  const intPart = s.slice(0, s.length - decimals);
  let fracPart = s.slice(s.length - decimals);
  if (trimTrailingZeros) {
    fracPart = fracPart.replace(/0+$/, "");
    return fracPart ? `${intPart}.${fracPart}` : intPart;
  }
  return `${intPart}.${fracPart}`;
}

/**
 * 将正十进制数字字符串格式化为上标科学计数法（如 `"1.23 × 10¹⁶"`）。
 * 自动处理尾数四舍五入进位（如 `9.999 × 10¹⁶` → `1.00 × 10¹⁷`）。
 */
function formatScientificDigits(
  digits: string,
  extraPow10: number,
  decimals: number,
  trimTrailingZeros = false,
): string {
  const len = digits.length;
  let exp = len - 1 + extraPow10;
  const sig = 1 + decimals;
  let mantissaInt: bigint;
  if (len <= sig) {
    mantissaInt = BigInt(digits.padEnd(sig, "0"));
  } else {
    mantissaInt = BigInt(digits.slice(0, sig)) + (digits[sig] >= "5" ? 1n : 0n);
  }
  const carryThreshold = 10n ** BigInt(sig);
  if (mantissaInt >= carryThreshold) {
    mantissaInt = 10n ** BigInt(decimals);
    exp += 1;
  }
  const mantissa = formatScaledFixed(mantissaInt, decimals, trimTrailingZeros);
  return `${mantissa} × 10${toSuperscript(exp)}`;
}

function formatFloatScientific(abs: number, extraPow10: number, decimals: number): string {
  const [rawMantissa, rawExp] = abs.toExponential(decimals).split("e");
  const mantissa = rawMantissa.replace(/(\.\d*?[1-9])0+$/, "$1").replace(/\.0+$/, "");
  const exp = Number(rawExp) + extraPow10;
  return `${mantissa} × 10${toSuperscript(exp)}`;
}

/**
 * 格式化总览指标卡「总 token」下方的换算单位行（响应式跟随当前语言，支持任意长 BigInt）。
 */
export function formatSummaryTokenUnit(total: TokenCountValue | bigint): string {
  const locale = localeRef.value;
  if (typeof total === "number" && !Number.isFinite(total)) {
    return t("dashboard.tokensUnit");
  }
  if (typeof total === "string" && !/^-?\d+$/.test(total.trim())) {
    return t("dashboard.tokensUnit");
  }
  const n = toTokenBigInt(total);
  const negative = n < 0n;
  const abs = negative ? -n : n;
  const sign = negative ? "-" : "";

  const config = TOKEN_SCALES[locale];
  const minDivisor = 10n ** BigInt(config.steps[0].exp);
  if (abs < minDivisor) {
    return t("dashboard.tokensUnit");
  }

  const digits = abs.toString();
  const exp10 = digits.length - 1;
  if (exp10 >= config.scientificExp) {
    return t("dashboard.tokensScientific", {
      value: sign + formatScientificDigits(digits, 0, 2, false),
    });
  }

  let index = 0;
  for (let i = config.steps.length - 1; i >= 0; i--) {
    if (exp10 >= config.steps[i].exp) {
      index = i;
      break;
    }
  }

  const decimals = 2;
  const decFactor = 100n;
  while (index < config.steps.length) {
    const step = config.steps[index];
    const nextExp =
      index + 1 < config.steps.length ? config.steps[index + 1].exp : config.scientificExp;
    const scaledInt = roundScaledDigits(digits, step.exp, decimals);
    const carryThreshold = 10n ** BigInt(nextExp - step.exp) * decFactor;
    if (scaledInt >= carryThreshold) {
      index += 1;
      continue;
    }
    const value = sign + formatScaledFixed(scaledInt, decimals, false);
    return t("dashboard.tokensApprox", { value, unit: step.unit });
  }

  return t("dashboard.tokensScientific", {
    value: sign + formatScientificDigits(digits, 0, 2, false),
  });
}

/**
 * 格式化图表数值轴的紧凑刻度（与总览卡片同量级阶梯；`scalePow10 > 0` 用于超出
 * `f64` 安全量级的超大聚合值缩放绘图）。
 */
export function formatCompactTokenAxis(value: number, scalePow10 = 0): string {
  const locale = localeRef.value;
  if (!Number.isFinite(value) || value === 0) return String(value);
  const sign = value < 0 ? "-" : "";
  const abs = Math.abs(value);

  const config = TOKEN_SCALES[locale];
  let effectiveAbs = abs;
  if (scalePow10 > 0) {
    const [rawMantissa, rawExp] = abs.toExponential(11).split("e");
    const realExp = Number(rawExp) + scalePow10;
    if (realExp >= config.scientificExp) {
      return sign + formatFloatScientific(abs, scalePow10, 1);
    }
    // 缩放查看小柱条时真实刻度落回常规量级，按十进制指数直接还原（避免 10 ** scalePow10 溢出）
    effectiveAbs = Number(`${rawMantissa}e${realExp}`);
  }

  if (effectiveAbs < 10 ** config.steps[0].exp) {
    return `${sign}${effectiveAbs}`;
  }
  if (effectiveAbs >= 10 ** config.scientificExp) {
    return sign + formatFloatScientific(effectiveAbs, 0, 1);
  }

  let index = 0;
  for (let i = config.steps.length - 1; i >= 0; i--) {
    if (effectiveAbs >= 10 ** config.steps[i].exp) {
      index = i;
      break;
    }
  }

  while (index < config.steps.length) {
    const step = config.steps[index];
    const nextExp =
      index + 1 < config.steps.length ? config.steps[index + 1].exp : config.scientificExp;
    const scaled = effectiveAbs / 10 ** step.exp;
    const rounded = Number(scaled.toFixed(step.axisDecimals));
    if (rounded >= 10 ** (nextExp - step.exp)) {
      index += 1;
      continue;
    }
    return `${sign}${rounded}${step.unit}`;
  }

  return sign + formatFloatScientific(effectiveAbs, 0, 1);
}
