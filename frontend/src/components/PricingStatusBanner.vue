<script setup lang="ts">
// Task 3：定价来源策略——首次启动无 models.dev 缓存时的全局同步横幅。
// 仅依据结构化 pricing_status 渲染（不解析 warning 文本）；不阻塞汇总页。
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { NAlert, NButton } from "naive-ui";
import type { PricingStatus } from "../types";

const status = ref<PricingStatus | null>(null);
const syncing = ref(false);
const syncError = ref<string | null>(null);

const visible = computed(() => status.value?.needsSync === true);

async function refreshStatus(): Promise<void> {
  try {
    status.value = await invoke<PricingStatus>("pricing_status");
  } catch {
    // 状态读取失败：不显示横幅（避免误报），错误由日志承载。
    status.value = null;
  }
}

async function syncNow(): Promise<void> {
  syncing.value = true;
  syncError.value = null;
  try {
    await invoke("sync_pricing_openrouter");
    await refreshStatus();
  } catch (e) {
    // 失败保留横幅并显示原因，不得吞掉。
    syncError.value = e instanceof Error ? e.message : String(e);
  } finally {
    syncing.value = false;
  }
}

onMounted(() => {
  void refreshStatus();
});

const bannerText = "尚未获取定价，需要联网同步价格；当前费用仅能显示为未知。";
</script>

<template>
  <NAlert v-if="visible" type="warning" style="margin-bottom: 12px" :closable="false">
    <div style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap">
      <span>{{ bannerText }}</span>
      <NButton size="tiny" type="primary" :loading="syncing" @click="syncNow"> 立即同步 </NButton>
    </div>
    <div v-if="syncError" style="font-size: 12px; margin-top: 4px">
      同步失败：{{ syncError }}（可重试）
    </div>
  </NAlert>
</template>
