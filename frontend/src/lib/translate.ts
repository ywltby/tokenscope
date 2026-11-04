// i18n 核心：语言 ref + vue-i18n composer + 翻译入口。
//
// **依赖纪律**：本模块只依赖 `vue`、`vue-i18n` 与两份消息资源。它被
// `tokenDisplay` / `costBreakdown` / `formatMoney` / `dates` / `rangePreset`
// 等纯函数库导入，因此**不得**引入 naive-ui 或 Tauri API——否则这些库的
// 每个消费者都会连带加载组件库（测试里表现为启动明显变慢）。
// 组件语言映射与设置持久化在 `lib/i18n.ts`（那里才引 naive-ui 与 invoke）。
import { ref } from "vue";
import { createI18n } from "vue-i18n";
import enUSMessages from "./locales/en-US";
import zhCNMessages from "./locales/zh-CN";

export type AppLocale = "zh-CN" | "en-US";

/** 合法语言取值（与后端 `LANGUAGE_CHOICES` 一致）。 */
export const LOCALES: readonly AppLocale[] = ["zh-CN", "en-US"];
/** 缺省语言（字段缺失、读取失败、非法值都回落到它）。 */
export const DEFAULT_LOCALE: AppLocale = "zh-CN";

const MESSAGES: Record<AppLocale, typeof zhCNMessages> = {
  "zh-CN": zhCNMessages,
  "en-US": enUSMessages,
};

/** 合法化存储值；非法 / 缺失一律回落默认（绝不抛错）。 */
export function normalizeLocale(value: unknown): AppLocale {
  return value === "en-US" ? "en-US" : DEFAULT_LOCALE;
}

export const i18n = createI18n({
  legacy: false,
  locale: DEFAULT_LOCALE,
  fallbackLocale: DEFAULT_LOCALE,
  // 缺键回退是设计行为（en-US 缺键时显示中文而不是键名）；不刷警告。
  missingWarn: false,
  fallbackWarn: false,
  messages: MESSAGES,
});

/** 当前语言（响应式；与 `i18n.global.locale` 保持同步）。 */
export const localeRef = ref<AppLocale>(DEFAULT_LOCALE);

const translate = i18n.global.t as unknown as (
  key: string,
  named?: Record<string, unknown>,
) => string;

/**
 * 通用翻译入口：读一次 `localeRef` 建立响应式依赖，再由 vue-i18n 查表插值。
 * 组件里 `const t = useT()` 后即可在模板中使用 `t("key", { … })`——语言切换
 * 时模板随依赖重新渲染。非组件代码（状态机、纯函数）直接调用同一函数，
 * 取到的是调用时刻的语言。
 */
export function t(key: string, named?: Record<string, unknown>): string {
  void localeRef.value;
  return translate(key, named);
}

/** 组件内使用的翻译函数（同 [`t`]，语义更明确）。 */
export function useT(): typeof t {
  return t;
}

/**
 * 落生效语言：内存 ref + vue-i18n composer + `html[lang]`。三处必须同步，
 * 否则组件文案与 Naive UI 的日期/分页文案会分叉。
 */
export function applyLocale(locale: AppLocale): void {
  localeRef.value = locale;
  i18n.global.locale.value = locale;
  if (typeof document !== "undefined") document.documentElement.lang = locale;
}

applyLocale(DEFAULT_LOCALE);
