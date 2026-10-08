/// 应用级时区偏好（用户 2026-10-04 决策：时区选择移入设置页，主界面不显示）。
/// localStorage 持久化；Dashboard 与 Settings 共享同一模块级 ref，设置页修改后
/// 回到汇总页即按新时区刷新。
import { ref, watchEffect } from "vue";
import { readItem, writeItem } from "../lib/localStorage";

const STORAGE_KEY = "tokenscope-tz";
const DEFAULT_TZ = "Asia/Shanghai";

/// R03：持久化值校验——`local` 特判放行；其余必须是合法 IANA 时区
/// （Intl 构造校验）。非法旧偏好回退默认，不因历史脏值抛异常导致
/// 首屏崩溃。
function validTz(tz: string): boolean {
  if (tz === "local") return true;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

function initial(): string {
  // RC09：存储不可用（隐私模式/策略禁用）时读不到偏好也不能抛——回落默认时区。
  const saved = readItem(STORAGE_KEY);
  if (saved != null && validTz(saved)) return saved;
  if (saved != null) {
    // 无日志后端可用（前端层），console.warn 足够——不影响渲染。
    console.warn(`持久化时区偏好非法（"{saved}"），回退默认 "${DEFAULT_TZ}"`);
  }
  return DEFAULT_TZ;
}

const tz = ref<string>(initial());

watchEffect(() => {
  // RC09：持久化失败只是"下次启动回到默认时区"，不得打断组件初始化。
  writeItem(STORAGE_KEY, tz.value);
});

export const TZ_OPTIONS: { label: string; value: string }[] = [
  { label: "本机时区", value: "local" },
  { label: "Asia/Shanghai", value: "Asia/Shanghai" },
  { label: "UTC", value: "UTC" },
];

export function useTimezone() {
  return { tz };
}
