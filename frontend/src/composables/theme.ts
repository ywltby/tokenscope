/// 主题偏好（设计系统 Task 1 / UX09）：light | dark | system。
/// 默认跟随系统；显式选择持久化到 localStorage（键与解析规则见
/// `lib/themePreference.ts`，首帧引导脚本 public/theme-boot.js 复用同一套）。
/// 系统主题变化只影响 system 偏好；组件只消费解析后的 mode。
import { computed, ref, watchEffect } from "vue";
import { writeItem } from "../lib/localStorage";
import {
  THEME_STORAGE_KEY,
  readStoredPreference,
  resolveMode,
  type ThemeMode,
  type ThemePreference,
} from "../lib/themePreference";

export type { ThemeMode, ThemePreference };

const mql = window.matchMedia("(prefers-color-scheme: dark)");
const systemDark = ref(mql.matches);
mql.addEventListener?.("change", (e) => {
  systemDark.value = e.matches;
});

const preference = ref<ThemePreference>(readStoredPreference());

const mode = computed<ThemeMode>(() => resolveMode(preference.value, systemDark.value));

watchEffect(() => {
  // RC09：localStorage 不可用（隐私模式、策略禁用、配额耗尽）时写入会直接
  // 抛错。偏好无法持久化不应打断应用启动——这里降级为"本次会话内生效"。
  writeItem(THEME_STORAGE_KEY, preference.value);
});

export function useTheme() {
  function setPreference(p: ThemePreference): void {
    preference.value = p;
  }
  /// 兼容旧调用点（切换即改为显式偏好）。
  function toggle(): void {
    preference.value = mode.value === "dark" ? "light" : "dark";
  }
  return { preference, mode, setPreference, toggle };
}
