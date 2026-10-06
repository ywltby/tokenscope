<script setup lang="ts">
// 应用壳（设计系统 Task 2）：56px 玻璃导航 + tab 页切换 + 带标签的主题
// 选择器（浅色/深色/跟随系统）。视觉改造不改数据流：回到汇总页仍强制
// 刷新一次（设置页可能重建了缓存）。
import { computed, ref, watch, watchEffect } from "vue";
import {
  dateZhCN,
  zhCN,
  darkTheme,
  NConfigProvider,
  NGlobalStyle,
  NMessageProvider,
  type GlobalTheme,
} from "naive-ui";
import { useTheme, type ThemePreference } from "./composables/theme";
import { naiveThemeOverrides } from "./styles/naiveTheme";
import PricingStatusBanner from "./components/PricingStatusBanner.vue";
import Dashboard from "./views/Dashboard.vue";
import Settings from "./views/Settings.vue";

const { preference, mode, setPreference } = useTheme();
const theme = computed<GlobalTheme | null>(() => (mode.value === "dark" ? darkTheme : null));
// Naive UI 覆盖与 CSS token 双轨消费同一语义色；html[data-theme] 驱动
// tokens.css 的深色分支（首渲染前绑定，切换不产生首帧闪烁）。
const themeOverrides = computed(() => naiveThemeOverrides(mode.value));
watchEffect(() => {
  document.documentElement.dataset.theme = mode.value;
});

const page = ref<"summary" | "settings">("summary");
// 回到汇总页时强制刷新一次数据（设置页可能重建了缓存）。
const refreshKey = ref(0);
watch(page, (p) => {
  if (p === "summary") refreshKey.value += 1;
});

const themeOptions: { value: ThemePreference; label: string }[] = [
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
  { value: "system", label: "跟随系统" },
];
</script>

<template>
  <NConfigProvider
    :theme="theme"
    :theme-overrides="themeOverrides"
    :locale="zhCN"
    :date-locale="dateZhCN"
  >
    <NGlobalStyle />
    <NMessageProvider>
      <div class="app-shell">
        <header class="app-nav ts-glass">
          <span class="brand">TokenScope</span>
          <nav class="tabs" role="tablist" aria-label="页面切换">
            <button
              type="button"
              role="tab"
              class="tab ts-focusable"
              :class="{ active: page === 'summary' }"
              :aria-selected="page === 'summary'"
              @click="page = 'summary'"
            >
              汇总
            </button>
            <button
              type="button"
              role="tab"
              class="tab ts-focusable"
              :class="{ active: page === 'settings' }"
              :aria-selected="page === 'settings'"
              @click="page = 'settings'"
            >
              设置
            </button>
          </nav>
          <div class="spacer" />
          <label class="theme-select">
            <span>主题</span>
            <select
              class="theme-select-control ts-focusable"
              :value="preference"
              aria-label="主题偏好（浅色/深色/跟随系统）"
              @change="setPreference(($event.target as HTMLSelectElement).value as ThemePreference)"
            >
              <option v-for="o in themeOptions" :key="o.value" :value="o.value">
                {{ o.label }}
              </option>
            </select>
          </label>
        </header>
        <!-- 全局状态横幅：渲染在内容之前，不遮挡主体 -->
        <div class="banner-slot">
          <PricingStatusBanner />
        </div>
        <main class="app-content">
          <Dashboard v-if="page === 'summary'" :refresh-key="refreshKey" />
          <Settings v-else :refresh-key="refreshKey" />
        </main>
      </div>
    </NMessageProvider>
  </NConfigProvider>
</template>

<style scoped>
.app-shell {
  height: 100vh;
  display: flex;
  flex-direction: column;
}

.app-nav {
  flex: 0 0 56px;
  display: flex;
  align-items: center;
  gap: var(--ts-space-4);
  padding: 0 var(--ts-space-6);
  z-index: 10;
}

.brand {
  font-size: 15px;
  font-weight: 700;
  letter-spacing: 0.2px;
  color: var(--ts-text);
}

.tabs {
  display: inline-flex;
  gap: var(--ts-space-1);
  background: var(--ts-accent-soft);
  padding: 3px;
  border-radius: var(--ts-radius);
}

.tab {
  border: none;
  background: transparent;
  color: var(--ts-text-secondary);
  font: inherit;
  font-size: 13px;
  padding: 4px var(--ts-space-4);
  border-radius: var(--ts-radius-control);
  cursor: pointer;
  transition:
    background-color var(--ts-motion) ease,
    color var(--ts-motion) ease;
}

.tab:hover {
  color: var(--ts-text);
}

.tab.active {
  background: var(--ts-accent);
  color: var(--ts-on-accent);
  font-weight: 600;
}

.spacer {
  flex: 1;
}

.theme-select {
  display: inline-flex;
  align-items: center;
  gap: var(--ts-space-2);
  font-size: 12px;
  color: var(--ts-text-secondary);
}

.theme-select-control {
  height: 28px;
  border-radius: var(--ts-radius-control);
  border: 1px solid var(--ts-border);
  background: var(--ts-surface-solid);
  color: var(--ts-text);
  font: inherit;
  padding: 0 var(--ts-space-2);
}

.banner-slot {
  padding: var(--ts-space-3) var(--ts-space-6) 0;
}

.app-content {
  flex: 1;
  overflow-y: auto;
  padding: var(--ts-space-6);
  max-width: 1440px;
  width: 100%;
  margin: 0 auto;
  box-sizing: border-box;
}

/* 窗口接近最小宽度 980px 时左右边距降为 16px（DESIGN.md §4） */
@media (max-width: 1024px) {
  .app-nav {
    padding: 0 var(--ts-space-4);
  }
  .banner-slot {
    padding: var(--ts-space-3) var(--ts-space-4) 0;
  }
  .app-content {
    padding: var(--ts-space-4);
  }
}
</style>
