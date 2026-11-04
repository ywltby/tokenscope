// i18n（2026-11-04）：界面语言状态——`vue-i18n` 单例 + Naive UI 映射 + 设置持久化。
//
// 为什么组件不用模板里的 `$t`：组件测试（App/MainApp/Settings/Dashboard 与各
// 子组件）直接 `mount()` 且不安装插件，`$t` 会取不到。因此统一经
// [`useT`] 拿到受语言 ref 驱动的翻译函数——消息仍由 vue-i18n 的 composer
// 查表、回退与插值，只是入口不依赖插件注入。
//
// 权威来源与隐私边界（用户要求 3）：
// - 唯一权威来源是 `settings.toml` 的 `language` 字段（与统计时区同模型）；
// - 同意成立**前**：允许在内存里切换（`setLocale`），但绝不调用
//   `settings_get` / `settings_set_language`，不读写 localStorage——隐私
//   门禁的既有断言（consent_screen_*）继续成立；
// - 同意成功后：App.vue 在进入主应用前调用 `flushPendingOnConsent()`——先把
//   弹窗内的选择写进设置，再读一次权威值；
// - 修改：设置页下拉 → `changeLocale` → `settings_set_language`（后端校验
//   zh-CN / en-US，非法值拒绝保存）→ 失败回滚到已落盘值并暴露局部错误。
//
// 与统计时区（composables/timezone）严格分离：语言只影响界面文案、Naive
// UI 组件语言与 `html[lang]`，绝不参与时间范围、时区换算、数据库键或
// token / 费用计算。
//
// 依赖纪律：纯函数库只应 import `./translate`（无 naive-ui / Tauri），
// 本模块才承载组件语言映射与持久化。
import { computed, ref, watch } from "vue";
import { dateEnUS, dateZhCN, enUS, zhCN } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import {
  applyLocale,
  DEFAULT_LOCALE,
  i18n,
  LOCALES,
  localeRef,
  normalizeLocale,
  t,
  useT,
  type AppLocale,
} from "./translate";

export { DEFAULT_LOCALE, i18n, LOCALES, normalizeLocale, t, useT };
export type { AppLocale };

/** Naive UI 组件语言（与界面语言同步切换）。 */
export function naiveLocale(locale: AppLocale) {
  return locale === "en-US" ? enUS : zhCN;
}
/** Naive UI 日期语言（同步切换；只影响组件文案，不改任何时间语义）。 */
export function naiveDateLocale(locale: AppLocale) {
  return locale === "en-US" ? dateEnUS : dateZhCN;
}

/** 已落盘值（失败回滚的落点；`adoptPersisted` 后为设置值）。 */
let persisted: AppLocale = DEFAULT_LOCALE;
/** 用户在主界面里改过语言（启动读取晚到时不得覆盖）。 */
let userTouched = false;
/** 最近一次保存失败的错误文本（设置页展示；成功后清空）。 */
const persistError = ref<string | null>(null);
/** 是否有保存请求在途。 */
const persisting = ref(false);
/** 同意前用户在弹窗里选的语言（null = 未动过）。 */
let pendingFromConsent: AppLocale | null = null;

/**
 * 同意前的内存切换（隐私弹窗的语言下拉）。不触发任何持久化；
 * `changeLocale`（已同意）才写设置。
 */
export function setLocale(locale: AppLocale): void {
  if (!LOCALES.includes(locale) || locale === localeRef.value) return;
  applyLocale(locale);
  pendingFromConsent = locale;
}

/** 读取设置的权威值并应用（只应在同意成功后调用）。 */
export async function adoptPersisted(): Promise<void> {
  try {
    const s = await invoke<{ language?: unknown } | null>("settings_get");
    const stored = normalizeLocale(s?.language);
    persisted = stored;
    if (userTouched) return; // 启动读取晚到不得覆盖用户修改
    if (stored !== localeRef.value) applyLocale(stored);
  } catch (e) {
    // 读取失败只是"不知道历史选择"：保持当前语言，不阻断、不清空设置。
    console.warn("读取界面语言设置失败:", e);
  }
}

async function persist(locale: AppLocale): Promise<void> {
  persisting.value = true;
  try {
    const committed = await invoke<string>("settings_set_language", { language: locale });
    persisted = normalizeLocale(committed);
    persistError.value = null;
  } catch (e) {
    persistError.value = e instanceof Error ? e.message : String(e);
    // 回滚到已落盘值：界面不保留一个并未写入的选择。
    applyLocale(persisted);
  } finally {
    persisting.value = false;
  }
}

/**
 * 同意成功后由 App.vue 调用：把弹窗内的选择（若用户动过）写进设置，再读一次
 * 权威值。保存失败不抛错（不阻断进入主应用），原因经 `persistError` 暴露。
 */
export async function flushPendingOnConsent(): Promise<void> {
  const chosen = pendingFromConsent;
  pendingFromConsent = null;
  if (chosen !== null && chosen !== persisted) await persist(chosen);
  await adoptPersisted();
}

/** 主界面里的语言切换（已同意）：立即保存，失败回滚并暴露错误。 */
export function changeLocale(locale: AppLocale): void {
  if (!LOCALES.includes(locale)) return;
  userTouched = true;
  if (locale === localeRef.value) return;
  applyLocale(locale);
  if (locale !== persisted) void persist(locale);
}

export function useAppLocale() {
  return {
    locale: computed(() => localeRef.value),
    persisting,
    persistError,
    clearPersistError: (): void => {
      persistError.value = null;
    },
    setLocale,
    changeLocale,
  };
}

// 语言 ref 是唯一真相：被外部直接改写时同步 composer 与 html[lang]。
watch(localeRef, (v) => {
  if (i18n.global.locale.value !== v) i18n.global.locale.value = v;
  if (typeof document !== "undefined") document.documentElement.lang = v;
});
