<script setup lang="ts">
// 关闭确认弹窗（关闭确认与配置文件计划 Task 3）：窗口关闭按钮在未记忆
// 默认动作时触发（后端拦截并 emit close-requested）。取消/Escape/点击
// 遮罩 = 不关闭；勾选记忆后由后端 close_resolve 持久化（设置页可改回）。
import { ref, watch } from "vue";
import { NButton, NCheckbox, NModal } from "naive-ui";

const props = defineProps<{
  open: boolean;
  /** R07：App 侧提交进行中（禁用按钮防重复） */
  submitting?: boolean;
  /** R07：提交失败原因（显示在弹窗内，弹窗保持打开可重试） */
  error?: string | null;
}>();
const emit = defineEmits<{
  (e: "resolve", v: { minimize: boolean; remember: boolean }): void;
  (e: "cancel"): void;
}>();

const remember = ref(false);
// R07：提交由 App 异步执行（写记忆配置可能失败）——失败时弹窗保持
// 打开并显示原因（error prop），用户可重试或取消；submitting prop
// 在 pending 期间禁用提交按钮防重复。
// 每次打开都从未勾选开始：记忆是否保留由设置页管理，弹窗不残留上次选择。
watch(
  () => props.open,
  (v) => {
    if (v) remember.value = false;
  },
);

function resolve(minimize: boolean): void {
  emit("resolve", { minimize, remember: remember.value });
}
</script>

<template>
  <NModal
    :show="open"
    @update:show="
      (v: boolean) => {
        if (!v) emit('cancel');
      }
    "
  >
    <div class="close-dialog" role="dialog" aria-modal="true" aria-label="关闭 TokenScope">
      <div class="close-title">关闭 TokenScope</div>
      <div class="close-desc">要最小化到托盘继续统计，还是直接退出程序？</div>
      <NCheckbox v-model:checked="remember" class="close-remember" :disabled="submitting">
        记住我的选择，以后不再询问（可在设置页修改）
      </NCheckbox>
      <div v-if="error" class="close-error" role="alert">
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
        <span>{{ error }}</span>
      </div>
      <div class="close-actions">
        <NButton size="small" :disabled="submitting" @click="emit('cancel')">取消</NButton>
        <span class="close-actions-spacer" />
        <NButton
          size="small"
          type="primary"
          secondary
          :disabled="submitting"
          @click="resolve(true)"
        >
          最小化到托盘
        </NButton>
        <NButton size="small" type="error" secondary :disabled="submitting" @click="resolve(false)">
          直接退出
        </NButton>
      </div>
    </div>
  </NModal>
</template>

<style scoped>
/* 浮层玻璃配方：elevated 85% + 16px 模糊 + 12px 圆角（DESIGN.md §2 浮层） */
.close-dialog {
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

.close-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.close-desc {
  font-size: 13px;
  line-height: 1.6;
  color: var(--ts-text-secondary);
  margin-top: var(--ts-space-2);
}

.close-remember {
  margin-top: var(--ts-space-4);
}

.close-error {
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
  margin-top: var(--ts-space-3);
  padding: var(--ts-space-2) var(--ts-space-3);
  border-radius: var(--ts-radius-control);
  background: rgba(215, 0, 21, 0.08);
  color: var(--ts-error);
  font-size: 12px;
  line-height: 1.5;
}
:root[data-theme="dark"] .close-error {
  background: rgba(255, 105, 97, 0.12);
}
.close-error .ts-notice-icon {
  color: var(--ts-error);
}

.close-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--ts-space-2);
  margin-top: var(--ts-space-5);
}

.close-actions-spacer {
  flex: 1;
}
</style>
