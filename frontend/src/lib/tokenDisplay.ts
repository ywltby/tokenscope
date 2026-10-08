// UX07：token 四桶显示词单一来源——表头、图例、摘要、说明与费用公式
// 全部消费同一份元数据（key/显示名/顺序），不再各自硬编码。
// U14：显示词统一为「缓存命中」，技术键 `cache_read` 与后端口径
// 「缓存命中率」保留，不机械改名。

export type TokenBucketKey = "input" | "output" | "cache_write" | "cache_read";

export interface TokenBucketMeta {
  key: TokenBucketKey;
  label: string;
}

/** 展示顺序 = 依次输入、输出、缓存写、缓存命中。 */
export const TOKEN_BUCKETS: readonly TokenBucketMeta[] = [
  { key: "input", label: "输入" },
  { key: "output", label: "输出" },
  { key: "cache_write", label: "缓存写" },
  { key: "cache_read", label: "缓存命中" },
];

export function tokenBucketLabel(key: TokenBucketKey): string {
  return TOKEN_BUCKETS.find((b) => b.key === key)?.label ?? key;
}

/**
 * RC07：单价量纲的**单一来源**——单价是「USD / 1M token」（每百万 token），
 * 不是每 token 价。列头、说明与费用公式共用同一份文案，避免各处自造单位
 * 写法（如只写 `$`、`/M` 而不解释量纲）。
 */
export const UNIT_PRICE_DENOMINATOR = "USD / 1M token";
/** 列头用的紧凑写法（与 {@link UNIT_PRICE_DENOMINATOR} 等价）。 */
export const UNIT_PRICE_SUFFIX = "$/M";
/**
 * 公式行跟在金额之后的量纲后缀：单价数值本身已带 `$`（如 `$8`），
 * 因此这里只补 `/M`，与 {@link UNIT_PRICE_SUFFIX} 同一口径、同一来源，
 * 避免各处再自造 "/百万"、"per 1M" 等写法。
 */
export const UNIT_PRICE_SUFFIX_AFTER_AMOUNT = "/M";
