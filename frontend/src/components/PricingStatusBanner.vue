<script setup lang="ts">
// Task 3/审阅修复：定价同步状态传播——
// - 同步成功与失败路径都重新读取 pricing_status（部分成功时横幅依据
//   新状态收敛，补充源失败原因仍可见）；
// - pricing_status 读取失败显示可重试提示，不再静默清空隐藏所有诊断；
// - 监听设置页派发的 pricing-status-changed 事件，跨组件刷新状态。
// 设计系统 Task 2：玻璃表面 + 语义状态色（左边框）+ alert 角色；
// 文案与编排逻辑保持不变。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { NButton } from "naive-ui";
import type { PricingStatus } from "../types";

const status = ref<PricingStatus | null>(null);
const statusFailed = ref(false);
const statusError = ref<string | null>(null);
// UX06：状态读取与同步各有独立 pending——重试按钮 loading 不混用；
// 防重复激活（进行中再点不发起第二次）。
const statusLoading = ref(false);
const syncing = ref(false);
const syncError = ref<string | null>(null);
// UX06：请求代次——晚到的旧状态响应不得覆盖新结果。
let statusSeq = 0;

const visible = computed(() => status.value?.needsSync === true);

async function refreshStatus(): Promise<void> {
  if (statusLoading.value) return; // 防重复激活
  statusLoading.value = true;
  const seq = ++statusSeq;
  try {
    const fresh = await invoke<PricingStatus>("pricing_status");
    if (seq !== statusSeq) return; // 晚到的旧响应丢弃
    status.value = fresh;
    statusFailed.value = false;
    statusError.value = null;
  } catch (e) {
    if (seq !== statusSeq) return;
    statusFailed.value = true;
    status.value = null;
    statusError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (seq === statusSeq) statusLoading.value = false;
  }
}

async function syncNow(): Promise<void> {
  if (syncing.value) return; // 防重复激活
  syncing.value = true;
  syncError.value = null;
  // 成功与失败路径都刷新状态：部分成功（主源 OK / 补充源失败）时横幅
  // 依据新状态收敛，同时保留补充源失败原因。
  try {
    await invoke("sync_pricing_openrouter");
  } catch (e) {
    syncError.value = e instanceof Error ? e.message : String(e);
  }
  // 状态读取失败也保留 syncError（两类错误都可读可操作）。
  await refreshStatus();
  syncing.value = false;
}

function onExternalChange(): void {
  void refreshStatus();
}

onMounted(() => {
  window.addEventListener("pricing-status-changed", onExternalChange);
  void refreshStatus();
});

onBeforeUnmount(() => {
  window.removeEventListener("pricing-status-changed", onExternalChange);
});

// UX06：暴露状态读取入口供测试验证防重复激活。
defineExpose({ refreshStatus });

const bannerText = "尚未获取定价，需要联网同步价格；当前费用仅能显示为未知。";
</script>

<template>
  <!-- 任务 3：全局横幅 = 内联通知条（.ts-notice 玻璃配方 + 状态图标 + 文字操作） -->
  <div v-if="!statusFailed && visible" class="ts-notice pricing-notice" role="alert">
    <svg
      class="ts-notice-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3.5 21 19.5H3z" />
      <path d="M12 10v4" />
      <path d="M12 17h.01" />
    </svg>
    <span class="ts-notice-content">{{ bannerText }}</span>
    <NButton size="tiny" type="primary" :loading="syncing" @click="syncNow"> 立即同步 </NButton>
  </div>
  <!-- Task 3/审阅修复：部分同步失败原因独立展示——主源已可用横幅收敛后仍可见 -->
  <div v-if="syncError" class="ts-notice pricing-notice is-error" role="alert">
    <svg
      class="ts-notice-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="m9 9 6 6M15 9l-6 6" />
    </svg>
    <span class="ts-notice-content text-small">
      同步部分失败：{{ syncError }}（可重试；主源已可用时费用仍会正常显示）
    </span>
  </div>
  <!-- pricing_status 读取失败 → 可重试提示而非静默空 DOM（审阅不变量 4）；
       UX06：与同步失败独立渲染（两类错误同时可见可操作） -->
  <div v-if="statusFailed" class="ts-notice pricing-notice is-error" role="alert">
    <svg
      class="ts-notice-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="m9 9 6 6M15 9l-6 6" />
    </svg>
    <span class="ts-notice-content">定价状态读取失败，可重试：{{ statusError }}</span>
    <NButton size="tiny" :loading="statusLoading" @click="refreshStatus">重试</NButton>
  </div>
</template>

<style scoped>
.pricing-notice {
  margin-bottom: var(--ts-space-3);
}

/* 状态只表达同步状态：警告/错误用图标着色，容器保持中性玻璃 */
.pricing-notice .ts-notice-icon {
  color: var(--ts-warning);
}

.pricing-notice.is-error .ts-notice-icon {
  color: var(--ts-error);
}

.text-small {
  font-size: 12px;
}
</style>
