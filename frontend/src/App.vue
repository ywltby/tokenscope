<script setup lang="ts">
// 应用壳（设计系统 Task 2，苹果风格）：吸顶玻璃导航 + 分段控件页面/主题
// 切换 + 滚动容器。视觉改造不改数据流：回到汇总页仍强制刷新一次（设置页
// 可能重建了缓存）。
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
import SegmentedControl from "./components/SegmentedControl.vue";
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

const pageOptions = [
  { value: "summary" as const, label: "汇总" },
  { value: "settings" as const, label: "设置" },
];

const themeOptions: { value: ThemePreference; label: string }[] = [
  { value: "light", label: "☀" },
  { value: "dark", label: "☾" },
  { value: "system", label: "自动" },
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
          <span class="brand">
            <!-- 品牌标记：表盘/示波器意象的线性图标（DESIGN.md §3 禁 emoji 图标） -->
            <svg
              class="brand-mark"
              viewBox="0 0 24 24"
              width="18"
              height="18"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              aria-hidden="true"
            >
              <circle cx="12" cy="12" r="9" />
              <path d="M12 12 17.2 6.8" />
              <circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none" />
            </svg>
            TokenScope
          </span>
          <SegmentedControl v-model="page" :options="pageOptions" aria-label="页面切换" />
          <div class="spacer" />
          <SegmentedControl
            v-model="preference"
            :options="themeOptions"
            aria-label="主题偏好（浅色/深色/跟随系统）"
            @update:model-value="setPreference"
          />
        </header>
        <div class="scroll-container">
          <!-- 全局状态横幅：渲染在内容之前，不遮挡主体 -->
          <div class="banner-slot">
            <PricingStatusBanner />
          </div>
          <main class="app-content">
            <Dashboard
              v-if="page === 'summary'"
              :refresh-key="refreshKey"
              @go-settings="page = 'settings'"
            />
            <Settings v-else :refresh-key="refreshKey" />
          </main>
        </div>
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
  position: sticky;
  top: 0;
  z-index: 10;
  flex: 0 0 52px;
  display: flex;
  align-items: center;
  gap: var(--ts-space-4);
  padding: 0 var(--ts-space-8);
}

.brand {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.spacer {
  flex: 1;
}

.scroll-container {
  flex: 1;
  overflow-y: auto;
}

.banner-slot {
  padding: var(--ts-space-4) var(--ts-space-8) 0;
}

.app-content {
  padding: var(--ts-space-6) var(--ts-space-8);
  max-width: 1200px;
  width: 100%;
  margin: 0 auto;
  box-sizing: border-box;
}

/* 窗口接近最小宽度 980px 时左右边距降为 20px（DESIGN.md §4） */
@media (max-width: 1100px) {
  .app-nav {
    padding: 0 var(--ts-space-5);
  }
  .banner-slot {
    padding: var(--ts-space-4) var(--ts-space-5) 0;
  }
  .app-content {
    padding: var(--ts-space-5);
  }
}
</style>
