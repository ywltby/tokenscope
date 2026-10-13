/// U03：统计时区（应用级唯一权威来源 = `settings.toml` 的 `timezone` 字段）。
///
/// 历史：2026-10-04 起时区存在 localStorage 键 `tokenscope-tz`，并被视图快照
/// 一起保存/恢复，导致「改了时区又被旧快照改回去」。现在时区只此一处权威：
/// 视图快照不含它，恢复也绝不改写它；旧键只在启动时做一次迁移。
///
/// 读写路径：
/// - 启动：`settings_get` 读 `timezone`（缺省 Asia/Shanghai），完成后 `ready`
///   置位——主应用在时区就绪后才挂载汇总页，首屏查询的 tz 一定是权威值；
/// - 修改：设置页下拉 → 串行保存队列（`lib/settingQueue`，与 U06 的读取间隔
///   共用）→ `settings_set_timezone`，失败回滚到已落盘值并暴露局部错误。
///
/// 启动读取**不消耗** `settingsPreload` 的预读缓存（设置页首次进入仍复用
/// 那份预读）：这里的 `settings_get` 是独立调用。
import { computed, ref, type Ref, type WritableComputedRef } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { readItem, removeItem } from "../lib/localStorage";
import { createSettingQueue } from "../lib/settingQueue";
import { adoptCollectInterval } from "./collectInterval";

/** 旧键（历史遗留，仅用于一次性迁移）。 */
const STORAGE_KEY = "tokenscope-tz";
/** 缺省时区（设置文件缺字段、读取失败、非法值都回落到它）。 */
export const DEFAULT_TIMEZONE = "Asia/Shanghai";

