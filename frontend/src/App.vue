<script setup lang="ts">
// 隐私同意引导壳（P04）：后端 Ready 之前**不导入、不挂载**任何业务模块。
//
// - 只用 Vue + Naive UI 基础组件 + 构建期打包的政策文本 + 引导 IPC；
// - 主题只跟随系统（首帧由 public/theme-boot.js 写 data-theme，不读
//   localStorage）；业务偏好等 Ready 后、挂载主应用之前恢复；
// - 主应用经**动态 import** 挂载：静态 import 会执行模块级副作用
//   （theme/tokenColors 在求值时就读取持久化偏好），v-if 拦不住。
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  darkTheme,
  dateZhCN,
  NButton,
  NConfigProvider,
  NGlobalStyle,
  zhCN,
  type GlobalTheme,
} from "naive-ui";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import PrivacyConsentDialog from "./components/PrivacyConsentDialog.vue";
import PrivacyExitDialog from "./components/PrivacyExitDialog.vue";
import { createPrivacyGate } from "./lib/privacyGate";
import { applyResolvedThemeAttribute } from "./lib/themePreference";

const gate = createPrivacyGate(invoke);

// ── 引导主题：只跟随系统 ────────────────────────────────────
// 不读持久化偏好，也不读写 localStorage（未同意时保持"系统默认"）。
const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
const systemDark = ref(darkQuery.matches);
darkQuery.addEventListener?.("change", (e) => {
  systemDark.value = e.matches;
});
const theme = computed<GlobalTheme | null>(() => (systemDark.value ? darkTheme : null));

// ── 引导期关闭事件（窗口 X / Alt+F4）────────────────────────
let unlisten: (() => void)[] = [];
onMounted(() => {
  listen("privacy-exit-requested", () => gate.onExitRequested())
    .then((un) => unlisten.push(un))
    .catch(() => {
      // 非 Tauri 环境（浏览器预览/测试）没有事件系统：静默跳过
    });
  // 监听就绪后再检查一次：窗口出现的瞬间用户可能已经点了 X（事件早于本次
  // 监听、前端收不到），bootstrap 响应里的 exitPromptPending 会恢复询问。
  void gate.bootstrap();
});
onBeforeUnmount(() => {
  for (const un of unlisten) un();
  unlisten = [];
});

// ── Ready 后挂载主应用 ──────────────────────────────────────
// 惰性组件：模块求值只在显式导入时发生（此处即"同意后"这一个时点）。
const MainApp = defineAsyncComponent(() => import("./MainApp.vue"));
const mainVisible = ref(false);

async function enterMainApp(): Promise<void> {
  if (mainVisible.value || gate.phase.value !== "ready") return;
  // 业务持久化偏好的恢复点：先落首帧主题属性（显式函数，可证明时点），
  // 再导入主应用——主应用 import 链同样只在这里求值一次。
  applyResolvedThemeAttribute();
  await import("./MainApp.vue");
  // 导入期间可能已进入退出流程：晚到响应不得复活主应用。
  if (gate.phase.value !== "ready") return;
  mainVisible.value = true;
}

watch(gate.phase, (phase) => {
  if (phase === "ready") void enterMainApp();
});
</script>

<template>
  <NConfigProvider :theme="theme" :locale="zhCN" :date-locale="dateZhCN">
    <!-- 全局样式只在这里注入一份：业务主应用 MainApp.vue 不再渲染它 -->
    <NGlobalStyle />
    <div class="privacy-shell">
      <div class="ts-ambient" aria-hidden="true"></div>
      <!-- 检查中：最短的加载提示，不闪现统计页背景与旧视图 -->
      <div v-if="gate.phase.value === 'checking'" class="privacy-center">
        <template v-if="gate.error.value">
          <div class="privacy-blocked" role="alert">
            <p class="privacy-blocked-text">{{ gate.error.value }}</p>
            <NButton size="small" @click="gate.retry()">重新检查</NButton>
          </div>
        </template>
        <span v-else class="privacy-checking" role="status">正在检查隐私设置…</span>
      </div>
      <PrivacyConsentDialog
        :open="gate.consentVisible.value"
        :policy="gate.policy.value"
        :saving="gate.saving.value"
        :blocked="gate.blocked.value"
        :message="gate.error.value ?? gate.blockedDetail.value"
        @accept="gate.accept()"
        @reject="gate.reject()"
        @retry="gate.retry()"
      />
      <PrivacyExitDialog
        :open="gate.exitPromptOpen.value"
        :busy="gate.exitBusy.value"
        :error="gate.exitError.value"
        @cancel="gate.cancelExit()"
        @exit="gate.confirmExit()"
      />
      <!-- 主应用：仅在后端 Ready 之后动态挂载 -->
      <MainApp v-if="mainVisible" />
    </div>
  </NConfigProvider>
</template>

<style scoped>
.privacy-shell {
  isolation: isolate;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

/* 政策区之外的引导画面：竖排居中，不做统计页骨架（不产生"先挂后撤"的观感） */
.privacy-center {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--ts-space-8);
}

.privacy-checking {
  font-size: 13px;
  color: var(--ts-text-tertiary);
}

.privacy-blocked {
  max-width: 560px;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--ts-space-3);
  padding: var(--ts-space-5);
  background: var(--ts-surface-elevated);
  border: 1px solid var(--ts-glass-stroke);
  border-radius: var(--ts-radius-popover);
  box-shadow: var(--ts-shadow-elevated);
}

.privacy-blocked-text {
  margin: 0;
  font-size: 13px;
  line-height: 1.7;
  color: var(--ts-error);
  white-space: pre-line;
}
</style>
