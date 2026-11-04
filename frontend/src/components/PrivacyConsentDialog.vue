<script setup lang="ts">
// 隐私政策弹窗（P04）：离线完整全文 + 明确选择。
//
// 不预选同意、不把关闭弹窗或继续使用当作同意：遮罩与 Esc 都不关闭弹窗
// （保持阻断，不露出可操作主页面），只有「同意并继续」会触发保存；
// 「不同意」进入专属退出确认。
//
// i18n（2026-11-04）：弹窗内的语言切换**只改内存状态**（lib/i18n 的
// setLocale），绝不触发 settings_get / settings_set_language 或任何
// 持久化——同意前的持久化边界由 App.test.ts 的既有断言守住；选择在
// 同意成功后由 App.vue 的 flushPendingOnConsent 保存。
// 政策/协议正文是后端编译进二进制的简体中文全文（准据语言），英文界面
// 下显示语言标注条，不冒充已翻译。
import { computed, ref } from "vue";
import { NButton, NModal, NSelect } from "naive-ui";
import { renderPolicyMarkdown } from "../lib/policyMarkdown";
import type { PolicyInfo } from "../lib/privacyGate";
import { LOCALES, setLocale, useAppLocale, useT } from "../lib/i18n";

const t = useT();

const props = defineProps<{
  open: boolean;
  policy: PolicyInfo;
  /** 正在保存同意记录（禁用按钮防重复）。 */
  saving: boolean;
  /** 后端阻断（设置不可读或保存失败）：提供「重新检查」。 */
  blocked: boolean;
  /** 阻断原因（后端 detail 或 IPC 错误）。 */
  message: string | null;
  /** WB05：政策已更新提示（旧修订同意触发重新询问时展示；null = 首次同意）。 */
  updateNotice: string | null;
}>();

const emit = defineEmits<{
  (e: "accept"): void;
  (e: "reject"): void;
  (e: "retry"): void;
}>();

const { locale } = useAppLocale();
const localeOptions = LOCALES.map((value) => ({
  value,
  label: value === "zh-CN" ? "简体中文" : "English",
}));
function onLocaleChange(value: string): void {
  setLocale(value as (typeof LOCALES)[number]);
}

const blocks = computed(() => renderPolicyMarkdown(props.policy.markdown));
// 十九轮复核 P2：政策引用 terms.md——同意前必须能离线阅读协议全文。
// 协议由后端编译进二进制（PolicyInfo.terms），弹窗内展开查看，不依赖
// 安装资源路径或远程链接。
const termsBlocks = computed(() =>
  props.policy.terms ? renderPolicyMarkdown(props.policy.terms) : [],
);
const termsOpen = ref(false);
/** 英文界面下政策正文仍是中文原文（准据语言）：显式标注，不冒充翻译。 */
const isForeignPolicy = computed(() => locale.value !== "zh-CN");
</script>

