// U06（UI 调整计划 2026-10-13）：自动读取间隔——唯一来源 `settings.toml`。
//
// 后端后台线程按 `collect_interval_secs` 定时采集并发出 `usage-updated`；
// 界面只负责展示与保存这个值（自动刷新本身由事件与时钟检查驱动，不按间隔
// 轮询）。写入走 U03 的串行保存队列（`lib/settingQueue`）：同一时刻只有一个
// 保存在途、在途选择只留最新、全部失败且无待保存值时回滚到最后落盘值并暴露
// 局部错误。取值只用后端白名单那六档，界面文案 `5 秒 / 10 秒 / 30 秒 /
// 1 分钟 / 5 分钟 / 10 分钟`。
//
// 权威值从哪来：设置页那次 `settings_get`（`loadSettings`）读取后 `adopt`。
// **刻意不在启动时多发一次 `settings_get`**：设置首读失败/挂起场景
// （`settings-read-failure` / `settings-deferred-race` 契约）依赖"应用启动后
// 第一次设置读取发生在设置页"，提前读一次会把失败/挂起挪到设置页之外，让
// 恢复分支走反。因此在用户进入设置页之前用缺省 300（与后端缺省一致）。
import { computed, type Ref, type WritableComputedRef } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { createSettingQueue } from "../lib/settingQueue";

/** 缺省间隔（秒）：与后端 `collect_interval_secs` 缺省一致。 */
export const DEFAULT_COLLECT_INTERVAL_SECS = 300;

/** 可选值白名单 + 界面文案（后端只接受这六个值）。 */
export const COLLECT_INTERVAL_OPTIONS: { label: string; value: number }[] = [
  { label: "5 秒", value: 5 },
  { label: "10 秒", value: 10 },
  { label: "30 秒", value: 30 },
  { label: "1 分钟", value: 60 },
  { label: "5 分钟", value: 300 },
  { label: "10 分钟", value: 600 },
];

const ALLOWED_SECS: readonly number[] = COLLECT_INTERVAL_OPTIONS.map((o) => o.value);

/** 间隔文案（状态胶囊的悬停提示与设置页下拉共用同一映射）。 */
export function collectIntervalText(secs: number): string {
  return COLLECT_INTERVAL_OPTIONS.find((o) => o.value === secs)?.label ?? `${secs} 秒`;
}

/** 是否属于后端白名单（设置文件里的脏值不得带进请求）。 */
export function isAllowedCollectInterval(value: unknown): value is number {
  return typeof value === "number" && ALLOWED_SECS.includes(value);
}

function saveCollectInterval(secs: number): Promise<number | null> {
  return invoke<number | null>("settings_set_collect_interval", { secs });
}

const queue = createSettingQueue<number>({
  save: saveCollectInterval,
  persisted: DEFAULT_COLLECT_INTERVAL_SECS,
});

/// 用户是否已经选过：启动/重新读取的结果晚到时不得覆盖用户的选择。
let userTouched = false;

const intervalSecs = computed<number>({
  get: () => queue.value.value,
  set: (v) => {
    userTouched = true;
    queue.set(v);
  },
});

/// 设置页读取成功后的权威值（缺字段、白名单外都回缺省）。
export function adoptCollectInterval(value: unknown): void {
  if (userTouched) return;
  queue.adopt(isAllowedCollectInterval(value) ? value : DEFAULT_COLLECT_INTERVAL_SECS);
}

export function useCollectInterval(): {
  /** 当前间隔（显示值 = 用户最新选择）；写入即入队串行保存。 */
  intervalSecs: WritableComputedRef<number>;
  /** 是否有保存请求在途（显示「保存中…」）。 */
  saving: Ref<boolean>;
  /** 最近一次保存失败的原因（成功后清空）。 */
  error: Ref<string | null>;
  /** 等待队列排空（含失败回滚已完成）。 */
  idle: () => Promise<void>;
} {
  return {
    intervalSecs,
    saving: queue.saving,
    error: queue.error,
    idle: () => queue.idle(),
  };
}

/** 测试钩子：重置模块级单例（队列基线回缺省值）。 */
export function resetCollectIntervalForTests(): void {
  userTouched = false;
  queue.resetForTests(DEFAULT_COLLECT_INTERVAL_SECS);
}
