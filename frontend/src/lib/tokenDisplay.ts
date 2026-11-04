// UX07：token 四桶显示词单一来源——表头、图例、摘要、说明与费用公式
// 全部消费同一份元数据（key/显示名/顺序），不再各自硬编码。
// U14：显示词统一为「缓存命中」，技术键 `cache_read` 与后端口径
// 「缓存命中率」保留，不机械改名。
//
// i18n（2026-11-04）：`label` 改为 **getter**，读取时按当前界面语言查表。
// 这样所有既有消费点（表头、图例、颜色设置、费用公式）都不必改写就能
// 跟随语言切换：模板 / computed 里读 `.label` 会经 `t()` 依赖语言 ref，
// 语言变化后重新渲染。键与顺序（chart 系列身份）始终不变。
import { t } from "./translate";

export type TokenBucketKey = "input" | "output" | "cache_write" | "cache_read";

export interface TokenBucketMeta {
  key: TokenBucketKey;
  /** 当前界面语言下的显示名（getter：读取时才解析）。 */
  readonly label: string;
}

const BUCKET_LABEL_KEY: Record<TokenBucketKey, string> = {
  input: "buckets.input",
  output: "buckets.output",
  cache_write: "buckets.cacheWrite",
  cache_read: "buckets.cacheRead",
};

/** 展示顺序 = 依次输入、输出、缓存写、缓存命中。 */
export const TOKEN_BUCKETS: readonly TokenBucketMeta[] = [
  {
    key: "input",
    get label() {
      return t(BUCKET_LABEL_KEY.input);
    },
  },
  {
    key: "output",
    get label() {
      return t(BUCKET_LABEL_KEY.output);
    },
  },
  {
    key: "cache_write",
    get label() {
      return t(BUCKET_LABEL_KEY.cache_write);
    },
  },
  {
    key: "cache_read",
    get label() {
      return t(BUCKET_LABEL_KEY.cache_read);
    },
  },
];

export function tokenBucketLabel(key: TokenBucketKey): string {
  return t(BUCKET_LABEL_KEY[key] ?? key);
}

/**
 * RC07：单价量纲的**单一来源**——单价是「USD / 1M token」（每百万 token），
 * 不是每 token 价。列头、说明与费用公式共用同一份文案，避免各处自造单位
 * 写法（如只写 `$`、`/M` 而不解释量纲）。
 *
 * i18n：量纲本身是技术口径（USD / 1M token 是计价定义，不随语言换算），
 * 因此**不翻译**，界面两种语言下使用同一字符串；数值与单位比例绝不改变。
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