<template>
  <NModal
    :show="open"
    :mask-closable="false"
    :close-on-esc="false"
    :auto-focus="true"
    transform-origin="center"
  >
    <div
      class="privacy-dialog"
      role="dialog"
      aria-modal="true"
      :aria-label="t('consent.dialogAria')"
    >
      <div class="privacy-head">
        <div class="privacy-title">{{ policy.title }}</div>
        <div v-if="policy.date" class="privacy-date">
          {{ t("consent.updatedPrefix") }}{{ policy.date }}
        </div>
        <div v-if="updateNotice" class="privacy-updated" role="status">{{ updateNotice }}</div>
      </div>
      <div class="privacy-lang-row">
        <span class="privacy-lang-label">{{ t("app.languageAria") }}</span>
        <NSelect
          class="privacy-lang-select"
          :value="locale"
          :options="localeOptions"
          size="tiny"
          :aria-label="t('app.languageAria')"
          @update:value="onLocaleChange"
        />
      </div>
      <p class="privacy-summary">{{ t("consent.summary") }}</p>
      <p class="privacy-consent-scope">{{ t("consent.scopeNote") }}</p>
      <p v-if="isForeignPolicy" class="privacy-lang-note" role="note">
        {{ t("consent.governingNote") }}
      </p>
      <div
        class="privacy-scroll"
        tabindex="0"
        role="region"
        :aria-label="t('consent.policyRegion')"
      >
        <template v-for="(block, index) in blocks" :key="index">
          <h2 v-if="block.type === 'h1'" class="privacy-h1">{{ block.text }}</h2>
          <h3 v-else-if="block.type === 'h2'" class="privacy-h2">{{ block.text }}</h3>
          <h4 v-else-if="block.type === 'h3'" class="privacy-h3">{{ block.text }}</h4>
          <ul v-else-if="block.type === 'ul'" class="privacy-ul">
            <li v-for="(item, i) in block.items" :key="i">
              <template v-for="(line, j) in item.split('\n')" :key="j">
                <br v-if="j > 0" />{{ line }}
              </template>
            </li>
          </ul>
          <p v-else class="privacy-p">
            <template v-for="(line, i) in block.text.split('\n')" :key="i">
              <br v-if="i > 0" />{{ line }}
            </template>
          </p>
        </template>
      </div>
      <div
        v-if="termsOpen"
        class="privacy-terms"
        role="region"
        :aria-label="t('consent.termsRegion')"
      >
        <template v-for="(block, i) in termsBlocks" :key="'terms-' + i">
          <h4 v-if="block.type === 'h1' || block.type === 'h2'" class="privacy-h3">
            {{ block.text }}
          </h4>
          <ul v-else-if="block.type === 'ul'" class="privacy-ul">
            <li v-for="(item, j) in block.items || []" :key="j">
              <template v-for="(line, k) in item.split('\n')" :key="k">
                <br v-if="k > 0" />{{ line }}
              </template>
            </li>
          </ul>
          <p v-else class="privacy-p">
            <template v-for="(line, k) in block.text.split('\n')" :key="k">
              <br v-if="k > 0" />{{ line }}
            </template>
          </p>
        </template>
      </div>
      <div v-if="message" class="privacy-error" role="alert">
        <span>{{ message }}</span>
      </div>
      <div v-if="!termsBlocks.length || !blocks.length" class="privacy-error" role="alert">
        {{ t("consent.missingDocs") }}
      </div>
      <div class="privacy-actions">
        <NButton
          v-if="termsBlocks.length"
          size="small"
          :disabled="saving"
          class="privacy-terms-toggle"
          @click="termsOpen = !termsOpen"
        >
          {{ termsOpen ? t("consent.termsHide") : t("consent.termsShow") }}
        </NButton>
        <NButton size="small" :disabled="saving" @click="emit('reject')">
          {{ t("consent.reject") }}
        </NButton>
        <NButton
          v-if="blocked || !termsBlocks.length || !blocks.length"
          size="small"
          :disabled="saving"
          @click="emit('retry')"
        >
          {{ t("consent.retry") }}
        </NButton>
        <span class="privacy-actions-spacer" />
        <NButton
          size="small"
          type="primary"
          :loading="saving"
          :disabled="saving || !termsBlocks.length || !blocks.length"
          @click="emit('accept')"
        >
          {{ saving ? t("consent.saving") : t("consent.accept") }}
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
  padding-bottom: var(--ts-space-4);
}

.privacy-lang-row {
  display: flex;
  align-items: center;
  gap: var(--ts-space-3);
  margin-bottom: var(--ts-space-3);
}
.privacy-lang-label {
  font-size: 12px;
  color: var(--ts-text-secondary);
}
.privacy-lang-select {
  width: 140px;
}
.privacy-lang-note {
  margin: 0 0 var(--ts-space-3);
  font-size: 12px;
  line-height: 1.7;
  color: var(--ts-text-secondary);
}

.privacy-date {
  margin-top: 4px;
  font-size: 12px;
  color: var(--ts-text-tertiary);
}

.privacy-updated {
  margin-top: 8px;
  padding: 8px 12px;
  border-radius: 8px;
  background: rgba(128, 128, 128, 0.12);
  font-size: 13px;
  line-height: 18px;
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

.privacy-consent-scope {
  margin: var(--ts-space-2) 0 0;
  font-size: 12.5px;
  line-height: 1.5;
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

.privacy-terms {
  max-height: 240px;
  overflow: auto;
  margin: 8px 0;
  padding: 10px 12px;
  border: 1px solid var(--ts-hairline);
  border-radius: 8px;
  background: var(--ts-surface-sunken, rgba(0, 0, 0, 0.02));
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
