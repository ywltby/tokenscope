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
//   门禁的既有断言（`consent_screen_*`）继续成立；
// - 同意成功后（`flushPendingOnConsent`）：把弹窗内**最后一次明确选择**
//   写进设置，再读一次权威值。即使该选择恰好等于默认值也照写——旧设置可能
//   是英文，跳过写入会被旧值顶回去（`i18n.consent.test.ts` 覆盖）；
// - 修改：设置页下拉 → `changeLocale` → 设置串行队列 → `settings_set_language`。
//
// 并发顺序：写入走 `lib/settingQueue` 的串行队列——同一时刻只有一个保存在途、
// 在途期间的选择只保留最新一个、较早请求**不可能**覆盖较晚选择；只有在没有
// 更新选择时才回滚到最后一次成功落盘值。界面语言取队列的显示值，因此界面、
// 设置文件与后端托盘（每次成功写入后重建）最终一致。
//
// 与统计时区（composables/timezone）严格分离：语言只影响界面文案、Naive
// UI 组件语言与 `html[lang]`，绝不参与时间范围、时区换算、数据库键或
// token / 费用计算。
//
// 依赖纪律：纯函数库只应 import `./translate`（无 naive-ui / Tauri），
// 本模块才承载组件语言映射与持久化。
import { computed, watch } from "vue";
import { dateEnUS, dateZhCN, enUS, zhCN } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import { createSettingQueue } from "./settingQueue";
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

/**
 * 队列里的"已落盘值"类型：`unknown` = 尚未读到/写过设置。
 *
 * 用哨兵而不是默认值：同意时用户明确选的语言可能**正好等于默认值**，若把
 * 默认值当基线，队列会认为"无需保存"而跳过写入，旧设置（例如英文）随后
 * 就会覆盖用户的显式选择。
 */
type SavedLocale = AppLocale | "unknown";
const UNKNOWN: SavedLocale = "unknown";

/** 语言写入后端；返回归一化后的落盘值。 */
async function saveLanguage(locale: AppLocale): Promise<AppLocale> {
  const committed = await invoke<string>("settings_set_language", { language: locale });
  return normalizeLocale(committed);
}

const queue = createSettingQueue<SavedLocale>({
  save: async (value) => saveLanguage(normalizeLocale(value)),
  persisted: UNKNOWN,
});

/** 是否有保存请求在途（设置页显示"保存中…"）。 */
const persisting = queue.saving;
/** 最近一次保存失败的原因（设置页展示；成功后清空）。 */
const persistError = queue.error;

/** 用户改过语言（同意弹窗或设置页）；启动读取晚到时不得覆盖。 */
let userTouched = false;
/** 同意前用户在弹窗里的**最后一次**选择（null = 未动过）。 */
let consentChoice: AppLocale | null = null;
/** 队列显示值 → 实际生效语言（未知基线不覆盖当前语言）。 */
watch(queue.value, (next) => {
  const locale = next === UNKNOWN ? DEFAULT_LOCALE : normalizeLocale(next);
  if (locale !== localeRef.value) applyLocale(locale);
});

/**
 * 同意前的内存切换（隐私弹窗的语言下拉）。不触发任何持久化；
 * `changeLocale`（已同意）才写设置。记录最后一次选择，切换回默认值也算。
 */
export function setLocale(locale: AppLocale): void {
  if (!LOCALES.includes(locale)) return;
  consentChoice = locale;
  if (locale !== localeRef.value) applyLocale(locale);
}

/**
 * 读取设置的权威值并应用（只应在同意成功后调用）。
 *
 * `userTouched` 时只跳过**应用**，不改写队列基线：界面不得被旧值顶回去。
 */
export async function adoptPersisted(): Promise<void> {
  try {
    const s = await invoke<{ language?: unknown } | null>("settings_get");
    const stored = normalizeLocale(s?.language);
    if (userTouched) {
      // 用户已明确选过：显示值必须服从用户；但基线仍要更新，否则后续"是否
      // 需要保存"与失败回滚都会用错落点（旧实现会因此把最后一次选择吞掉）。
      queue.adoptBaseline(stored);
      return;
    }
    queue.adopt(stored);
  } catch (e) {
    // 读取失败只是"不知道历史选择"：保持当前语言，不阻断、不清空设置。
    console.warn("读取界面语言设置失败:", e);
  }
}

/**
 * 同意成功后由 App.vue 调用：把弹窗内的选择（若用户动过）写进设置，再读一次
 * 权威值。保存失败不抛错（不阻断进入主应用），原因经 `persistError` 暴露，
 * 且**本次会话仍保留用户的选择**。
 */
export async function flushPendingOnConsent(): Promise<void> {
  const chosen = consentChoice;
  consentChoice = null;
  if (chosen !== null) {
    userTouched = true;
    queue.set(chosen);
    await queue.idle();
    // 写入失败时队列会回滚显示值；用户的显式选择仍应在本会话生效。
    if (localeRef.value !== chosen) applyLocale(chosen);
  }
  await adoptPersisted();
}

/** 主界面里的语言切换（已同意）：立即保存，失败回滚到最后成功落盘值。 */
export function changeLocale(locale: AppLocale): void {
  if (!LOCALES.includes(locale)) return;
  userTouched = true;
  queue.set(locale);
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
