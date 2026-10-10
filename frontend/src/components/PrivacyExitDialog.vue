<script setup lang="ts">
// 未同意状态的退出确认（P05）：与已记忆的关闭动作完全隔离——不记忆选择、
// 不最小化到托盘、不保存窗口状态。
//
// Escape / 遮罩 / 「返回隐私政策」= 取消（回到仍然阻断的政策界面）；
// 「退出程序」由后端直接结束进程。
import { NButton, NModal } from "naive-ui";

defineProps<{
  open: boolean;
  /** 退出调用进行中（禁用按钮防重复）。 */
  busy: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (e: "cancel"): void;
  (e: "exit"): void;
}>();
</script>

<template>
  <NModal
    :show="open"
    :auto-focus="true"
    transform-origin="center"
    @update:show="
      (value: boolean) => {
        if (!value) emit('cancel');
      }
    "
  >
    <div class="exit-dialog" role="dialog" aria-modal="true" aria-label="退出 TokenScope">
      <div class="exit-title">退出 TokenScope？</div>
      <div class="exit-desc">尚未同意隐私政策，应用不会开始读取使用数据或同步价格。</div>
      <div v-if="error" class="exit-error" role="alert">{{ error }}</div>
      <div class="exit-actions">
        <NButton size="small" :disabled="busy" @click="emit('cancel')">返回隐私政策</NButton>
        <span class="exit-actions-spacer" />
        <NButton
          size="small"
          type="error"
          secondary
          :loading="busy"
          :disabled="busy"
          @click="emit('exit')"
        >
          退出程序
        </NButton>
      </div>
    </div>
  </NModal>
</template>

<style scoped>
/* 浮层玻璃配方：elevated 85% + 16px 模糊 + 12px 圆角（DESIGN.md §2 浮层） */
.exit-dialog {
  width: 420px;
  max-width: calc(100vw - 48px);
  background: var(--ts-surface-elevated);
  -webkit-backdrop-filter: var(--ts-glass-blur-popover);
  backdrop-filter: var(--ts-glass-blur-popover);
  border: 1px solid var(--ts-glass-stroke);
  border-radius: var(--ts-radius-popover);
  box-shadow: var(--ts-shadow-elevated);
  padding: var(--ts-space-5);
}

.exit-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.exit-desc {
  font-size: 13px;
  line-height: 1.6;
  color: var(--ts-text-secondary);
  margin-top: var(--ts-space-2);
}

.exit-error {
  margin-top: var(--ts-space-3);
  padding: var(--ts-space-2) var(--ts-space-3);
  border-radius: var(--ts-radius-control);
  background: rgba(215, 0, 21, 0.08);
  color: var(--ts-error);
  font-size: 12px;
  line-height: 1.5;
}

.exit-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--ts-space-2);
  margin-top: var(--ts-space-5);
}

.exit-actions-spacer {
  flex: 1;
}
</style>