/// R03：取值校验——`local` 特判放行；其余必须是合法 IANA 时区（Intl 构造
/// 校验）。用于迁移旧键与兜底手工写入设置文件的脏值，绝不因历史脏值抛异常。
function validTz(tz: string): boolean {
  if (tz === "local") return true;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

function saveTimezone(value: string): Promise<string | null> {
  return invoke<string | null>("settings_set_timezone", { timezone: value });
}

const queue = createSettingQueue<string>({
  save: saveTimezone,
  persisted: DEFAULT_TIMEZONE,
});

let bootPromise: Promise<void> | null = null;
/// 用户是否已经改过时区：启动读取的结果晚到时不得覆盖用户的修改。
let userTouched = false;

/// 对外形态不变：`const { tz } = useTimezone()`；写入即入队串行保存。
const tz = computed<string>({
  get: () => queue.value.value,
  set: (v) => {
    userTouched = true;
    queue.set(v);
  },
});

/// 非阻断提示（迁移失败/读取失败/非法值），由设置页展示；不阻断任何功能。
const notice = ref<string | null>(null);
/// 启动读取是否已完成：主应用据此决定何时挂载汇总页（时区就绪前不发查询）。
const ready = ref(false);

function removeOldKey(): void {
  removeItem(STORAGE_KEY);
}

function textOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/// 启动读取 + 一次性迁移。三条分支（计划 U03）：
/// 1. TOML 已有 `timezone` → 以 TOML 为准，删除旧键（不等保存）；
/// 2. 字段缺失且旧键合法 → 保存到 TOML，成功后删除旧键；失败则保留旧键、
///    本次会话用旧值，并显示非阻断提示；
/// 3. 读取设置失败 → 旧键合法就用旧键，否则用默认值；不迁移、不删除，
///    显示非阻断提示。
async function boot(): Promise<void> {
  const legacy = readItem(STORAGE_KEY);
  let settings: { timezone?: unknown; collect_interval_secs?: unknown } | null = null;
  let readFailed = false;
  try {
    settings = await invoke<{ timezone?: unknown; collect_interval_secs?: unknown } | null>(
      "settings_get",
    );
  } catch (e) {
    readFailed = true;
    console.warn("读取设置失败，时区改用本地兜底:", e);
  }
  // 启动读取结果晚于用户修改：以用户修改为准，不覆盖、不迁移、不删旧键。
  if (userTouched) {
    ready.value = true;
    return;
  }
  if (readFailed) {
    if (legacy != null && validTz(legacy)) {
      queue.adopt(legacy);
      notice.value = `读取设置文件失败，本次会话沿用旧的本地时区偏好（${legacy}）；重新选择后会尝试保存到设置文件。`;
    } else {
      notice.value = `读取设置文件失败，本次会话使用默认时区 ${DEFAULT_TIMEZONE}；重新选择后会尝试保存到设置文件。`;
    }
    ready.value = true;
    return;
  }
  // U06：同一次启动读取顺带采用自动读取间隔——不新增 `settings_get` 调用，
  // 因此不改变"启动后第一次设置读取发生在设置页"的既有契约语义；界面在首次
  // 进入设置页之前也能显示真实的间隔文案（设置页那次读取会再 adopt 一次）。
  adoptCollectInterval(settings?.collect_interval_secs);
  const stored = typeof settings?.timezone === "string" ? settings.timezone.trim() : "";
  if (stored !== "") {
    // 分支 1：TOML 为准；非法值（手工编辑）回退默认并提示。
    const value = validTz(stored) ? stored : DEFAULT_TIMEZONE;
    if (value !== stored) {
      notice.value = `设置文件里的时区 “${stored}” 无法识别，本次使用默认 ${DEFAULT_TIMEZONE}；可在设置页重新选择。`;
    }
    queue.adopt(value);
    removeOldKey();
    ready.value = true;
    return;
  }
  if (legacy != null && validTz(legacy)) {
    // 分支 2：迁移到 TOML；成功才删除旧键。
    queue.adopt(legacy);
    try {
      const saved = await saveTimezone(legacy);
      if (!userTouched) {
        queue.adopt(typeof saved === "string" && saved !== "" ? saved : legacy);
        removeOldKey();
      }
    } catch (e) {
      if (!userTouched) {
        notice.value = `时区偏好未能写入设置文件（${textOf(e)}），本次会话仍使用 ${legacy}；稍后在设置页重新选择即可保存。`;
      }
    }
    ready.value = true;
    return;
  }
  if (legacy != null) {
    // 旧键存在但非法：回退默认，清掉无意义的旧键（不算迁移成功）。
    notice.value = `旧的本地时区偏好 “${legacy}” 无法识别，已回退默认 ${DEFAULT_TIMEZONE}。`;
    removeOldKey();
  }
  ready.value = true;
}

/// 幂等：主应用启动时调用一次；重复调用只读取一次设置。
export function initTimezone(): Promise<void> {
  if (!bootPromise) bootPromise = boot();
  return bootPromise;
}

export const TZ_OPTIONS: { label: string; value: string }[] = [
  { label: "本机时区", value: "local" },
  { label: "Asia/Shanghai", value: "Asia/Shanghai" },
  { label: "UTC", value: "UTC" },
];

export function useTimezone(): {
  /** 当前时区（显示值 = 用户最新选择）；写入走串行保存。 */
  tz: WritableComputedRef<string>;
  /** 是否有保存请求在途。 */
  saving: Ref<boolean>;
  /** 最近一次保存失败的原因（成功后清空）。 */
  error: Ref<string | null>;
  /** 非阻断提示（迁移/读取失败）。 */
  notice: Ref<string | null>;
  /** 启动读取是否已完成。 */
  ready: Ref<boolean>;
  clearNotice: () => void;
} {
  return {
    tz,
    saving: queue.saving,
    error: queue.error,
    notice,
    ready,
    clearNotice: () => {
      notice.value = null;
    },
  };
}

/** 测试钩子：重置模块级单例（启动状态、提示、队列基线）。 */
export function resetTimezoneForTests(): void {
  bootPromise = null;
  userTouched = false;
  notice.value = null;
  ready.value = false;
  queue.resetForTests(DEFAULT_TIMEZONE);
}
