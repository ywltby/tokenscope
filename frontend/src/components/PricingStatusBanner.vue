<script setup lang="ts">
// Task 3/审阅修复：定价同步状态传播——
// - 同步成功与失败路径都重新读取 pricing_status（部分成功时横幅依据
//   新状态收敛，补充源失败原因仍可见）；
// - pricing_status 读取失败显示可重试提示，不再静默清空隐藏所有诊断；
// - 监听设置页派发的 pricing-status-changed 事件，跨组件刷新状态。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { NAlert, NButton } from "naive-ui";
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
  <NAlert
    v-if="!statusFailed && visible"
    type="warning"
    style="margin-bottom: 12px"
    :closable="false"
  >
    <div style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap">
      <span>{{ bannerText }}</span>
      <NButton size="tiny" type="primary" :loading="syncing" @click="syncNow"> 立即同步 </NButton>
    </div>
  </NAlert>
  <!-- Task 3/审阅修复：部分同步失败原因独立展示——主源已可用横幅收敛后仍可见 -->
  <NAlert v-if="syncError" type="warning" style="margin-bottom: 12px" :closable="false">
    <div style="font-size: 12px">
      同步部分失败：{{ syncError }}（可重试；主源已可用时费用仍会正常显示）
    </div>
  </NAlert>
  <!-- pricing_status 读取失败 → 可重试提示而非静默空 DOM（审阅不变量 4） -->
  <NAlert v-else-if="statusFailed" type="error" style="margin-bottom: 12px" :closable="false">
    <div style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap">
      <span>定价状态读取失败，可重试：{{ statusError }}</span>
      <NButton size="tiny" :loading="syncing" @click="refreshStatus">重试</NButton>
    </div>
  </NAlert>
</template>
