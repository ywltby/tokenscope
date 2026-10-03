/// 明暗双模式（用户决策）：默认跟随系统，切换后持久化到 localStorage。
import { ref, watchEffect } from "vue";

type ThemeMode = "light" | "dark";

const STORAGE_KEY = "tokenscope-theme";

function initial(): ThemeMode {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === "light" || saved === "dark") return saved;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

const mode = ref<ThemeMode>(initial());

watchEffect(() => {
  localStorage.setItem(STORAGE_KEY, mode.value);
});

export function useTheme() {
  function toggle(): void {
    mode.value = mode.value === "dark" ? "light" : "dark";
  }
  return { mode, toggle };
}
