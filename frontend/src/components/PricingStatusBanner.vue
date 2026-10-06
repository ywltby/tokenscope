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
const syncing = ref(false);
const syncError = ref<string | null>(null);

const visible = computed(() => status.value?.needsSync === true);

async function refreshStatus(): Promise<void> {
  try {
    status.value = await invoke<PricingStatus>("pricing_status");
    statusFailed.value = false;
    statusError.value = null;
  } catch (e) {
    statusFailed.value = true;
    status.value = null;
    statusError.value = e instanceof Error ? e.message : String(e);
  }
}

async function syncNow(): Promise<void> {
  syncing.value = true;
  syncError.value = null;
  // 成功与失败路径都刷新状态：部分成功（主源 OK / 补充源失败）时横幅
  // 依据新状态收敛，同时保留补充源失败原因。
  try {
    await invoke("sync_pricing_openrouter");
  } catch (e) {
    syncError.value = e instanceof Error ? e.message : String(e);
  }
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

const bannerText = "尚未获取定价，需要联网同步价格；当前费用仅能显示为未知。";
</script>

<template>
  <div v-if="!statusFailed && visible" class="pricing-banner ts-glass" role="alert">
    <div class="banner-body">
      <span>{{ bannerText }}</span>
      <NButton size="tiny" type="primary" :loading="syncing" @click="syncNow"> 立即同步 </NButton>
    </div>
  </div>
  <!-- Task 3/审阅修复：部分同步失败原因独立展示——主源已可用横幅收敛后仍可见 -->
  <div v-if="syncError" class="pricing-banner ts-glass is-warning" role="alert">
    <div class="banner-body text-small">
      同步部分失败：{{ syncError }}（可重试；主源已可用时费用仍会正常显示）
    </div>
  </div>
  <!-- pricing_status 读取失败 → 可重试提示而非静默空 DOM（审阅不变量 4） -->
  <div v-else-if="statusFailed" class="pricing-banner ts-glass is-error" role="alert">
    <div class="banner-body">
      <span>定价状态读取失败，可重试：{{ statusError }}</span>
      <NButton size="tiny" :loading="syncing" @click="refreshStatus">重试</NButton>
    </div>
  </div>
</template>

<style scoped>
.pricing-banner {
  margin-bottom: var(--ts-space-3);
  border-radius: var(--ts-radius);
  border-left: 3px solid var(--ts-warning);
  color: var(--ts-text);
}

.pricing-banner.is-error {
  border-left-color: var(--ts-error);
}

.banner-body {
  display: flex;
  align-items: center;
  gap: var(--ts-space-3);
  flex-wrap: wrap;
  padding: var(--ts-space-2) var(--ts-space-3);
  font-size: 13px;
}

.text-small {
  font-size: 12px;
}
</style>
