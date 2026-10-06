<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { dateZhCN, zhCN } from "naive-ui";
import {
  darkTheme,
  NConfigProvider,
  NGlobalStyle,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NMessageProvider,
  NRadioButton,
  NRadioGroup,
  NSwitch,
  type GlobalTheme,
} from "naive-ui";
import { useTheme } from "./composables/theme";
import { naiveThemeOverrides } from "./styles/naiveTheme";
import { watchEffect } from "vue";
import PricingStatusBanner from "./components/PricingStatusBanner.vue";
import Dashboard from "./views/Dashboard.vue";
import Settings from "./views/Settings.vue";

const { mode, toggle } = useTheme();
const theme = computed<GlobalTheme | null>(() => (mode.value === "dark" ? darkTheme : null));
// Task 1：Naive UI 覆盖与 CSS token 双轨消费同一语义色；html[data-theme]
// 驱动 tokens.css 的深色分支，切换不产生首帧闪烁（首渲染前已绑定）。
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
      <NLayout style="height: 100vh">
        <NLayoutHeader
          bordered
          style="padding: 12px 20px; display: flex; align-items: center; gap: 16px"
        >
          <strong style="font-size: 18px">TokenScope</strong>
          <NRadioGroup v-model:value="page" size="small">
            <NRadioButton value="summary" label="汇总" />
            <NRadioButton value="settings" label="设置" />
          </NRadioGroup>
          <div style="flex: 1" />
          <span style="font-size: 12px; opacity: 0.65">暗色</span>
          <NSwitch :value="mode === 'dark'" size="small" @update:value="toggle" />
        </NLayoutHeader>
        <!-- Task 3：首次启动无 models.dev 缓存时的全局同步横幅 -->
        <div style="padding: 12px 20px 0">
          <PricingStatusBanner />
        </div>
        <NLayoutContent style="padding: 16px 20px">
          <Dashboard v-if="page === 'summary'" :refresh-key="refreshKey" />
          <Settings v-else :refresh-key="refreshKey" />
        </NLayoutContent>
      </NLayout>
    </NMessageProvider>
  </NConfigProvider>
</template>
