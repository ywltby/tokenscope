/// 主题偏好（设计系统 Task 1）：light | dark | system。
/// 默认跟随系统；显式选择持久化到 localStorage（沿用 tokenscope-theme
/// 键——旧版本存的是解析后的 light/dark，读取时直接作为显式偏好兼容）。
/// 系统主题变化只影响 system 偏好；组件只消费解析后的 mode。
import { computed, ref, watchEffect } from "vue";

export type ThemePreference = "light" | "dark" | "system";
export type ThemeMode = "light" | "dark";

const STORAGE_KEY = "tokenscope-theme";

function loadPreference(): ThemePreference {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === "light" || saved === "dark" || saved === "system") return saved;
  return "system";
}

const mql = window.matchMedia("(prefers-color-scheme: dark)");
const systemDark = ref(mql.matches);
mql.addEventListener?.("change", (e) => {
  systemDark.value = e.matches;
});

const preference = ref<ThemePreference>(loadPreference());

const mode = computed<ThemeMode>(() =>
  preference.value === "system" ? (systemDark.value ? "dark" : "light") : preference.value,
);

watchEffect(() => {
  localStorage.setItem(STORAGE_KEY, preference.value);
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
