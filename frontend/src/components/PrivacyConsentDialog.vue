<script setup lang="ts">
// 隐私政策弹窗（P04）：离线完整全文 + 明确选择。
//
// 不预选同意、不把关闭弹窗或继续使用当作同意：遮罩与 Esc 都不关闭弹窗
// （保持阻断，不露出可操作主页面），只有「同意并继续」会触发保存；
// 「不同意」进入专属退出确认。
import { computed } from "vue";
import { NButton, NModal } from "naive-ui";
import { renderPolicyMarkdown } from "../lib/policyMarkdown";
import type { PolicyInfo } from "../lib/privacyGate";

const props = defineProps<{
  open: boolean;
  policy: PolicyInfo;
  /** 正在保存同意记录（禁用按钮防重复）。 */
  saving: boolean;
  /** 后端阻断（设置不可读或保存失败）：提供「重新检查」。 */
  blocked: boolean;
  /** 阻断原因（后端 detail 或 IPC 错误）。 */
  message: string | null;
}>();

const emit = defineEmits<{
  (e: "accept"): void;
  (e: "reject"): void;
  (e: "retry"): void;
}>();

const blocks = computed(() => renderPolicyMarkdown(props.policy.markdown));
</script>

<template>
  <NModal
    :show="open"
    :mask-closable="false"
    :close-on-esc="false"
    :auto-focus="true"
    transform-origin="center"
  >
    <div class="privacy-dialog" role="dialog" aria-modal="true" aria-label="TokenScope 隐私政策">
      <div class="privacy-head">
        <div class="privacy-title">{{ policy.title }}</div>
        <div v-if="policy.date" class="privacy-date">更新日期：{{ policy.date }}</div>
      </div>
      <p class="privacy-summary">
        同意前应用不读取任何使用数据：不扫描日志来源、不建立统计缓存、不写诊断日志、不联网同步价格。
        同意后仅在本机读取已启用来源的会话日志用于统计与费用估算，不上传会话与统计数据；
        价格同步会访问 models.dev 与 OpenRouter 的公开目录。完整说明见下方全文。
      </p>
      <div class="privacy-scroll" tabindex="0" role="region" aria-label="隐私政策全文">
        <template v-for="(block, index) in blocks" :key="index">
          <h2 v-if="block.type === 'h1'" class="privacy-h1">{{ block.text }}</h2>
          <h3 v-else-if="block.type === 'h2'" class="privacy-h2">{{ block.text }}</h3>
          <h4 v-else-if="block.type === 'h3'" class="privacy-h3">{{ block.text }}</h4>
          <ul v-else-if="block.type === 'ul'" class="privacy-ul">
            <li v-for="(item, i) in block.items" :key="i">{{ item }}</li>
          </ul>
          <p v-else class="privacy-p">{{ block.text }}</p>
        </template>
      </div>
      <div v-if="message" class="privacy-error" role="alert">
        <span>{{ message }}</span>
      </div>
      <div class="privacy-actions">
        <NButton size="small" :disabled="saving" @click="emit('reject')">不同意</NButton>
        <NButton v-if="blocked" size="small" :disabled="saving" @click="emit('retry')">
          重新检查
        </NButton>
        <span class="privacy-actions-spacer" />
        <NButton
          size="small"
          type="primary"
          :loading="saving"
          :disabled="saving"
          @click="emit('accept')"
        >
          {{ saving ? "正在保存…" : "同意并继续" }}
        </NButton>
      </div>
    </div>
  </NModal>
</template>

<style scoped>
/* 浮层玻璃配方：elevated 85% + 16px 模糊 + 12px 圆角（DESIGN.md §2 浮层） */
.privacy-dialog {
  width: 640px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 80px);
  display: flex;
  flex-direction: column;
  background: var(--ts-surface-elevated);
  -webkit-backdrop-filter: var(--ts-glass-blur-popover);
  backdrop-filter: var(--ts-glass-blur-popover);
  border: 1px solid var(--ts-glass-stroke);
  border-radius: var(--ts-radius-popover);
  box-shadow: var(--ts-shadow-elevated);
  padding: var(--ts-space-5);
}

.privacy-title {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--ts-text);
}

.privacy-date {
  margin-top: 4px;
  font-size: 12px;
  color: var(--ts-text-tertiary);
}

.privacy-summary {
  margin: var(--ts-space-3) 0 0;
  padding: 10px 12px;
  border-radius: var(--ts-radius-control);
  background: var(--ts-fill-subtle);
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--ts-text-secondary);
}

.privacy-scroll {
  margin-top: var(--ts-space-3);
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-right: 6px;
  border: 1px solid var(--ts-glass-stroke);
  border-radius: var(--ts-radius-control);
  padding: var(--ts-space-4);
  background: var(--ts-fill-subtle);
}

.privacy-scroll:focus-visible {
  outline: 2px solid var(--ts-accent);
  outline-offset: 1px;
}

.privacy-h1 {
  margin: 0 0 10px;
  font-size: 15px;
  font-weight: 600;
  color: var(--ts-text);
}

.privacy-h2 {
  margin: 16px 0 8px;
  font-size: 13.5px;
  font-weight: 600;
  color: var(--ts-text);
}

.privacy-h3 {
  margin: 12px 0 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--ts-text-secondary);
}

.privacy-p,
.privacy-ul {
  margin: 0 0 8px;
  font-size: 12.5px;
  line-height: 1.75;
  color: var(--ts-text-secondary);
}

.privacy-ul {
  padding-left: 18px;
}

.privacy-error {
  margin-top: var(--ts-space-3);
  padding: var(--ts-space-2) var(--ts-space-3);
  border-radius: var(--ts-radius-control);
  background: rgba(215, 0, 21, 0.08);
  color: var(--ts-error);
  font-size: 12px;
  line-height: 1.6;
}

.privacy-actions {
  display: flex;
  align-items: center;
  gap: var(--ts-space-2);
  margin-top: var(--ts-space-4);
}

.privacy-actions-spacer {
  flex: 1;
}
</style>
