/// 应用级时区偏好（用户 2026-10-04 决策：时区选择移入设置页，主界面不显示）。
/// localStorage 持久化；Dashboard 与 Settings 共享同一模块级 ref，设置页修改后
/// 回到汇总页即按新时区刷新。
import { ref, watchEffect } from "vue";

const STORAGE_KEY = "tokenscope-tz";

function initial(): string {
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved ?? "Asia/Shanghai";
}

const tz = ref<string>(initial());

watchEffect(() => {
  localStorage.setItem(STORAGE_KEY, tz.value);
});

export const TZ_OPTIONS: { label: string; value: string }[] = [
  { label: "本机时区", value: "local" },
  { label: "Asia/Shanghai", value: "Asia/Shanghai" },
  { label: "UTC", value: "UTC" },
];

export function useTimezone() {
  return { tz };
}
