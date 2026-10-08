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
// RC04：卸载后晚到响应不得写状态。
let disposed = false;
/**
 * RC04：状态读取**挂起期间**到达的"状态已变化"信号（同步完成 / 外部
 * `pricing-status-changed`）不得丢失——记下补读意图，当前读取结束后立即
 * 再读一次，最终采用变化后的状态。
 * 重复点击"重试"不设该意图（在途读取本身就会返回最新状态），避免把
 * 用户连点变成额外的重复请求。
 */
let statusReread = false;

const visible = computed(() => status.value?.needsSync === true);

/**
 * RC04：只有**状态已知且已可用价格**时才能说"部分失败"——同步 IPC 失败
 * 只说明这一次同步没完全成功，不能在没有状态佐证时统一宣称"部分成功"
 *（状态未知时可能是整体失败）。
 */
const syncPartial = computed(() => !statusFailed.value && status.value?.hasAnyPricing === true);
const syncErrorText = computed(() => {
  const reason = syncError.value ?? "";
  return syncPartial.value
    ? `同步部分失败：${reason}（可重试；主源已可用时费用仍会正常显示）`
    : `同步失败：${reason}（状态未知，重试同步后恢复）`;
});

async function refreshStatus(opts: { changed?: boolean } = {}): Promise<void> {
  if (statusLoading.value) {
    // RC04：在途时不得丢弃刷新意图——状态变化信号要补读一次。
    if (opts.changed) statusReread = true;
    return;
  }
  statusLoading.value = true;
  try {
    do {
      statusReread = false;
      const seq = ++statusSeq;
      try {
        const fresh = await invoke<PricingStatus>("pricing_status");
        if (seq !== statusSeq || disposed) continue; // 晚到的旧响应丢弃
        status.value = fresh;
        statusFailed.value = false;
        statusError.value = null;
      } catch (e) {
        if (seq !== statusSeq || disposed) continue;
        statusFailed.value = true;
        status.value = null;
        statusError.value = e instanceof Error ? e.message : String(e);
      }
    } while (statusReread);
  } finally {
    statusLoading.value = false;
  }
}

async function syncNow(): Promise<void> {
  if (syncing.value || disposed) return; // 防重复激活
  syncing.value = true;
  // RC04：错误只在**成功**时清理——重试期间保留上次失败原因，避免
  // "点了重试就看不到为什么失败"。
  try {
    await invoke("sync_pricing_openrouter");
    if (!disposed) syncError.value = null;
  } catch (e) {
    // AP04：成功与失败路径适用同一卸载守卫——卸载后不得再写状态。
    if (!disposed) syncError.value = e instanceof Error ? e.message : String(e);
  }
  if (disposed) return;
  // 成功与失败路径都刷新状态：部分成功（主源 OK / 补充源失败）时横幅
  // 依据新状态收敛，同时保留补充源失败原因。状态变化信号用 changed=true
  // ——挂起期间到达也不丢。
  await refreshStatus({ changed: true });
  syncing.value = false;
}

function onExternalChange(): void {
  void refreshStatus({ changed: true });
}

onMounted(() => {
  window.addEventListener("pricing-status-changed", onExternalChange);
  void refreshStatus();
});

onBeforeUnmount(() => {
  // RC04：卸载时使在途请求失效并移除监听。
  disposed = true;
  window.removeEventListener("pricing-status-changed", onExternalChange);
});

// UX06：暴露状态读取入口供测试验证防重复激活。
defineExpose({ refreshStatus });

/**
 * AP05：`needsSync` 只表示**主源（models.dev）没有有效候选**，不等于
 * "没有任何价格"——外置价格表或 OpenRouter 快照可以有价，估算仍在用它们。
 * 固定宣称「当前费用仅能显示为未知」是错的（修复前即如此）：
 * - 完全无价（`hasAnyPricing=false`）→ 当前无可用价格，需联网同步；
 * - 有价但主源缺失 → 主源待同步，已有价格仍参与估算，未收录模型仍未知。
 * 不把原始条目数当有效价格，也不承诺所有请求都能完整计价。
 */
const bannerText = computed(() => {
  const s = status.value;
  if (!s) return "";
  if (!s.hasAnyPricing) {
    return "尚未获取定价，需要联网同步价格；当前费用仅能显示为未知。";
  }
  const external = s.externalValidCount ?? 0;
  const openrouter = s.openrouterValidCount ?? 0;
  const sources = [
    external > 0 ? `外置价格表 ${external} 条` : null,
    openrouter > 0 ? `OpenRouter ${openrouter} 条` : null,
  ]
    .filter(Boolean)
    .join("、");
  return `主源（models.dev）尚待同步，其定价暂不可用；当前费用按已有价格估算（${sources}有效候选），未收录的模型仍显示为未知。`;
});
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
  <!-- Task 3/审阅修复：同步失败原因独立展示——主源已可用横幅收敛后仍可见。
       RC04：补充失败可直接**重试同步**（复用 syncNow），不再是只有文案没有动作 -->
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
    <span class="ts-notice-content text-small">{{ syncErrorText }}</span>
    <NButton size="tiny" :loading="syncing" @click="syncNow">重试同步</NButton>
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
    <NButton size="tiny" :loading="statusLoading" @click="refreshStatus()">重试</NButton>
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
